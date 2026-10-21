@echo off
rem Updates the server to a new Enshrouded build without losing items (see UPDATE-SERVER.ps1).
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0UPDATE-SERVER.ps1" %*
pause
