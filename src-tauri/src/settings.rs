use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

pub type SharedSettings = Mutex<SettingsStore>;

const SETTINGS_VERSION: u32 = 1;
const DEFAULT_WEEK_TEMPLATE: &str =
    "# 本周周报\n\n## 概览\n{{overview}}\n\n## 待办\n{{todos}}\n\n## 计划推进\n{{plans}}";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: String,
    pub launch_at_startup: bool,
    pub window: WindowSettings,
    pub pet: PetSettings,
    pub repeat: RepeatSettings,
    pub work_hours: WorkHoursSettings,
    pub week_report: WeekReportSettings,
    pub calendar: CalendarSettings,
    pub feishu: WebhookSettings,
    pub wecom: WebhookSettings,
    pub quick_capture: QuickCaptureSettings,
    pub deep_seek: DeepSeekSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "celadon".into(),
            launch_at_startup: false,
            window: WindowSettings::default(),
            pet: PetSettings::default(),
            repeat: RepeatSettings::default(),
            work_hours: WorkHoursSettings::default(),
            week_report: WeekReportSettings::default(),
            calendar: CalendarSettings::default(),
            feishu: WebhookSettings::default(),
            wecom: WebhookSettings::default(),
            quick_capture: QuickCaptureSettings::default(),
            deep_seek: DeepSeekSettings::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PetSettings {
    pub enabled: bool,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub pinches: u32,
    pub hidden_until: i64,
    #[serde(default = "default_idle_min_seconds")]
    pub idle_min_seconds: u32,
    #[serde(default = "default_idle_max_seconds")]
    pub idle_max_seconds: u32,
}

fn default_idle_min_seconds() -> u32 {
    12
}

fn default_idle_max_seconds() -> u32 {
    28
}

