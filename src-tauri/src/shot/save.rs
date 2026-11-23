//! 截图像素的编码、另存对话框和原子落盘。
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::settings::SharedSettings;

use super::{
    conceal_shot_windows, reveal_shots, shots_dir, unix_ms, ACTIVE, LAST_SAVED, MAX_PNG_BYTES,
    SAVING, TMP_SEQ,
};

pub(super) fn flatten_alpha(image: &mut image::RgbaImage) {
    for pixel in image.pixels_mut() {
        pixel[3] = 255;
    }
}

pub(super) fn write_png(path: &Path, image: &image::RgbaImage) -> Result<(), String> {
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use std::io::BufWriter;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let file = fs::File::create(path).map_err(|err| format!("保存截图失败: {err}"))?;
    PngEncoder::new_with_quality(
        BufWriter::new(file),
        CompressionType::Fast,
        FilterType::NoFilter,
    )
    .write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|err| format!("保存截图失败: {err}"))
}

pub(super) fn decode_png(raw: &str) -> Result<image::RgbaImage, String> {
    let trimmed = raw.trim();
    let encoded = trimmed
        .split_once(',')
        .map(|(_, data)| data)
        .unwrap_or(trimmed);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|err| format!("截图数据无效: {err}"))?;
    if bytes.is_empty() || bytes.len() > MAX_PNG_BYTES {
        return Err("截图数据无效".into());
    }
    let image = image::load_from_memory(&bytes)
        .map_err(|err| format!("截图读不出来: {err}"))?
        .to_rgba8();
    if image.width() == 0 || image.height() == 0 {
        return Err("截图是空的".into());
    }
    Ok(image)
}

pub(crate) fn copy_bgra(width: u32, height: u32, pixels: &[u8]) -> Result<(), String> {
    let image = rgba_image(width, height, pixels)?;
    copy_image(&image)
}

pub(crate) fn png_bytes_from_bgra(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, String> {
    encode_png(&rgba_image(width, height, pixels)?)
}

fn rgba_image(width: u32, height: u32, pixels: &[u8]) -> Result<image::RgbaImage, String> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or("截图区域无效")?;
    if pixels.len() != expected || width == 0 || height == 0 {
        return Err("截图区域无效".into());
    }
    let mut rgba = pixels.to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    image::RgbaImage::from_raw(width, height, rgba).ok_or_else(|| "截图区域无效".into())
}

pub(crate) fn save_native_async(app: AppHandle, bytes: Vec<u8>) {
    if std::thread::Builder::new()
        .name("shot-save".into())
        .spawn(move || {
            if let Err(err) = save_native_dialog(&app, &bytes) {
                crate::logging::warn(format_args!("原生截图保存失败: {err}"));
            }
        })
        .is_err()
    {
        crate::logging::warn(format_args!("原生截图保存失败: 启动保存失败"));
    }
}

fn save_native_dialog(app: &AppHandle, bytes: &[u8]) -> Result<(), String> {
    let _saving = SavingGuard::enter();
    #[cfg(windows)]
    crate::shot_native::set_visible(false);
    std::thread::sleep(Duration::from_millis(80));
    let start = match dialog_directory(app) {
        Ok(start) => start,
        Err(err) => {
            #[cfg(windows)]
            crate::shot_native::set_visible(true);
            return Err(err);
        }
    };
    let mut dialog = app
        .dialog()
        .file()
        .add_filter("PNG 图片", &["png"])
        .set_directory(&start.directory)
        .set_file_name(&screenshot_file_name());
    if let Some(main) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&main);
    }
    let picked = dialog.blocking_save_file();
    #[cfg(windows)]
    crate::shot_native::set_visible(true);
    let Some(picked) = picked else {
        return Ok(());
    };
    let path = picked
        .into_path()
        .map(ensure_png)
        .map_err(|err| format!("保存路径无效: {err}"))?;
    write_bytes_atomic(&path, bytes)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            remember_last_save_dir(app, parent);
        }
    }
    remember_saved(&path);
    crate::logging::info(format_args!("screenshot saved: {}", path.display()));
    Ok(())
}

pub(super) fn copy_image(image: &image::RgbaImage) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|err| format!("打不开剪贴板: {err}"))?;
    clipboard
        .set_image(arboard::ImageData {
            width: image.width() as usize,
            height: image.height() as usize,
            bytes: std::borrow::Cow::Borrowed(image.as_raw()),
        })
        .map_err(|err| format!("复制到剪贴板失败: {err}"))
}

pub(super) fn ensure_png(path: PathBuf) -> PathBuf {
    // 只补缺失的扩展名。用户在对话框里写了别的扩展名时，按他选的名字写。
    if path.extension().is_some() {
        return path;
    }
    let mut name = path
        .file_name()
        .map(|value| value.to_os_string())
        .unwrap_or_else(|| "到点截图".into());
    name.push(".png");
    path.with_file_name(name)
}

