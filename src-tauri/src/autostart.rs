const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Daodian";

#[cfg(windows)]
pub fn sync(enabled: bool) -> Result<(), String> {
    // 开发程序路径位于 target 目录，不能覆盖用户已经安装好的启动项。
    if cfg!(debug_assertions) {
        return Ok(());
    }

    use windows_registry::CURRENT_USER;

    let key = CURRENT_USER
        .create(RUN_KEY)
        .map_err(|err| err.to_string())?;
    if enabled {
        let executable = std::env::current_exe().map_err(|err| err.to_string())?;
        let command = format!("\"{}\" --autostart", executable.display());
        key.set_string(VALUE_NAME, &command)
            .map_err(|err| err.to_string())
    } else {
        if key.get_string(VALUE_NAME).is_ok() {
            key.remove_value(VALUE_NAME)
                .map_err(|err| err.to_string())?;
        }
        Ok(())
    }
}

#[cfg(not(windows))]
pub fn sync(_enabled: bool) -> Result<(), String> {
    Err("当前平台暂不支持开机启动".into())
}

pub fn launched_automatically() -> bool {
    std::env::args().any(|argument| argument == "--autostart")
}
