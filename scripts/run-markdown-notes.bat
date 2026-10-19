@echo off
REM Open Markdown Notes in the mini host.
pushd "%~dp0.."
call ".\scripts\build.bat"
pushd "markdown-notes"
call ".\run.bat" "--demo" %*
set CODE=%ERRORLEVEL%
popd
popd
exit /b %CODE%