fn screenshot_file_name() -> String {
    format_screenshot_name(&chrono::Local::now().format("%Y%m%d-%H%M%S").to_string())
}

pub(super) fn format_screenshot_name(stamp: &str) -> String {
    format!("到点截图-{stamp}.png")
}

/// 没有对话框的自动保存才改名。用户在另存框里确认的路径不能再加 `-2`。
#[cfg(test)]
pub(super) fn unique_png(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("到点截图");
    for index in 2..1000 {
        let candidate = parent.join(format!("{stem}-{index}.png"));
        if !candidate.exists() {
            return candidate;
        }
    }
    path
}

pub(in crate::shot) struct SaveStart {
    pub(in crate::shot) directory: PathBuf,
    pub(in crate::shot) warning: String,
}

struct SavingGuard;

impl SavingGuard {
    fn enter() -> Self {
        SAVING.store(true, Ordering::SeqCst);
        Self
    }
}

impl Drop for SavingGuard {
    fn drop(&mut self) {
        SAVING.store(false, Ordering::SeqCst);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShotSaveResult {
    saved: bool,
    path: String,
    warning: String,
}

pub(super) fn encode_png(image: &image::RgbaImage) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    let mut bytes = Vec::new();
    PngEncoder::new_with_quality(&mut bytes, CompressionType::Fast, FilterType::NoFilter)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|err| format!("保存截图失败: {err}"))?;
    Ok(bytes)
}

pub(super) fn validate_export_bytes(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 8 || bytes.len() > MAX_PNG_BYTES || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("截图数据无效".into());
    }
    Ok(())
}

