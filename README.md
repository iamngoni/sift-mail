# Sift Mail

A calm, fast, keyboard-first desktop email client for Windows, macOS and Linux,
built in Rust on [GPUI Kit](https://gpui-kit.com) (Zed's GPUI plus Longbridge's
component library).

> **Status: early.** The app shell, theme and domain types are in place.
> Account connection (Microsoft 365, Google, iCloud, IMAP) is the next piece, so
> there is no mail to show yet.

## Naming

| Where | Name |
| --- | --- |
| Installer, Start menu, window title, About screen | **Sift Mail** |
| Title-bar wordmark, short in-app copy | **Sift** |
| Executable | `SiftMail.exe` (`SiftMail` on macOS and Linux) |

The constants live in [`src/brand.rs`](src/brand.rs).

## Layout

```text
src/
  main.rs        process wiring only (excluded from coverage)
  lib.rs         everything testable
  brand.rs       product naming
  theme/
    color.rs     validated #RRGGBB colours, WCAG contrast
    tokens.rs    the design tokens from the style spec (single source of truth)
    mod.rs       turns tokens into the GPUI Kit theme; light, dark, match system
  domain/
    email.rs     EmailAddress, parsed once at the boundary
    provider.rs  provider detection and sign-in method
    view.rs      the inbox views shown as tabs
  ui/
    mod.rs       window options and opening the main window
    app.rs       title bar, view tabs, empty state, floating toolbar
```

Design tokens come from the approved style spec. Change colours, radii, spacing
or type in `src/theme/tokens.rs` and both GPUI Kit components and Sift's own
components follow. A test fails if any text pair drops below WCAG AA contrast.

## Building

You need the Rust toolchain pinned in `rust-toolchain.toml` (rustup installs it
automatically).

```sh
cargo run
```

Platform notes:

- **Windows:** Visual Studio Build Tools with the C++ workload and the Windows SDK.
  If the linker fails with `LNK1104` in a deeply nested folder, build with a
  shorter target dir: `CARGO_TARGET_DIR=C:\sift-target cargo run`.
- **macOS:** Xcode command line tools. Apple silicon only for releases.
- **Linux:** the GPUI system libraries, for example on Debian or Ubuntu:
  `sudo apt install build-essential pkg-config libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev libfontconfig-dev libfreetype-dev libx11-xcb-dev libxcb1-dev libssl-dev libzstd-dev`

## Quality gates

Every change must pass, in order:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --locked
cargo llvm-cov --all-features --workspace --ignore-filename-regex "src[\\/]main\.rs" --fail-under-lines 95
```

Production code may not `unwrap`, `expect` or `panic` (enforced by Clippy);
tests may. UI behaviour is tested headlessly with GPUI Kit's test harness.

## CI and releases

Workflows run on **self-hosted runners** with these labels:

| Runner | Labels |
| --- | --- |
| Linux x64 | `self-hosted`, `Linux`, `X64` |
| Windows x64 | `self-hosted`, `Windows`, `X64` |
| macOS Apple silicon | `self-hosted`, `macOS`, `ARM64` |

Each runner needs `rustup`, Git, and (on Windows) Git Bash on the `PATH`.
Linux runners also need the libraries listed above.

- **CI** runs on every push to `main` and every pull request: format, Clippy and
  tests on all three platforms, plus the 95% coverage gate on Linux.
- **Release** runs when a `v*` tag is pushed. It checks the tag matches the
  version in `Cargo.toml`, builds installers with
  [cargo-packager](https://github.com/crabnebula-dev/cargo-packager) (NSIS
  installer for Windows, `.app` and `.dmg` for macOS, `.deb` and AppImage for
  Linux), and attaches them to a GitHub release.

```sh
git tag v0.1.0
git push origin v0.1.0
```

Builds are not code-signed yet, so Windows SmartScreen and macOS Gatekeeper
will warn on first launch.
