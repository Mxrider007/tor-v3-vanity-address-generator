# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [1.0.0] - 2026-09-24

First public release: a complete rewrite focused on speed, safety and ease of use.

### Security

- **Fixed a critical key-generation flaw in the old "external CUDA" GPU mode.**
  It stored the public key as the secret nonce prefix of `hs_ed25519_secret_key`,
  which makes signature nonces predictable. Once such an address is in use,
  its private key can be recovered from a published signature. **Keys generated
  that way by earlier development versions are unsafe; replace them.** That mode
  has been removed; every key now uses a secret nonce prefix from the OS random
  number generator.
- Every key is re-derived independently, checked with a signing self-test,
  written, read back and verified before it is reported. Keys are also checked
  in CI with a real Tor client.
- Key folders are created with permissions 700 and key files 600 from the start
  (Linux/macOS), existing files are never overwritten, and data is flushed to disk.
- Removed outdated dependencies with known advisories (`chrono` 0.4.13, `time` 0.1).

### Added

- New search engine: about 5 million keys per second per CPU core (0.17M → 22M
  keys/s on a 4-core machine, over 100× faster).
- GPU acceleration on any NVIDIA, AMD, Intel or Apple GPU through Vulkan,
  DirectX 12 or Metal. No CUDA toolkit is needed. GPU results are verified on the CPU.
- New app (GUI) with live difficulty and time estimates, progress, a results list
  with *Copy* and *Show keys folder* buttons, remembered settings and an icon.
- Command line: `--count`, `--estimate`, `--benchmark`, `--verify`, `--list-gpus`,
  `--quiet`, clearer messages and exit codes, and an explanation when
  `torv3-vanity.exe` is double-clicked in Windows Explorer.
- Ready-to-run downloads for Windows (x64), macOS (universal) and Linux (x64),
  with quick-start instructions, checksums and third-party licenses.
- Continuous integration on Windows, macOS and Linux, including GPU-kernel and
  Tor-compatibility tests.

### Changed

- Patterns are validated with helpful messages (e.g. `0`, `1`, `8` and `9` never
  appear in onion addresses) and impossible patterns are rejected.
- The output folder is created if it doesn't exist.
- Windows programs no longer need the Visual C++ Redistributable.
- Upgraded to Rust 2024 and current versions of all dependencies.

### Removed

- The built-in CUDA backend. Its precompiled kernel referenced functions it did
  not contain, so it could never load, and it depended on unmaintained crates.
- The external CUDA backend and the scripts that downloaded and compiled it.

### Fixed

- `--threads` was ignored by the command-line tool.
- Progress and estimates were misleading (e.g. an estimated 2M keys/s when the
  real speed was 0.17M keys/s).

[1.0.0]: https://github.com/Mxrider007/tor-v3-vanity-address-generator/releases/tag/v1.0.0
