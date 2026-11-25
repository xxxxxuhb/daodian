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
    pub screenshot: ScreenshotSettings,
    pub deep_seek: DeepSeekSettings,
    pub cloud: CloudSettings,
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
            screenshot: ScreenshotSettings::default(),
            deep_seek: DeepSeekSettings::default(),
            cloud: CloudSettings::default(),
        }
    }
}

/// 珠子的外观。每个槽位一个 id，空表示不戴；渲染只在页面上做，Rust 只负责存。
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PetLook {
    pub hair: Option<String>,
    pub hair_color: Option<String>,
    pub palette: Option<String>,
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
    #[serde(default = "default_true")]
    pub follow_input: bool,
    #[serde(default = "default_true")]
    pub show_key_names: bool,
    #[serde(default = "default_interaction")]
    pub interaction: String,
    pub look: PetLook,
}

fn default_true() -> bool {
    true
}

fn default_idle_min_seconds() -> u32 {
    12
}

fn default_idle_max_seconds() -> u32 {
    28
}

fn default_interaction() -> String {
    "normal".into()
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
            follow_input: true,
            show_key_names: true,
            interaction: default_interaction(),
            look: PetLook::default(),
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

fn validate_cloud(settings: &CloudSettings) -> Result<(), String> {
    let dir_ok = settings
        .dir
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        && !settings.dir.is_empty();
    if !dir_ok {
        return Err("同步目录只能包含字母、数字、- 和 _".into());
    }
    let name_ok = |value: &str| {
        value.is_empty()
            || value
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    };
    let branch_ok = settings.branch.is_empty()
        || settings
            .branch
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '/' | '.'));
    if !name_ok(&settings.owner) || !name_ok(&settings.repo) || !branch_ok {
        return Err("仓库名只能包含字母、数字、- 和 _".into());
    }
    Ok(())
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
    if !matches!(settings.interaction.as_str(), "calm" | "normal" | "playful") {
        return Err("珠子互动档位不正确".into());
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
pub struct ScreenshotSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_screenshot_shortcut")]
    pub shortcut: String,
    pub last_status: String,
    /// 用户指定的「保存到」。空字符串表示图片\到点截图。
    #[serde(default)]
    pub save_dir: String,
    /// 上次另存实际使用的目录。设置表单不改它，避免把新记住的路径写回去。
    #[serde(default)]
    pub last_save_dir: String,
    /// freeze：先冻结再选。live：实时选择。旧的 native 会在读取时改成 freeze。
    #[serde(default = "default_capture_mode")]
    pub capture_mode: String,
    /// 截图只走独立进程。字段保留，读出来一律是 true。
    #[serde(default = "default_true")]
    pub sidecar: bool,
}

pub(crate) fn normalize_screenshot(shot: &mut ScreenshotSettings) {
    if shot.capture_mode != "live" {
        shot.capture_mode = "freeze".into();
    }
    shot.sidecar = true;
}

fn default_capture_mode() -> String {
    "freeze".into()
}

fn default_screenshot_shortcut() -> String {
    "Ctrl+1".into()
}

impl Default for ScreenshotSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            shortcut: default_screenshot_shortcut(),
            last_status: String::new(),
            save_dir: String::new(),
            last_save_dir: String::new(),
            capture_mode: default_capture_mode(),
            sidecar: true,
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct CloudSettings {
    pub enabled: bool,
    pub provider: String,
    pub owner: String,
    pub repo: String,
    pub branch: String,
    pub dir: String,
    pub auto_sync: bool,
    pub interval_minutes: u32,
    pub device_id: String,
    pub last_sync_at: i64,
    pub last_status: String,
    pub has_token: bool,
}

impl Default for CloudSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "gitee".into(),
            owner: String::new(),
            repo: String::new(),
            branch: String::new(),
            dir: "daodian".into(),
            auto_sync: true,
            interval_minutes: 10,
            device_id: String::new(),
            last_sync_at: 0,
            last_status: "idle".into(),
            has_token: false,
        }
    }
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
    token_path: PathBuf,
    doc: SettingsDocument,
}

impl SettingsStore {
    pub fn open(dir: &Path) -> Result<Self, std::io::Error> {
        if !dir.exists() {
            fs::create_dir_all(dir)?;
        }
        let path = dir.join("settings.json");
        let secret_path = dir.join("deepseek-key.dpapi");
        let token_path = dir.join("gitee-token.dpapi");
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
        let mut store = Self {
            path,
            secret_path,
            token_path,
            doc,
        };
        normalize_screenshot(&mut store.doc.settings.screenshot);
        store.persist().map_err(std::io::Error::other)?;
        Ok(store)
    }

