# Kore Vanity Generator - Windows Build Script (Rust CLI + GUI)
#
# Builds the Rust CLI (kore-vanity.exe) and GUI (kore-vanity-gui.exe).
#
# By default it builds the CPU + *external* CUDA backend (no CUDA toolkit needed
# to compile; GPU acceleration then comes from vanity_torv3_cuda.exe placed next
# to the binaries). Pass -Cuda to also compile the built-in rustacuda GPU/hybrid
# backend, which requires the CUDA Toolkit, a working libcuda, and ptx/kernel.ptx.
#
# Prerequisites:
#   - Rust toolchain (https://rustup.rs/)
#   - Visual Studio Build Tools with the "Desktop development with C++" workload
#   - (only with -Cuda) NVIDIA CUDA Toolkit 12.x and ptx/kernel.ptx
#
# Usage:
#   .\build-windows.ps1            # CPU + external CUDA (recommended)
#   .\build-windows.ps1 -Cuda      # also build the built-in CUDA/hybrid backend

param(
    [switch]$Cuda,
    [switch]$Help
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

if ($Help) {
    Write-Host @"
Kore Vanity Generator - Windows Build (Rust CLI + GUI)

Usage: .\build-windows.ps1 [-Cuda] [-Help]

  -Cuda   Also compile the built-in rustacuda GPU/hybrid backend
          (requires CUDA Toolkit + libcuda + ptx\kernel.ptx).
  -Help   Show this help.

Without -Cuda the build is CPU + external CUDA: GPU acceleration is provided by
vanity_torv3_cuda.exe (build it with build-all-windows.ps1) placed in the same
folder as kore-vanity.exe.
"@
    exit 0
}

Write-Host ""
Write-Host "=== Kore Vanity Generator - Windows Build ===" -ForegroundColor Cyan
Write-Host ""

# Check for Rust
Write-Host "[INFO] Checking Rust installation..." -ForegroundColor White
try {
    $rustVersion = rustc --version
    Write-Host "[OK] Rust found: $rustVersion" -ForegroundColor Green
} catch {
    Write-Host "[ERROR] Rust not found. Please install from https://rustup.rs/" -ForegroundColor Red
    Write-Host "  1. Download: https://win.rustup.rs/x86_64" -ForegroundColor Gray
    Write-Host "  2. Run the installer (choose the MSVC toolchain)" -ForegroundColor Gray
    Write-Host "  3. Restart this terminal" -ForegroundColor Gray
    exit 1
}

# Decide on feature flags
$buildCuda = $false
if ($Cuda) {
    $ptxPath = Join-Path $ScriptDir "ptx\kernel.ptx"
    $cudaPath = $env:CUDA_PATH
    if (-not $cudaPath) {
        foreach ($p in @(
            "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.6",
            "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.5",
            "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.4",
            "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.3",
            "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.2")) {
            if (Test-Path $p) { $cudaPath = $p; $env:CUDA_PATH = $p; break }
        }
    }
    if (-not $cudaPath) {
        Write-Host "[WARN] -Cuda requested but CUDA Toolkit not found. Building CPU + external CUDA instead." -ForegroundColor Yellow
    } elseif (-not (Test-Path $ptxPath)) {
        Write-Host "[WARN] -Cuda requested but ptx\kernel.ptx is missing. Building CPU + external CUDA instead." -ForegroundColor Yellow
    } else {
        Write-Host "[OK] CUDA found at: $cudaPath" -ForegroundColor Green
        $buildCuda = $true
    }
}

# cargo feature arguments
$cliFeatures = if ($buildCuda) { @("--features", "cuda") } else { @() }
$guiFeatures = if ($buildCuda) { @("--features", "cuda") } else { @() }

Write-Host ""
Write-Host "=== Building ===" -ForegroundColor Cyan
if ($buildCuda) {
    Write-Host "Mode: built-in CUDA (rustacuda) + CPU" -ForegroundColor White
} else {
    Write-Host "Mode: CPU + external CUDA" -ForegroundColor White
}
Write-Host ""

Push-Location $ScriptDir
try {
    # Build CLI
    Write-Host "[INFO] Building CLI (kore-vanity.exe)..." -ForegroundColor White
    & cargo build -p kore-vanity --bin kore-vanity --release @cliFeatures
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[ERROR] Failed to build CLI" -ForegroundColor Red
        exit 1
    }
    Write-Host "[OK] CLI built" -ForegroundColor Green

    # Build GUI
    Write-Host "[INFO] Building GUI (kore-vanity-gui.exe)..." -ForegroundColor White
    & cargo build -p kore-vanity-gui --release @guiFeatures
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[ERROR] Failed to build GUI" -ForegroundColor Red
        exit 1
    }
    Write-Host "[OK] GUI built" -ForegroundColor Green
} finally {
    Pop-Location
}

Write-Host ""
Write-Host "=== Build Complete! ===" -ForegroundColor Cyan
Write-Host ""

# Copy to dist
$outputDir = Join-Path $ScriptDir "dist"
if (-not (Test-Path $outputDir)) {
    New-Item -ItemType Directory -Path $outputDir | Out-Null
}

$targetDir = Join-Path $ScriptDir "target\release"
foreach ($name in @("kore-vanity.exe", "kore-vanity-gui.exe")) {
    $src = Join-Path $targetDir $name
    if (Test-Path $src) {
        Copy-Item $src (Join-Path $outputDir $name) -Force
        $size = [math]::Round((Get-Item $src).Length / 1MB, 2)
        Write-Host "[OK] $name ($size MB) -> dist\$name" -ForegroundColor Green
    }
}

# Note: the built-in CUDA backend embeds the PTX kernel into the binary at build
# time (via include_str!), so no kernel.ptx file is needed next to the .exe.

Write-Host ""
Write-Host "Output files are in: $outputDir" -ForegroundColor Cyan
Write-Host ""
Write-Host "Usage:" -ForegroundColor White
Write-Host "  CLI: .\dist\kore-vanity.exe myprefix,cool --dst .\keys" -ForegroundColor Gray
Write-Host "  GUI: .\dist\kore-vanity-gui.exe" -ForegroundColor Gray
if ($buildCuda) {
    Write-Host ""
    Write-Host "Built-in CUDA: at runtime this only needs the NVIDIA driver (no CUDA" -ForegroundColor Gray
    Write-Host "Toolkit, no kernel.ptx file). Run with --mode cuda or --mode hybrid." -ForegroundColor Gray
} else {
    Write-Host ""
    Write-Host "For GPU acceleration, build vanity_torv3_cuda.exe (build-all-windows.ps1)" -ForegroundColor Gray
    Write-Host "and place it next to kore-vanity.exe, then run with --mode cuda." -ForegroundColor Gray
}
Write-Host ""
