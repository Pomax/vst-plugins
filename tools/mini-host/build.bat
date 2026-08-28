@echo off
REM Build, then put the executable in the shared binaries directory.
cargo build --release || exit /b %ERRORLEVEL%
if not exist "..\..\binaries" mkdir "..\..\binaries"
copy /Y ".cache\release\mini-host.exe" "..\..\binaries\mini-host.exe" >nul || exit /b %ERRORLEVEL%
echo binary: ..\..\binaries\mini-host.exe
exit /b 0
