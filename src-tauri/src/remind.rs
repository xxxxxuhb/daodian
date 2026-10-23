use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
#[cfg(not(windows))]
use tauri_plugin_notification::NotificationExt;

use crate::domain::{RemindAction, RemindTarget, now_ms};
use crate::store::{SharedStore, due_open_count, mark_reminded, next_due, plan_from_store};

/// 每小时最多几条系统通知，超出的先攒着。
const MAX_PER_HOUR: usize = 3;
/// 免打扰时段（本地时间）：22 点到次日 8 点。
const QUIET_FROM: u32 = 22;
const QUIET_TO: u32 = 8;
/// 兜底扫描：就算算错了时间，最多 5 分钟也会醒一次。
const FALLBACK_MS: i64 = 300_000;
const MIN_WAIT_MS: i64 = 2_000;
/// 同一条最多重试几次，之后只记账、不再弹。
const MAX_RETRY: u32 = 3;

#[derive(Default)]
struct Runtime {
    /// 本地时区偏移（分钟），由前端同步过来。
    offset_min: i32,
    /// 最近一小时发过通知的时间戳。
    sent: VecDeque<i64>,
    /// 发送失败次数，按 target 记。
    failures: HashMap<String, u32>,
}

static RUNTIME: LazyLock<Mutex<Runtime>> = LazyLock::new(|| Mutex::new(Runtime::default()));
static CHECKING: AtomicBool = AtomicBool::new(false);
static WAKE: OnceLock<Sender<()>> = OnceLock::new();

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReminderEvent {
    key: String,
    title: String,
    detail: String,
    at: i64,
    todo_id: Option<String>,
    plan_id: Option<String>,
    stage_id: Option<String>,
}

