use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 2 * 1024 * 1024;

struct LogState {
    file: Mutex<File>,
}

static LOG: OnceLock<Option<LogState>> = OnceLock::new();

pub fn init() {
    LOG.get_or_init(|| {
        let path = log_path()?;
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        rotate(&path);
        let mut file = OpenOptions::new().create(true).append(true).open(&path).ok()?;
        if file.metadata().map(|meta| meta.len() == 0).unwrap_or(false) {
            let _ = file.write_all(b"\xEF\xBB\xBF");
        }
        Some(LogState {
            file: Mutex::new(file),
        })
    });
    info(&format!(
        "shot logger ready; pid={}; version={}; protocol={}",
        std::process::id(),
        env!("CARGO_PKG_VERSION"),
        shot_core::PROTOCOL_VERSION
    ));
}

pub fn info(message: &str) {
    write("INFO", message);
}

pub fn warn(message: &str) {
    write("WARN", message);
}

fn log_path() -> Option<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("logs"));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join("daodian").join("logs"));
    }
    dirs.push(std::env::temp_dir().join("daodian-logs"));
    for dir in dirs {
        if dir.exists() || fs::create_dir_all(&dir).is_ok() {
            return Some(dir.join("shot.log"));
        }
    }
    None
}

fn rotate(path: &std::path::Path) {
    if !matches!(fs::metadata(path), Ok(meta) if meta.len() >= MAX_BYTES) {
        return;
    }
    let old = path.with_file_name("shot.log.1");
    let _ = fs::remove_file(&old);
    let _ = fs::rename(path, old);
}

fn write(level: &str, message: &str) {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default();
    let line = format!("[{millis}] [{level}] [shot] {message}");
    if cfg!(debug_assertions) {
        eprintln!("{line}");
    }
    let Some(Some(state)) = LOG.get() else {
        return;
    };
    if let Ok(mut file) = state.file.lock() {
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }
}
