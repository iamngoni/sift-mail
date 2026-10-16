# Sift Mail

A calm, fast, keyboard-first desktop email client for Windows, macOS and Linux,
built in Rust on [GPUI Kit](https://gpui-kit.com) (Zed's GPUI plus Longbridge's
component library).

> **Status: early.** Add an account with **Add account**: sign in with
> Microsoft 365 or Google in your browser (see [Sign-in setup](#sign-in-setup)),
> or a password for iCloud, Fastmail or any IMAP server (server settings are
> filled in and editable). Choose a name, colour and how far back to sync, then
> watch the first sync. New mail is checked every 3 minutes.
> Passwords and sign-in tokens live in the OS credential store. Reply from any
> thread or write a new message; mail goes out over SMTP with proper threading
> headers and a copy is saved to your server's Sent folder. Reading a thread
> marks it read on the server too. The inbox has views, custom views you
> describe, day sections, a sidebar with every account, full-text search and
> commands in one palette (Ctrl K), bulk selection, attachments that open in
> your usual apps, and Done, Snooze, read/unread and Undo from the keyboard. Try it with sample mail:
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
  assets.rs      extra Lucide icons embedded on top of GPUI Kit's set
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
    search.rs    full-text search (FTS5) with user text kept out of query syntax
    prefs.rs     preferences, custom views, per-account settings, send-as aliases
    files.rs     attachments saved beside the database
    demo.rs      sample mail, only with --features demo and in tests
  oauth/
    mod.rs       browser sign-in: PKCE, token exchange and refresh, error mapping
    loopback.rs  one-shot 127.0.0.1 listener for the redirect
    http.rs      token endpoint client (rustls, timeouts, no redirects)
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
    chrome.rs    sidebar, account filter, palette, dialogs, bulk selection, preferences
    chrome_view.rs  how those look
    accounts.rs  background sync and status
    add_account.rs  the four-step Add account flow: choose, sign in, set up, connect
    add_account_view.rs  how that flow looks
    settings.rs  Settings: accounts (name, colour, send-as, signature, sync, AI, notifications,
                 connection, removal), default From, appearance, notifications, keyboard
    settings_view.rs  how Settings looks
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
| `S`, then `1`–`4` | Snooze: later today, tomorrow, Monday, next week |
| `X` | Select the thread under the cursor (Esc clears) |
| `U` | Toggle read |
| `Z` | Undo |
| `R` | Reply to the open thread |
| `C` | New message |
| `Ctrl` + `K` or `/` | Search and commands |
| `?` | Keyboard shortcuts |
| `A` | AI summaries instead of previews |
| `[` | Sidebar |
| `Ctrl` + `,` | Settings |
| `7`–`9` | Custom views |
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

## Sign-in setup

Microsoft 365 and Google sign in through the browser (OAuth 2.0 with PKCE;
Sift never sees the password). Each needs an app registration whose client
ID is baked in at build time, or set at run time with the same variable.
Sift then talks to the normal IMAP and SMTP servers with XOAUTH2.

**Microsoft 365 / Outlook.com** (Microsoft Entra admin center → App registrations → New registration):

1. Supported account types: *Accounts in any organizational directory and personal Microsoft accounts*.
2. Platform: *Mobile and desktop applications*, redirect URI `http://127.0.0.1/callback`
   (the port is chosen at run time; Entra ignores it for loopback addresses).
3. API permissions → Add → *APIs my organization uses* → **Office 365 Exchange Online**:
   `IMAP.AccessAsUser.All`, `SMTP.Send`; plus Microsoft Graph `offline_access`, `openid`, `email`, `profile`.
4. Build with the Application (client) ID:

```sh
SIFT_MS_CLIENT_ID=00000000-0000-0000-0000-000000000000 cargo build --release
```

Organisations that only allow approved apps will show Sift's *needs approval*
screen, which offers a ready-made request for the IT team (they approve it under
*Enterprise applications → Admin consent requests*).

**Google** (Google Cloud console → APIs & Services → Credentials):

1. Create an OAuth client ID of type **Desktop app**.
2. On the OAuth consent screen, add the scope `https://mail.google.com/` and your
   address as a test user (Google reviews this scope before public release).
3. Build with both values:

```sh
SIFT_GOOGLE_CLIENT_ID=… SIFT_GOOGLE_CLIENT_SECRET=… cargo build --release
```

Google's "secret" for desktop apps is not confidential; Google requires it anyway.

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
