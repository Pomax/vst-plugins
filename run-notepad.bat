@echo off
REM Open the notepad plugin in the mini host.
pushd notepad
call ".\run.bat" %*
set CODE=%ERRORLEVEL%
popd
exit /b %CODE%