pub fn spawn(app: AppHandle) {
    #[cfg(windows)]
    apply_system_offset();
    let (wake, sleeper) = mpsc::channel();
    if WAKE.set(wake).is_err() {
        crate::logging::warn(format_args!("reminder worker already exists"));
        return;
    }
    std::thread::spawn(move || {
        // 等事件循环起来再发第一条通知，避免启动瞬间丢提醒。
        let _ = sleeper.recv_timeout(Duration::from_secs(2));
        loop {
            check(&app);
            match sleeper.recv_timeout(next_wait(&app)) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

/// 下一次醒来：算到最近一个到点；没有就 5 分钟后再看。
fn next_wait(app: &AppHandle) -> Duration {
    let now = now_ms();
    let soonest = {
        let store = app.state::<SharedStore>();
        next_due(store.inner(), now)
    };
    let delta = match soonest {
        Some(at) => (at - now).clamp(MIN_WAIT_MS, FALLBACK_MS),
        None => FALLBACK_MS,
    };
    Duration::from_millis(delta as u64)
}

pub fn check(app: &AppHandle) {
    if CHECKING.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Err(err) = run_check(app) {
        crate::logging::error(format_args!("reminder check failed: {err}"));
    }
    CHECKING.store(false, Ordering::SeqCst);
}

fn run_check(app: &AppHandle) -> Result<(), String> {
    let now = now_ms();
    let actions = {
        let store = app.state::<SharedStore>();
        plan_from_store(store.inner(), now)?
    };
    let open = {
        let store = app.state::<SharedStore>();
        due_open_count(store.inner(), now)?
    };
    if actions.is_empty() {
        return Ok(());
    }

    let muted = in_quiet_hours(now) || !under_cap(now);
    crate::logging::info(format_args!(
        "reminder actions found: count={}; due_open={open}; muted={muted}",
        actions.len()
    ));

    let mut ring: Vec<RemindAction> = Vec::new();
    let mut ack: Vec<(RemindTarget, i64, bool)> = Vec::new();
    for action in actions {
        // 阶段里「错过的」那些只记账，不再单独响。
        if !action.notify {
            ack.push((action.target, action.at, action.lead));
            continue;
        }
        if failures(&action.target) >= MAX_RETRY {
            // 连续发不出去：先记账，别再每一轮都试。
            ack.push((action.target, action.at, action.lead));
            continue;
        }
        if muted {
            // 免打扰或配额用完：留着，等恢复后合并成一条再响。
            continue;
        }
        ring.push(action);
    }

    if ring.is_empty() {
        return ack_only(app, &ack, now);
    }

    match send(app, &ring) {
        Ok(()) => {
            crate::logging::info(format_args!("notification sent: count={}", ring.len()));
            crate::tray::alert(app);
            let payload = ring.iter().map(reminder_event).collect::<Vec<_>>();
            match app.emit("reminder-fired", payload) {
                Ok(()) => crate::logging::info(format_args!(
                    "reminder event emitted: count={}",
                    ring.len()
                )),
                Err(err) => {
                    crate::logging::error(format_args!("reminder event emit failed: {err}"))
                }
            }
            remember_sent(now);
            for action in &ring {
                clear_failure(&action.target);
                ack.push((action.target.clone(), action.at, action.lead));
            }
            ack_only(app, &ack, now)
        }
        Err(err) => {
            crate::logging::error(format_args!(
                "notification send failed: count={}; error={err}",
                ring.len()
            ));
            for action in &ring {
                bump_failure(&action.target);
            }
            let _ = ack_only(app, &ack, now);
            Err(err)
        }
    }
}

fn reminder_event(action: &RemindAction) -> ReminderEvent {
    let (key, todo_id, plan_id, stage_id) = match &action.target {
        RemindTarget::Todo { id } => (
            format!("todo:{id}:{}", action.at),
            Some(id.clone()),
            None,
            None,
        ),
        RemindTarget::Stage { plan_id, stage_id } => (
            format!("stage:{plan_id}:{stage_id}:{}", action.at),
            None,
            Some(plan_id.clone()),
            Some(stage_id.clone()),
        ),
    };
    ReminderEvent {
        key,
        title: action.title.clone(),
        detail: action.detail.clone(),
        at: action.at,
        todo_id,
        plan_id,
        stage_id,
    }
}

/// 一条时用事项名当标题；多条合成一条，别连着弹。
fn send(app: &AppHandle, batch: &[RemindAction]) -> Result<(), String> {
    let (title, body) = compose(batch);
    // 开发态 exe 在 target\debug 里。插件这时不设应用 ID，改用 PowerShell 的身份，
    // Windows 11 会把这条通知丢掉，而且 show() 仍返回成功。
    #[cfg(windows)]
    {
        match crate::win_toast::show(&title, &body) {
            Ok(()) => Ok(()),
            Err(primary) => {
                crate::logging::warn(format_args!(
                    "native toast unavailable; trying tray balloon: {primary}"
                ));
                match crate::win_balloon::show(app, &title, &body) {
                    Ok(()) => {
                        crate::logging::info(format_args!("notification shown as tray balloon"));
                        Ok(())
                    }
                    Err(balloon) => {
                        // 插件在开发目录里会借用 PowerShell 的身份，Windows 11 把通知丢掉，
                        // 但 show() 仍然返回成功。不能把它当成已经通知到了。
                        crate::logging::error(format_args!(
                            "system notification failed; showing main window; toast={primary}; balloon={balloon}"
                        ));
                        crate::tray::show_main(app);
                        Ok(())
                    }
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        app.notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|err| err.to_string())
    }
}

/// Windows 的 Bias 和 JS getTimezoneOffset() 同号：中国是 -480。
#[cfg(windows)]
fn apply_system_offset() {
    use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    unsafe {
        let mut info: TIME_ZONE_INFORMATION = std::mem::zeroed();
        let kind = GetTimeZoneInformation(&mut info);
        if kind == u32::MAX {
            return;
        }
        let extra = match kind {
            2 => info.DaylightBias,
            1 => info.StandardBias,
            _ => 0,
        };
        if let Ok(mut runtime) = RUNTIME.lock() {
            runtime.offset_min = info.Bias + extra;
        }
    }
}

fn compose(batch: &[RemindAction]) -> (String, String) {
    if let [one] = batch {
        return (one.title.clone(), one.detail.clone());
    }
    let names: Vec<&str> = batch
        .iter()
        .take(2)
        .map(|item| item.title.as_str())
        .collect();
    let mut body = names.join("、");
    if batch.len() > 2 {
        body.push_str(&format!(" 等 {} 件", batch.len()));
    }
    (format!("{} 件到点", batch.len()), body)
}

fn ack_only(app: &AppHandle, ack: &[(RemindTarget, i64, bool)], now: i64) -> Result<(), String> {
    if ack.is_empty() {
        return Ok(());
    }
    {
        let store = app.state::<SharedStore>();
        mark_reminded(store.inner(), ack, now)?;
    }
    let _ = app.emit("data-changed", ());
    Ok(())
}

/// 前端把时区偏移同步过来，免打扰才按本地时间算。
#[tauri::command]
pub fn sync_clock(offset_minutes: i32) {
    if let Ok(mut runtime) = RUNTIME.lock() {
        runtime.offset_min = offset_minutes;
    }
}

/// 列表一变就让提醒重算一次，别等下一轮。
#[tauri::command]
pub fn sync_reminders(app: AppHandle) {
    check(&app);
    if let Some(wake) = WAKE.get() {
        let _ = wake.send(());
    }
}

fn local_hour(now: i64) -> u32 {
    let offset = RUNTIME
        .lock()
        .map(|runtime| runtime.offset_min as i64)
        .unwrap_or(0);
    let seconds = now / 1000 - offset * 60;
    (((seconds % 86_400) + 86_400) % 86_400 / 3_600) as u32
}

fn in_quiet_hours(now: i64) -> bool {
    let hour = local_hour(now);
    if QUIET_FROM <= QUIET_TO {
        hour >= QUIET_FROM && hour < QUIET_TO
    } else {
        hour >= QUIET_FROM || hour < QUIET_TO
    }
}

fn under_cap(now: i64) -> bool {
    let Ok(mut runtime) = RUNTIME.lock() else {
        return true;
    };
    while let Some(front) = runtime.sent.front().copied() {
        if now - front > 3_600_000 {
            runtime.sent.pop_front();
        } else {
            break;
        }
    }
    runtime.sent.len() < MAX_PER_HOUR
}

fn remember_sent(now: i64) {
    if let Ok(mut runtime) = RUNTIME.lock() {
        runtime.sent.push_back(now);
    }
}

fn key_of(target: &RemindTarget) -> String {
    match target {
        RemindTarget::Todo { id } => format!("todo:{id}"),
        RemindTarget::Stage { plan_id, stage_id } => format!("stage:{plan_id}:{stage_id}"),
    }
}

fn failures(target: &RemindTarget) -> u32 {
    RUNTIME
        .lock()
        .ok()
        .and_then(|runtime| runtime.failures.get(&key_of(target)).copied())
        .unwrap_or(0)
}

fn bump_failure(target: &RemindTarget) {
    if let Ok(mut runtime) = RUNTIME.lock() {
        let count = runtime.failures.entry(key_of(target)).or_insert(0);
        *count += 1;
    }
}

fn clear_failure(target: &RemindTarget) {
    if let Ok(mut runtime) = RUNTIME.lock() {
        runtime.failures.remove(&key_of(target));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(title: &str, detail: &str) -> RemindAction {
        RemindAction {
            target: RemindTarget::Todo {
                id: title.to_string(),
            },
            at: 0,
            title: title.to_string(),
            detail: detail.to_string(),
            notify: true,
            lead: false,
        }
    }

    #[test]
    fn single_notification_keeps_the_item_name() {
        let (title, body) = compose(&[action("提测评审", "1.2 版本 · 阶段 2/4")]);
        assert_eq!(title, "提测评审");
        assert_eq!(body, "1.2 版本 · 阶段 2/4");
    }

    #[test]
    fn several_notifications_merge_into_one() {
        let (title, body) = compose(&[
            action("提测评审", ""),
            action("交周报", ""),
            action("回复王工", ""),
        ]);
        assert_eq!(title, "3 件到点");
        assert_eq!(body, "提测评审、交周报 等 3 件");
    }

    #[test]
    fn quiet_hours_wrap_midnight() {
        assert!(in_quiet_hours_at(23));
        assert!(in_quiet_hours_at(3));
        assert!(in_quiet_hours_at(22));
        assert!(!in_quiet_hours_at(8));
        assert!(!in_quiet_hours_at(14));
    }

    fn in_quiet_hours_at(hour: u32) -> bool {
        if QUIET_FROM <= QUIET_TO {
            hour >= QUIET_FROM && hour < QUIET_TO
        } else {
            hour >= QUIET_FROM || hour < QUIET_TO
        }
    }
}
