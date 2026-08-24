@echo off
REM Build every project. Stops at the first failure.
setlocal

for %%p in (mini-host vst3-loader notepad) do (
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
