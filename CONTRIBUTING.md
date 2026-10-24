# Contributing

Thanks for helping! Bug reports, ideas and pull requests are all welcome.

## Getting started

Install [Rust](https://rustup.rs/) 1.95 or newer, then:

```bash
cargo build --workspace            # CLI (torv3-vanity) and GUI (torv3-vanity-gui)
cargo test --workspace             # unit and end-to-end tests
cargo run -p torv3-vanity-gui       # run the app
cargo run -- shop --gpu off        # run the command line
```

Before opening a pull request, please run:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Project layout

| Path | What it is |
|---|---|
| `src/curve/` | Field and point arithmetic (fiat-crypto) and the grouped key walk |
| `src/search/` | Search engine: CPU workers, GPU worker (`gpu/kernel.wgsl` for wgpu, `gpu/cuda/kernel.cu` for CUDA), key verification |
| `src/pattern.rs` | Pattern parsing and matching |
| `src/onion.rs` | Onion addresses and Tor key files |
| `src/estimate.rs` | Difficulty and time estimates |
| `src/main.rs` | Command-line program |
| `gui/` | The app (egui/eframe) |
| `packaging/`, `.github/workflows/` | Release packaging and CI |

## Extra tests

- **GPU (wgpu):** the WGSL kernel is compared bit-for-bit with the CPU walker. Without a GPU,
  install Mesa's software Vulkan driver (`sudo apt install mesa-vulkan-drivers`) and run
  `TORV3_VANITY_SOFTWARE_GPU=1 cargo test -p torv3-vanity --lib -- --ignored`.
- **CUDA:** `cargo test -p torv3-vanity --lib --features kernel-test` also compiles
  `src/search/gpu/cuda/kernel.cu` as C++ for the CPU (needs a C++ compiler) and runs
  the same comparisons on it, plus checks of its field arithmetic. No NVIDIA GPU is
  needed. On a machine with an NVIDIA GPU, `torv3-vanity --benchmark --gpu-api cuda`
  runs it on the real GPU: it should print `Using the GPU: … (CUDA)` and no warning.
  The start-up self-test compares a sample of the GPU's results with the CPU and
  rejects the GPU if they differ; it is a quick check, not a full comparison.
- **Tor:** `scripts/tor-compat-test.sh target/release/torv3-vanity` (needs `tor`) checks
  that Tor derives the same addresses from our secret keys.

The CUDA kernel is shipped as PTX (`src/search/gpu/cuda/kernel.ptx`), generated
from `kernel.cu`. After changing `kernel.cu`, run `scripts/build-cuda-kernel.sh`
(needs clang 18, not the CUDA toolkit) and commit both files; CI checks that they
match and that NVIDIA's PTX assembler accepts the result. In the library, the CUDA
driver calls in `src/search/gpu/cuda/` are the only place where `unsafe` code is
allowed.

Any change to key generation must keep every candidate going through
`Shared::submit` / `Shared::submit_key` in `src/search/mod.rs`, which
re-derive, self-test and verify each key before it's saved.

## Making a release

The recommended way is to let GitHub Actions build every platform:

1. Set the version in `Cargo.toml` (`[workspace.package]`), run `cargo check` so
   `Cargo.lock` picks it up, and give the `CHANGELOG.md` section its release
   date (replace `Unreleased`, e.g. `## [1.2.3] - 2026-11-30`).
2. Commit, then tag and push: `git tag v1.2.3 && git push origin v1.2.3`.
3. The *Release* workflow builds Windows, macOS and Linux, runs a smoke test,
   and creates a **draft** release with the archives, `SHA256SUMS.txt`, release
   notes taken from the changelog, and build-provenance attestations.
4. Open the draft on the *Releases* page, check it, and press **Publish release**.

If a release for the tag already exists (for example because you created it in
GitHub's web interface, which also pushes the tag), the workflow only uploads
archives that are not attached yet. It never replaces files or release notes.

### Building a release archive by hand (Windows)

Use this if you want to build the Windows download yourself instead of with
GitHub Actions. You need the [Windows build prerequisites](README.md#building-from-source),
[Git for Windows](https://gitforwindows.org/) (for Git Bash) and cargo-about:

```bash
# In Git Bash, in the repository folder:
cargo install --locked --features cli cargo-about
cargo build --release --locked --workspace
cargo about generate --workspace --locked -o THIRD-PARTY-LICENSES.html about.hbs
bash packaging/package.sh windows-x64 v1.2.3
```

This creates `dist/torv3-vanity-v1.2.3-windows-x64.zip` containing both
programs, `README.txt`, `LICENSE.txt` and `THIRD-PARTY-LICENSES.html`. Ship the
zip rather than the bare `.exe` files: the licenses of the bundled libraries
must travel with the programs. Then create its checksum for the release page:

```powershell
Get-FileHash dist\torv3-vanity-v1.2.3-windows-x64.zip
```

If you upload a hand-built zip to a release that the workflow also builds, the
workflow keeps your file (it does not overwrite files with the same name).
