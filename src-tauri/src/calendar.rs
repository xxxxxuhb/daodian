use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::str::FromStr;

use chrono::{
    Datelike, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc,
    Weekday,
};
use chrono_tz::Tz;
use serde::Serialize;

const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub starts_at: i64,
    pub ends_at: i64,
    pub all_day: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarLoad {
    pub events: Vec<CalendarEvent>,
    pub errors: Vec<String>,
    pub status: String,
}

#[derive(Clone)]
enum EventZone {
    Utc,
    Local,
    Named(Tz),
}

#[derive(Clone)]
struct EventTime {
    local: NaiveDateTime,
    zone: EventZone,
    all_day: bool,
}

impl EventTime {
    fn millis(&self) -> Result<i64, String> {
        resolve(&self.zone, self.local)
    }

    fn on_date(&self, date: NaiveDate) -> Result<i64, String> {
        resolve(&self.zone, date.and_time(self.local.time()))
    }
}

#[derive(Default)]
struct RawEvent {
    values: HashMap<String, (HashMap<String, String>, String)>,
}

#[derive(Default)]
struct Rule {
    frequency: String,
    interval: i64,
    count: Option<usize>,
    until: Option<i64>,
    by_days: Vec<Weekday>,
}

#[tauri::command]
pub fn refresh_calendar(path: String) -> Result<CalendarLoad, String> {
    let now = Local::now();
    let start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .ok_or("无法计算今天")?;
    let start = resolve(&EventZone::Local, start)?;
    let raw =
        fs::read_to_string(Path::new(&path)).map_err(|err| format!("读取日历文件失败: {err}"))?;
    Ok(parse_ics_window(&raw, start, start + 7 * DAY_MS))
}

fn parse_ics_window(raw: &str, window_start: i64, window_end: i64) -> CalendarLoad {
    let lines = unfold(raw);
    let mut raw_events = Vec::new();
    let mut current: Option<RawEvent> = None;
    for line in lines {
        match line.as_str() {
            "BEGIN:VEVENT" => current = Some(RawEvent::default()),
            "END:VEVENT" => {
                if let Some(event) = current.take() {
                    raw_events.push(event);
                }
            }
            _ => {
                if let Some(event) = current.as_mut() {
                    if let Some((head, value)) = line.split_once(':') {
                        let mut parts = head.split(';');
                        let name = parts.next().unwrap_or_default().to_ascii_uppercase();
                        let params = parts
                            .filter_map(|part| part.split_once('='))
                            .map(|(key, value)| {
                                (
                                    key.to_ascii_uppercase(),
                                    value.trim_matches('"').to_string(),
                                )
                            })
                            .collect();
                        event.values.insert(name, (params, value.to_string()));
                    }
                }
            }
        }
    }

    let mut events = Vec::new();
    let mut errors = Vec::new();
    for (index, raw_event) in raw_events.iter().enumerate() {
        match expand_event(raw_event, index, window_start, window_end) {
            Ok(mut expanded) => events.append(&mut expanded),
            Err(err) => errors.push(format!("事件 {}: {err}", index + 1)),
        }
    }
    events.sort_by_key(|event| (event.starts_at, event.ends_at));
    let status = if errors.is_empty() {
        format!("已读取 {} 个日程", events.len())
    } else {
        format!(
            "已读取 {} 个日程，跳过 {} 个坏事件",
            events.len(),
            errors.len()
        )
    };
    CalendarLoad {
        events,
        errors,
        status,
    }
}

fn unfold(raw: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in raw.replace("\r\n", "\n").replace('\r', "\n").lines() {
        if (line.starts_with(' ') || line.starts_with('\t')) && !lines.is_empty() {
            lines.last_mut().unwrap().push_str(&line[1..]);
        } else {
            lines.push(line.to_string());
        }
    }
    lines
}

