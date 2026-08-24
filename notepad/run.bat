@echo off
REM Open the built plugin in the mini host. Builds nothing: run build first.
REM Prefers the shared dist, then a bundle in the cache, then the bare plugin
REM binary a test run leaves behind.
set HOST=..\dist\mini-host.exe
if not exist "%HOST%" set HOST=..\mini-host\.cache\release\mini-host.exe

set PLUGIN=..\dist\Notepad.vst3
if not exist "%PLUGIN%" set PLUGIN=.cache\release\bundle\Notepad.vst3
if not exist "%PLUGIN%" set PLUGIN=.cache\debug\bundle\Notepad.vst3
if not exist "%PLUGIN%" set PLUGIN=.cache\release\notepad_plugin.dll
if not exist "%PLUGIN%" set PLUGIN=.cache\debug\notepad_plugin.dll

if not exist "%HOST%" (
    echo no mini-host built - run build first
    exit /b 1
)
if not exist "%PLUGIN%" (
    echo no Notepad plugin built - run build or test first
    exit /b 1
)

"%HOST%" "%PLUGIN%"
exit /b %ERRORLEVEL%
