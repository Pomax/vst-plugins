@echo off
REM Build, then put the executable in the shared dist directory.
cargo build --release || exit /b %ERRORLEVEL%
if not exist "..\dist" mkdir "..\dist"
copy /Y ".cache\release\vst3-loader.exe" "..\dist\vst3-loader.exe" >nul || exit /b %ERRORLEVEL%
echo dist:   ..\dist\vst3-loader.exe
exit /b 0
