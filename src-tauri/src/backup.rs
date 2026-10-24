use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use chrono::{Datelike, Local, NaiveDate, Timelike, Weekday};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::settings::{RepeatSettings, SharedSettings, WorkHoursSettings};
use crate::store::SharedStore;

const FILES: [&str; 3] = ["store.json", "templates.json", "settings.json"];
const CHECK_INTERVAL: Duration = Duration::from_secs(10 * 60);

pub struct BackupState {
    root: PathBuf,
}

impl BackupState {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            root: data_dir.join("backups"),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    id: String,
    label: String,
    files: Vec<String>,
    created_at: i64,
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            if let Err(err) = check_daily(&app) {
                crate::logging::warn(format_args!("每日备份检查失败: {err}"));
            }
            std::thread::sleep(CHECK_INTERVAL);
        }
    });
}

fn check_daily(app: &AppHandle) -> Result<(), String> {
    let now = Local::now();
    let (hours, calendar) = app
        .state::<SharedSettings>()
        .lock()
        .map_err(|err| err.to_string())?
        .backup_schedule();
    if !is_workday(now.date_naive(), &calendar) || !in_work_hours(now.hour(), now.minute(), &hours)
    {
        return Ok(());
    }
    let date = now
        .date_naive()
        .pred_opt()
        .ok_or_else(|| "无法计算前一天日期".to_string())?;
    let id = date.format("%Y-%m-%d").to_string();
    if create_snapshot(app, &id)? {
        crate::logging::info(format_args!("daily backup created: {id}"));
    }
    Ok(())
}

fn minutes(value: &str) -> Option<u32> {
    let (hour, minute) = value.split_once(':')?;
    let hour = hour.parse::<u32>().ok()?;
    let minute = minute.parse::<u32>().ok()?;
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}

fn in_work_hours(hour: u32, minute: u32, hours: &WorkHoursSettings) -> bool {
    let now = hour * 60 + minute;
    let periods = [
        (minutes(&hours.morning_start), minutes(&hours.morning_end)),
        (
            minutes(&hours.afternoon_start),
            minutes(&hours.afternoon_end),
        ),
    ];
    periods
        .into_iter()
        .any(|(start, end)| matches!((start, end), (Some(start), Some(end)) if now >= start && now < end))
}

fn is_workday(date: NaiveDate, calendar: &RepeatSettings) -> bool {
    let key = date.format("%Y-%m-%d").to_string();
    if calendar.workdays.contains(&key) {
        return true;
    }
    if calendar.holidays.contains(&key) {
        return false;
    }
    !matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
}

fn create_snapshot(app: &AppHandle, id: &str) -> Result<bool, String> {
    let state = app.state::<BackupState>();
    let target = state.root.join(id);
    if target.exists() {
        return Ok(false);
    }
    let store_state = app.state::<SharedStore>();
    let library = store_state.lock().map_err(|err| err.to_string())?;
    let settings_state = app.state::<SharedSettings>();
    let _settings = settings_state.lock().map_err(|err| err.to_string())?;
    let data_dir = library.data_dir()?.to_path_buf();
    fs::create_dir_all(&state.root).map_err(|err| err.to_string())?;
    let temporary = state.root.join(format!(".{id}.tmp"));
    if temporary.exists() {
        fs::remove_dir_all(&temporary).map_err(|err| err.to_string())?;
    }
    fs::create_dir_all(&temporary).map_err(|err| err.to_string())?;
    let result = (|| {
        for name in FILES {
            let source = data_dir.join(name);
            if source.is_file() {
                fs::copy(&source, temporary.join(name)).map_err(|err| err.to_string())?;
            }
        }
        if !temporary.join("store.json").is_file() {
            return Err("没有可备份的 store.json".to_string());
        }
        fs::rename(&temporary, &target).map_err(|err| err.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result.map(|_| true)
}

#[tauri::command]
pub fn list_backups(state: State<BackupState>) -> Result<Vec<BackupInfo>, String> {
    if !state.root.exists() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(&state.root).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        if !entry.file_type().map_err(|err| err.to_string())?.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        if id.starts_with('.') || !entry.path().join("store.json").is_file() {
            continue;
        }
        let files = FILES
            .iter()
            .filter(|name| entry.path().join(name).is_file())
            .map(|name| (*name).to_string())
            .collect();
        let created_at = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);
        let label = if id.starts_with("before-restore-") {
            format!("恢复前快照 · {}", id.trim_start_matches("before-restore-"))
        } else {
            format!("{id} 数据")
        };
        result.push(BackupInfo {
            id,
            label,
            files,
            created_at,
        });
    }
    result.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(result)
}

#[tauri::command]
pub fn restore_backup(app: AppHandle, id: String) -> Result<String, String> {
    if id.is_empty()
        || id.contains("..")
        || !id
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_'))
    {
        return Err("备份标识无效".into());
    }
    let root = app.state::<BackupState>().root.clone();
    let source = root.join(&id);
    if !source.join("store.json").is_file() {
        return Err("这份备份不存在或不完整".into());
    }

    let safety_id = format!("before-restore-{}", Local::now().format("%Y%m%d-%H%M%S"));
    let _ = create_snapshot(&app, &safety_id)?;

    let store_state = app.state::<SharedStore>();
    let mut library = store_state.lock().map_err(|err| err.to_string())?;
    let settings_state = app.state::<SharedSettings>();
    let mut settings = settings_state.lock().map_err(|err| err.to_string())?;
    let data_dir = library.data_dir()?.to_path_buf();
    for name in FILES {
        let backup = source.join(name);
        if backup.is_file() {
            fs::copy(backup, data_dir.join(name))
                .map_err(|err| format!("恢复 {name} 失败: {err}"))?;
        }
    }
    library.reload_from_disk()?;
    settings.reload_from_disk()?;
    let restored_settings = settings.public_settings();
    drop(settings);
    drop(library);

    let _ = app.emit("data-changed", ());
    let _ = app.emit("templates-changed", ());
    let _ = app.emit("settings-changed", restored_settings);
    let _ = crate::quick_capture::persist_registration_status(&app);
    let _ = crate::pet::sync_visibility(&app);
    crate::remind::sync_reminders(app);
    Ok(format!("已恢复 {id} 的数据，并保留恢复前快照"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_hours_include_both_periods_but_not_lunch() {
        let hours = WorkHoursSettings::default();
        assert!(in_work_hours(9, 0, &hours));
        assert!(!in_work_hours(12, 30, &hours));
        assert!(in_work_hours(17, 59, &hours));
        assert!(!in_work_hours(18, 0, &hours));
    }

    #[test]
    fn calendar_overrides_weekends_and_holidays() {
        let mut calendar = RepeatSettings::default();
        calendar.workdays.push("2026-10-24".into());
        calendar.holidays.push("2026-10-23".into());
        assert!(is_workday(
            NaiveDate::from_ymd_opt(2026, 10, 24).unwrap(),
            &calendar
        ));
        assert!(!is_workday(
            NaiveDate::from_ymd_opt(2026, 10, 23).unwrap(),
            &calendar
        ));
    }
}
