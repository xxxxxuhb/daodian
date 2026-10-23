use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const FILE_NAME: &str = "daodian.log";

struct LogState {
    file: Mutex<File>,
    path: PathBuf,
}

static LOG: OnceLock<Option<LogState>> = OnceLock::new();

pub fn init() -> Option<PathBuf> {
    let state = LOG.get_or_init(open_log);
    install_panic_hook();
    let path = state.as_ref().map(|item| item.path.clone());
    info(format_args!(
        "logger ready; pid={}; version={}; debug={}; path={}",
        std::process::id(),
        env!("CARGO_PKG_VERSION"),
        cfg!(debug_assertions),
        path.as_deref()
            .map(Path::display)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<unavailable>".to_string())
    ));
    path
}

pub fn info(message: std::fmt::Arguments<'_>) {
    write("INFO", message);
}

pub fn warn(message: std::fmt::Arguments<'_>) {
    write("WARN", message);
}

pub fn error(message: std::fmt::Arguments<'_>) {
    write("ERROR", message);
}

fn open_log() -> Option<LogState> {
    for dir in candidates() {
        if !dir.exists() && fs::create_dir_all(&dir).is_err() {
            continue;
        }
        let path = dir.join(FILE_NAME);
        rotate(&path);
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            if file.metadata().map(|meta| meta.len() == 0).unwrap_or(false) {
                let _ = file.write_all(b"\xEF\xBB\xBF");
            }
            return Some(LogState {
                file: Mutex::new(file),
                path,
            });
        }
    }
    None
}

fn candidates() -> Vec<PathBuf> {
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
    dirs
}

fn rotate(path: &Path) {
    if !matches!(fs::metadata(path), Ok(meta) if meta.len() >= MAX_BYTES) {
        return;
    }
    let old = path.with_file_name(format!("{FILE_NAME}.1"));
    let _ = fs::remove_file(&old);
    let _ = fs::rename(path, old);
}

fn write(level: &str, message: std::fmt::Arguments<'_>) {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default();
    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("unnamed");
    let line = format!("[{millis}] [{level}] [{thread_name}] {message}");

    if cfg!(debug_assertions) {
        eprintln!("{line}");
    }
    let Some(state) = LOG.get().and_then(Option::as_ref) else {
        return;
    };
    if let Ok(mut file) = state.file.lock() {
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }
}

fn install_panic_hook() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            error(format_args!("panic: {panic}"));
            previous(panic);
        }));
    });
}
