@echo off
rem Build Tor V3 Vanity Address Generator from source on Windows.
rem
rem Needs Rust (https://rustup.rs) with the default MSVC toolchain, and the
rem Visual Studio Build Tools with the "Desktop development with C++" workload
rem (which includes the Windows SDK). rustup offers to install them.
cd /d "%~dp0"
where cargo >nul 2>nul || (echo Rust is not installed. Get it from https://rustup.rs and run this again. & pause & exit /b 1)
echo Building Tor V3 Vanity Address Generator (the first build takes a few minutes)...
cargo build --release --locked --workspace || goto :failed
if not exist dist mkdir dist
copy /y target\release\torv3-vanity-gui.exe dist\ >nul || goto :copyfailed
copy /y target\release\torv3-vanity.exe dist\ >nul || goto :copyfailed
copy /y LICENSE dist\LICENSE.txt >nul || goto :copyfailed
echo.
echo Done: dist\torv3-vanity-gui.exe (app) and dist\torv3-vanity.exe (command line).
echo To share these files, see "Making a release" in CONTRIBUTING.md: a release
echo also needs README.txt and THIRD-PARTY-LICENSES.html.
pause
exit /b 0

:failed
echo.
echo Build failed. If the error mentions link.exe, install the Visual Studio Build
echo Tools with the "Desktop development with C++" workload:
echo   https://visualstudio.microsoft.com/visual-cpp-build-tools/
pause
exit /b 1

:copyfailed
echo.
echo Could not copy the programs to dist\ (is CARGO_TARGET_DIR set, or is a program still running?).
pause
exit /b 1
