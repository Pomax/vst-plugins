@echo off
REM Open the built plugin in the mini host. Builds nothing: run build first.
REM Prefers the shared binaries, then a bundle in the cache, then the bare
REM plugin binary a test run leaves behind.
set HOST=..\binaries\mini-host.exe
if not exist "%HOST%" set HOST=..\tools\mini-host\.cache\release\mini-host.exe

set PLUGIN=..\binaries\Markdown Notes.vst3
if not exist "%PLUGIN%" set PLUGIN=.cache\release\bundle\Markdown Notes.vst3
if not exist "%PLUGIN%" set PLUGIN=.cache\debug\bundle\Markdown Notes.vst3
if not exist "%PLUGIN%" set PLUGIN=.cache\release\markdown_notes_plugin.dll
if not exist "%PLUGIN%" set PLUGIN=.cache\debug\markdown_notes_plugin.dll

if not exist "%HOST%" (
    echo no mini-host built - run build first
    exit /b 1
)
if not exist "%PLUGIN%" (
    echo no Markdown Notes plugin built - run build or test first
    exit /b 1
)

"%HOST%" "%PLUGIN%"
exit /b %ERRORLEVEL%
