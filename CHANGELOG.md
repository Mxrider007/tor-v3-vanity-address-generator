# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [1.0.0] - Unreleased

First public release: a complete rewrite focused on speed, safety and ease of
use. The project is now called **Tor V3 Vanity Address Generator** (previously
Kore Vanity); the programs are `torv3-vanity` and `torv3-vanity-gui`.

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
- Keys are saved atomically: each key folder is written under a temporary name,
  flushed to disk, read back and verified, and only then renamed to the address.
  A folder named after an address is always complete.
- If a key cannot be saved (disk full, drive removed), it is kept in memory and
  saving is retried every 10 seconds instead of the key being thrown away. The
  output folder is also checked before the search starts.
- After a key is saved, or if saving it fails, a worker never continues with
  keys related to it: every saved key comes from a fresh random start.
- Key folders are created with permissions 700 and key files 600 from the start
  (Linux/macOS), new output folders are 700 too, and existing files are never
  overwritten.
- The GPU is checked against the CPU before each search, and dropped if it
  returns wrong results, so a faulty driver cannot silently waste a search.
- The app saves keys in your home folder by default (`Tor Vanity Keys`) rather
  than in Documents, which is often uploaded to OneDrive or iCloud, and warns
  when the chosen folder looks cloud-synced.
- Removed outdated dependencies with known advisories (`chrono` 0.4.13, `time` 0.1).

### Added

- New search engine: about 5 million keys per second per CPU core (0.17M → 22M
  keys/s on a 4-core machine, over 100× faster).
- GPU acceleration on any NVIDIA, AMD, Intel or Apple GPU through Vulkan,
  DirectX 12 or Metal. No CUDA toolkit is needed. GPU results are verified on the CPU.
- New app (GUI) with live difficulty and time estimates, progress, a results list
  with *Copy* and *Show keys folder* buttons, remembered settings and an icon.
  If the GPU cannot be used for a search, the app continues on the CPU.
- Command line: `--count`, `--estimate`, `--benchmark`, `--verify`, `--list-gpus`,
  `--quiet`, clearer messages and exit codes, and an explanation when
  `torv3-vanity.exe` is double-clicked in Windows Explorer.
- Ready-to-run downloads for Windows (x64), macOS (universal) and Linux (x64,
  glibc 2.28 or newer), with quick-start instructions, checksums, third-party
  licenses and build-provenance attestations.
- Continuous integration on Windows, macOS and Linux, including GPU-kernel and
  Tor-compatibility tests.

### Changed

- Patterns are validated with helpful messages (e.g. `0`, `1`, `8` and `9` never
  appear in onion addresses) and impossible patterns are rejected.
- When an address matches several patterns, it counts for the rarest one.
- Time estimates for several patterns account for having to find all of them.
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
