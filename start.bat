@echo off
rem Double-click launcher: bypasses the default ExecutionPolicy to run start.ps1
rem Pauses only when start.ps1 exits with an error, so you can read the message.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0start.ps1" %*
if errorlevel 1 pause
