# Sift Mail

A calm, fast, keyboard-first desktop email client for Windows, macOS and Linux,
built in Rust on [GPUI Kit](https://gpui-kit.com) (Zed's GPUI plus Longbridge's
component library).

> **Status: early.** IMAP accounts work: add one with **Add account** (iCloud,
> Fastmail, Gmail with an app password, or any IMAP server) and Sift fetches
> the last 90 days of your INBOX, then checks for new mail every 3 minutes.
> Passwords live in the OS credential store. Reply from any thread or write a
> new message; mail goes out over SMTP with proper threading headers and a
> copy is saved to your server's Sent folder. Reading a thread marks it read
> on the server too. The inbox supports views, day sections, Done, Snooze,
> read/unread and Undo from the keyboard. Done and Snooze are local to Sift
> for now. Microsoft 365 and Google sign-in are next. Try it with sample mail:
>
> ```sh
> SIFT_MAIL_DB=/tmp/sift-demo.db cargo run --features demo
> ```

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
    message.rs   messages, threads, contacts, triage state
    triage.rs    rule-based sorting into views
    time.rs      day sections, short timestamps, snooze times
    provider.rs  provider detection and sign-in method
    view.rs      the inbox views shown as tabs
  store/
    mod.rs       SQLite mail store: threads, counts, triage actions, undo, IMAP cursors
    schema.rs    migrations (PRAGMA user_version)
    demo.rs      sample mail, only with --features demo and in tests
  sync/
    parse.rs     raw RFC 5322 mail to messages: threading, triage signals, HTML to text
    imap.rs      IMAP over TLS: incremental UID sync, read flags, Sent folder
    smtp.rs      building MIME with threading headers; SMTP over TLS
    send.rs      send flow: SMTP, Sent copy, local copy, thread done; read write-back
    secrets.rs   passwords in the OS credential store
    service.rs   background sync with its own database connection
  ui/
    mod.rs       window options, opening the store and the main window
    app.rs       inbox state and actions, keyboard handling
    accounts.rs  Add account dialog, background sync and status
    compose.rs   reply box and New message dialog
    render.rs    title bar, list, reader, snooze menu, toasts
```

## Keyboard

| Key | Action |
| --- | --- |
| `1`–`6` | Switch view |
| `J` / `K` or arrows | Next / previous thread |
| `Enter` / `O` | Open thread |
| `Esc` | Back to the list, or close the snooze menu |
| `E` | Done |
| `S`, then `1`–`3` | Snooze: later today, tomorrow, next week |
| `U` | Toggle read |
| `Z` | Undo |
| `R` | Reply to the open thread |
| `C` | New message |
| `Ctrl` + `Enter` (`Cmd` on macOS) | Send |

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

## License

[MIT](LICENSE)
