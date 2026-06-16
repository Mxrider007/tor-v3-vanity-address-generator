@echo off
REM Kore Vanity Generator - Windows Build
REM Double-click to build the CLI and GUI (CPU + external CUDA).
REM Pass -Cuda to also build the built-in CUDA backend: build-windows.bat -Cuda

cd /d "%~dp0"

echo.
echo === Kore Vanity Generator - Windows Build ===
echo.
echo Running build script...
echo.

powershell -ExecutionPolicy Bypass -File "%~dp0build-windows.ps1" %*

if %ERRORLEVEL% neq 0 (
    echo.
    echo Build failed! Press any key to exit...
    pause > nul
    exit /b 1
)

echo.
echo Build complete! Press any key to exit...
pause > nul
