use std::fs;
use std::path::Path;

/// 用临时文件换掉目标。直接改名失败时，先把旧文件挪开再换上。
pub(crate) fn replace_file(tmp: &Path, dest: &Path) -> Result<(), String> {
    if fs::rename(tmp, dest).is_ok() {
        return Ok(());
    }
    let backup = dest.with_extension("replacing");
    let _ = fs::remove_file(&backup);
    if dest.exists() {
        fs::rename(dest, &backup).map_err(|err| err.to_string())?;
    }
    if let Err(err) = fs::rename(tmp, dest) {
        let _ = fs::rename(&backup, dest);
        return Err(err.to_string());
    }
    let _ = fs::remove_file(&backup);
    Ok(())
}