pub(super) fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;

    if bytes.is_empty() || bytes.len() > MAX_PNG_BYTES {
        return Err("截图数据无效".into());
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| format!("保存截图失败: {err}"))?;
        }
    }
    let tmp = temp_beside(path);
    let written = (|| -> Result<(), String> {
        let mut file = fs::File::create(&tmp).map_err(|err| format!("保存截图失败: {err}"))?;
        file.write_all(bytes)
            .map_err(|err| format!("保存截图失败: {err}"))?;
        file.sync_all()
            .map_err(|err| format!("保存截图失败: {err}"))?;
        Ok(())
    })();
    if let Err(err) = written {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    if let Err(err) = replace_file(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

fn temp_beside(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("shot.png");
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(".{name}.{}.{seq}.tmp", std::process::id()))
}

fn replace_file(tmp: &Path, dest: &Path) -> Result<(), String> {
    if !dest.exists() {
        return fs::rename(tmp, dest).map_err(|err| format!("保存截图失败: {err}"));
    }
    replace_existing(tmp, dest)
}

#[cfg(windows)]
fn replace_existing(tmp: &Path, dest: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_WRITE_THROUGH, ReplaceFileW};

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
    let dest_w = wide(dest);
    let tmp_w = wide(tmp);
    let ok = unsafe {
        ReplaceFileW(
            dest_w.as_ptr(),
            tmp_w.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_WRITE_THROUGH,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok == 0 {
        Err(format!("保存截图失败: {}", std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_existing(tmp: &Path, dest: &Path) -> Result<(), String> {
    fs::rename(tmp, dest).map_err(|err| format!("保存截图失败: {err}"))
}

fn pictures_dir() -> PathBuf {
    #[cfg(windows)]
    if let Some(path) = known_pictures_dir() {
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    home_pictures()
}

#[cfg(windows)]
fn known_pictures_dir() -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Pictures, SHGetKnownFolderPath};

    unsafe {
        let mut raw = std::ptr::null_mut();
        let hr = SHGetKnownFolderPath(&FOLDERID_Pictures, 0, std::ptr::null_mut(), &mut raw);
        if hr < 0 || raw.is_null() {
            return None;
        }
        let mut len = 0usize;
        while *raw.add(len) != 0 {
            len += 1;
            if len > 32_768 {
                CoTaskMemFree(raw.cast());
                return None;
            }
        }
        let path = String::from_utf16_lossy(std::slice::from_raw_parts(raw, len));
        CoTaskMemFree(raw.cast());
        Some(PathBuf::from(path))
    }
}

fn home_pictures() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Pictures")
}

fn dir_writable(path: &Path) -> bool {
    if path.as_os_str().is_empty() {
        return false;
    }
    if fs::create_dir_all(path).is_err() {
        return false;
    }
    let probe = path.join(format!(
        ".daodian-write-{}-{}-{}",
        std::process::id(),
        unix_ms(),
        TMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let opened = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe);
    let Ok(mut file) = opened else {
        return false;
    };
    use std::io::Write;
    let ok = file.write_all(b"ok").is_ok();
    drop(file);
    let _ = fs::remove_file(&probe);
    ok
}

pub(super) fn resolve_save_start(
    last: &str,
    save_dir: &str,
    pictures: &Path,
    shots: &Path,
) -> Result<SaveStart, String> {
    let configured = {
        let trimmed = save_dir.trim();
        if trimmed.is_empty() {
            pictures.join("到点截图")
        } else {
            PathBuf::from(trimmed)
        }
    };
    let last_path = PathBuf::from(last.trim());
    if !last.trim().is_empty() && last_path.is_dir() && dir_writable(&last_path) {
        return Ok(SaveStart {
            directory: last_path,
            warning: String::new(),
        });
    }
    if dir_writable(&configured) {
        return Ok(SaveStart {
            directory: configured,
            warning: String::new(),
        });
    }
    if configured.as_path() != shots && dir_writable(shots) {
        return Ok(SaveStart {
            directory: shots.to_path_buf(),
            warning: format!("保存目录不可写：{}", configured.display()),
        });
    }
    Err(format!("保存目录不可写：{}", configured.display()))
}

fn dialog_directory(app: &AppHandle) -> Result<SaveStart, String> {
    let settings = app
        .state::<SharedSettings>()
        .lock()
        .map_err(|err| err.to_string())?
        .screenshot_settings();
    let shots = shots_dir(app)?;
    resolve_save_start(
        &settings.last_save_dir,
        &settings.save_dir,
        &pictures_dir(),
        &shots,
    )
}

fn remember_last_save_dir(app: &AppHandle, dir: &Path) {
    let text = dir.display().to_string();
    let state = app.state::<SharedSettings>();
    let Ok(mut store) = state.lock() else {
        return;
    };
    if let Err(err) = store.set_last_shot_dir(&text) {
        crate::logging::warn(format_args!("记住截图目录失败: {err}"));
    }
}

fn remember_saved(path: &Path) {
    if let Ok(mut slot) = LAST_SAVED.lock() {
        *slot = path.display().to_string();
    }
}

pub(super) fn save_with_dialog(app: &AppHandle, bytes: &[u8]) -> Result<ShotSaveResult, String> {
    let _saving = SavingGuard::enter();
    // 只藏窗口，不等抓屏线程。对话框要马上出来；取消后还靠原会话恢复选区。
    conceal_shot_windows();
    ACTIVE.store(false, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(80));
    let start = match dialog_directory(app) {
        Ok(start) => start,
        Err(err) => {
            reveal_shots(app);
            return Err(err);
        }
    };
    crate::logging::info(format_args!("screenshot save dialog opening"));
    // 必须在异步命令里等保存框。同步命令跑在界面线程上，框又要界面线程抽消息，两边会一直卡住。
    let mut dialog = app
        .dialog()
        .file()
        .add_filter("PNG 图片", &["png"])
        .set_directory(&start.directory)
        .set_file_name(&screenshot_file_name());
    if let Some(main) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&main);
    }
    let picked = dialog.blocking_save_file();
    crate::logging::info(format_args!("screenshot save dialog closed"));
    let Some(picked) = picked else {
        reveal_shots(app);
        return Ok(ShotSaveResult {
            saved: false,
            path: String::new(),
            warning: String::new(),
        });
    };
    let path = match picked.into_path() {
        Ok(path) => ensure_png(path),
        Err(err) => {
            reveal_shots(app);
            return Err(format!("保存路径无效: {err}"));
        }
    };
    if let Err(err) = write_bytes_atomic(&path, &bytes) {
        reveal_shots(app);
        return Err(err);
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            remember_last_save_dir(app, parent);
        }
    }
    remember_saved(&path);
    crate::logging::info(format_args!("screenshot saved: {}", path.display()));
    reveal_shots(app);
    Ok(ShotSaveResult {
        saved: true,
        path: path.display().to_string(),
        warning: start.warning,
    })
}

#[cfg(windows)]
pub(super) fn reveal_in_folder(path: &Path) -> Result<(), String> {
    let mut arg = std::ffi::OsString::from("/select,");
    arg.push(path);
    std::process::Command::new("explorer")
        .arg(arg)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("打不开文件夹: {err}"))
}

#[cfg(not(windows))]
pub(super) fn reveal_in_folder(_path: &Path) -> Result<(), String> {
    Err("当前只支持在 Windows 上打开文件夹".into())
}

pub(crate) fn cleanup_leftovers(app: &AppHandle) {
    let Ok(root) = shots_dir(app) else {
        return;
    };
    clear_children(&root.join("live"));
    clear_children(&root.join("pins"));
}

fn clear_children(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        remove_tree(&entry.path());
    }
}

pub(super) fn remove_tree(path: &Path) {
    let result = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    if let Err(err) = result {
        if err.kind() != std::io::ErrorKind::NotFound {
            crate::logging::warn(format_args!("清理截图文件失败: {err}"));
        }
    }
}
