@echo off
REM Build, then put the executable in the shared binaries directory.
cargo build --release || exit /b %ERRORLEVEL%
if not exist "..\..\binaries" mkdir "..\..\binaries"
copy /Y ".cache\release\vst3-loader.exe" "..\..\binaries\vst3-loader.exe" >nul || exit /b %ERRORLEVEL%
echo binary: ..\..\binaries\vst3-loader.exe
exit /b 0
