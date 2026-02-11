# Amberize — Project Plan

Working tasks, milestones, and roadmap. Architecture and developer docs live in `README.md`.

---

## Milestone status

| Milestone | Scope | Status |
|---|---|---|
| **A — Archive foundation** | Schema, integrity verifier, event chain, proof snapshots | Done |
| **B — IMAP ingest** | Accounts, Keychain, mailbox discovery, folder selection, durable cursors | Done |
| **C — Search and view** | FTS5, main window, HTML rendering, inline images, attachments | Done |
| **D — Export + polish** | `.eml` export, auditor ZIP, Verfahrensdokumentation, menu bar, launch-at-login | Done |
| **E — Security hardening** | Path traversal, IMAP injection, XSS, CSP, mutex safety, input validation | Done |
| **F — OAuth** | Gmail OAuth2/XOAUTH2 (authorization code + PKCE) | Done |
| **G — Distribution** | DMG packaging, code signing, notarization, release automation | Open |
| **H — Landing page + docs** | SvelteKit static landing page, README rewrite, GoBD honesty audit | Done |

---

## Open tasks (priority order)

### 1. Distribution (Milestone G)

- [ ] Code signing with Apple Developer ID
- [ ] Notarization for Gatekeeper
- [ ] Test DMG installation end-to-end on a clean macOS machine
- [x] Update `tauri.conf.json` with real updater public key and GitHub repo URL
- [ ] Verify auto-update flow: tag → release workflow → draft release → published → app detects update
- [x] Landing page: replace `OWNER/REPO` placeholders with actual GitHub URLs

### 2. GoBD compliance gaps

Critical analysis (Feb 2026): the app covers the core technical requirements (Unveränderbarkeit, Nachvollziehbarkeit, Maschinelle Auswertbarkeit, Verfahrensdokumentation) but has gaps that are now **transparently documented** in the README and landing page. Key remaining work:

**Technical improvements (open):**
- [x] **Archive integrity check on launch**: verify hash chain on startup and warn if tampered
- [x] **Retention period tracking**: add optional retention period display in UI (informational, not enforcement)

**Will NOT implement (by design):**
- SMTP journal archiving (requires mail server access, out of scope for a desktop client)
- WORM storage (not possible on a single-user macOS desktop)
- Automatic retention period enforcement with deletion (too risky for a local-only tool)

### 3. Sync robustness

- [ ] Handle IMAP connection drops mid-sync gracefully (retry with backoff)

### 4. Testing

| Area | Status |
|---|---|
| UI component tests (Vitest) | 19 tests, coverage gates enforced |
| Rust unit tests (storage, adapters) | 33 tests, coverage informational |
| Rust coverage gates | Open — define scope boundaries and enforce |
| IMAP integration tests (mock server) | Open |
| End-to-end tests | Open |

### 5. Microsoft 365 / Outlook OAuth

- [ ] Implement MSAL OAuth2 flow for Microsoft 365
- [ ] XOAUTH2 SASL for Outlook IMAP
- [ ] Test with personal and organizational Microsoft accounts

### 6. UX polish

- [x] Folder-picker UX: warn if user selects iCloud/Dropbox folder (sync corruption risk)
- [x] Move archive path from localStorage to OS-level app config
- [x] Keyboard shortcuts for common actions (search focus, sync, settings)
- [x] Empty state illustrations in dashboard, preview, and settings

### 7. CI/CD improvements

- [x] GitHub Actions pinned to commit SHAs
- [x] Concurrency groups to cancel stale runs
- [x] Cargo cache for faster builds
- [x] Dependency audit (cargo audit)
- [ ] Add `npm audit` step to UI job
- [ ] Dependabot or Renovate for automated dependency updates
- [ ] Branch protection rules: require CI pass + 1 review before merge
- [ ] Add a GitHub issue template and PR template

### 8. Landing page polish

- [ ] Add app screenshot or animated GIF to hero section
- [ ] Add Open Graph / Twitter Card meta tags for social sharing
- [ ] Deploy to GitHub Pages (add workflow)
- [ ] Custom domain setup (if applicable)

---

## Authentication roadmap

| Provider | Method | Status |
|---|---|---|
| Any IMAP | Password / app-password | Done |
| Google / Gmail | OAuth2 + XOAUTH2 SASL | Done |
| Microsoft 365 / Outlook | OAuth2 + XOAUTH2 SASL | Open |

---

## Architecture notes

### Crate responsibilities

| Crate | Responsibility |
|---|---|
| `crates/storage` | SQLite schema, queries, integrity hashing, proof generation, audit chain |
| `crates/adapters` | IMAP client, sync orchestration, Keychain secrets, OAuth token management |
| `apps/desktop/src-tauri` | Tauri commands, app state, background sync, menubar, documentation, auditor export |
| `apps/desktop/src` | Svelte 5 UI (runes), Tauri bridge, config |
| `apps/landing` | SvelteKit static landing page |

### Design principles (summary)

1. **Simplicity gate** — add UI only if the user can't complete the job without it
2. **Correctness over convenience** — archive too much rather than miss an email
3. **No secrets in the database** — passwords and tokens live in the OS credential store
4. **Local-first privacy** — no server, no telemetry
5. **Tamper-evidence, not tamper-proof** — detection over prevention on a single-user machine