impl Default for PetSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            x: None,
            y: None,
            pinches: 0,
            hidden_until: 0,
            idle_min_seconds: default_idle_min_seconds(),
            idle_max_seconds: default_idle_max_seconds(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkHoursSettings {
    pub morning_start: String,
    pub morning_end: String,
    pub afternoon_start: String,
    pub afternoon_end: String,
}

impl Default for WorkHoursSettings {
    fn default() -> Self {
        Self {
            morning_start: "09:00".into(),
            morning_end: "12:00".into(),
            afternoon_start: "13:30".into(),
            afternoon_end: "18:00".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowSettings {
    pub frames: BTreeMap<String, WindowFrame>,
    pub last_tier: String,
    pub last_non_mini_tier: String,
    pub default_tier: String,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            frames: BTreeMap::new(),
            last_tier: String::new(),
            last_non_mini_tier: String::new(),
            default_tier: "standard".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowFrame {
    pub w: f64,
    pub h: f64,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

impl Default for WindowFrame {
    fn default() -> Self {
        Self {
            w: 900.0,
            h: 600.0,
            x: None,
            y: None,
        }
    }
}

impl WindowFrame {
    pub fn sized(w: f64, h: f64) -> Self {
        Self {
            w,
            h,
            x: None,
            y: None,
        }
    }
}

fn validate_pet_idle(settings: &PetSettings) -> Result<(), String> {
    if settings.idle_min_seconds < 5 {
        return Err("随机动画最短间隔不能小于 5 秒".into());
    }
    if settings.idle_max_seconds < settings.idle_min_seconds {
        return Err("随机动画最长间隔不能小于最短间隔".into());
    }
    if settings.idle_max_seconds > 30 * 60 {
        return Err("随机动画最长间隔不能超过 30 分钟".into());
    }
    Ok(())
}

fn validate_work_hours(settings: &WorkHoursSettings) -> Result<(), String> {
    fn minutes(value: &str) -> Option<u16> {
        let (hour, minute) = value.split_once(':')?;
        let hour = hour.parse::<u16>().ok()?;
        let minute = minute.parse::<u16>().ok()?;
        (hour < 24 && minute < 60).then_some(hour * 60 + minute)
    }

    let values = [
        minutes(&settings.morning_start),
        minutes(&settings.morning_end),
        minutes(&settings.afternoon_start),
        minutes(&settings.afternoon_end),
    ];
    let [
        Some(morning_start),
        Some(morning_end),
        Some(afternoon_start),
        Some(afternoon_end),
    ] = values
    else {
        return Err("工作时间格式无效".into());
    };
    if !(morning_start < morning_end
        && morning_end <= afternoon_start
        && afternoon_start < afternoon_end)
    {
        return Err("工作时间需按上午开始、上午结束、下午开始、下午结束依次设置".into());
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct RepeatSettings {
    pub default_skip_weekends: bool,
    pub holidays: Vec<String>,
    pub workdays: Vec<String>,
}

impl Default for RepeatSettings {
    fn default() -> Self {
        Self {
            default_skip_weekends: true,
            holidays: Vec::new(),
            workdays: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct WeekReportSettings {
    pub template: String,
    pub range_offset: i32,
}

impl Default for WeekReportSettings {
    fn default() -> Self {
        Self {
            template: DEFAULT_WEEK_TEMPLATE.into(),
            range_offset: 0,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct CalendarSettings {
    pub enabled: bool,
    pub path: String,
    pub last_status: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct WebhookSettings {
    pub enabled: bool,
    pub webhook: String,
    pub last_status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuickCaptureSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_quick_capture_shortcut")]
    pub shortcut: String,
    #[serde(default)]
    pub last_status: String,
}

fn default_quick_capture_shortcut() -> String {
    "Alt+Space".into()
}

impl Default for QuickCaptureSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            shortcut: default_quick_capture_shortcut(),
            last_status: String::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct DeepSeekSettings {
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    pub has_api_key: bool,
    pub last_status: String,
}

impl Default for DeepSeekSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: "https://api.deepseek.com".into(),
            model: "deepseek-chat".into(),
            has_api_key: false,
            last_status: String::new(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
struct SettingsDocument {
    version: u32,
    legacy_migrated: bool,
    settings: Settings,
}

impl Default for SettingsDocument {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            legacy_migrated: false,
            settings: Settings::default(),
        }
    }
}

pub struct SettingsStore {
    path: PathBuf,
    secret_path: PathBuf,
    doc: SettingsDocument,
}

impl SettingsStore {
    pub fn open(dir: &Path) -> Result<Self, std::io::Error> {
        if !dir.exists() {
            fs::create_dir_all(dir)?;
        }
        let path = dir.join("settings.json");
        let secret_path = dir.join("deepseek-key.dpapi");
        let doc = if path.exists() {
            let raw = fs::read_to_string(&path)?;
            serde_json::from_str(&raw).map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("设置读不出来: {err}"),
                )
            })?
        } else {
            SettingsDocument::default()
        };
        let store = Self {
            path,
            secret_path,
            doc,
        };
        store.persist().map_err(std::io::Error::other)?;
        Ok(store)
    }

    pub(crate) fn public_settings(&self) -> Settings {
        let mut settings = self.doc.settings.clone();
        settings.deep_seek.has_api_key = self.secret_path.exists();
        settings
    }

    fn persist(&self) -> Result<(), String> {
        let raw = serde_json::to_string_pretty(&self.doc).map_err(|err| err.to_string())?;
        fs::write(&self.path, raw).map_err(|err| err.to_string())
    }

    pub(crate) fn reload_from_disk(&mut self) -> Result<(), String> {
        let raw = fs::read_to_string(&self.path).map_err(|err| err.to_string())?;
        self.doc = serde_json::from_str(&raw).map_err(|err| format!("备份设置读不出来: {err}"))?;
        Ok(())
    }

    fn save(
        &mut self,
        mut settings: Settings,
        api_key: Option<String>,
        clear_api_key: bool,
    ) -> Result<Settings, String> {
        crate::webhook::validate_settings(&settings.feishu, crate::webhook::Provider::Feishu)?;
        crate::webhook::validate_settings(&settings.wecom, crate::webhook::Provider::Wecom)?;
        crate::deepseek::validate_settings(&settings.deep_seek)?;
        validate_work_hours(&settings.work_hours)?;
        validate_pet_idle(&settings.pet)?;
        if !matches!(
            settings.window.default_tier.as_str(),
            "mini" | "standard" | "wide"
        ) {
            return Err("默认窗口大小无效".into());
        }
        let default_tier = settings.window.default_tier.clone();
        // Window frames and active tier are owned by the native window service.
        // A settings form can otherwise overwrite a newer resize/move event.
        settings.window = self.doc.settings.window.clone();
        settings.window.default_tier = default_tier;
        // 位置和互动次数由桌面珠子窗口写入，设置抽屉只负责启用开关。
        settings.pet.x = self.doc.settings.pet.x;
        settings.pet.y = self.doc.settings.pet.y;
        settings.pet.pinches = self.doc.settings.pet.pinches;
        settings.pet.hidden_until = self.doc.settings.pet.hidden_until;
        settings.deep_seek.has_api_key = false;
        if clear_api_key {
            match fs::remove_file(&self.secret_path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(format!("删除 DeepSeek API Key 失败: {err}")),
            }
        }
        if let Some(key) = api_key.filter(|value| !value.trim().is_empty()) {
            protect_secret(&self.secret_path, key.trim().as_bytes())?;
        }
        self.doc.settings = settings;
        self.persist()?;
        Ok(self.public_settings())
    }

    pub(crate) fn window_settings(&self) -> WindowSettings {
        self.doc.settings.window.clone()
    }

    pub(crate) fn pet_settings(&self) -> PetSettings {
        self.doc.settings.pet.clone()
    }

    pub(crate) fn save_pet_position(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.doc.settings.pet.x = Some(x);
        self.doc.settings.pet.y = Some(y);
        self.persist()
    }

    pub(crate) fn record_pet_pinch(&mut self) -> Result<u32, String> {
        self.doc.settings.pet.pinches = self.doc.settings.pet.pinches.saturating_add(1);
        self.persist()?;
        Ok(self.doc.settings.pet.pinches)
    }

    pub(crate) fn hide_pet_until(&mut self, until: i64) -> Result<(), String> {
        self.doc.settings.pet.hidden_until = until;
        self.persist()
    }

    pub(crate) fn repeat_calendar(&self) -> (Vec<String>, Vec<String>) {
        (
            self.doc.settings.repeat.holidays.clone(),
            self.doc.settings.repeat.workdays.clone(),
        )
    }

    pub(crate) fn work_hours_settings(&self) -> WorkHoursSettings {
        self.doc.settings.work_hours.clone()
    }

    pub(crate) fn backup_schedule(&self) -> (WorkHoursSettings, RepeatSettings) {
        (
            self.doc.settings.work_hours.clone(),
            self.doc.settings.repeat.clone(),
        )
    }

    pub(crate) fn webhook_settings(&self) -> (WebhookSettings, WebhookSettings) {
        (
            self.doc.settings.feishu.clone(),
            self.doc.settings.wecom.clone(),
        )
    }

    pub(crate) fn quick_capture_settings(&self) -> QuickCaptureSettings {
        self.doc.settings.quick_capture.clone()
    }

    pub(crate) fn deep_seek_credentials(&self) -> Result<(DeepSeekSettings, Vec<u8>), String> {
        let settings = self.doc.settings.deep_seek.clone();
        if !settings.enabled {
            return Err("DeepSeek 尚未启用".into());
        }
        let key = unprotect_secret(&self.secret_path)?;
        if key.is_empty() {
            return Err("DeepSeek API Key 未设置".into());
        }
        Ok((settings, key))
    }

    pub(crate) fn set_deep_seek_status(&mut self, status: &str) -> Result<(), String> {
        self.doc.settings.deep_seek.last_status = status.to_string();
        self.persist()
    }

    pub(crate) fn set_quick_capture_status(&mut self, status: &str) -> Result<(), String> {
        self.doc.settings.quick_capture.last_status = status.to_string();
        self.persist()
    }

    pub(crate) fn set_webhook_status(
        &mut self,
        provider: crate::webhook::Provider,
        status: &str,
    ) -> Result<(), String> {
        match provider {
            crate::webhook::Provider::Feishu => {
                self.doc.settings.feishu.last_status = status.to_string()
            }
            crate::webhook::Provider::Wecom => {
                self.doc.settings.wecom.last_status = status.to_string()
            }
        }
        self.persist()
    }

    pub(crate) fn set_active_window_tier(&mut self, tier: &str) -> Result<(), String> {
        self.doc.settings.window.last_tier = tier.to_string();
        if tier != "mini" {
            self.doc.settings.window.last_non_mini_tier = tier.to_string();
        }
        self.persist()
    }

    pub(crate) fn save_window_frame(
        &mut self,
        tier: &str,
        frame: WindowFrame,
    ) -> Result<(), String> {
        self.doc
            .settings
            .window
            .frames
            .insert(tier.to_string(), frame);
        self.set_active_window_tier(tier)
    }

    fn migrate_legacy(
        &mut self,
        theme: Option<String>,
        frames: Option<BTreeMap<String, WindowFrame>>,
        week_template: Option<String>,
    ) -> Result<Settings, String> {
        if !self.doc.legacy_migrated {
            if matches!(theme.as_deref(), Some("celadon" | "night")) {
                self.doc.settings.theme = theme.unwrap();
            }
            if let Some(frames) = frames.filter(|items| !items.is_empty()) {
                self.doc.settings.window.frames = frames;
            }
            if let Some(template) = week_template.filter(|value| !value.trim().is_empty()) {
                self.doc.settings.week_report.template = template;
            }
            self.doc.legacy_migrated = true;
            self.persist()?;
        }
        Ok(self.public_settings())
    }
}

fn lock(store: &SharedSettings) -> Result<MutexGuard<'_, SettingsStore>, String> {
    store.lock().map_err(|err| err.to_string())
}

#[tauri::command]
pub fn get_settings(store: State<SharedSettings>) -> Result<Settings, String> {
    Ok(lock(&store)?.public_settings())
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    store: State<SharedSettings>,
    settings: Settings,
    deep_seek_api_key: Option<String>,
    clear_deep_seek_api_key: Option<bool>,
) -> Result<Settings, String> {
    {
        lock(&store)?.save(
            settings,
            deep_seek_api_key,
            clear_deep_seek_api_key.unwrap_or(false),
        )?;
    }
    let launch_at_startup = lock(&store)?.public_settings().launch_at_startup;
    crate::autostart::sync(launch_at_startup).map_err(|err| format!("更新开机启动失败: {err}"))?;
    crate::quick_capture::persist_registration_status(&app)?;
    crate::pet::sync_visibility(&app)?;
    let saved = lock(&store)?.public_settings();
    let _ = app.emit("settings-changed", saved.clone());
    Ok(saved)
}

#[tauri::command]
pub fn migrate_legacy_settings(
    store: State<SharedSettings>,
    theme: Option<String>,
    frames: Option<BTreeMap<String, WindowFrame>>,
    week_template: Option<String>,
) -> Result<Settings, String> {
    lock(&store)?.migrate_legacy(theme, frames, week_template)
}

#[cfg(windows)]
fn protect_secret(path: &Path, secret: &[u8]) -> Result<(), String> {
    use std::ptr::null;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: secret.len().try_into().map_err(|_| "API Key 过长")?,
        pbData: secret.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        CryptProtectData(
            &input,
            null(),
            null(),
            null(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(format!(
            "Windows DPAPI 加密失败: {}",
            std::io::Error::last_os_error()
        ));
    }
    let encrypted = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let result = fs::write(path, encrypted).map_err(|err| err.to_string());
    unsafe {
        LocalFree(output.pbData.cast());
    }
    result
}

#[cfg(windows)]
fn unprotect_secret(path: &Path) -> Result<Vec<u8>, String> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };

    let encrypted = fs::read(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            "DeepSeek API Key 未设置".to_string()
        } else {
            "读取 DeepSeek API Key 失败".to_string()
        }
    })?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: encrypted
            .len()
            .try_into()
            .map_err(|_| "DeepSeek API Key 数据无效")?,
        pbData: encrypted.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("Windows DPAPI 解密 API Key 失败".into());
    }
    let plain =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(plain)
}

#[cfg(not(windows))]
fn protect_secret(_path: &Path, _secret: &[u8]) -> Result<(), String> {
    Err("当前平台不支持 Windows DPAPI，未保存 API Key".into())
}

#[cfg(not(windows))]
fn unprotect_secret(_path: &Path) -> Result<Vec<u8>, String> {
    Err("当前平台不支持 Windows DPAPI".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::current_dir()
            .expect("测试工作目录")
            .join("target")
            .join("settings-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn defaults_keep_all_external_features_off() {
        let settings = Settings::default();
        assert!(!settings.launch_at_startup);
        assert!(!settings.calendar.enabled);
        assert!(!settings.feishu.enabled);
        assert!(!settings.wecom.enabled);
        assert!(!settings.quick_capture.enabled);
        assert_eq!(settings.quick_capture.shortcut, "Alt+Space");
        assert!(!settings.deep_seek.enabled);
        assert!(!settings.deep_seek.has_api_key);
        assert!(settings.repeat.default_skip_weekends);
        assert_eq!(settings.window.default_tier, "standard");
        assert_eq!(settings.pet.idle_min_seconds, 12);
        assert_eq!(settings.pet.idle_max_seconds, 28);
    }

    #[test]
    fn pet_idle_keeps_a_five_second_minimum() {
        let legacy: PetSettings = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert_eq!(legacy.idle_min_seconds, 12);
        assert!(validate_pet_idle(&legacy).is_ok());

        let mut too_short = PetSettings::default();
        too_short.idle_min_seconds = 4;
        assert!(validate_pet_idle(&too_short).is_err());
        too_short.idle_min_seconds = 20;
        too_short.idle_max_seconds = 10;
        assert!(validate_pet_idle(&too_short).is_err());
    }

    #[test]
    fn legacy_quick_capture_gets_default_shortcut() {
        let settings: QuickCaptureSettings = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert!(settings.enabled);
        assert_eq!(settings.shortcut, "Alt+Space");
        assert!(settings.last_status.is_empty());
    }

    #[test]
    fn work_hours_default_and_order_are_validated() {
        let mut settings = WorkHoursSettings::default();
        assert_eq!(settings.morning_start, "09:00");
        assert_eq!(settings.afternoon_end, "18:00");
        assert!(validate_work_hours(&settings).is_ok());
        settings.afternoon_start = "11:30".into();
        assert!(validate_work_hours(&settings).is_err());
    }

    #[test]
    fn persists_settings_beside_store() {
        let dir = temp_dir("persist");
        let mut store = SettingsStore::open(&dir).unwrap();
        let mut settings = store.public_settings();
        settings.theme = "night".into();
        settings.repeat.default_skip_weekends = true;
        settings.window.default_tier = "wide".into();
        store.save(settings, None, false).unwrap();

        let reopened = SettingsStore::open(&dir).unwrap();
        assert_eq!(reopened.public_settings().theme, "night");
        assert!(reopened.public_settings().repeat.default_skip_weekends);
        assert_eq!(reopened.public_settings().window.default_tier, "wide");
        assert!(dir.join("settings.json").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_values_are_migrated_only_once() {
        let dir = temp_dir("migration");
        let mut store = SettingsStore::open(&dir).unwrap();
        store
            .migrate_legacy(Some("night".into()), None, Some("旧模板".into()))
            .unwrap();
        store
            .migrate_legacy(Some("celadon".into()), None, Some("覆盖".into()))
            .unwrap();
        assert_eq!(store.public_settings().theme, "night");
        assert_eq!(store.public_settings().week_report.template, "旧模板");
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(windows)]
    #[test]
    fn api_key_is_encrypted_and_never_serialized() {
        let dir = temp_dir("secret");
        let mut store = SettingsStore::open(&dir).unwrap();
        let key = "sk-super-secret-test";
        let public = store
            .save(Settings::default(), Some(key.into()), false)
            .unwrap();
        assert!(public.deep_seek.has_api_key);
        assert!(
            !fs::read_to_string(dir.join("settings.json"))
                .unwrap()
                .contains(key)
        );
        assert!(
            !String::from_utf8_lossy(&fs::read(dir.join("deepseek-key.dpapi")).unwrap())
                .contains(key)
        );
        let _ = fs::remove_dir_all(dir);
    }
}
