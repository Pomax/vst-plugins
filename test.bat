@echo off
REM Run every project's tests. Stops at the first failure.
setlocal

for %%p in (mini-host vst3-loader markdown-notes) do (
    echo === %%p ===
    pushd "%%p"
    call ".\test.bat" %*
    if errorlevel 1 (
        popd
        echo FAILED: %%p
        exit /b 1
    )
    popd
)

echo === all projects passed ===
exit /b 0
