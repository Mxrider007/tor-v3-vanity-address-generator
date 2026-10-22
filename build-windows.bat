@echo off
rem Build Tor V3 Vanity Address Generator from source on Windows.
rem Needs Rust (https://rustup.rs) with the default MSVC toolchain.
cd /d "%~dp0"
where cargo >nul 2>nul || (echo Rust is not installed. Get it from https://rustup.rs and run this again. & pause & exit /b 1)
echo Building Tor V3 Vanity Address Generator (the first build takes a few minutes)...
cargo build --release --workspace || (echo. & echo Build failed. & pause & exit /b 1)
if not exist dist mkdir dist
copy /y target\release\torv3-vanity-gui.exe dist\ >nul
copy /y target\release\torv3-vanity.exe dist\ >nul
echo.
echo Done: dist\torv3-vanity-gui.exe (app) and dist\torv3-vanity.exe (command line).
pause
