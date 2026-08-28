@echo off
REM Build the text finder into the shared binaries directory.
setlocal

set HERE=%~dp0
set BINARIES=%HERE%..\..\binaries

pushd "%HERE%"
cargo build --release
if errorlevel 1 (
    popd
    exit /b 1
)
popd

if not exist "%BINARIES%" mkdir "%BINARIES%"
copy /y "%HERE%.cache\release\find-text.exe" "%BINARIES%\find-text.exe" >nul
if errorlevel 1 exit /b 1

echo binary: %BINARIES%\find-text.exe
exit /b 0
