# Kore Vanity

High-performance vanity onion address generator with GPU acceleration and CPU fallback.

Generate custom `.onion` addresses that start with your chosen prefix (e.g., `korexxxxxxx...xxxx.onion`).

## Features

- **GPU Acceleration**: CUDA support for high-speed key generation (~2-3M keys/sec on modern GPUs)
- **CPU Fallback**: Multi-threaded CPU backend when GPU is unavailable
- **Hybrid Mode**: Use both CPU and GPU simultaneously for maximum throughput
- **Multi-word Search**: Find addresses matching a prefix AND containing additional words anywhere
- **GUI Application**: Easy-to-use graphical interface
- **CLI Tool**: Command-line interface for scripting and automation
- **Tor-Compatible Output**: Generates proper hidden service directory structure

## Installation

### Prerequisites

- **Rust** (1.70+): https://rustup.rs/
- **CUDA Toolkit 12.x** (for GPU mode): https://developer.nvidia.com/cuda-downloads
- **CMake** (for CUDA build): https://cmake.org/download/

### Windows Quick Build

```powershell
.\build-all-windows.ps1
```

The script will:
1. Detect and configure Visual Studio Build Tools (or offer to install them)
2. Build the CUDA accelerator (if CUDA is available)
3. Build the GUI and CLI applications

Output files will be in the `dist/` folder.

### Linux Build

```bash
# Install dependencies (Ubuntu/Debian)
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev libgtk-3-dev

# Optional: Install CUDA Toolkit for GPU support
# Download from: https://developer.nvidia.com/cuda-downloads

# Clone the repository
git clone https://github.com/Mxrider420z/kore-vanity.git
cd kore-vanity

# Build CLI and library
cargo build --release

# Build GUI
cargo build -p kore-vanity-gui --release

# Binaries will be in target/release/
./target/release/kore-vanity --help
./target/release/kore-vanity-gui
```

### For GPU support on Linux

```bash
# Install CUDA dependencies
sudo apt install -y cmake nvidia-cuda-toolkit

# Clone and build the external CUDA accelerator
git clone https://github.com/Danukeru/torv3_vanity_addr_cuda.git
cd torv3_vanity_addr_cuda
mkdir build && cd build
cmake ..
make -j$(nproc)

# Copy to same directory as kore-vanity
cp vanity_torv3_cuda ../kore-vanity/target/release/
```

### macOS Build

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build (CPU only - CUDA not supported on macOS)
cargo build --release
cargo build -p kore-vanity-gui --release
```

## Usage

### GUI

```bash
./kore-vanity-gui
```

1. Enter desired prefix(es) in the "Prefixes" field
2. Optionally add words that must appear anywhere in the address
3. Select output directory
4. Click "Start Generation"

### CLI

```bash
# Basic usage - find address starting with "kore"
kore-vanity kore

# Multiple prefixes
kore-vanity kore,block,mysite

# With contains filter - prefix "kore" + must contain "block" anywhere
kore-vanity kore -c block

# Specify output directory
kore-vanity kore --dst ./keys

# CPU-only mode
kore-vanity kore --mode cpu
```

### Options

```
Usage: kore-vanity [OPTIONS] <PREFIXES>

Arguments:
  <PREFIXES>  Desired prefixes (comma-separated)

Options:
  -d, --dst <DST>           Output directory [default: .]
  -m, --mode <MODE>         Backend mode [auto|cpu|cuda|hybrid]
  -t, --threads <THREADS>   CPU threads
  -c, --contains <WORD>     Words that must appear anywhere in address
  -h, --help                Print help
  -V, --version             Print version
```

## Output Format

Generated keys are stored in Tor-compatible hidden service directory format:

```
output_directory/
└── korexxxx...xxxx/
    ├── hostname              # Full .onion address
    ├── hs_ed25519_public_key # Public key (64 bytes)
    ├── hs_ed25519_secret_key # Secret key (96 bytes)
    └── authorized_clients/   # Empty directory
```

### Using with Tor/Kore

```bash
# Stop your node
kore-cli stop

# Backup existing onion directory
mv ~/.kore/testnet3/tor/onion ~/.kore/testnet3/tor/onion.backup

# Copy generated keys
cp -r /path/to/generated_dir ~/.kore/testnet3/tor/onion

# Set permissions
chmod 700 ~/.kore/testnet3/tor/onion
chmod 600 ~/.kore/testnet3/tor/onion/*

# Restart
kored -testnet
```

## Performance

Estimated generation times at 2.79M keys/sec (GPU):

| Prefix Length | Combinations | Time |
|---------------|--------------|------|
| 4 chars | ~1 million | instant |
| 5 chars | ~33 million | ~12 seconds |
| 6 chars | ~1 billion | ~6 minutes |
| 7 chars | ~34 billion | ~3.4 hours |
| 8 chars | ~1.1 trillion | ~4.5 days |

**Tip**: Use the contains filter to search for multiple shorter words instead of one long prefix.

## Credits

This project is built upon and inspired by:

- **[tor-v3-vanity](https://github.com/AidenMcClelland/tor-v3-vanity)** by Aiden McClelland - Original Rust implementation with hybrid CPU/GPU support
- **[torv3_vanity_addr_cuda](https://github.com/Danukeru/torv3_vanity_addr_cuda)** by Danukeru - High-performance CUDA kernel for GPU acceleration

Additional features and modifications:
- Multi-word contains filter (prefix + substring matching)
- Improved Windows build system with automatic VS Build Tools detection/installation
- Enhanced GUI with contains filter support
- Tor-compatible hidden service directory output
- Cross-platform improvements

## License

MIT License
