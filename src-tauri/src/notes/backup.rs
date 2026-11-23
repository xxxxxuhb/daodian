//! 笔记和附件的备份复制、恢复。
use std::fs;
use std::path::Path;

pub fn copy_backup(
    data_dir: &Path,
    snapshot: &Path,
    previous: Option<&Path>,
) -> Result<(), String> {
    let notes_src = data_dir.join("notes");
    if notes_src.is_dir() {
        let dest = snapshot.join("notes");
        fs::create_dir_all(&dest).map_err(|err| err.to_string())?;
        let previous_notes = previous.map(|dir| dir.join("notes"));
        for entry in fs::read_dir(&notes_src).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            let name = entry.file_name();
            let label = name.to_string_lossy();
            if label.starts_with('.') || !label.ends_with(".json") {
                continue;
            }
            let src = entry.path();
            if !src.is_file() {
                continue;
            }
            let previous_file = previous_notes.as_ref().map(|dir| dir.join(&name));
            place_unchanged(&src, previous_file.as_deref(), &dest.join(&name))?;
        }
    }
    let attachments = data_dir.join("attachments");
    if attachments.is_dir() {
        let dest = snapshot.join("attachments");
        fs::create_dir_all(&dest).map_err(|err| err.to_string())?;
        for entry in fs::read_dir(&attachments).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || entry.path().is_dir() {
                continue;
            }
            link_or_copy(&entry.path(), &dest.join(&name))?;
        }
    }
    Ok(())
}

fn place_unchanged(live: &Path, previous: Option<&Path>, dest: &Path) -> Result<(), String> {
    if let Some(previous) = previous {
        if previous.is_file() && !newer(live, previous) {
            return link_or_copy(previous, dest);
        }
    }
    fs::copy(live, dest)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn newer(live: &Path, previous: &Path) -> bool {
    let Ok(live_meta) = live.metadata() else {
        return true;
    };
    let Ok(prev_meta) = previous.metadata() else {
        return true;
    };
    if live_meta.len() != prev_meta.len() {
        return true;
    }
    match (live_meta.modified(), prev_meta.modified()) {
        (Ok(live_time), Ok(prev_time)) => live_time > prev_time,
        _ => true,
    }
}

fn link_or_copy(source: &Path, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        return Ok(());
    }
    if fs::hard_link(source, dest).is_ok() {
        return Ok(());
    }
    fs::copy(source, dest)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

pub fn restore_backup(data_dir: &Path, snapshot: &Path) -> Result<(), String> {
    let src_notes = snapshot.join("notes");
    if src_notes.is_dir() {
        let dest = data_dir.join("notes");
        fs::create_dir_all(&dest).map_err(|err| err.to_string())?;
        for entry in fs::read_dir(&dest).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            let name = entry.file_name();
            let label = name.to_string_lossy();
            if label.ends_with(".json") && !src_notes.join(&name).is_file() {
                let _ = fs::remove_file(entry.path());
            }
        }
        for entry in fs::read_dir(&src_notes).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            if !entry.path().is_file() {
                continue;
            }
            fs::copy(entry.path(), dest.join(entry.file_name())).map_err(|err| err.to_string())?;
        }
    }
    let src_attachments = snapshot.join("attachments");
    if src_attachments.is_dir() {
        let dest = data_dir.join("attachments");
        fs::create_dir_all(&dest).map_err(|err| err.to_string())?;
        for entry in fs::read_dir(&src_attachments).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            if entry.path().is_dir() {
                continue;
            }
            link_or_copy(&entry.path(), &dest.join(entry.file_name()))?;
        }
    }
    Ok(())
}
