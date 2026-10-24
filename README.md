<div align="center">

<img src="gui/assets/icon.png" width="96" alt="Tor V3 Vanity Address Generator icon">

# Tor V3 Vanity Address Generator

**Create a custom Tor `.onion` address that starts with the text you choose.**

Fast, free and open source: runs on your CPU and on any modern graphics card, with an easy app and a command-line tool for Windows, macOS and Linux.

[![CI](https://github.com/Mxrider007/tor-v3-vanity-address-generator/actions/workflows/ci.yml/badge.svg)](https://github.com/Mxrider007/tor-v3-vanity-address-generator/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/Mxrider007/tor-v3-vanity-address-generator)](https://github.com/Mxrider007/tor-v3-vanity-address-generator/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**[⬇ Download for Windows, macOS and Linux](https://github.com/Mxrider007/tor-v3-vanity-address-generator/releases/latest)**

</div>

---

A Tor v3 onion address is 56 random-looking characters, like
`2gzyxa5ihm7nsggfxnu52rck2vv4rvmdlkiu3zzui5du4xyclen53wid.onion`. This tool
generates keys until it finds one whose address starts with the text you want,
for example **`shop`**`qmr7vp4…`**`.onion`**, and saves it in the format Tor
uses, ready to go.

- [Features](#features)
- [Quick start](#quick-start)
- [Using the app](#using-the-app)
- [Command line](#command-line)
- [How long will it take?](#how-long-will-it-take)
- [What you get](#what-you-get)
- [Using your address with Tor](#using-your-address-with-tor)
- [Security](#security)
- [Troubleshooting](#troubleshooting)
- [Building from source](#building-from-source)
- [How it works](#how-it-works)

## Features

- **Easy app**: type your text, press *Start*, done. There's also a full command-line tool.
- **Fast**: about 6 million keys per second per CPU core.
- **Uses your graphics card**: NVIDIA, AMD, Intel or Apple GPUs through Vulkan, DirectX 12 or Metal. No CUDA or other toolkit is needed.
- **Safe keys**: every key is re-derived independently, signature-tested, written, read back and verified before it is shown. Keys are checked in CI with a real Tor client.
- **Ready for Tor**: saves the exact files Tor expects (`hostname`, `hs_ed25519_secret_key`, …).
- **Flexible patterns**: several patterns at once, `?` wildcards, "anywhere in the address", "must also contain", and several results per pattern.
- **Honest estimates**: shows the expected time and the chance that it has found your address by now.

## Quick start

### Windows 10/11

1. Download **`torv3-vanity-…-windows-x64.zip`** from the [latest release](https://github.com/Mxrider007/tor-v3-vanity-address-generator/releases/latest).
2. Right-click the zip and choose **Extract All**.
3. Double-click **`torv3-vanity-gui.exe`**.
   If Windows shows *"Windows protected your PC"*, click **More info → Run anyway**. The app isn't code-signed, which is why Windows asks.
4. Type the text your address should start with (for example `shop`) and click **Start search**.
5. When it's done, click **Show keys folder**.

Nothing needs to be installed.

### macOS 11+ (Apple Silicon and Intel)

Download **`…-macos-universal.zip`**, unzip it and open **Tor V3 Vanity.app**. The app is not notarized by Apple, so macOS refuses to open it the first time. Go to **System Settings → Privacy & Security**, scroll down and click **Open Anyway** (on macOS 11–14 you can instead Control-click the app and choose **Open**). Or run this in Terminal, in the unzipped folder:

```bash
xattr -dr com.apple.quarantine "Tor V3 Vanity.app" torv3-vanity
```

### Linux (x86-64)

Download **`…-linux-x64.tar.gz`**, extract it and run `./torv3-vanity-gui` (app) or `./torv3-vanity shop` (command line). It runs on any distribution with glibc 2.28 or newer. For GPU support, install your Vulkan driver (for example `mesa-vulkan-drivers` or the NVIDIA driver).

## Using the app

1. **Your address**: type the text the address should start with. Separate several patterns with commas to search for all of them at once. Use `?` for "any character". Tick *Allow the text anywhere* to accept it at any position; that is about 50 times more likely per key, but it doesn't use the graphics card. *Must also contain* adds words that must appear somewhere in the address as well.
2. **Where to save the keys**: by default `Tor Vanity Keys` in your home folder (`C:\Users\<you>\Tor Vanity Keys` on Windows). The app warns you if the folder looks like it's synced to OneDrive, iCloud, Dropbox or Google Drive, because your secret keys would be uploaded there.
3. **Hardware**: the number of CPU threads and whether to also use the graphics card. The box is ticked by default for dedicated cards and Apple Silicon; tick it to use integrated graphics too.

The **Estimate** box shows how hard your pattern is and how long it should take on your computer. While searching you see the speed, the keys checked and the chance that the address has been found by now. Found addresses appear in a list with *Copy address* and *Show keys folder* buttons. Settings are remembered between launches.

## Command line

The command-line program is `torv3-vanity` (`torv3-vanity.exe` on Windows; in PowerShell run it as `.\torv3-vanity.exe` from its folder).

```text
torv3-vanity [OPTIONS] <PATTERN>...

  torv3-vanity shop                   address starting with "shop"
  torv3-vanity shop -o keys           ...saved in the folder "keys"
  torv3-vanity shop blog news         several patterns at once
  torv3-vanity "sh?p"                 ? matches any single character
  torv3-vanity shop -n 5              find 5 addresses to choose from
  torv3-vanity shop --anywhere        "shop" anywhere in the address
  torv3-vanity shop -c blog           starts with "shop" and contains "blog"
  torv3-vanity shop --estimate        how long will it take?
  torv3-vanity --benchmark            measure the speed of this computer
  torv3-vanity --verify keys/shop...  check a saved key folder
  torv3-vanity --list-gpus            show usable graphics cards
```

Put patterns that contain `?` in double quotes, so the shell doesn't treat them as file names.

| Option | Meaning |
|---|---|
| `-o, --output <DIR>` | Folder to save keys in, created if needed (default: the current folder) |
| `-n, --count <N>` | Addresses to find per pattern; `0` keeps going until you stop it (default: 1) |
| `-a, --anywhere` | Match the patterns anywhere in the address, not only at the start |
| `-c, --contains <TEXT>` | Also require this text somewhere in the address (can be repeated) |
| `-t, --threads <N>` | CPU threads to use (default: all) |
| `-g, --gpu <auto\|on\|off>` | `auto` (default) uses a dedicated graphics card or Apple Silicon; `on` also uses integrated graphics; `off` uses only the CPU |
| `--estimate` | Measure the speed for a few seconds, show the expected time, and exit |
| `--benchmark [SECONDS]` | Measure the speed (default: 10 seconds) and exit |
| `--verify <DIR>` | Check a saved key folder: the files are complete, consistent and can sign |
| `--list-gpus` | List the graphics cards that can be used |
| `-q, --quiet` | Only print the found addresses, one per line |

Found addresses go to standard output and everything else (progress, messages) to standard error, so `torv3-vanity -q -n 0 shop | head -n 5` works as expected. Press **Ctrl+C** to stop; keys that are being saved are finished first. The exit code is 0 when everything was found, 1 after an error and 130 when stopped with Ctrl+C.

The GPU searches only for text at the start of the address. For "anywhere" searches, and for a short pattern combined with `--contains`, the CPU does the work.

## How long will it take?

Every extra character makes the search **32 times** longer. These are average times; the search is luck-based, so it can be faster or slower. The app (or `--estimate`) shows the estimate for your own computer.

| Characters | Keys to try (average) | 4 CPU cores (~25M keys/s) | 16 CPU cores (~100M keys/s) |
|:---:|---:|---:|---:|
| 4 | 1 million | instant | instant |
| 5 | 34 million | ~1 second | under a second |
| 6 | 1.1 billion | ~45 seconds | ~10 seconds |
| 7 | 34 billion | ~25 minutes | ~6 minutes |
| 8 | 1.1 trillion | ~12 hours | ~3 hours |
| 9 | 35 trillion | ~16 days | ~4 days |
| 10 | 1.1 quadrillion | ~1.4 years | ~4 months |

The 4-core figure was measured on a cloud server; the 16-core figure assumes the same speed per core. A graphics card adds its own speed on top.

Tips:

- Searching for **several patterns at once** (`shop, sh0p, store`) is almost free: each key is compared with all of them. The search ends when every pattern has been found, and you can keep the one you like best.
- Onion addresses only use the letters `a`–`z` and the digits `2`–`7` (**no 0, 1, 8 or 9**). Use `?` for "any character".
- You can only choose part of the address; the rest stays random.

## What you get

Each address is saved as a folder named after it:

```text
shopqmr7…xyzd/
├── hostname                 shopqmr7…xyzd.onion
├── hs_ed25519_public_key    public key (Tor format)
├── hs_ed25519_secret_key    PRIVATE key (Tor format)
└── authorized_clients/      empty; Tor expects it
```

> [!WARNING]
> **`hs_ed25519_secret_key` *is* your address.** Anyone who gets a copy can impersonate your site, and if you lose it the address is gone forever. Keep it private and keep a backup (for example on an encrypted USB drive).

## Using your address with Tor

**Linux** (and other Unix systems):

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

**Windows** ([Tor Expert Bundle](https://www.torproject.org/download/tor/)): copy the folder to a short path without spaces, such as `C:\tor\my_site`, and add the same two lines to your `torrc` with that path (`HiddenServiceDir C:\tor\my_site`). Then restart Tor.

To check a key folder before you use it, run `torv3-vanity --verify <folder>`.

## Security

- Keys come from your operating system's secure random number generator. Each worker starts from a fresh random key and walks consecutive keys from it, the technique [mkp224o](https://github.com/cathugger/mkp224o) uses. After an address is saved, the worker starts again from a new random key, so saved keys are never related to each other.
- Every candidate is **re-derived from scratch** with [curve25519-dalek](https://github.com/dalek-cryptography/curve25519-dalek), re-checked, and **used to sign and verify a message** exactly as Tor does. It is then written under a temporary name, **read back and verified**, and only then renamed to its address. A bug in the fast search can at worst miss a match; it cannot produce a wrong key.
- The graphics card is checked against the CPU before every search, and the search continues on the CPU if the GPU returns wrong results.
- If a key can't be saved (for example because the disk is full), it stays in memory and saving is retried every 10 seconds, so a key that took hours to find isn't lost.
- The tool never uses the network.

Where the keys are stored matters:

- **Linux and macOS**: key folders are created with permissions `700` and key files with `600`, so other users can't read them. Output folders that the tool creates are `700` too.
- **Windows**: key folders get the permissions of the folder they are saved in. Folders inside your user folder (`C:\Users\<you>\…`, the app's default) can only be read by you and administrators. Folders elsewhere, such as `C:\keys` or a USB stick, are often readable by every user of the computer.
- **Cloud sync**: don't save keys in folders that OneDrive, iCloud Drive, Dropbox or Google Drive upload. Documents and Desktop often are, which is why the app uses your home folder by default.

> [!IMPORTANT]
> **If you generated keys with a pre-1.0 development version of this project using the "external CUDA" GPU mode**, those keys are **unsafe**: their secret nonce was set to the public key, which lets others recover the private key once the address is in use. Stop using them and generate new ones. See [CHANGELOG.md](CHANGELOG.md) and [SECURITY.md](SECURITY.md).

To report a vulnerability, see [SECURITY.md](SECURITY.md).

## Troubleshooting

<details>
<summary><b>Windows says "Windows protected your PC"</b></summary>

The program isn't signed with a paid code-signing certificate. Click **More info → Run anyway**. You can check your download against `SHA256SUMS.txt` from the release page (`Get-FileHash .\file.zip` in PowerShell). Archives built by this repository's GitHub Actions release workflow can also be checked with `gh attestation verify <file> --repo Mxrider007/tor-v3-vanity-address-generator`.
</details>

<details>
<summary><b>My antivirus complains</b></summary>

Key generators are sometimes flagged by heuristics. The tool is open source: check the SHA-256 checksum, or build it yourself (see [Building from source](#building-from-source)).
</details>

<details>
<summary><b>The graphics card isn't used</b></summary>

Run `torv3-vanity --list-gpus`. By default only dedicated graphics cards and Apple Silicon are used; use `--gpu on` (or tick the box in the app) to also use integrated graphics. If no card is listed, update your graphics driver (on Linux, install the Vulkan driver). The GPU only searches for text at the start of the address, so "anywhere" searches, and short patterns combined with *must also contain*, run on the CPU. The messages at the start of a search say why the GPU isn't used.
</details>

<details>
<summary><b>"Cannot save keys in …"</b></summary>

The output folder is checked before the search starts. Choose a folder you can write to. On Windows, *Controlled folder access* (ransomware protection) may block new programs from writing to Documents and Desktop; the default folder in your home folder isn't affected.
</details>

<details>
<summary><b>"shop0" is rejected</b></summary>

Onion addresses never contain `0`, `1`, `8` or `9`. Use letters that look similar, such as `o`, `l`/`i`, `b` or `g`.
</details>

<details>
<summary><b>The app doesn't start on Linux</b></summary>

The app needs an X11 or Wayland desktop with `libxkbcommon` (on X11 also `libxkbcommon-x11`) and an OpenGL or Vulkan driver. Start it from a terminal to see the error message. The command-line tool works without a desktop.
</details>

<details>
<summary><b>A folder called <code>.torv3-vanity-tmp-…</code> appeared</b></summary>

Keys are written there first and then renamed. If the program was killed in the middle of saving, the folder can be left behind; it is safe to delete.
</details>

## Building from source

You need [Rust](https://rustup.rs/) 1.95 or newer. The finished programs are `torv3-vanity` (command line) and `torv3-vanity-gui` (app).

### Windows

1. Install the **[Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)** with the **"Desktop development with C++"** workload. It includes the linker and the Windows SDK, which is needed to embed the program icon. If you install Rust first, `rustup-init.exe` offers to install these for you.
2. Install **Rust** with [`rustup-init.exe`](https://rustup.rs/) and accept the defaults (the `x86_64-pc-windows-msvc` toolchain).
3. Get the source code: download it as a zip from the [repository page](https://github.com/Mxrider007/tor-v3-vanity-address-generator) (*Code → Download ZIP*) or with Git:
   ```powershell
   git clone https://github.com/Mxrider007/tor-v3-vanity-address-generator.git
   cd tor-v3-vanity-address-generator
   ```
4. Double-click **`build-windows.bat`**, or run in PowerShell:
   ```powershell
   cargo build --release --locked --workspace
   ```

The first build downloads the dependencies and takes a few minutes. `build-windows.bat` copies both programs to `dist\`; with `cargo` they are in `target\release\`. They are standalone `.exe` files that need no Visual C++ runtime. To package them as a release download (with the license files), see [CONTRIBUTING.md](CONTRIBUTING.md#building-a-release-archive-by-hand-windows).

### Linux

Install a C toolchain (for example `sudo apt install build-essential`), then:

```bash
git clone https://github.com/Mxrider007/tor-v3-vanity-address-generator.git
cd tor-v3-vanity-address-generator
cargo build --release --locked --workspace
# target/release/torv3-vanity-gui  and  target/release/torv3-vanity
```

### macOS

Install the Xcode command-line tools (`xcode-select --install`), then run the same commands as on Linux.

### Command line only

The app pulls in a graphical toolkit. To build only the command-line tool:

```bash
cargo build --release --locked -p torv3-vanity
```

Tests, the GPU kernel test with a software GPU, the Tor compatibility test and release instructions are described in [CONTRIBUTING.md](CONTRIBUTING.md).

## How it works

Onion addresses are the Ed25519 public key in base32, so the first 51 characters are just the key's bits. The search walks consecutive public keys `(a + 8k)·B` from a random start. It visits them in groups around a center point, where the neighbours `P ± i·8B` share most of their arithmetic and one batched field inversion serves 513 keys. That costs about 5.5 field multiplications per key, and a pattern is tested with a single bit-mask comparison on the raw key. The same walk runs on the GPU as a WGSL compute shader with 13-bit limb arithmetic. Field arithmetic on the CPU uses the formally verified [fiat-crypto](https://github.com/mit-plv/fiat-crypto) code.

## Credits

- Originally based on [tor-v3-vanity](https://github.com/dr-bonez/tor-v3-vanity) by Aiden McClelland.
- Search technique from [mkp224o](https://github.com/cathugger/mkp224o); key grouping inspired by [VanitySearch](https://github.com/JeanLucPons/VanitySearch).
- Built with [curve25519-dalek](https://github.com/dalek-cryptography/curve25519-dalek), [fiat-crypto](https://github.com/mit-plv/fiat-crypto), [wgpu](https://wgpu.rs) and [egui](https://github.com/emilk/egui).

## License

[MIT](LICENSE). Release downloads include the licenses of all third-party components (`THIRD-PARTY-LICENSES.html`).
