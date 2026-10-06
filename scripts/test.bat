@echo off
REM Run every project's tests. Stops at the first failure.
REM With a project's name first, run that project's tests alone and hand it
REM the rest of the arguments:
REM     scripts\test.bat markdown-notes --only mermaid_blocks
setlocal
cd /d "%~dp0.."

set ONE=
if /I "%~1"=="mini-host" set ONE=tools\mini-host
if /I "%~1"=="vst3-loader" set ONE=tools\vst3-loader
if /I "%~1"=="markdown-notes" set ONE=markdown-notes
if not defined ONE goto all

set REST=
for /f "tokens=1,*" %%a in ("%*") do set REST=%%b
pushd "%ONE%"
call ".\test.bat" %REST%
set RESULT=%ERRORLEVEL%
popd
exit /b %RESULT%

:all
for %%p in (tools\mini-host tools\vst3-loader markdown-notes) do (
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
