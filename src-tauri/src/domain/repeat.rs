use super::model::Repeat;

/// 完成这一次之后，下一次的本地时间。`offset_secs` 是本地时区相对 UTC 的秒数，中国是 28800。
pub fn next_due(repeat: &Repeat, due_ms: i64, offset_secs: i64) -> Option<i64> {
    let local = due_ms / 1000 + offset_secs;
    let days = local.div_euclid(86_400);
    let clock = local.rem_euclid(86_400);
    let next_days = match repeat.rule.as_str() {
        "weekly" => days + 7,
        "weekdays" => next_weekday(days),
        "monthEnd" => next_month_end(days),
        _ => return None,
    };
    Some((next_days * 86_400 + clock - offset_secs) * 1000)
}

fn next_weekday(days: i64) -> i64 {
    let mut cursor = days + 1;
    while matches!(weekday(cursor), 0 | 6) {
        cursor += 1;
    }
    cursor
}

fn next_month_end(days: i64) -> i64 {
    let (year, month, _, _) = civil(days);
    let (year, month) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    days_from_civil(year, month, last_day(year, month))
}

fn last_day(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
    }
}

/// 和 `Date.getDay()` 一样：0 是周日。
fn weekday(days: i64) -> i64 {
    (days + 4).rem_euclid(7)
}

fn civil(days: i64) -> (i32, u32, u32, i64) {
    let (year, month, day) = civil_from_days(days);
    (year, month, day, weekday(days))
}

fn civil_from_days(mut z: i64) -> (i32, u32, u32) {
    z += 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    (year as i32, month as u32, day as u32)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if month > 2 { month - 3 } else { month + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + day as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

#[cfg(windows)]
pub fn local_offset_secs() -> i64 {
    use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    unsafe {
        let mut info: TIME_ZONE_INFORMATION = std::mem::zeroed();
        let kind = GetTimeZoneInformation(&mut info);
        if kind == u32::MAX {
            return 0;
        }
        let extra = match kind {
            2 => info.DaylightBias,
            1 => info.StandardBias,
            _ => 0,
        };
        -((info.Bias + extra) as i64) * 60
    }
}

#[cfg(not(windows))]
pub fn local_offset_secs() -> i64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repeat(rule: &str) -> Repeat {
        Repeat {
            rule: rule.to_string(),
            weekday: None,
        }
    }

    #[test]
    fn weekly_steps_seven_days() {
        let due = days_from_civil(2026, 10, 23) * 86_400 * 1000 + 18 * 3_600_000;
        let next = next_due(&repeat("weekly"), due, 0).unwrap();
        assert_eq!(next - due, 7 * 86_400_000);
    }

    #[test]
    fn weekdays_skip_the_weekend() {
        let friday = days_from_civil(2026, 10, 23) * 86_400 * 1000;
        assert_eq!(weekday(days_from_civil(2026, 10, 23)), 5);
        let next = next_due(&repeat("weekdays"), friday, 0).unwrap();
        let next_days = next / 1000 / 86_400;
        assert_eq!(civil_from_days(next_days), (2026, 10, 26));
    }

    #[test]
    fn month_end_lands_on_the_last_day() {
        let due = days_from_civil(2026, 1, 31) * 86_400 * 1000 + 9 * 3_600_000;
        let next = next_due(&repeat("monthEnd"), due, 0).unwrap();
        let next_days = (next / 1000).div_euclid(86_400);
        assert_eq!(civil_from_days(next_days), (2026, 2, 28));
        assert_eq!(next.rem_euclid(86_400_000), 9 * 3_600_000);
    }
}
