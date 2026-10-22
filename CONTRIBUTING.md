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
| `src/search/` | Search engine: CPU workers, GPU worker (`gpu/kernel.wgsl`), key verification |
| `src/pattern.rs` | Pattern parsing and matching |
| `src/onion.rs` | Onion addresses and Tor key files |
| `src/estimate.rs` | Difficulty and time estimates |
| `src/main.rs` | Command-line program |
| `gui/` | The app (egui/eframe) |
| `packaging/`, `.github/workflows/` | Release packaging and CI |

## Extra tests

- **GPU:** the kernel is compared bit-for-bit with the CPU walker. Without a GPU,
  install Mesa's software Vulkan driver (`sudo apt install mesa-vulkan-drivers`) and run
  `TORV3_VANITY_SOFTWARE_GPU=1 cargo test -p torv3-vanity --lib -- --ignored`.
- **Tor:** `scripts/tor-compat-test.sh target/release/torv3-vanity` (needs `tor`) checks
  that Tor derives the same addresses from our secret keys.

Any change to key generation must keep every candidate going through
`Shared::submit` in `src/search/mod.rs`, which re-derives, self-tests and
verifies each key before it's saved.

## Making a release

1. Update the version in `Cargo.toml` (`[workspace.package]`) and add a section to `CHANGELOG.md`.
2. Commit, then tag and push: `git tag v1.2.3 && git push origin v1.2.3`.
3. The *Release* workflow builds all platforms and publishes the GitHub release.
