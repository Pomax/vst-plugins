@echo off
REM Build every project. Stops at the first failure.
setlocal

for %%p in (tools\mini-host tools\vst3-loader markdown-notes) do (
    echo === %%p ===
    pushd "%%p"
    call ".\build.bat" %*
    if errorlevel 1 (
        popd
        echo FAILED: %%p
        exit /b 1
    )
    popd
)

echo === all projects built ===
exit /b 0
