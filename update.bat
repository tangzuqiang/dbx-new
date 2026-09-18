@echo off
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\update.ps1" %*
if errorlevel 1 (
    echo.
    echo 脚本失败，窗口不会自动关闭。
)
exit /b %ERRORLEVEL%

