@echo off
REM Kore Vanity Generator - Complete Build
REM Builds GUI + CLI + external CUDA generator

cd /d "%~dp0"
echo.
echo === Building Kore Vanity Generator ===
echo.
echo This will build:
echo   1. kore-vanity-gui.exe (GUI)
echo   2. kore-vanity.exe (CLI)
echo   3. vanity_torv3_cuda.exe (external GPU accelerator)
echo.
powershell -ExecutionPolicy Bypass -File "%~dp0build-all-windows.ps1" %*
if %ERRORLEVEL% neq 0 (
    echo.
    echo Build encountered errors. Press any key to exit...
    pause > nul
    exit /b 1
)
echo.
echo Press any key to exit...
pause > nul
