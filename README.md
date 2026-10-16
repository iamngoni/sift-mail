# Sift Mail

A calm, fast, keyboard-first desktop email client for Windows, macOS and Linux,
built in Rust on [GPUI Kit](https://gpui-kit.com) (Zed's GPUI plus Longbridge's
component library).

> **Status: feature-complete preview.** Not yet code-signed or tested against
> every provider in the wild.

- **Onboarding**: a short welcome that connects your first account and sets up
  views, AI helpers, notifications, theme and keyboard habits.
- **Accounts**: Microsoft 365 or Google in your browser (see
  [Sign-in setup](#sign-in-setup)), or a password for iCloud, Fastmail or any
  IMAP server with editable server settings. Name, colour, sync range, AI and
  notifications per account; passwords and tokens live in the OS credential store.
- **Inbox**: mail sorted into Needs you, Meetings, Updates, Newsletters and
  Other; custom views you describe in words; All inboxes or one account; a
  sidebar; day sections; Done, Snooze, read/unread and Undo from the keyboard;
  bulk selection; full-text search and commands in one palette (`Ctrl K`).
- **Reading and writing**: threads with collapsed history, attachments that open
  in your usual apps, replies and new mail over SMTP with proper threading and a
  Sent copy, recipient suggestions, and drafts kept when you close them.
- **AI** through the Claude Code or Codex you already use (no API keys):
  summaries, to-dos, reply drafts, tone rewrites, Ask your inbox, Write it.
- **Calendar** from standard invitations: RSVP from the inbox, a week view,
  quick events, Find a time, and invites sent from your own account.
- **Focus**: hold notifications so only Needs you interrupts, with check-ins.

Try it with sample mail:

```sh
SIFT_MAIL_DB=/tmp/sift-demo.db cargo run --features demo
```

## Fonts

The interface uses [Geist and Geist Mono](https://github.com/vercel/geist-font),
bundled in `assets/fonts` under the SIL Open Font License (`assets/fonts/OFL.txt`).

## App icon

The icon is drawn in `assets/app-icon/sift-mail.svg` (macOS grid) and
`sift-mail-square.svg` (Windows and Linux). `tools/make-icons.ps1` renders them
with Microsoft Edge and writes the PNG sizes, `SiftMail.ico` and `SiftMail.icns`;
rerun it after editing either SVG. `build.rs` puts the `.ico` into `SiftMail.exe`,
and cargo-packager puts the rest into the installers.

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
  ../build.rs    puts the app icon into SiftMail.exe on Windows
  lib.rs         everything testable
  assets.rs      extra Lucide icons embedded on top of GPUI Kit's set
  brand.rs       product naming
  theme/
    color.rs     validated #RRGGBB colours, WCAG contrast
    tokens.rs    the design tokens from the style spec (single source of truth)
    mod.rs       turns tokens into the GPUI Kit theme; light, dark, match system
  domain/
    ical.rs      reading invitations and writing iMIP requests and replies
    schedule.rs  weeks, free time, and reading "find a time" requests
    choices.rs   onboarding choices: sort mode, views, AI helpers, notifications
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
    events.rs    the calendar: invitations by UID and SEQUENCE, events made in Sift
    memo.rs      cached AI answers and contact suggestions
    demo.rs      sample mail, only with --features demo and in tests
  ai/
    mod.rs       the Assistant trait; Claude Code and Codex as subprocesses
    prompts.rs   prompts and tolerant JSON parsing of answers
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
    freebusy.rs  other people's busy times from Microsoft Graph and Google Calendar
  ui/
    mod.rs       window options, opening the store and the main window
    app.rs       inbox state and actions, keyboard handling
    chrome.rs    sidebar, account filter, palette, dialogs, bulk selection, preferences
    chrome_view.rs  how those look
    accounts.rs  background sync and status
    add_account.rs  the four-step Add account flow: choose, sign in, set up, connect
    add_account_view.rs  how that flow looks
    onboarding.rs  first-run onboarding: welcome, six choices, summary
    onboarding_view.rs  how onboarding looks
    calendar.rs  the Calendar and answering invitations
    calendar_view.rs  the week grid, popovers and side panel
    notify.rs    desktop notifications that respect Focus and check-ins
    settings.rs  Settings: accounts (name, colour, send-as, signature, sync, AI, notifications,
                 connection, removal), default From, appearance, notifications, keyboard
    settings_view.rs  how Settings looks
    compose.rs   reply box and sending
    compose_form.rs  New message: recipient chips, suggestions, Write it, drafts
    ai_ui.rs     AI state: summaries, drafts, rewrites, Ask, Write it
    ai_view.rs   how AI shows up (sparkles, summary card, draft chips)
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
   `IMAP.AccessAsUser.All`, `SMTP.Send`; plus Microsoft Graph `offline_access`, `openid`, `email`, `profile`
   and `Calendars.Read.Shared` (for Find a time).
4. Build with the Application (client) ID:

```sh
SIFT_MS_CLIENT_ID=00000000-0000-0000-0000-000000000000 cargo build --release
```

Organisations that only allow approved apps will show Sift's *needs approval*
screen, which offers a ready-made request for the IT team (they approve it under
*Enterprise applications → Admin consent requests*).

**Google** (Google Cloud console → APIs & Services → Credentials):

1. Create an OAuth client ID of type **Desktop app**.
2. Enable the Google Calendar API. On the OAuth consent screen, add the scopes
   `https://mail.google.com/` and `https://www.googleapis.com/auth/calendar.freebusy`, and your
   address as a test user (Google reviews this scope before public release).
3. Build with both values:

```sh
SIFT_GOOGLE_CLIENT_ID=… SIFT_GOOGLE_CLIENT_SECRET=… cargo build --release
```

Google's "secret" for desktop apps is not confidential; Google requires it anyway.

## Calendar

The calendar is built from standard meeting invitations (iCalendar/iMIP), so it
works with every provider: invites that arrive by mail appear on the calendar,
Accept / Maybe / Decline send a proper reply to the organizer from your own
account, and invites you send go out the same way. Open it from the calendar
button in the title bar. Click an empty slot to add an event; Find a time reads
requests like "30 min with Priya next week" and offers free times straight away
from your own calendar, then asks your provider when the other people are busy
(Microsoft Graph `getSchedule` for Microsoft 365, the Calendar `freeBusy` API for
Google; busy blocks only, never titles) and re-ranks to times you're both free.
Password accounts can't see other calendars, and Find a time says so. Hold a
time, then send it as an invite. Keys: `←` `→` weeks, `T` today, `N` new event, `Esc`.

Set `SIFT_OPEN=calendar` (or `settings`) to start on that screen.

## AI

Sift has no API keys and no AI sign-in of its own. It runs the assistant you
already use, signed in with your own subscription:

- **Claude Code**: install it, run `claude` once and sign in.
- **Codex**: `npm install -g @openai/codex`, run `codex` once and sign in.

Pick one under Settings → AI. Each request runs the CLI non-interactively in an
empty temporary folder with its tools turned off, sends the prompt on standard
input, and stops after 90 seconds. AI only reads a thread when you use a
feature on it, only for accounts with AI on, and answers are cached locally so
nothing is asked twice. Features: one-line summaries in the list (`A`), a
summary card with to-dos in the reader, reply drafts and Shorter / Friendlier /
More formal rewrites, Ask your inbox (end a palette search with `?`), and Write
it in New message.

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
