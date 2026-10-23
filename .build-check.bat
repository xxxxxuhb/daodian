@echo off
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
cd /d D:\Youself\daodian\src-tauri
cargo build --message-format short
exit /b %ERRORLEVEL%