    pub(crate) fn public_settings(&self) -> Settings {
        let mut settings = self.doc.settings.clone();
        normalize_screenshot(&mut settings.screenshot);
        settings.deep_seek.has_api_key = self.secret_path.exists();
        settings.cloud.has_token = self.token_path.exists();
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
        let previous_cloud = self.doc.settings.cloud.clone();
        settings.cloud.device_id = previous_cloud.device_id;
        settings.cloud.last_sync_at = previous_cloud.last_sync_at;
        settings.cloud.last_status = previous_cloud.last_status.clone();
        settings.cloud.has_token = false;
        let previous_shot = self.doc.settings.screenshot.clone();
        if settings.screenshot.save_dir.trim() != previous_shot.save_dir.trim() {
            settings.screenshot.last_save_dir.clear();
        } else {
            settings.screenshot.last_save_dir = previous_shot.last_save_dir;
        }
        settings.cloud.provider = "gitee".into();
        if settings.cloud.dir.trim().is_empty() {
            settings.cloud.dir = "daodian".into();
        }
        if settings.cloud.interval_minutes < 5 {
            settings.cloud.interval_minutes = 5;
        }
        if settings.cloud.interval_minutes > 24 * 60 {
            settings.cloud.interval_minutes = 24 * 60;
        }
        normalize_screenshot(&mut settings.screenshot);
        validate_cloud(&settings.cloud)?;
        if settings.cloud.enabled && settings.cloud.device_id.is_empty() {
            settings.cloud.device_id = crate::cloud::state::new_device_id();
        }
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

    pub(crate) fn directory(&self) -> PathBuf {
        self.path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub(crate) fn quick_capture_settings(&self) -> QuickCaptureSettings {
        self.doc.settings.quick_capture.clone()
    }

    pub(crate) fn screenshot_settings(&self) -> ScreenshotSettings {
        let mut shot = self.doc.settings.screenshot.clone();
        normalize_screenshot(&mut shot);
        shot
    }

    /// 截图进程只更新内存里的截图配置，避免把过期的整份设置写回磁盘。
    pub(crate) fn adopt_shot_runtime(
        &mut self,
        enabled: bool,
        shortcut: &str,
        save_dir: &str,
        capture_mode: &str,
    ) {
        let shot = &mut self.doc.settings.screenshot;
        shot.enabled = enabled;
        shot.shortcut = shortcut.to_string();
        shot.save_dir = save_dir.to_string();
        if !capture_mode.is_empty() {
            shot.capture_mode = capture_mode.to_string();
        }
        normalize_screenshot(shot);
    }

    pub(crate) fn deep_seek_credentials(&self) -> Result<(DeepSeekSettings, Vec<u8>), String> {
        let settings = self.doc.settings.deep_seek.clone();
        if !settings.enabled {
            return Err("DeepSeek 尚未启用".into());
        }
        let key = unprotect_secret(&self.secret_path, "DeepSeek API Key 未设置")?;
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

    pub(crate) fn set_screenshot_status(&mut self, status: &str) -> Result<(), String> {
        self.doc.settings.screenshot.last_status = status.to_string();
        self.persist()
    }

    pub(crate) fn set_last_shot_dir(&mut self, dir: &str) -> Result<(), String> {
        self.doc.settings.screenshot.last_save_dir = dir.to_string();
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

    pub(crate) fn cloud_token(&self) -> Result<Vec<u8>, String> {
        unprotect_secret(&self.token_path, "还没有保存访问令牌")
    }

    pub(crate) fn set_cloud_token(&mut self, token: &str) -> Result<(), String> {
        let token = token.trim();
        if token.is_empty() {
            return Err("请先填写访问令牌".into());
        }
        protect_secret(&self.token_path, token.as_bytes())
    }

    pub(crate) fn clear_cloud_token(&mut self) -> Result<(), String> {
        match fs::remove_file(&self.token_path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("删除访问令牌失败: {err}")),
        }
    }

    pub(crate) fn save_cloud(&mut self, mut cloud: CloudSettings) -> Result<(), String> {
        cloud.device_id = self.doc.settings.cloud.device_id.clone();
        cloud.last_sync_at = self.doc.settings.cloud.last_sync_at;
        cloud.last_status = self.doc.settings.cloud.last_status.clone();
        cloud.has_token = false;
        cloud.provider = "gitee".into();
        if cloud.dir.trim().is_empty() {
            cloud.dir = "daodian".into();
        }
        if cloud.interval_minutes < 5 {
            cloud.interval_minutes = 5;
        }
        validate_cloud(&cloud)?;
        if cloud.enabled && cloud.device_id.is_empty() {
            cloud.device_id = crate::cloud::state::new_device_id();
        }
        self.doc.settings.cloud = cloud;
        self.persist()
    }

    pub(crate) fn set_cloud_branch(&mut self, branch: &str) -> Result<(), String> {
        self.doc.settings.cloud.branch = branch.to_string();
        self.persist()
    }

    pub(crate) fn set_cloud_status(
        &mut self,
        status: &str,
        synced_at: Option<i64>,
    ) -> Result<(), String> {
        self.doc.settings.cloud.last_status = status.to_string();
        if let Some(at) = synced_at {
            self.doc.settings.cloud.last_sync_at = at;
        }
        self.persist()
    }

    pub(crate) fn disconnect_cloud(&mut self) -> Result<(), String> {
        self.doc.settings.cloud.enabled = false;
        self.doc.settings.cloud.last_status = "idle".into();
        self.persist()
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
    crate::shot::persist_registration_status(&app)?;
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
pub(crate) fn protect_secret(path: &Path, secret: &[u8]) -> Result<(), String> {
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
pub(crate) fn unprotect_secret(path: &Path, missing: &str) -> Result<Vec<u8>, String> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };

    let encrypted = fs::read(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            missing.to_string()
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
pub(crate) fn protect_secret(_path: &Path, _secret: &[u8]) -> Result<(), String> {
    Err("当前平台不支持 Windows DPAPI，未保存 API Key".into())
}

#[cfg(not(windows))]
pub(crate) fn unprotect_secret(_path: &Path, _missing: &str) -> Result<Vec<u8>, String> {
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
        assert!(settings.screenshot.enabled);
        assert_eq!(settings.screenshot.shortcut, "Ctrl+1");
        assert!(!settings.deep_seek.enabled);
        assert!(!settings.deep_seek.has_api_key);
        assert!(settings.repeat.default_skip_weekends);
        assert_eq!(settings.window.default_tier, "standard");
        assert_eq!(settings.pet.idle_min_seconds, 12);
        assert_eq!(settings.pet.idle_max_seconds, 28);
    }

    #[test]
    fn missing_screenshot_settings_stay_enabled() {
        let settings: Settings = serde_json::from_str(r#"{"theme":"celadon"}"#).unwrap();
        assert!(settings.screenshot.enabled);
        assert_eq!(settings.screenshot.shortcut, "Ctrl+1");
        assert!(settings.screenshot.save_dir.is_empty());
        assert!(settings.screenshot.last_save_dir.is_empty());
        assert_eq!(settings.screenshot.capture_mode, "freeze");
        assert!(settings.screenshot.sidecar);
    }

    #[test]
    fn old_screenshot_settings_keep_empty_save_dirs() {
        let settings: ScreenshotSettings = serde_json::from_str(
            r#"{"enabled":true,"shortcut":"Ctrl+1","lastStatus":"已启用：Ctrl+1"}"#,
        )
        .unwrap();
        assert!(settings.save_dir.is_empty());
        assert!(settings.last_save_dir.is_empty());
        assert_eq!(settings.capture_mode, "freeze");
    }

    #[test]
    fn native_capture_mode_becomes_freeze() {
        let mut live: ScreenshotSettings =
            serde_json::from_str(r#"{"captureMode":"live"}"#).unwrap();
        normalize_screenshot(&mut live);
        assert_eq!(live.capture_mode, "live");
        assert!(live.sidecar);
        let mut native: ScreenshotSettings =
            serde_json::from_str(r#"{"captureMode":"native","sidecar":false}"#).unwrap();
        normalize_screenshot(&mut native);
        assert_eq!(native.capture_mode, "freeze");
        assert!(native.sidecar);
    }

    #[test]
    fn changing_save_dir_forgets_the_previous_folder() {
        let dir = temp_dir("shot-dir");
        let mut store = SettingsStore::open(&dir).unwrap();
        store.set_last_shot_dir(r"D:\旧目录").unwrap();
        let mut settings = store.public_settings();
        settings.screenshot.save_dir = r"D:\新目录".into();
        store.save(settings, None, false).unwrap();
        assert_eq!(store.public_settings().screenshot.save_dir, r"D:\新目录");
        assert!(store.public_settings().screenshot.last_save_dir.is_empty());

        store.set_last_shot_dir(r"D:\新目录").unwrap();
        let mut again = store.public_settings();
        again.theme = "night".into();
        store.save(again, None, false).unwrap();
        assert_eq!(
            store.public_settings().screenshot.last_save_dir,
            r"D:\新目录"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn pet_idle_keeps_a_five_second_minimum() {
        let legacy: PetSettings = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert_eq!(legacy.idle_min_seconds, 12);
        assert!(legacy.follow_input);
        assert!(legacy.show_key_names);
        assert_eq!(legacy.interaction, "normal");
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
