@echo off
REM Open Markdown Notes in the mini host.
pushd "%~dp0..\markdown-notes"
call ".\run.bat" %*
set CODE=%ERRORLEVEL%
popd
exit /b %CODE%
