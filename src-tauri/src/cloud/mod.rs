mod api;
mod merge;
pub(crate) mod state;
mod sync;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::cloud::api::Gitee;
use crate::cloud::state::new_device_id;
use crate::settings::{CloudSettings, Settings, SharedSettings};
use crate::store::SharedStore;

pub use sync::{CloudReport, run as sync_now, spawn};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudStatus {
    pub enabled: bool,
    pub has_token: bool,
    pub owner: String,
    pub repo: String,
    pub branch: String,
    pub dir: String,
    pub auto_sync: bool,
    pub interval_minutes: u32,
    pub last_sync_at: i64,
    pub last_status: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    pub owner: String,
    pub repo: String,
    pub branch: String,
    pub created: bool,
}

fn status_of(settings: &Settings, message: &str) -> CloudStatus {
    let cloud = &settings.cloud;
    CloudStatus {
        enabled: cloud.enabled,
        has_token: cloud.has_token,
        owner: cloud.owner.clone(),
        repo: cloud.repo.clone(),
        branch: cloud.branch.clone(),
        dir: cloud.dir.clone(),
        auto_sync: cloud.auto_sync,
        interval_minutes: cloud.interval_minutes,
        last_sync_at: cloud.last_sync_at,
        last_status: cloud.last_status.clone(),
        message: message.to_string(),
    }
}

fn current(app: &AppHandle, message: &str) -> Result<CloudStatus, String> {
    let settings = app.state::<SharedSettings>();
    let guard = settings.lock().map_err(|err| err.to_string())?;
    Ok(status_of(&guard.public_settings(), message))
}

fn emit_status(app: &AppHandle, status: &CloudStatus) {
    let _ = app.emit("cloud-status", status);
}

#[tauri::command]
pub fn cloud_status(app: AppHandle) -> Result<CloudStatus, String> {
    current(&app, "")
}

#[tauri::command]
pub fn cloud_save_config(app: AppHandle, settings: State<SharedSettings>, cloud: CloudSettings) -> Result<CloudStatus, String> {
    let saved = {
        let mut guard = settings.lock().map_err(|err| err.to_string())?;
        guard.save_cloud(cloud)?;
        status_of(&guard.public_settings(), "")
    };
    emit_status(&app, &saved);
    Ok(saved)
}

#[tauri::command]
pub fn cloud_set_token(app: AppHandle, settings: State<SharedSettings>, token: String) -> Result<CloudStatus, String> {
    let saved = {
        let mut guard = settings.lock().map_err(|err| err.to_string())?;
        guard.set_cloud_token(&token)?;
        status_of(&guard.public_settings(), "令牌已保存")
    };
    emit_status(&app, &saved);
    Ok(saved)
}

#[tauri::command]
pub fn cloud_clear_token(app: AppHandle, settings: State<SharedSettings>) -> Result<CloudStatus, String> {
    let saved = {
        let mut guard = settings.lock().map_err(|err| err.to_string())?;
        guard.clear_cloud_token()?;
        status_of(&guard.public_settings(), "令牌已删除")
    };
    emit_status(&app, &saved);
    Ok(saved)
}

#[tauri::command]
pub fn cloud_create_repo(app: AppHandle, settings: State<SharedSettings>, name: String) -> Result<RepoInfo, String> {
    let token = {
        let guard = settings.lock().map_err(|err| err.to_string())?;
        String::from_utf8(guard.cloud_token()?).map_err(|_| "令牌读不出来，请重新填写".to_string())?
    };
    let name = clean_repo_name(&name)?;
    let gitee = Gitee::new(&token).map_err(|err| err.message)?;
    let login = gitee.login().map_err(|err| err.message)?;
    let info = match gitee.repo(&login, &name) {
        Ok(repo) if !repo.private => {
            return Err("已有同名仓库，但是公开的。换个名字，或把仓库改成私有。".into());
        }
        Ok(repo) => repo,
        Err(err) if err.code == 404 => gitee.create_repo(&name).map_err(|err| err.message)?,
        Err(err) => return Err(err.message),
    };
    if info.branch.is_empty() {
        return Err("仓库已建好，但还没有默认分支。请确认创建时勾选了自动初始化。".into());
    }
    {
        let mut guard = settings.lock().map_err(|err| err.to_string())?;
        let mut cloud = guard.public_settings().cloud;
        cloud.enabled = true;
        cloud.owner = info.owner.clone();
        cloud.repo = info.repo.clone();
        cloud.branch = info.branch.clone();
        if cloud.dir.trim().is_empty() {
            cloud.dir = "daodian".into();
        }
        if cloud.device_id.is_empty() {
            cloud.device_id = new_device_id();
        }
        guard.save_cloud(cloud)?;
    }
    let report = sync_now(&app).map_err(|err| sync::failure_status(&err).1.to_string())?;
    let saved = current(&app, &report.message)?;
    emit_status(&app, &saved);
    Ok(RepoInfo {
        owner: info.owner,
        repo: info.repo,
        branch: info.branch,
        created: info.created,
    })
}

