@echo off
REM --full is accepted and ignored: this project has no UI tests to add.
setlocal enabledelayedexpansion
set ARGS=
:next
if "%~1"=="" goto run
if /I not "%~1"=="--full" set ARGS=!ARGS! %~1
shift
goto next
:run
cargo test%ARGS%
exit /b %ERRORLEVEL%
