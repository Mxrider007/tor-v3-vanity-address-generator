<div align="center">

<img src="gui/assets/icon.png" width="96" alt="Kore Vanity icon">

# Kore Vanity

**Create a custom Tor `.onion` address that starts with the text you choose.**

Fast, free and open source: runs on your CPU and on any modern graphics card, with an easy app for Windows, macOS and Linux.

[![CI](https://github.com/Mxrider007/tor-v3-vanity-address-generator/actions/workflows/ci.yml/badge.svg)](https://github.com/Mxrider007/tor-v3-vanity-address-generator/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/Mxrider007/tor-v3-vanity-address-generator)](https://github.com/Mxrider007/tor-v3-vanity-address-generator/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**[⬇ Download for Windows, macOS and Linux](https://github.com/Mxrider007/tor-v3-vanity-address-generator/releases/latest)**

</div>

---

A Tor v3 onion address is 56 random-looking characters, like
`2gzyxa5ihm7nsggfxnu52rck2vv4rvmdlkiu3zzui5du4xyclen53wid.onion`. Kore Vanity
generates keys until it finds one whose address starts with the text you want,
for example **`kore`**`qmr7vp4…`**`.onion`**, and saves it ready to use with Tor.

## Features

- **Easy app** – type your text, press *Start*, done. There's also a full command-line tool.
- **Fast** – about 5 million keys per second per CPU core, over 100× faster than earlier versions.
- **Uses your graphics card** – NVIDIA, AMD, Intel or Apple GPUs through Vulkan, DirectX 12 or Metal. No CUDA or other toolkit is needed.
- **Safe keys** – every key is independently re-derived, signature-tested, written, read back and verified before it is shown. Keys are checked in CI with a real Tor client.
- **Ready for Tor** – saves the exact files Tor expects (`hostname`, `hs_ed25519_secret_key`, …) with private permissions.
- **Flexible patterns** – several patterns at once, `?` wildcards, "anywhere in the address", "must also contain", and N results per pattern.
- **Honest estimates** – shows the expected time and the chance you've found it so far.

## Quick start

### Windows 10/11

1. Download **`torv3-vanity-…-windows-x64.zip`** from the [latest release](https://github.com/Mxrider007/tor-v3-vanity-address-generator/releases/latest).
2. Right-click the zip → **Extract All**.
3. Double-click **`torv3-vanity-gui.exe`**.
   If Windows shows *"Windows protected your PC"*, click **More info → Run anyway**. The app isn't code-signed, which is why Windows asks.
4. Type the text your address should start with (e.g. `kore`) and click **Start search**.
5. When it's done, click **Show keys folder**.

Nothing needs to be installed.

### macOS 11+ (Apple Silicon and Intel)

Download **`…-macos-universal.zip`**, unzip it and open **Tor V3 Vanity.app**. The app is not notarized, so the first time you open it, right-click it and choose **Open**. Alternatively, run `xattr -dr com.apple.quarantine "Tor V3 Vanity.app"`.

### Linux (x86-64)

Download **`…-linux-x64.tar.gz`**, extract it and run `./torv3-vanity-gui` (app) or `./torv3-vanity kore` (command line). For GPU support, install your Vulkan driver (e.g. `mesa-vulkan-drivers` or the NVIDIA driver).

## How long will it take?

Every extra character makes the search **32 times** longer. The app shows an estimate for your own computer before you start.

| Characters | Keys to try (average) | 4-core laptop (~20M keys/s) | 16-core desktop (~80M keys/s) |
|:---:|---:|---:|---:|
| 4 | 1 million | instant | instant |
| 5 | 34 million | ~2 seconds | under a second |
| 6 | 1.1 billion | ~1 minute | ~15 seconds |
| 7 | 34 billion | ~30 minutes | ~7 minutes |
| 8 | 1.1 trillion | ~15 hours | ~4 hours |
| 9 | 35 trillion | ~3 weeks | ~5 days |

A graphics card adds its own speed on top of these figures. Tips:

- Searching for **several patterns at once** (`kore, kor3, k0re`) costs nothing extra; you get whichever appears first.
- Onion addresses only use the letters `a`–`z` and digits `2`–`7` (**no 0, 1, 8 or 9**). Use `?` for "any character".
- You can't choose the whole address, only a part of it.

## Command line

```text
torv3-vanity [OPTIONS] <PATTERN>...

  torv3-vanity kore                   address starting with "kore"
  torv3-vanity kore -o keys           ...saved in the folder "keys"
  torv3-vanity kore blog shop         several patterns at once
  torv3-vanity k?re                   ? matches any single character
  torv3-vanity kore -n 5              find 5 addresses to choose from
  torv3-vanity kore --anywhere        "kore" anywhere in the address
  torv3-vanity kore -c shop           starts with "kore" and contains "shop"
  torv3-vanity kore --estimate        how long will it take on this computer?
  torv3-vanity --benchmark            measure your speed
  torv3-vanity --verify keys/kore...  check a saved key folder
  torv3-vanity --list-gpus            show usable graphics cards
```

| Option | Meaning |
|---|---|
| `-o, --output <DIR>` | Folder to save keys in (created if needed; default: current folder) |
| `-n, --count <N>` | Addresses to find per pattern; `0` = keep going until stopped (default 1) |
| `-a, --anywhere` | Match patterns anywhere in the address, not only at the start |
| `-c, --contains <TEXT>` | Also require this text somewhere in the address (repeatable) |
| `-t, --threads <N>` | CPU threads (default: all cores) |
| `-g, --gpu <auto\|on\|off>` | `auto` uses a dedicated graphics card if present; `on` also uses integrated graphics |
| `-q, --quiet` | Only print the found addresses |

Press **Ctrl+C** to stop. Found addresses are printed to standard output, and progress goes to standard error.

## What you get

Each address is saved as a folder named after it:

```text
kore3fkq…xyzd/
├── hostname                 kore3fkq…xyzd.onion
├── hs_ed25519_public_key    public key (Tor format)
├── hs_ed25519_secret_key    PRIVATE key (Tor format)
└── authorized_clients/      empty; Tor expects it
```

> [!WARNING]
> **`hs_ed25519_secret_key` *is* your address.** Anyone who gets a copy can impersonate your site, and if you lose it the address is gone forever. Keep it private and keep a backup (for example on an encrypted USB drive).

## Using your address with Tor

1. Copy the whole folder to the computer that runs your onion service, for example to `/var/lib/tor/my_site/`.
2. Give it to the Tor user with private permissions:
   ```bash
   sudo chown -R debian-tor:debian-tor /var/lib/tor/my_site   # the user is "tor" on some systems
   sudo chmod 700 /var/lib/tor/my_site
   ```
3. Add to your `torrc` (usually `/etc/tor/torrc`):
   ```text
   HiddenServiceDir /var/lib/tor/my_site/
   HiddenServicePort 80 127.0.0.1:8080
   ```
4. Restart Tor (`sudo systemctl restart tor`). Your site is now reachable at the address in `hostname`.

On Windows with the Tor Expert Bundle, the same `HiddenServiceDir` / `HiddenServicePort` lines work with a Windows path.

## Security

- Keys come from your operating system's secure random number generator. Each worker starts from a fresh random key and walks consecutive keys from it, the technique [mkp224o](https://github.com/cathugger/mkp224o) uses. After an address is saved, the worker starts from a new random key, so saved keys are never related to each other.
- Every candidate is **re-derived from scratch** with [curve25519-dalek](https://github.com/dalek-cryptography/curve25519-dalek), re-checked, **used to sign and verify a message** exactly as Tor does, written to disk, then **read back and verified**. A bug in the fast search can at worst miss a match; it can never produce a wrong key.
- On Linux and macOS the key folder is created with permissions `700` and the files `600` from the start.
- Kore Vanity never uses the network.

> [!IMPORTANT]
> **If you generated keys with a pre-1.0 version of this project using the "external CUDA" GPU mode**, those keys are **unsafe**: their secret nonce was set to the public key, which can let others recover the private key once the address is in use. Stop using them and generate new ones. See [CHANGELOG.md](CHANGELOG.md) and [SECURITY.md](SECURITY.md).

To report a vulnerability, see [SECURITY.md](SECURITY.md).

## Troubleshooting

<details>
<summary><b>Windows says "Windows protected your PC"</b></summary>

The program isn't signed with a paid code-signing certificate. Click **More info → Run anyway**. You can check your download against `SHA256SUMS.txt` from the release page (`Get-FileHash .\file.zip` in PowerShell).
</details>

<details>
<summary><b>My antivirus complains</b></summary>

Key generators are sometimes flagged by heuristics. Kore Vanity is open source, and the release files are built by GitHub Actions directly from this repository. Check the SHA-256 checksum, or build it yourself (see below).
</details>

<details>
<summary><b>The graphics card isn't used</b></summary>

Run `torv3-vanity --list-gpus`. In *auto* mode only dedicated graphics cards are used; use `--gpu on` (or tick the box in the app) to also use integrated graphics. Update your graphics driver if no card is listed. GPU search supports patterns at the start of the address; "anywhere" searches run on the CPU.
</details>

<details>
<summary><b>"kore0" is rejected</b></summary>

Onion addresses never contain `0`, `1`, `8` or `9`. Use letters that look similar, such as `o`, `l`/`i`, `b` or `g`.
</details>

## Building from source

Install [Rust](https://rustup.rs/) (1.95 or newer), then:

```bash
git clone https://github.com/Mxrider007/tor-v3-vanity-address-generator.git
cd torv3-vanity
cargo build --release --workspace
# target/release/torv3-vanity-gui  and  target/release/torv3-vanity
```

On Windows you can double-click `build-windows.bat` instead (it needs Rust with the default MSVC toolchain); the programs end up in `dist\`. See [CONTRIBUTING.md](CONTRIBUTING.md) for tests and development notes.

## How it works

Onion addresses are the Ed25519 public key in base32, so the first 51 characters are just the key's bits. Kore Vanity walks consecutive public keys `(a + 8k)·B` from a random start. It visits them in groups around a center point, where the neighbours `P ± i·8B` share most of their arithmetic and one batched field inversion serves 513 keys. That costs about 5.5 field multiplications per key, and a pattern is tested with a single bit-mask comparison on the raw key. The same walk runs on the GPU as a WGSL compute shader with 13-bit limb arithmetic. Field arithmetic on the CPU uses the formally verified [fiat-crypto](https://github.com/mit-plv/fiat-crypto) code.

## Credits

- Originally based on [tor-v3-vanity](https://github.com/dr-bonez/tor-v3-vanity) by Aiden McClelland.
- Search technique from [mkp224o](https://github.com/cathugger/mkp224o); key grouping inspired by [VanitySearch](https://github.com/JeanLucPons/VanitySearch).
- Built with [curve25519-dalek](https://github.com/dalek-cryptography/curve25519-dalek), [fiat-crypto](https://github.com/mit-plv/fiat-crypto), [wgpu](https://wgpu.rs) and [egui](https://github.com/emilk/egui).

## License

[MIT](LICENSE). Release downloads include the licenses of all third-party components (`THIRD-PARTY-LICENSES.html`).