#[tauri::command]
pub fn cloud_test(app: AppHandle) -> Result<String, String> {
    let (cloud, token) = {
        let settings = app.state::<SharedSettings>();
        let guard = settings.lock().map_err(|err| err.to_string())?;
        let cloud = guard.public_settings().cloud;
        let token = String::from_utf8(guard.cloud_token()?).map_err(|_| "令牌读不出来，请重新填写".to_string())?;
        (cloud, token)
    };
    if cloud.owner.trim().is_empty() || cloud.repo.trim().is_empty() {
        return Err("先填写仓库的所有者和名字，或点创建并连接".into());
    }
    let gitee = Gitee::new(&token).map_err(|err| err.message)?;
    let _login = gitee.login().map_err(|err| err.message)?;
    let repo = gitee.repo(&cloud.owner, &cloud.repo).map_err(|err| {
        if err.code == 404 {
            "找不到这个仓库。私有库也会这样，请确认令牌勾选了 projects。".into()
        } else {
            err.message
        }
    })?;
    if !repo.private {
        return Err("这个仓库是公开的，不能用来同步。".into());
    }
    let branch = if cloud.branch.trim().is_empty() { repo.branch } else { cloud.branch };
    let path = format!("{}/store.json", if cloud.dir.trim().is_empty() { "daodian" } else { cloud.dir.as_str() });
    match gitee.read(&cloud.owner, &cloud.repo, &path, &branch) {
        Ok(_) => {}
        Err(err) if err.code == 404 => {}
        Err(err) => return Err(err.message),
    }
    let settings = app.state::<SharedSettings>();
    if let Ok(mut guard) = settings.lock() {
        let _ = guard.set_cloud_status("ok", None);
    }
    Ok("令牌有效，仓库是私有的，可以同步".into())
}

#[tauri::command]
pub fn cloud_sync_now(app: AppHandle) -> Result<CloudReport, String> {
    match sync_now(&app) {
        Ok(report) => Ok(report),
        Err(err) => {
            let (status, message) = sync::failure_status(&err);
            if let Some(settings) = app.try_state::<SharedSettings>() {
                if let Ok(mut guard) = settings.lock() {
                    let _ = guard.set_cloud_status(status, None);
                    let public = guard.public_settings();
                    drop(guard);
                    emit_status(&app, &status_of(&public, message));
                }
            }
            Err(message.to_string())
        }
    }
}

#[tauri::command]
pub fn cloud_disconnect(app: AppHandle, settings: State<SharedSettings>, store: State<SharedStore>) -> Result<CloudStatus, String> {
    let saved = {
        let mut guard = settings.lock().map_err(|err| err.to_string())?;
        guard.disconnect_cloud()?;
        status_of(&guard.public_settings(), "已断开，本地数据还在")
    };
    if let Ok(library) = store.lock() {
        if let Ok(dir) = library.data_dir() {
            state::clear_sync_dir(dir);
        }
    }
    emit_status(&app, &saved);
    Ok(saved)
}

#[tauri::command]
pub fn cloud_open_token_page() -> Result<(), String> {
    open_token_page()
}

#[cfg(windows)]
fn open_token_page() -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb: Vec<u16> = std::ffi::OsStr::new("open").encode_wide().chain(std::iter::once(0)).collect();
    let file: Vec<u16> = std::ffi::OsStr::new("https://gitee.com/profile/personal_access_tokens/new")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let code = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if (code as isize) <= 32 {
        return Err("打不开 Gitee 令牌页面".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn open_token_page() -> Result<(), String> {
    Err("当前系统打不开浏览器".into())
}

fn clean_repo_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    let name = if name.is_empty() { "daodian-sync" } else { name };
    let ok = !name.is_empty()
        && name.len() <= 100
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if !ok {
        return Err("仓库名只能包含字母、数字、- 和 _".into());
    }
    Ok(name.to_string())
}