fn expand_event(
    event: &RawEvent,
    index: usize,
    window_start: i64,
    window_end: i64,
) -> Result<Vec<CalendarEvent>, String> {
    let start_property = event.values.get("DTSTART").ok_or("缺少 DTSTART")?;
    let start = parse_event_time(&start_property.1, &start_property.0)?;
    let first_start = start.millis()?;
    let first_end = match event.values.get("DTEND") {
        Some((params, value)) => parse_event_time(value, params)?.millis()?,
        None if start.all_day => first_start + DAY_MS,
        None => first_start + 3_600_000,
    };
    if first_end <= first_start {
        return Err("DTEND 不晚于 DTSTART".into());
    }
    let duration = first_end - first_start;
    let uid = text_value(event, "UID").unwrap_or_else(|| format!("local-event-{index}"));
    let summary = text_value(event, "SUMMARY").unwrap_or_else(|| "未命名日程".into());
    let location = text_value(event, "LOCATION").unwrap_or_default();
    let rule = match event.values.get("RRULE") {
        Some((_, value)) => Some(parse_rule(value, &start)?),
        None => None,
    };

    let make = |occurrence: usize, starts_at: i64| CalendarEvent {
        uid: if occurrence == 0 {
            uid.clone()
        } else {
            format!("{uid}#{starts_at}")
        },
        summary: summary.clone(),
        location: location.clone(),
        starts_at,
        ends_at: starts_at + duration,
        all_day: start.all_day,
    };

    let Some(rule) = rule else {
        return Ok(if first_start < window_end && first_end > window_start {
            vec![make(0, first_start)]
        } else {
            Vec::new()
        });
    };

    let mut output = Vec::new();
    let mut sequence = 0usize;
    let mut scanned = 0i64;
    let interval = rule.interval.max(1);
    while scanned < 36600 && sequence < rule.count.unwrap_or(usize::MAX) {
        let date = start.local.date() + Duration::days(scanned);
        let matches = match rule.frequency.as_str() {
            "DAILY" => scanned % interval == 0,
            "WEEKLY" => {
                let weeks = scanned / 7;
                weeks % interval == 0
                    && (rule.by_days.is_empty() && date.weekday() == start.local.weekday()
                        || rule.by_days.contains(&date.weekday()))
            }
            _ => return Err(format!("不支持的 RRULE FREQ={}", rule.frequency)),
        };
        scanned += 1;
        if !matches {
            continue;
        }
        let starts_at = start.on_date(date)?;
        if rule.until.is_some_and(|until| starts_at > until) {
            break;
        }
        let occurrence = sequence;
        sequence += 1;
        if starts_at >= window_end {
            break;
        }
        if starts_at + duration > window_start {
            output.push(make(occurrence, starts_at));
        }
    }
    Ok(output)
}

fn text_value(event: &RawEvent, name: &str) -> Option<String> {
    event
        .values
        .get(name)
        .map(|(_, value)| unescape_text(value))
}

fn unescape_text(value: &str) -> String {
    value
        .replace("\\n", "\n")
        .replace("\\N", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

fn parse_rule(value: &str, start: &EventTime) -> Result<Rule, String> {
    let mut rule = Rule {
        interval: 1,
        ..Rule::default()
    };
    for part in value.split(';') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key.to_ascii_uppercase().as_str() {
            "FREQ" => rule.frequency = value.to_ascii_uppercase(),
            "INTERVAL" => {
                rule.interval = value.parse().map_err(|_| "RRULE INTERVAL 无效")?;
            }
            "COUNT" => rule.count = Some(value.parse().map_err(|_| "RRULE COUNT 无效")?),
            "UNTIL" => rule.until = Some(parse_until(value, start)?),
            "BYDAY" => {
                rule.by_days = value
                    .split(',')
                    .map(parse_weekday)
                    .collect::<Result<Vec<_>, _>>()?;
            }
            _ => {}
        }
    }
    if !matches!(rule.frequency.as_str(), "DAILY" | "WEEKLY") {
        return Err(format!("不支持的 RRULE FREQ={}", rule.frequency));
    }
    Ok(rule)
}

fn parse_weekday(value: &str) -> Result<Weekday, String> {
    match value.trim_start_matches(|c: char| c.is_ascii_digit() || c == '-') {
        "MO" => Ok(Weekday::Mon),
        "TU" => Ok(Weekday::Tue),
        "WE" => Ok(Weekday::Wed),
        "TH" => Ok(Weekday::Thu),
        "FR" => Ok(Weekday::Fri),
        "SA" => Ok(Weekday::Sat),
        "SU" => Ok(Weekday::Sun),
        other => Err(format!("RRULE BYDAY 无效: {other}")),
    }
}

fn parse_until(value: &str, start: &EventTime) -> Result<i64, String> {
    if value.len() == 8 {
        let date =
            NaiveDate::parse_from_str(value, "%Y%m%d").map_err(|_| "RRULE UNTIL 日期无效")?;
        return resolve(
            &start.zone,
            date.and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap()),
        );
    }
    let params = HashMap::new();
    let mut parsed = parse_event_time(value, &params)?;
    if !value.ends_with('Z') {
        parsed.zone = start.zone.clone();
    }
    parsed.millis()
}

