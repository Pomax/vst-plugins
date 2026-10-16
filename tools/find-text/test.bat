@echo off
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
