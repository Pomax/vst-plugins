@echo off
cargo run -p xtask -- test %*
exit /b %ERRORLEVEL%
