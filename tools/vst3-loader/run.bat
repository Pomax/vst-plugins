@echo off
REM Load a VST3 plugin and report on it. Builds nothing: run build first.
REM   run <path-to-plugin-or-bundle> [options]
set LOADER=..\..\binaries\vst3-loader.exe
if not exist "%LOADER%" set LOADER=.cache\release\vst3-loader.exe

if not exist "%LOADER%" (
    echo no vst3-loader built - run build first
    exit /b 1
)
if "%~1"=="" (
    echo usage: run ^<path-to-plugin-or-bundle^> [options]
    exit /b 1
)

"%LOADER%" %*
exit /b %ERRORLEVEL%
