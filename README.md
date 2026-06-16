# Kore Vanity

High-performance generator for **Tor v3 (`.onion`) vanity hidden-service addresses**, with optional GPU acceleration and a multi-threaded CPU fallback.

Generate custom addresses that start with (or contain) your chosen text, e.g. `korexxxx…xxxx.onion`, ready to drop straight into a Tor hidden service.

---

## Features

- **CPU backend** – multi-threaded (Rayon), works everywhere, no special hardware.
- **GPU acceleration** – two options:
  - **External CUDA** (recommended): drives the standalone [`vanity_torv3_cuda`](https://github.com/Danukeru/torv3_vanity_addr_cuda) binary. Builds with no CUDA toolchain; the binary just has to sit next to `kore-vanity`.
  - **Built-in CUDA** (`--features cuda`): an in-process `rustacuda` kernel. Single binary, but requires the CUDA Toolkit to compile/link.
- **Hybrid mode** – run CPU and GPU together.
- **Pattern language** – `?` single-character wildcard, multiple comma-separated prefixes, `--contains` words, and `--anywhere` matching (see [Matching & wildcards](#matching--wildcards) for which backends support what).
- **Tor-compatible output** – every backend writes a complete hidden-service directory (`hostname`, `hs_ed25519_public_key`, `hs_ed25519_secret_key`, `authorized_clients/`) with correct permissions.
- **GUI and CLI**, live probability/ETA estimates, and a `Ctrl-C`-safe CLI.

---

## Quick start

> ⚠️ **Security:** a `.onion` secret key *is* the identity of your service. Anyone with `hs_ed25519_secret_key` can impersonate it. Keep generated directories private and never commit or share them.

### Linux / macOS

```bash
# Build (CPU + external-CUDA support; links on any machine)
cargo build --release

# Find an address starting with "kore"
./target/release/kore-vanity kore --dst ./keys

# Launch the GUI
cargo build -p kore-vanity-gui --release
./target/release/kore-vanity-gui
```

### Windows

```powershell
# From the project folder, in PowerShell:
.\build-windows.bat            # builds kore-vanity.exe + kore-vanity-gui.exe
.\dist\kore-vanity.exe kore --dst .\keys
```

See the full [Windows guide](#windows-prepare-compile-run) below for installing the prerequisites and enabling the GPU.

---

## Installation & build

### Prerequisites

| Component | Needed for | Where |
|-----------|-----------|-------|
| **Rust** (stable, 1.74+) | everything | <https://rustup.rs/> |
| **C/C++ build tools** | linking on Windows (`cl.exe`) / `build-essential` on Linux | see below |
| **CUDA Toolkit 12.x** | *built-in* CUDA (`--features cuda`) **or** building the external `vanity_torv3_cuda` | <https://developer.nvidia.com/cuda-downloads> |
| **CMake** | building the external `vanity_torv3_cuda` | <https://cmake.org/download/> |

> The default build (`cargo build --release`) is **CPU + external CUDA** and needs **no CUDA toolkit to compile** — GPU acceleration is then provided by a separately-built `vanity_torv3_cuda` binary placed next to `kore-vanity`. You only need the CUDA Toolkit if you build the *built-in* backend with `--features cuda`, or if you build the external binary yourself.

### Linux

```bash
# Build tools + GUI system libs (Ubuntu/Debian)
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev libgtk-3-dev

git clone https://github.com/Mxrider420z/kore-vanity.git
cd kore-vanity

# CLI + library (CPU + external CUDA)
cargo build --release

# GUI
cargo build -p kore-vanity-gui --release

./target/release/kore-vanity --help
```

**Built-in CUDA on Linux** (optional):

```bash
sudo apt install -y nvidia-cuda-toolkit       # provides libcuda for linking
cargo build --release --features cuda          # uses the committed ptx/kernel.ptx
```

**External CUDA binary on Linux** (optional, recommended for GPU):

```bash
sudo apt install -y cmake nvidia-cuda-toolkit
git clone https://github.com/Danukeru/torv3_vanity_addr_cuda.git
cd torv3_vanity_addr_cuda && mkdir build && cd build
cmake .. && make -j"$(nproc)"
cp vanity_torv3_cuda ../../kore-vanity/target/release/   # next to kore-vanity
```

### macOS

CUDA is not available on macOS, so macOS is CPU-only:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo build --release
cargo build -p kore-vanity-gui --release
```

---

## Windows: prepare, compile, run

This is the full path from a clean Windows machine to a running build.

### 1. Install the prerequisites

1. **Visual Studio Build Tools (C++)** — required to link Rust programs on Windows.
   - Download from <https://visualstudio.microsoft.com/downloads/> → "Tools for Visual Studio" → **Build Tools for Visual Studio**.
   - In the installer, select the **"Desktop development with C++"** workload and install.
   - (The `build-all-windows.ps1` script can auto-install this for you — see step 3.)

2. **Rust** (MSVC toolchain):
   ```powershell
   winget install Rustlang.Rustup
   # or download from https://rustup.rs/  (choose the x86_64 MSVC default)
   ```
   Close and reopen PowerShell afterwards so `cargo` is on `PATH`.

3. **(GPU only) NVIDIA CUDA Toolkit 12.x** and **CMake** — only if you want GPU acceleration:
   - CUDA Toolkit: <https://developer.nvidia.com/cuda-downloads> (this also installs the NVIDIA driver / `libcuda`). Make sure the `CUDA_PATH` environment variable is set (the installer normally does this).
   - CMake: <https://cmake.org/download/> (add to `PATH` during install).

### 2. Get the source

```powershell
git clone https://github.com/Mxrider420z/kore-vanity.git
cd kore-vanity
```

If PowerShell blocks the scripts, allow them for the current session:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
```

### 3. Compile

Pick the option that matches the hardware you have.

**A. CPU + external CUDA (recommended, simplest).** Builds the CLI and GUI; GPU acceleration is provided by `vanity_torv3_cuda.exe`.

```powershell
.\build-windows.bat
```

To also build the external GPU binary in one shot (this can auto-install the C++ Build Tools and downloads/compiles the CUDA project; needs CUDA Toolkit + CMake):

```powershell
.\build-all-windows.ps1
```

Both produce binaries in the **`dist\`** folder.

**B. Built-in CUDA (single binary, requires CUDA Toolkit).** Compiles the in-process `rustacuda` GPU/hybrid backend using the committed `ptx\kernel.ptx`:

```powershell
.\build-windows.ps1 -Cuda
```

> Manual equivalents if you prefer raw cargo:
> ```powershell
> cargo build -p kore-vanity --bin kore-vanity --release          # CPU + external CUDA
> cargo build -p kore-vanity-gui --release
> cargo build --release --features cuda                           # built-in CUDA
> ```

### 4. Run

```powershell
# CLI — find an address starting with "kore", save into .\keys
.\dist\kore-vanity.exe kore --dst .\keys

# GUI
.\dist\kore-vanity-gui.exe
```

For GPU via the **external** path, make sure `vanity_torv3_cuda.exe` is in the same folder as `kore-vanity.exe` (the scripts copy both into `dist\`), then add `--mode cuda` (or leave `--mode auto`).

### Runtime requirements (running a prebuilt binary)

None of the build tools (Rust, Visual Studio, CMake, CUDA Toolkit) are needed just to **run** an already-built binary. Requirements:

- **Any mode:** 64-bit Windows 10/11. If Windows reports a missing `VCRUNTIME140.dll`, install the [Microsoft Visual C++ 2015–2022 Redistributable (x64)](https://aka.ms/vs/17/release/vc_redist.x64.exe) (usually already present). CPU mode needs nothing else.
- **GPU — built-in CUDA build** (`--features cuda`): only a **recent NVIDIA driver**. No CUDA Toolkit and no `kernel.ptx` file are needed at runtime — the kernel is embedded in the `.exe` and JIT-compiled by the driver. Run with `--mode cuda` / `--mode hybrid`.
- **GPU — external CUDA build** (default): a recent NVIDIA driver **and** `vanity_torv3_cuda.exe` next to `kore-vanity.exe`. That tool may also need its CUDA runtime DLL (`cudart64_*.dll`) bundled alongside or provided by an installed CUDA Toolkit.

Modern NVIDIA cards (e.g. an RTX 2080 Ti, compute capability 7.5) need nothing card-specific beyond a current driver. If the GPU can't be initialized, the program prints the reason and falls back to CPU.

---

## Usage

### CLI

```bash
# Basic — address starting with "kore"
kore-vanity kore

# Multiple prefixes (stops when each is found once)
kore-vanity kore,block,mysite

# Wildcards: ? matches any single base32 char
kore-vanity k?re,????wink

# Prefix "kore" AND the address must also contain "block" somewhere
kore-vanity kore -c block

# Match the pattern anywhere in the address, not just the start
kore-vanity kore --anywhere

# Output directory, backend and thread count
kore-vanity kore --dst ./keys --mode cpu --threads 8
```

### Options

```
Usage: kore-vanity [OPTIONS] <PREFIXES>...

Arguments:
  <PREFIXES>...  Desired prefix patterns (comma-separated). Use ? as a wildcard.

Options:
  -d, --dst <DST>          Output directory (must exist)        [default: .]
  -m, --mode <MODE>        auto | cpu | cuda | hybrid           [default: auto]
  -t, --threads <THREADS>  CPU threads (cpu/hybrid modes)       [default: #cores]
  -c, --contains <PATTERN> Pattern(s) that must appear anywhere (repeatable)
  -a, --anywhere           Match the prefix pattern anywhere, not just at start
  -h, --help               Print help
  -V, --version            Print version
```

`--mode auto` prefers, in order: built-in CUDA hybrid → built-in CUDA → external CUDA → CPU, falling back automatically to whatever is available.

### GUI

Launch `kore-vanity-gui`, then:
1. Choose a backend mode (Auto / CPU / GPU / Hybrid).
2. Enter one or more patterns (comma-separated; `?` is a wildcard).
3. Optionally add "must also contain" words and toggle "match anywhere".
4. Pick an output directory and click **Start Generation**.

---

## Matching & wildcards

The `?` wildcard and `--anywhere` are evaluated host-side, so support varies by backend:

| Feature | CPU | External CUDA | Built-in CUDA (`--features cuda`) | Hybrid |
|---|---|---|---|---|
| Literal prefix | ✅ | ✅ | ✅ | ✅ |
| `--contains` words | ✅ | ✅ | ✅ | ✅ |
| `?` wildcard in prefix | ✅ | limited¹ | ❌ (rejected with a message) | ✅ (CPU half) |
| `--anywhere` | ✅ | limited¹ | ❌ (rejected with a message) | ✅ (CPU half) |

¹ The GPU kernels search for a *literal* leading prefix; wildcard/anywhere matching can only be applied to what the kernel already emits. **For wildcard or "anywhere" searches, use `--mode cpu`** (or hybrid, where the CPU half handles them). The built-in CUDA backend rejects `?`/`--anywhere` with a clear message rather than silently ignoring them.

---

## Output format

Each found key is written as a ready-to-use Tor hidden-service directory:

```
output_directory/
└── korexxxx…xxxx/                 # onion address without the ".onion" suffix
    ├── hostname                   # "<address>.onion"
    ├── hs_ed25519_public_key      # 32-byte header + 32-byte public key  (64 bytes)
    ├── hs_ed25519_secret_key      # 32-byte header + 64-byte expanded key (96 bytes)
    └── authorized_clients/        # empty (required by Tor)
```

On Unix the directory is created `0700` and the secret key `0600` so Tor will accept it.

### Using the key with Tor

Point a `HiddenServiceDir` at the generated directory, for example in `torrc`:

```
HiddenServiceDir /var/lib/tor/korexxxx…xxxx/
HiddenServicePort 80 127.0.0.1:8080
```

Copy the directory into place, fix ownership/permissions, and reload Tor:

```bash
sudo cp -r korexxxx…xxxx /var/lib/tor/
sudo chown -R debian-tor:debian-tor /var/lib/tor/korexxxx…xxxx   # tor's user
sudo chmod 700 /var/lib/tor/korexxxx…xxxx
sudo systemctl reload tor
```

(The exact user and paths depend on your distro/Tor packaging.)

---

## Performance & expectations

Difficulty grows by **32× per extra fixed character** (base32). Rough order-of-magnitude only — real speed depends heavily on your CPU/GPU:

| Backend | Typical throughput (very approximate) |
|---------|----------------------------------------|
| CPU | ~0.5M keys/sec per core |
| GPU (external CUDA, modern NVIDIA) | tens to hundreds of millions/sec |

Expected work to find a start-anchored prefix is about `32^N` keys for `N` fixed characters:

| Fixed chars | Combinations | Feel |
|-------------|--------------|------|
| 4 | ~1 million | instant |
| 5 | ~33 million | seconds (GPU) / a minute (CPU) |
| 6 | ~1 billion | minutes |
| 7 | ~34 billion | hours |
| 8 | ~1.1 trillion | days |

The CLI and GUI show a live probability and ETA based on the measured key rate. **Tip:** searching for several shorter prefixes, or a short prefix plus a `--contains` word, is far faster than one long prefix.

---

## Project layout & development

```
kore-vanity/
├── src/
│   ├── main.rs            # CLI
│   ├── lib.rs             # library exports
│   ├── onion.rs           # onion address + Tor key/dir helpers (shared by all backends)
│   ├── pattern.rs         # ? wildcard matching
│   ├── probability.rs     # probability / ETA model
│   └── backend/
│       ├── mod.rs         # backend selection + dispatch
│       ├── cpu.rs         # Rayon CPU backend
│       ├── external_cuda.rs  # drives the external vanity_torv3_cuda binary
│       ├── cuda.rs        # built-in rustacuda backend (feature = "cuda")
│       └── hybrid.rs      # CPU + built-in CUDA (feature = "cuda")
├── gui/                   # egui/eframe GUI
├── core/                  # no_std CUDA kernel crate (compiled to PTX)
├── ptx/kernel.ptx         # pre-compiled kernel (so normal builds need no nightly)
├── build.rs              # locates CUDA libs + the PTX when feature = "cuda"
├── build-windows.ps1     # Windows: build CLI+GUI (-Cuda for built-in CUDA)
├── build-all-windows.ps1 # Windows: build CLI+GUI + external CUDA binary
└── BUILD_INSTRUCTIONS.md # developer build reference
```

Run the tests with:

```bash
cargo test                 # CPU/library tests (includes Tor key-format checks)
cargo clippy --all-targets
```

The committed `ptx/kernel.ptx` means building `--features cuda` works on **stable** Rust; you only need nightly + `nvptx64-nvidia-cuda` to *regenerate* the kernel (see `BUILD_INSTRUCTIONS.md`).

---

## Credits

Built upon and inspired by:

- **[tor-v3-vanity](https://github.com/AidenMcClelland/tor-v3-vanity)** by Aiden McClelland — Rust implementation with hybrid CPU/GPU support.
- **[torv3_vanity_addr_cuda](https://github.com/Danukeru/torv3_vanity_addr_cuda)** by Danukeru — high-performance CUDA kernel used by the external GPU backend.

## License

MIT — see [LICENSE](LICENSE).
