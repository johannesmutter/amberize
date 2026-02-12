# Amberize — Seal your emails in time

Amberize is a free, open-source desktop app that archives your IMAP emails into a local, tamper-evident SQLite database. It's built for German freelancers, small businesses, and tax advisors who need to meet GoBD email archiving requirements without the complexity or cost of enterprise solutions.

**One file. Fully searchable. Audit-ready.**

**Website**: [amberize.fly.dev](https://amberize.fly.dev/)

## What it does

- Connects to any IMAP account (password or Google OAuth: Gmail or Google Workspace)
- Archives raw email messages into a single portable SQLite file
- Runs silently in the system tray, syncing every few minutes
- Full-text search across all archived emails
- Renders HTML emails safely with images and attachments
- Exports individual `.eml` files or a complete auditor package (ZIP)
- Generates Verfahrensdokumentation (procedural documentation) for tax audits
- Tamper-evident audit trail with hash-chained events and integrity proofs

## Getting started

### Install

Download the latest release from the [Releases](https://github.com/johannesmutter/amberize/releases) page:

- **macOS**: `.dmg` installer — open it and drag Amberize to your Applications folder
- **Windows**: `.msi` installer — run it and follow the setup wizard
- **Linux**: `.deb` package or `.AppImage` — install via your package manager or run directly

### First run

1. **Choose an archive location** — create a new SQLite file or open an existing one.
2. **Add an email account** — enter your IMAP credentials or sign in with Google.
3. **Select folders** to archive (folders marked "GoBD" are recommended for compliance).
4. Amberize begins syncing immediately and continues in the background.

### Google / Gmail

Click **Add Account → Google / Gmail**, enter your Gmail address, and sign in via the browser. Amberize handles the OAuth flow automatically.

On first use, you'll need to enter a Google OAuth Client ID and Secret. You can create one for free at the [Google Cloud Console](https://console.cloud.google.com/apis/credentials) (type: Desktop app, enable the Gmail API). These credentials are stored in your system's credential store, not in the archive.

> Passwords and OAuth tokens never touch the archive database — they live exclusively in your OS credential store (macOS Keychain, Windows Credential Manager, or Linux Secret Service).

## GoBD compliance

Amberize **supports** GoBD-compliant email archiving — it is **not** GoBD-compliant on its own. No software is: the [BMF guidelines](https://www.bundesfinanzministerium.de/Content/DE/Downloads/BMF_Schreiben/Weitere_Steuerthemen/Abgabenordnung/2019-11-28-GoBD.html) (GoBD — Grundsätze zur ordnungsmäßigen Führung und Aufbewahrung von Büchern, Aufzeichnungen und Unterlagen in elektronischer Form) require both technical measures *and* organizational policies. Even commercially certified archiving software carries the disclaimer "bei sachgerechter Anwendung" (when used properly).

> **Important context**: GoBD is a Verwaltungsvorschrift (administrative regulation), not a law. Not following it doesn't mean you broke a law — but it may cause a tax auditor to question your bookkeeping, which can lead to estimated additions to your tax base (Hinzuschätzungen).

### What Amberize handles (technical measures)

| GoBD requirement | How Amberize addresses it |
|---|---|
| **Immutability** (Unveränderbarkeit) | Raw MIME bytes stored once, never modified. SHA-256 hashes verify integrity. Any tampering is detected. |
| **Completeness** (Vollständigkeit) | All messages in selected IMAP folders are archived automatically every few minutes. |
| **Traceability** (Nachvollziehbarkeit) | Hash-chained audit trail records every sync, export, and configuration change. |
| **Machine readability** (Maschinelle Auswertbarkeit) | Full-text search (FTS5). Export to `.eml` or auditor ZIP. |
| **Verfahrensdokumentation** | Auto-generated procedural documentation (German template). |

### What remains your responsibility (organizational measures)

| GoBD requirement | What you need to do |
|---|---|
| **Completeness guarantee** | Amberize archives via IMAP *after delivery*, not at the mail server (no SMTP journaling). This is how all desktop archiving tools work. Emails deleted before the next sync are lost. Set a short sync interval and do not delete tax-relevant emails from your inbox. |
| **Retention periods** (Aufbewahrungsfristen) | GoBD requires 6-year retention for commercial correspondence and 10 years for accounting documents. Amberize does not track or enforce these periods. Keep backups of your archive file for the required duration. |
| **Deletion prevention** | The archive is a regular SQLite file that can be deleted or corrupted. Use backups (ideally on a separate drive) and consider write-protecting the file. |
| **Tax authority access** (Datenzugriff) | GoBD requires three forms of auditor access (Z1/Z2/Z3). Amberize provides an auditor ZIP export (partial Z3). It does not provide interactive auditor login (Z1) or IDEA/GDPdU export. |

### Practical recommendation

For sole proprietors (Einzelunternehmer), freelancers (Freiberufler), and small businesses using Amberize:

1. Keep the default sync interval (15 min) or shorter
2. Don't delete emails from your server before verifying they're in the archive
3. Back up the archive file regularly (external drive, safe, or Steuerberater)
4. Keep a copy of the auto-generated Verfahrensdokumentation alongside the archive
5. Discuss with your Steuerberater whether this setup meets your specific obligations

## Security and privacy

- **Local-first**: no server, no telemetry, no analytics
- **No secrets in the database**: passwords and OAuth tokens live in the OS credential store (Keychain / Credential Manager / Secret Service)
- **Network traffic**: only IMAP connections and optional update checks
- **Remote content blocked**: external images in emails are not loaded by default
- **Read-only IMAP**: uses `BODY.PEEK[]` — never alters server flags or messages
- **HTML sanitization**: DOMPurify + sandboxed iframe with strict CSP

---

## Development

### Requirements

- **macOS**, **Windows**, or **Linux**
- **Rust** stable toolchain
- **Node.js 20+** and npm
- **Linux only**: system libraries — `libgtk-3-dev`, `libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `patchelf`

### Setup

```bash
cd apps/desktop
npm ci
```

### Run in dev mode

```bash
cd apps/desktop
npx tauri dev
```

This starts the Vite dev server and the Tauri backend with hot-reload.

> **Credential store prompts in dev mode**: each rebuild produces a new binary signature, so your OS may prompt for credential access on each restart. This does not happen with signed production builds.

### Run without Tauri CLI

```bash
cd apps/desktop
npm run build          # Build the frontend
cd src-tauri
cargo run              # Run the backend
```

### Tests

```bash
# Rust
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace

# Frontend (19 tests, coverage gates enforced)
cd apps/desktop
npm test -- --coverage
```

### Google OAuth credentials

Amberize supports two ways to provide Google OAuth credentials:

1. **Build-time embedding** (for distribution): set `GOOGLE_OAUTH_CLIENT_ID` and `GOOGLE_OAUTH_CLIENT_SECRET` in `.cargo/config.toml`. They're compiled into the binary via `option_env!` and saved to the credential store on first use. Users get a seamless "Sign in with Google" without needing their own Google Cloud project.

2. **User-provided** (fallback): if no credentials are embedded, users enter their own Client ID and Secret in the app UI. Stored in the credential store.

To set up for development:

1. Create OAuth credentials at [Google Cloud Console](https://console.cloud.google.com/apis/credentials) (type: Desktop app). Enable the Gmail API.
2. Copy `.cargo/config.toml.example` to `.cargo/config.toml` and fill in your values.

`.cargo/config.toml` is gitignored — credentials are never committed.

> **Why is the client secret not really secret?** Google [documents](https://developers.google.com/identity/protocols/oauth2) that native/desktop apps "cannot keep secrets" — the client secret for a Desktop-type OAuth client is not expected to remain confidential. Security relies on PKCE + user consent, not the client secret. This is the same approach used by Thunderbird, which [ships its Google OAuth credentials in public source code](https://searchfox.org/comm-central/source/mailnews/base/src/OAuth2Providers.sys.mjs).

### Architecture

```
apps/desktop/              Tauri v2 desktop application
├── src/                   Svelte 5 frontend (runes, no SvelteKit)
│   ├── components/        UI components
│   ├── lib/               Shared utilities (Tauri bridge, config)
│   └── App.svelte         Root component
└── src-tauri/             Rust backend
    └── src/
        ├── main.rs            Entry point + plugin setup
        ├── app_commands.rs    IPC command handlers
        ├── app_state.rs       Application state
        ├── auditor_export.rs  Compliance ZIP export
        ├── background_sync.rs Periodic sync scheduler
        ├── documentation.rs   Verfahrensdokumentation generator
        └── menubar.rs         Menu bar + system tray

crates/
├── storage/               SQLite schema, queries, integrity, FTS5
└── adapters/              IMAP client, credential secrets, sync engine
```

**Rule**: The UI contains no business logic. It calls Tauri commands and renders results.

### Data model

The archive is a single SQLite file. Key tables:

| Table | Purpose |
|---|---|
| `accounts` | IMAP account config (passwords in credential store, not here) |
| `mailboxes` | Per-account folder state (uidvalidity, cursor, sync status) |
| `message_blobs` | Deduplicated raw MIME + metadata (SHA-256 keyed) |
| `message_locations` | Where each message was seen (account × mailbox × UID) |
| `messages_fts` | FTS5 virtual table for full-text search |
| `events` | Hash-chained audit trail |

### IMAP sync design

- Durable cursors per mailbox (`uidvalidity` + `last_seen_uid`)
- `UID SEARCH` + batched `UID FETCH` with `BODY.PEEK[]` (read-only)
- `UIDVALIDITY` change triggers full re-scan
- Ingestion is atomic: blob + location in a single transaction
- Default cadence: every 5 minutes (configurable)

### Adding a new Tauri command

1. Define the function in `app_commands.rs`
2. Register it in `main.rs` → `tauri::generate_handler![...]`
3. Add permission to `src-tauri/capabilities/default.json` if needed
4. Call from frontend via `tauri_invoke('command_name', { args })`

### Building for distribution

```bash
cd apps/desktop

# macOS — Debug .app bundle
CI=false npx tauri build --debug --bundles app

# macOS — Release DMG installer
CI=false npx tauri build --bundles dmg

# Windows — MSI installer
npx tauri build --bundles msi

# Linux — .deb and .AppImage
npx tauri build --bundles deb,appimage
```

> The `CI=false` prefix may be needed when building from Cursor's integrated terminal on macOS.

### Release and auto-update verification

For the full end-to-end release checklist (key setup, draft release flow, DMG install smoke test, and updater verification), see:

- `docs/release-checklist.md`

### Code signing

#### macOS

For distribution, set these environment variables before building:

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
export APPLE_ID="your@apple.id"
export APPLE_PASSWORD="app-specific-password"
export APPLE_TEAM_ID="TEAMID"
```

See [Tauri's macOS distribution guide](https://v2.tauri.app/distribute/sign/macos/).

#### Windows

Windows builds can be signed with an EV or standard code signing certificate. See [Tauri's Windows distribution guide](https://v2.tauri.app/distribute/sign/windows/).

### Conventions

| Context | Convention | Example |
|---|---|---|
| JS variables & functions | `snake_case` | `load_messages` |
| Svelte components | `PascalCase` | `MainDashboard.svelte` |
| CSS classes | `kebab-case` | `.message-list-item` |
| Rust functions | `snake_case` | `get_message_detail` |
| Rust types | `PascalCase` | `UiMessageDetail` |

### CI pipeline

GitHub Actions (`.github/workflows/ci.yml`) runs on every push:

- **Rust**: fmt → clippy → tests on macOS, Windows, and Linux
- **Frontend**: npm ci → tests with coverage → vite build

All GitHub Actions are pinned to commit SHAs for supply chain security.

---

## License

MIT