fn parse_event_time(value: &str, params: &HashMap<String, String>) -> Result<EventTime, String> {
    let all_day = params.get("VALUE").is_some_and(|value| value == "DATE")
        || (value.len() == 8 && !value.contains('T'));
    let local = if all_day {
        NaiveDate::parse_from_str(value, "%Y%m%d")
            .map_err(|_| format!("日期格式无效: {value}"))?
            .and_hms_opt(0, 0, 0)
            .unwrap()
    } else {
        let value = value.trim_end_matches('Z');
        NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S")
            .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M"))
            .map_err(|_| format!("日期时间格式无效: {value}"))?
    };
    let zone = if value.ends_with('Z') {
        EventZone::Utc
    } else if let Some(tzid) = params.get("TZID") {
        EventZone::Named(parse_timezone(tzid)?)
    } else {
        EventZone::Local
    };
    Ok(EventTime {
        local,
        zone,
        all_day,
    })
}

fn parse_timezone(value: &str) -> Result<Tz, String> {
    let normalized = value.trim_start_matches('/');
    let alias = match normalized {
        "China Standard Time" | "PRC" => "Asia/Shanghai",
        "Eastern Standard Time" => "America/New_York",
        "Central Standard Time" => "America/Chicago",
        "Mountain Standard Time" => "America/Denver",
        "Pacific Standard Time" => "America/Los_Angeles",
        "UTC" | "GMT" => "Etc/UTC",
        other => other,
    };
    Tz::from_str(alias).map_err(|_| format!("不认识时区 {value}"))
}

fn resolve(zone: &EventZone, value: NaiveDateTime) -> Result<i64, String> {
    let result = match zone {
        EventZone::Utc => return Ok(Utc.from_utc_datetime(&value).timestamp_millis()),
        EventZone::Local => Local
            .from_local_datetime(&value)
            .map(|date| date.with_timezone(&Utc)),
        EventZone::Named(zone) => zone
            .from_local_datetime(&value)
            .map(|date| date.with_timezone(&Utc)),
    };
    match result {
        LocalResult::Single(value) => Ok(value.timestamp_millis()),
        LocalResult::Ambiguous(first, _) => Ok(first.timestamp_millis()),
        LocalResult::None => Err(format!("时间落在不存在的夏令时刻: {value}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(value: &str) -> i64 {
        NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S")
            .unwrap()
            .and_utc()
            .timestamp_millis()
    }

    #[test]
    fn parses_utc_local_tzid_and_all_day() {
        let raw = "BEGIN:VCALENDAR\r\n\
BEGIN:VEVENT\r\nUID:u\r\nDTSTART:20261023T090000Z\r\nDTEND:20261023T100000Z\r\nSUMMARY:UTC\r\nLOCATION:Online\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:t\r\nDTSTART;TZID=Asia/Shanghai:20261024T090000\r\nDTEND;TZID=Asia/Shanghai:20261024T100000\r\nSUMMARY:上海\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:a\r\nDTSTART;VALUE=DATE:20261025\r\nDTEND;VALUE=DATE:20261026\r\nSUMMARY:全天\r\nEND:VEVENT\r\nEND:VCALENDAR";
        let result = parse_ics_window(raw, utc("20261023T000000"), utc("20261030T000000"));
        assert_eq!(result.events.len(), 3);
        assert_eq!(result.events[0].location, "Online");
        assert!(result.events.iter().any(|event| event.all_day));
        assert!(result.errors.is_empty());
    }

    #[test]
    fn expands_daily_and_weekly_with_limits() {
        let raw = "BEGIN:VCALENDAR\n\
BEGIN:VEVENT\nUID:d\nDTSTART:20261023T090000Z\nSUMMARY:每日\nRRULE:FREQ=DAILY;COUNT=3\nEND:VEVENT\n\
BEGIN:VEVENT\nUID:w\nDTSTART:20261019T100000Z\nSUMMARY:周会\nRRULE:FREQ=WEEKLY;BYDAY=MO,FR;UNTIL=20261030T235959Z\nEND:VEVENT\nEND:VCALENDAR";
        let result = parse_ics_window(raw, utc("20261023T000000"), utc("20261030T000000"));
        assert_eq!(
            result
                .events
                .iter()
                .filter(|event| event.summary == "每日")
                .count(),
            3
        );
        assert_eq!(
            result
                .events
                .iter()
                .filter(|event| event.summary == "周会")
                .count(),
            2
        );
    }

    #[test]
    fn skips_bad_events_and_reports_them() {
        let raw = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:坏事件\nEND:VEVENT\n\
BEGIN:VEVENT\nDTSTART:20261023T090000Z\nSUMMARY:好事件\nEND:VEVENT\nEND:VCALENDAR";
        let result = parse_ics_window(raw, utc("20261023T000000"), utc("20261030T000000"));
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(result.status.contains("跳过 1 个"));
    }
}
