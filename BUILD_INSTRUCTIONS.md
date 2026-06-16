# Kore Vanity — Build Instructions (developer reference)

For installation and usage, see [README.md](README.md). This file is the detailed
build/feature reference.

## Workspace layout

```
kore-vanity/
├── src/                  # root crate: library + `kore-vanity` CLI
│   ├── lib.rs            # library exports
│   ├── main.rs           # CLI
│   ├── onion.rs          # onion address + Tor key/HS-dir helpers (shared)
│   ├── pattern.rs        # `?` wildcard matching
│   ├── probability.rs    # probability / ETA model
│   └── backend/
│       ├── mod.rs        # backend selection & dispatch
│       ├── cpu.rs        # CPU (Rayon)
│       ├── external_cuda.rs  # spawns the external vanity_torv3_cuda binary
│       ├── cuda.rs       # built-in CUDA (feature = "cuda")
│       └── hybrid.rs     # CPU + built-in CUDA (feature = "cuda")
├── gui/                  # egui/eframe GUI crate (`kore-vanity-gui`)
├── core/                 # no_std CUDA kernel crate -> compiled to PTX
├── ptx/kernel.ptx        # committed, pre-compiled kernel
├── build.rs              # CUDA lib discovery + PTX wiring (feature = "cuda")
└── Cargo.toml            # workspace root
```

## Cargo features

| Feature | Default | Effect |
|---------|---------|--------|
| *(none)* | ✅ | CPU backend **plus** the external CUDA backend (spawns `vanity_torv3_cuda`). Links with no CUDA toolchain. |
| `cuda` | ❌ | Adds the built-in `rustacuda` GPU + hybrid backends. Pulls `rustacuda`, which links `libcuda`, so the CUDA Toolkit/driver must be present to build. |

The default has **no** features so a plain `cargo build` links on any machine. The
`kore-vanity` CLI always exposes all four `--mode` values; `cuda`/`hybrid` fall
back to the external binary (or CPU) when the `cuda` feature is not compiled in.

## Prerequisites

### Linux
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
sudo apt install -y build-essential pkg-config libssl-dev libgtk-3-dev
# Built-in CUDA only: sudo apt install -y nvidia-cuda-toolkit
```

### Windows
1. **Visual Studio Build Tools** with the "Desktop development with C++" workload.
2. **Rust** (MSVC): `winget install Rustlang.Rustup`.
3. **Built-in/External CUDA only:** NVIDIA CUDA Toolkit 12.x (set `CUDA_PATH`) and, for the external binary, CMake.

## Build commands

### CPU + external CUDA (default, portable)
```bash
cargo build --release                              # CLI + library
cargo build -p kore-vanity-gui --release           # GUI
```
Outputs: `target/release/kore-vanity[.exe]`, `target/release/kore-vanity-gui[.exe]`.

### Built-in CUDA backend
Requires the CUDA Toolkit + `libcuda` to link. Uses the committed `ptx/kernel.ptx`,
so **stable** Rust is sufficient — no nightly needed:
```bash
cargo build --release --features cuda
cargo build -p kore-vanity-gui --release --features cuda
```

### Windows helper scripts
```powershell
.\build-windows.ps1            # CPU + external CUDA -> dist\
.\build-windows.ps1 -Cuda      # built-in CUDA backend -> dist\
.\build-all-windows.ps1        # CPU+GUI + builds external vanity_torv3_cuda.exe -> dist\
```

## Regenerating the PTX kernel (rarely needed)

The kernel in `core/` is committed pre-compiled as `ptx/kernel.ptx`. You only need
this if you change `core/src/kernel.rs`:

```bash
rustup toolchain install nightly
rustup target add nvptx64-nvidia-cuda
cargo +nightly build -p kore-vanity-core --target nvptx64-nvidia-cuda --release
cp target/nvptx64-nvidia-cuda/release/kore_vanity_core.ptx ptx/kernel.ptx
```

## Tests

```bash
cargo test                 # library tests, incl. Tor key-format regression tests
cargo clippy --all-targets
cargo clippy --features cuda
```

## Troubleshooting

- **`cannot find -lcuda` / linker errors with `--features cuda`** — the CUDA Toolkit
  isn't installed or `CUDA_PATH`/library paths aren't set. Build without `--features cuda`
  (the default) for CPU + external CUDA.
- **GPU not used at runtime** — for the external path, `vanity_torv3_cuda[.exe]` must be
  next to `kore-vanity[.exe]` (or in the working dir / `dist/`); run with `--mode cuda`.
  Confirm the NVIDIA driver is installed (`nvidia-smi`).
- **Windows: `cl.exe` not found** — install the VS Build Tools C++ workload, or run from
  a "Developer PowerShell for VS"; `build-all-windows.ps1` can auto-install it.
- **PowerShell won't run the scripts** — `Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass`.

## License

MIT
