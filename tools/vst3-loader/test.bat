@echo off
REM --all and --ui are accepted and ignored: this project has no UI tests.
setlocal enabledelayedexpansion
set ARGS=
:next
if "%~1"=="" goto run
if /I not "%~1"=="--all" if /I not "%~1"=="--ui" set ARGS=!ARGS! %~1
shift
goto next
:run
cargo test%ARGS%
exit /b %ERRORLEVEL%
