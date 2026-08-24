@echo off
REM Open a VST3 plugin in the host's window. Builds nothing: run build first.
REM   run <path-to-plugin-or-bundle>
set HOST=..\dist\mini-host.exe
if not exist "%HOST%" set HOST=.cache\release\mini-host.exe

if not exist "%HOST%" (
    echo no mini-host built - run build first
    exit /b 1
)
if "%~1"=="" (
    echo usage: run ^<path-to-plugin-or-bundle^>
    exit /b 1
)

"%HOST%" %*
exit /b %ERRORLEVEL%
