@echo off
REM Build, then put the executable in the shared dist directory.
cargo build --release || exit /b %ERRORLEVEL%
if not exist "..\dist" mkdir "..\dist"
copy /Y ".cache\release\mini-host.exe" "..\dist\mini-host.exe" >nul || exit /b %ERRORLEVEL%
echo dist:   ..\dist\mini-host.exe
exit /b 0
