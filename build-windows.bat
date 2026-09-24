@echo off
rem Build Kore Vanity from source on Windows.
rem Needs Rust (https://rustup.rs) with the default MSVC toolchain.
cd /d "%~dp0"
where cargo >nul 2>nul || (echo Rust is not installed. Get it from https://rustup.rs and run this again. & pause & exit /b 1)
echo Building Kore Vanity (the first build takes a few minutes)...
cargo build --release --workspace || (echo. & echo Build failed. & pause & exit /b 1)
if not exist dist mkdir dist
copy /y target\release\kore-vanity-gui.exe dist\ >nul
copy /y target\release\kore-vanity.exe dist\ >nul
echo.
echo Done: dist\kore-vanity-gui.exe (app) and dist\kore-vanity.exe (command line).
pause
