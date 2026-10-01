# Amberize codebase review — 2026-09-30

Reviewed commit: `ee9bccae5d464e3a1a4e6ec1b89d0716e300cb79`. Scope: desktop Svelte/Tauri application, Rust storage and IMAP/OAuth adapters, background scheduling, exports, credential storage, release tooling, and landing application. Application source was left unchanged. Reproductions used an isolated checkout and synthetic messages/databases.

Release/packaging files changed concurrently during this review: `.github/workflows/release.yml`, desktop `package.json`/`package-lock.json`, and `docs/release-checklist.md`. Those edits were preserved. Runtime findings concern unchanged application logic; Rust reproductions use the reviewed commit, while web builds used the working checkout. The report is the only repository file authored by this review.

The reported restart and archive-location problems have credible causes in the code. Configuration failures are converted into first-run setup, failed saves are hidden, a missing archive is silently recreated, and Windows/Linux credentials use a nonpersistent mock store. macOS login registration also becomes stale if the application moves after registration. These are distinct failure paths; the review does not establish which one occurred on a particular user's machine.

There are additional urgent problems: selected-email export includes the entire archive, exports can truncate the archive itself, oversized messages are silently skipped, and routine integrity checks can both miss changed email contents and falsely accuse an interrupted sync of tampering.

## Priority and evidence

P1 means fix before the next broadly distributed release because of data loss, unintended disclosure, archive incompleteness, or a substantial availability failure. P2 means a significant correctness, recovery, or usability defect. P3 means lower-impact maintenance work. “Reproduced” means a synthetic test exercised the behavior. “Source-confirmed” means the relevant control flow was inspected; native OS behavior and live provider interactions were not exercised.

| Reported symptom | Relevant findings | What the code does |
| --- | --- | --- |
| Asked to choose the archive again after restarting | R01, R02 | Hides failed persistence; treats unreadable/corrupt configuration and failed archive reads as initial setup. |
| Archive appears empty after moving/disconnecting its storage | R02 | Creates a new empty database at the remembered path when that path is writable. |
| Worked before quitting; no longer syncs afterward on Windows/Linux | R03, R06 | Credentials were cached in memory over a mock store; subsequent failures can appear as successful synchronization. |
| Does not launch after login | R04, R05 | Registration can reference an obsolete executable path; plugin errors are not safely handled. Actual login behavior requires OS testing. |
| Background activity stops or remains “Syncing” | R08 | Network operations have no application deadlines while the global synchronization lock remains held. |
| Resource use persists with the window closed | R19, performance plan | Native close hides the webview; sync events reload hidden state. |

## Startup, persistence, and recovery

### R01 — [P1] Configuration failures masquerade as successful setup or first run

Locations: [App.svelte:317](/Users/johannes/GIT/email-archiver/apps/desktop/src/App.svelte:317), [config.js:13](/Users/johannes/GIT/email-archiver/apps/desktop/src/lib/config.js:13), [app_commands.rs:1748](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/app_commands.rs:1748), [app_commands.rs:1766](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/app_commands.rs:1766).

`handle_archive_selected()` ignores `save_config()` failures and opens the dashboard. Native configuration saves have no localStorage fallback, so a disk-full or permission error leaves an apparently working session whose chosen archive is forgotten at restart. Conversely, native configuration read errors become `null`, leading to initial setup or an older localStorage fallback. `config.json` is overwritten directly with `std::fs::write`; interruption can leave it truncated, empty, or invalid.

Evidence: reproduced failed native reads becoming no configuration, and a rejected native save opening the dashboard with no persisted configuration and no visible error. The non-atomic write is source-confirmed.

Repair: write configuration atomically in the app-data directory, with a recoverable previous version and appropriate durability; distinguish missing, malformed, inaccessible, and unsupported configuration. Confirm successful persistence before completing archive selection. Preserve the last known archive path and diagnostic cause during recovery. Test process interruption during configuration replacement and a real native save/load round trip across processes.

### R02 — [P1] Restoring an archive can create an empty replacement

Locations: [Storage::open_or_create:66](/Users/johannes/GIT/email-archiver/crates/storage/src/lib.rs:66), [open_connection:1470](/Users/johannes/GIT/email-archiver/crates/storage/src/lib.rs:1470), [list_accounts:506](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/app_commands.rs:506), [App.svelte:374](/Users/johannes/GIT/email-archiver/apps/desktop/src/App.svelte:374).

The UI's “DB exists” check calls `list_accounts`, which opens or creates the database. `SQLITE_OPEN_CREATE` and automatic parent-directory creation make a disappeared archive look like a valid empty archive whenever its remembered path is writable. An archive error instead returns the user to the same archive chooser used on first run. “Open Existing” and “Create New” ultimately pass the same path-only callback and do not enforce different backend opening modes.

Evidence: removed a synthetic archive after creating an account; reopening through the storage API recreated its file and returned zero accounts. This does not overwrite a moved original, but conceals its absence and creates a misleading replacement.

Repair: expose separate create-new and open-existing APIs. Restoration and read commands must never create a database. Validate file identity/header/schema/access once in backend bootstrap. Show the saved path with “Retry”, “Locate archive”, and an explanation appropriate to missing storage, permissions, corruption, or locking. Test moved files, unavailable volumes, read-only directories, malformed SQLite, and unsupported schema versions.

### R03 — [P1] Windows/Linux credentials disappear across application restarts

Locations: [adapters/Cargo.toml:9](/Users/johannes/GIT/email-archiver/crates/adapters/Cargo.toml:9), [secrets.rs:49](/Users/johannes/GIT/email-archiver/crates/adapters/src/secrets.rs:49), [app_commands.rs:593](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/app_commands.rs:593).

The only keyring feature is `apple-native`. Keyring 3.6.3 has no default credential-store features and uses its mock store when none apply to the target. Windows/Linux therefore do not use the persistent credential stores promised in the README. The process-wide `SECRET_CACHE` hides this until exit; the immediate password verification also reads that cache. This affects passwords and OAuth configuration/tokens. [Keyring 3.6.3 documentation](https://docs.rs/keyring/3.6.3/keyring/#credential-store-features).

Evidence: source-confirmed against the locked keyring version and its platform-selection implementation. No user's credential store was accessed, and Windows/Linux binaries were not run.

Repair: use target-specific persistent credential backends, including Windows Credential Manager and a persistent Linux Secret Service option; surface unavailable/locked stores without substituting mocks. Verify durability using separate writer and reader processes on each supported platform. Retain cached credentials only under a deliberate lifecycle policy, and distinguish “missing credential” from “temporarily locked credential store”.

### R04 — [P1] Launch-at-login can remain enabled while referencing a missing executable

Locations: [menubar.rs:75](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/menubar.rs:75), [menubar.rs:309](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/menubar.rs:309). Dependency evidence: locked `tauri-plugin-autostart` 2.5.1 and `auto-launch` 0.5.0 source.

The plugin captures the current executable path. The macOS LaunchAgent stores that path in `ProgramArguments`; `is_enabled()` checks whether the plist exists, rather than whether its executable path matches the current installation. Amberize does not reconcile it after the application moves. Enabling login launch from a DMG, temporary directory, or older installation and then removing that location leaves the checkbox enabled while login launch points to a missing executable. A move or path change is the trigger; an in-place replacement at the same path does not by itself cause this failure.

Evidence: source-confirmed registration/read semantics. This is a concrete failure scenario supported by the code, not a reproduced macOS logout/login experiment. The plugin path capture is visible in its [versioned source](https://raw.githubusercontent.com/tauri-apps/plugins-workspace/autostart-v2.5.1/plugins/autostart/src/lib.rs).

Repair: reconcile an already-enabled registration with the current stable installation path when the application starts. Report enabled, disabled, unavailable, and stale-registration states separately. Test installation from DMG, relocation, update, login, and opening from a second installation. Keep launch-at-login opt-in, but explain its relationship to background archiving during setup.

### R05 — [P2] An optional autostart initialization error can abort application startup

Locations: [menubar.rs:75](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/menubar.rs:75), [menubar.rs:184](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/menubar.rs:184), [menubar.rs:309](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/menubar.rs:309).

`setup_autostart_plugin()` discards plugin initialization errors. Menu construction then calls `autolaunch()`, which uses Tauri's panicking managed-state getter. If initialization failed before installing `AutoLaunchManager`—for example, during executable-path resolution—the supposed graceful fallback can become a startup panic. Other registration/query errors are turned into false/None, hiding the actual cause. [Autostart 2.5.1 implementation](https://raw.githubusercontent.com/tauri-apps/plugins-workspace/autostart-v2.5.1/plugins/autostart/src/lib.rs).

Evidence: source-confirmed; failure was not injected into a native application instance.

Repair: retain initialization results and use a non-panicking state lookup. Allow archive viewing and ordinary synchronization to continue with a clear “Launch at login unavailable” state. Add a startup test with plugin initialization deliberately rejected.

### R06 — [P2] Background failures are rendered as successful synchronization

Locations: [background_sync.rs:326](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/background_sync.rs:326), [MainDashboard.svelte:291](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/MainDashboard.svelte:291), [StatusBar.svelte:74](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/StatusBar.svelte:74), [App.svelte:269](/Users/johannes/GIT/email-archiver/apps/desktop/src/App.svelte:269).

The backend sets `last_sync_at` for failed/partial attempts and records their status, but `load_sync_status()` ignores `last_sync_status`. The status bar renders “Synced just now” with its success indicator when no separate frontend manual-sync error exists. Background account errors lose their detailed causes, and native-tray/initial-account sync handlers discard results. The timestamp is taken before the run, so a long run also reports the attempt's start as its completion. Restart resets the displayed last-sync state rather than restoring durable history.

Evidence: reproduced a backend status containing a credential failure being shown as “Synced just now” with no error text.

Repair: expose a typed status with last attempt, last successful completion, per-account/mailbox failures, and partial coverage. Drive tray, dashboard, and account settings from that same state. Preserve useful failure details and an appropriate action such as unlocking credentials, reconnecting an account, or retrying offline work. Test background, tray, automatic first-sync, and partial-success paths.

### R07 — [P2] The configured sync interval is ignored after restart until Settings opens

Locations: [GeneralSettings.svelte:25](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/GeneralSettings.svelte:25), [GeneralSettings.svelte:42](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/GeneralSettings.svelte:42), [app_state.rs:10](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/app_state.rs:10), [app_commands.rs:689](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/app_commands.rs:689).

Only the General Settings component loads the saved interval from localStorage and sends it to Rust. Every backend restart uses five minutes until that component mounts. A user selecting one minute gets poorer coverage; a user selecting several hours gets substantially more network and disk activity than expected. Updating the interval does not interrupt the current sleep.

Evidence: source-confirmed initialization and persistence paths.

Repair: persist the interval with backend-owned configuration and restore it before starting the scheduler. Notify the scheduler when it changes. Test saved short/long intervals across process restarts without visiting Settings.

### R08 — [P1] A stalled IMAP operation can stop synchronization for every account

Locations: [imap.rs:99](/Users/johannes/GIT/email-archiver/crates/adapters/src/imap.rs:99), [imap.rs:130](/Users/johannes/GIT/email-archiver/crates/adapters/src/imap.rs:130), [imap.rs:169](/Users/johannes/GIT/email-archiver/crates/adapters/src/imap.rs:169), [background_sync.rs:344](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/background_sync.rs:344).

TCP/TLS, greeting, authentication, mailbox listing/selecting, and fetches have no application timeout/cancellation boundary. A server that keeps a connection open without completing a response can retain the global sync lock indefinitely. Later accounts, subsequent background runs, and manual synchronization then wait behind that operation.

Evidence: source-confirmed absence of deadlines in the IMAP adapter and lock ownership in the callers. A live provider stall was not induced.

Repair: use bounded connection/authentication deadlines and inactivity deadlines for streaming fetches, with account-level cancellation and safe cursor persistence. Release the synchronization guard and reset status on every exit path. Apply bounded backoff with jitter after transient failures. Test a synthetic server that stalls at each protocol phase and verify other accounts still progress.

## Completeness, integrity, and exports

### R09 — [P1] Oversized messages are skipped permanently and the size check does not bound allocation

Locations: [sync.rs:263](/Users/johannes/GIT/email-archiver/crates/adapters/src/sync.rs:263), [sync.rs:378](/Users/johannes/GIT/email-archiver/crates/adapters/src/sync.rs:378).

Messages over 50 MiB are skipped after the full body has been received and copied with `to_vec()`. The cursor nevertheless advances over their UID. No durable skip event or mailbox failure is recorded, and a later successful fetch can complete the mailbox normally. Subsequent incremental runs never revisit that message. A mailbox containing only oversized messages may hit the separate zero-fetched error; the ordinary mixed-message case still silently loses coverage. Large literals can already consume substantial memory before the rejection.

Evidence: source-confirmed in both fetch paths.

Repair: inspect `RFC822.SIZE` before body retrieval, use bounded fetch/spooling, and persist any deliberately unarchived UID with a visible reason and retry policy. Avoid advancing a coverage cursor past unresolved holes. Test a mailbox with a small message, an oversized message, and a later small message, including restart/resume.

### R10 — [P1] Routine “full” integrity verification does not check email contents

Locations: [verify_integrity:1207](/Users/johannes/GIT/email-archiver/crates/storage/src/lib.rs:1207), [compute_message_blobs_root_hash:2341](/Users/johannes/GIT/email-archiver/crates/storage/src/lib.rs:2341), [verify_message_blobs_integrity:1145](/Users/johannes/GIT/email-archiver/crates/storage/src/lib.rs:1145).

Startup and periodic full checks validate event hashes and a root of stored `sha256` values. They do not recompute those values from `raw_mime`. Changing stored email bytes while leaving their stored hash untouched passes these checks. The existing raw-byte verifier catches the change, but the application only invokes it during auditor export. Delete triggers do not prevent updates, and routine verification does not validate trigger/schema presence.

Evidence: changed a synthetic message's raw bytes using SQL; `verify_integrity().ok` remained true while `verify_message_blobs_integrity()` reported a mismatch.

Repair: define and expose separately what has been verified: content, event chain, checkpoint consistency, and schema guards. Schedule bounded content verification and latch detected failures. Validate new messages during ingestion, but retain periodic verification of persisted bytes. Do not present a metadata-only check as complete content integrity.

### R11 — [P2] A legitimate interrupted sync is reported as tampering

Locations: [ingest_message:466](/Users/johannes/GIT/email-archiver/crates/storage/src/lib.rs:466), [create_sync_finished_event:910](/Users/johannes/GIT/email-archiver/crates/storage/src/lib.rs:910), [verify_integrity:1230](/Users/johannes/GIT/email-archiver/crates/storage/src/lib.rs:1230), [background_sync.rs:178](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/background_sync.rs:178).

Each message ingestion commits its blob, location, and valid event. The archive root checkpoint is written at account-sync completion. If the process exits after ingestion but before that completion, the next startup compares legitimate new state with the previous checkpoint and labels the mismatch as tampering.

Evidence: created a clean checkpoint, ingested a message through the public atomic ingestion API, omitted the final checkpoint as a crash would, and verified that the chain passed while root verification and overall status failed.

Repair: distinguish a verifiable incomplete ingestion tail from inconsistent/tampered data. Make checkpoint/recovery semantics match the transaction boundaries, and retain an explicit interrupted-sync state. Test forced process exits before/after ingestion and checkpoint commits.

### R12 — [P1] A quick integrity check can clear a known event-chain failure

Locations: [verify_root_hash_only:1259](/Users/johannes/GIT/email-archiver/crates/storage/src/lib.rs:1259), [background_sync.rs:465](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/background_sync.rs:465), [App.svelte:386](/Users/johannes/GIT/email-archiver/apps/desktop/src/App.svelte:386).

Quick verification sets `chain_ok = true` even though it never checks the chain. Its result replaces the last full integrity status. A previously detected broken chain can therefore become an overall success on the next quick cycle. The frontend also has no dedicated integrity-status event subscription, so periodic discoveries do not reliably update the visible warning.

Evidence: changed a synthetic event's detail; full verification failed, then root-only verification returned success. The state overwrite and missing UI subscription are source-confirmed.

Repair: represent unchecked scopes explicitly and preserve the most recent verified result per scope, including time and failure state. A weaker check must not clear a stronger failure. Emit integrity updates to all relevant views, and test full-failure → quick-check → UI notification.

### R13 — [P1] “Export selected emails” exports every email in the archive

Locations: [MainDashboard.svelte:627](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/MainDashboard.svelte:627), [auditor_export.rs:102](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/auditor_export.rs:102).

Selection only determines whether the export button is enabled. The command receives no selected IDs and iterates every message blob. Selecting one message, including under an account/date/search filter, can disclose unrelated correspondence and accounts in the resulting ZIP.

Evidence: selected one synthetic message in the dashboard and confirmed that the whole-archive command was invoked with only the database and output path. The backend's complete-archive iteration is source-confirmed.

Repair: implement a selected-message export API whose backend validates and enforces the selected blob IDs. Define which related metadata/proof is included without leaking unrelated message bodies. Keep full auditor export a separately labelled operation. Verify ZIP contents with multiple accounts and a one-message selection.

### R14 — [P1] Export destinations can overwrite the active archive

Locations: [auditor_export.rs:77](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/auditor_export.rs:77), [export_message_blob_eml:996](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/app_commands.rs:996), [export_events_csv:1321](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/app_commands.rs:1321), [validate_output_path:1248](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/app_commands.rs:1248).

Output validation does not reject the database's own path or an alias to it. `File::create`/`std::fs::write` truncate existing files. ZIP export creates its output before reading the final blob list; when output and archive are the same file, it corrupts the database and then fails. Dialog extensions reduce accidental direct selection but cannot provide backend protection against path aliases or other command callers.

Evidence: exported a disposable synthetic database to its own path. The command returned an error after its SQLite header had been destroyed.

Repair: reject destinations sharing file identity with the active DB or its WAL/SHM files, including symlink/hardlink aliases. Write exports into a separate temporary file and atomically publish only after successful completion. Test same-path, aliased-path, disk-full, cancellation, and export failure.

### R15 — [P2] Auditor exports do not represent one consistent archive snapshot

Locations: [auditor_export.rs:57](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/auditor_export.rs:57), [create_proof_snapshot:1034](/Users/johannes/GIT/email-archiver/crates/storage/src/lib.rs:1034).

Proof, chain verification, raw integrity, index, events, and message blobs are read in separate operations/connections while synchronization can continue. They can describe different archive states. The event CSV exporter also uses moving OFFSET pages while events can be appended, causing overlap or omission.

Evidence: source-confirmed lack of a shared read snapshot or export/sync exclusion. Concurrent live export was not run.

Repair: obtain one SQLite snapshot, using a stable read transaction or online backup into an export snapshot, and stream every export component from it. Bound snapshot lifetime to avoid unchecked WAL growth. Test concurrent ingestion and verify proof counts, event tail, index, and EML membership agree.

### R25 — [P2] Partial sync errors lose the effective UIDVALIDITY reset

Locations: [sync.rs:193](/Users/johannes/GIT/email-archiver/crates/adapters/src/sync.rs:193), [sync.rs:142](/Users/johannes/GIT/email-archiver/crates/adapters/src/sync.rs:142).

When UIDVALIDITY changes, `sync_mailbox()` resets its local cursor to zero. A failure returns only an error string and partial maximum UID. The caller persists the previous UIDVALIDITY and takes the maximum with the old cursor, undoing the generation reset. Subsequent failing attempts can repeatedly refetch the new generation rather than resume safely.

Evidence: source-confirmed error-path state mismatch; no claim is made about actual server response ordering or permanent loss from this particular case.

Repair: carry the effective UIDVALIDITY and verified progress in a typed sync outcome, preserving generation resets on success, error, timeout, and cancellation. Test a reset to lower UIDs followed by failure and resume.

## Browsing, account setup, and window lifecycle

### R16 — [P2] Date filters can make existing emails inaccessible; attachment filters do nothing

Locations: [MainDashboard.svelte:369](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/MainDashboard.svelte:369), [MainDashboard.svelte:387](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/MainDashboard.svelte:387), [MainDashboard.svelte:925](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/MainDashboard.svelte:925).

Date filtering happens after backend pagination. If the first 100 newest rows do not match last year, the filtered list becomes empty and the VirtualList is removed, eliminating the scroll trigger that would fetch older matching pages. The UI says “No archived emails yet” and suggests synchronization. Attachment filtering is entirely commented out, despite an active dropdown.

Evidence: reproduced an empty last-year result with older matching rows available on the next mocked backend page; no later page was requested. Also reproduced unchanged results after choosing “Has attachments”.

Repair: push date and attachment predicates into the backend query before pagination, with indexed attachment metadata. Distinguish an empty archive, no filter matches, and a failed query. Test matching rows beyond page one and combined account/search/date/attachment predicates.

### R17 — [P2] The 500-message memory cap removes previously browsed pages

Locations: [MainDashboard.svelte:401](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/MainDashboard.svelte:401), [VirtualList.svelte:74](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/VirtualList.svelte:74).

Once more than 500 rows have been fetched, the oldest cached pages are discarded with `slice()`. VirtualList receives only that shortened array; it has no absolute page origin or backward fetching. Earlier rows can no longer be reached by scrolling back. Selection-view rows also disappear when their pages leave the cache, and changes to array length/order can shift the user's scroll position.

Evidence: source-confirmed cache eviction and one-directional paging design.

Repair: keep a bounded bidirectional page cache with stable absolute positions, keyset cursors, and preserved scroll anchors. Track selection independently of loaded row objects. Test browsing beyond 1,000 messages, scrolling back, and reopening a selection whose pages were evicted.

### R18 — [P2] A stale message-detail response replaces the current selection

Location: [MainDashboard.svelte:537](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/MainDashboard.svelte:537).

Message-detail requests have no generation guard. Clicking A then B can leave B selected while A's slower response replaces the preview. Export uses the selected detail, so the displayed/exported message can disagree with the highlighted row. A pending response can also repopulate preview memory after the window is hidden.

Evidence: resolved B's synthetic detail first and A's second; A was displayed while B stayed highlighted.

Repair: accept details only if the request still matches the selected blob and current window/view generation. Invalidate requests on hide, archive change, and unmount. Test response reordering, failed prior requests, and hide during detail loading.

### R19 — [P2] Background events repopulate the hidden webview

Locations: [MainDashboard.svelte:98](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/MainDashboard.svelte:98), [MainDashboard.svelte:148](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/MainDashboard.svelte:148), [menubar.rs:287](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/menubar.rs:287).

Close-to-tray clears list/selection/preview state, but the next `sync_status_updated` handler unconditionally reloads messages and archive stats. The hidden webview is retained, and per-message progress continues to cross IPC and update reactive state. Every sync start/completion also resets visible lists and previews, even without relevant new messages.

Evidence: emitted the native hide event in the dashboard test, observed the rows disappear, then emitted a sync-status event and observed them reload while still hidden.

Repair: track native visibility explicitly and mark hidden data dirty without loading it. Suppress detail/list/progress work until the window is shown, then refresh once while preserving view state. Coalesce visible progress updates. For larger memory savings, make backend bootstrap independent of the webview, then destroy/recreate the webview on hide/show using a validated lifecycle.

### R20 — [P2] Removing an account hides its previously archived correspondence

Locations: [remove_account:1024](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/app_commands.rs:1024), [list query:739](/Users/johannes/GIT/email-archiver/crates/storage/src/lib.rs:739).

Removal disables the account and retains its data, but normal browsing/searching filters out disabled accounts. There is no archive-only disabled-account browser or restoration action. Re-adding the account creates a new identity and cannot recover messages that were already deleted from the server into the normal account view. Full auditor export still includes retained blobs.

Evidence: source-confirmed; an existing storage test explicitly asserts that disabled accounts are hidden.

Repair: separate synchronization eligibility from archival visibility. Offer retained accounts in an archive-only view and allow explicit reconnection when appropriate. Test removal followed by viewing and exporting messages no longer present on the server.

### R21 — [P2] OAuth browser launching is macOS-specific

Location: [oauth.rs:447](/Users/johannes/GIT/email-archiver/crates/adapters/src/oauth.rs:447).

The Google flow launches `Command::new("open")` on every platform. This is the macOS launcher; Windows/Linux release targets lack a reliable equivalent under that command. Authentication therefore fails before the browser opens on ordinary Windows installations and can fail on Linux.

Evidence: source-confirmed; Windows/Linux desktop interaction was not tested.

Repair: use a cross-platform system browser opener and display a copyable authorization link if launch fails. Test each release platform with a real default browser and a missing browser association.

### R22 — [P2] Release builds have no configured route to working Google OAuth

Locations: [app_commands.rs:1405](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/app_commands.rs:1405), [app_commands.rs:1443](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/app_commands.rs:1443), [release.yml:44](/Users/johannes/GIT/email-archiver/.github/workflows/release.yml:44), [AddAccountForm.svelte:159](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/AddAccountForm.svelte:159).

Google client configuration comes from the credential store or compile-time `GOOGLE_OAUTH_CLIENT_ID/SECRET`. The committed release workflow supplies neither. The UI has no path to the existing `set_google_oauth_client` command, although the README describes entering those values on first use. Under this workflow on a clean machine, Gmail setup fails with “Google OAuth credentials are not configured”. An externally customized build environment could change this; existing shipped binaries were not inspected.

Evidence: source-confirmed build/UI configuration gap.

Repair: choose and implement the supported distribution policy: configured application OAuth with the necessary build/release checks, or an explicit advanced client-configuration interface. Test a clean installation of the actual release artifact rather than a developer build with local environment settings.

### R23 — [P2] Canceling Google authorization does not cancel backend account creation

Locations: [AddAccountForm.svelte:159](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/AddAccountForm.svelte:159), [AddAccountForm.svelte:192](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/AddAccountForm.svelte:192), [app_commands.rs:1530](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/app_commands.rs:1530).

Cancel only increments a JavaScript generation counter. The authorization task can still finish, store tokens, and create an account after its UI result has been discarded. Retrying can create additional accounts. The OAuth callback timeout only bounds accepting a connection; reading from the accepted socket is unbounded, so an incomplete callback can also leave the task stuck.

Evidence: source-confirmed cancellation and callback read paths; a real OAuth authorization was not completed during review.

Repair: introduce backend operation IDs and cancellation tokens, bound callback reads, and make account creation a deliberate final commit with cleanup of canceled credentials. Validate the authenticated Google identity rather than assuming the login hint is the chosen identity. Test cancellation before callback, during token exchange, and immediately before account persistence.

### R24 — [P2] Opening native Settings during first run dereferences null configuration

Locations: [App.svelte:48](/Users/johannes/GIT/email-archiver/apps/desktop/src/App.svelte:48), [App.svelte:334](/Users/johannes/GIT/email-archiver/apps/desktop/src/App.svelte:334), [App.svelte:466](/Users/johannes/GIT/email-archiver/apps/desktop/src/App.svelte:466).

The native Settings listener is installed before an archive is configured. It changes the current page to Settings, where the template reads `config.db_path` even if configuration is null. The same path is reachable after a failed configuration read.

Evidence: source-confirmed reachable null dereference; native menu interaction was not exercised.

Repair: support archive-independent General Settings or route archive-dependent sections through a safe recovery/setup state. Test native Settings and keyboard shortcuts with no configuration and with a failed startup read.

## Performance and background-memory plan

The largest likely idle-memory opportunity is eliminating a retained hidden webview after making Rust responsible for startup and synchronization. There was no native-process RSS/energy profile in this review, so no MB saving or idle-memory percentage is claimed. The JavaScript bundle is already modest (155.54 kB, 54.28 kB gzip); shaving its transfer size is unlikely to address the main background-memory concerns.

| Order | Change | Evidence and expected effect | Validation required |
| --- | --- | --- | --- |
| 1 | Backend-owned bootstrap and optional lazy webview | `main.rs` starts with no active DB; Svelte must restore it. Close only hides the webview. Decoupling permits reliable login sync and later webview destruction. | Cold/login startup with no webview; show/hide cycles; native RSS including WebKit/WebView child processes; preserved settings and archive state. |
| 2 | Gate hidden-window loads and throttle progress | R19 is reproduced; sync emits IPC progress for every message. Avoid hidden queries/reactive work and cap visible updates, while sending a final result immediately. | Query/IPC counts and CPU during a hidden initial sync; fresh state on show; no stale previews. |
| 3 | Reuse a bounded set of SQLite connections and prepared statements | Every storage operation opens a connection and reapplies WAL/pragmas. Each ingestion commits separately; closing the last connection repeatedly adds open/close and checkpoint work. Reuse one writer and a small bounded read pool, with explicit cache limits. | Ingest throughput, disk writes, lock waits, WAL size, peak RSS; crash/restart integrity. Benchmark before considering bounded transaction batches. |
| 4 | Store an indexed canonical sort timestamp; use keyset pagination | Current ordering uses a cross-table `coalesce` expression and OFFSET. EXPLAIN shows a location scan plus temporary sort. Index the actual filter/sort predicates and use stable `(timestamp,id)` cursors. Combine with R16/R17's correct bounded cache. | Query plans and timings at 10k/100k/1M locations; account/date/search combinations; insertion during browsing; bidirectional navigation. |
| 5 | Stream export metadata from one snapshot | ZIP EML bodies are already written one at a time, but index rows, all events, CSV/JSONL strings, and the blob list are accumulated. Stream metadata and EML from R15's stable snapshot. | Peak export RSS grows with bounded page/message size rather than total metadata count; cancellation/disk-full leaves no published partial archive. |
| 6 | Reduce MIME and image copies; preflight message size | IMAP buffers a message literal and sync copies it. Preview reparses raw MIME, creates data URIs, duplicates them in attachments/CID maps/HTML, and serializes those strings into IPC/srcdoc. Six MiB of inline images can expand into much larger transient representations. | Large-message/attachment profiles; concurrency cap; decoded image pixel limits; no broken CID rendering or external-content permission changes. |
| 7 | Separate bounded content audits from incremental verification | Each account completion computes a root over the full stored hash set; periodic checks repeat it, with an ever-growing event walk every tenth cycle. Multiple accounts cause repeated O(N) scans per cycle. Develop transactionally valid checkpoint/indexed incremental verification, plus scheduled bounded raw-content audits. | Preserve detection and interruption recovery in R10–R12; measure bytes read and time versus archive size; never clear failures merely to improve speed. |
| 8 | Add deadlines, cancellation, and retry scheduling | R08 can stall all work. A sync longer than its interval leads to zero sleep; integrity time is excluded from the elapsed subtraction. Failed runs have no differentiated backoff. | Offline, sleep/wake, slow server, and repeated auth failure tests; bounded retry rate; responsive interval changes. |

Synthetic query evidence: Python SQLite 3.53.4, exact current schema and unfiltered list SQL, 100,000 small message locations. EXPLAIN reported `SCAN ml` and `USE TEMP B-TREE FOR ORDER BY`. Three runs for the first 100 rows took 25.6/38.2/127.4 ms; at OFFSET 50,000 they took 347.3/253.5/376.9 ms. These measurements ran alongside compilation, use tiny synthetic bodies, and are indicative of query scaling, not a native production benchmark or a clean before/after comparison.

For native profiling, record the entire process tree's resident/physical footprint, idle CPU, wakeups, disk writes, and WAL size in the same release build for: cold start, visible idle, hidden idle, hidden sync, a 50 MiB message, repeated preview/open/close cycles, and a large auditor export. Test several archive sizes and account counts. Avoid increasing sync parallelism before per-message allocation and writer contention are bounded.

## Other findings and review notes

- **[P2] Error messages confuse command failure with missing Tauri.** [tauri_bridge.js:13](/Users/johannes/GIT/email-archiver/apps/desktop/src/lib/tauri_bridge.js:13) wraps both dynamic import and backend rejection as “Tauri invoke unavailable”. Several views suppress load errors and display ordinary empty states. Separate transport/runtime errors from typed database, credential, and network failures; retain detailed diagnostics alongside clear recovery copy. Polling also cannot “ensure” a message is captured before the user deletes it, as claimed in [GeneralSettings.svelte:140](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/GeneralSettings.svelte:140).
- **[P2] Concurrent app instances are not excluded.** [main.rs:14](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/main.rs:14) has no single-instance guard or interprocess archive writer ownership. The mutex only protects one process. Multiple instances can independently sync, update cursors, and create overlapping checkpoints. Add explicit process/archive ownership and forward secondary launches to the running application. Native duplicate-launch behavior was not tested.
- **[P2] Inline images can be rendered twice.** The backend replaces CID references with data URIs at [app_commands.rs:320](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/app_commands.rs:320), but [MessagePreview.svelte:230](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/MessagePreview.svelte:230) detects already-embedded attachments by looking for the removed `cid:` text. Return an explicit embedded flag/set to avoid repeated logos/images and unnecessary rendering.
- **[P2, requires native verification] External-image consent is inconsistent.** [MessagePreview.svelte:70](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/MessagePreview.svelte:70) detects only quoted absolute HTTP(S) `src`/`background` references, missing unquoted attributes, protocol-relative URLs, and `srcset`. When detection misses, its own iframe CSP permits HTTP(S) before consent. The packaged parent CSP prohibits remote images and may conversely prevent the explicit “Load images” action. Default blocking should depend directly on consent, with DOM-based URL handling and a reviewed packaged policy. This review does **not** establish a production tracking leak; actual webview CSP inheritance/network requests need testing.
- **[P2] Accessibility gaps remain.** [MainDashboard.svelte:807](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/MainDashboard.svelte:807) has unlabeled filter selects; [VirtualList.svelte:160](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/VirtualList.svelte:160) has unlabeled selection checkboxes and Enter-only button semantics; [ConfirmDialog.svelte:36](/Users/johannes/GIT/email-archiver/apps/desktop/src/components/ConfirmDialog.svelte:36) lacks modal focus containment/restoration. Escape handling does exist. Add accessible names, expected Space behavior, and dialog focus management; verify keyboard-only use and screen-reader announcements. No rendered contrast/responsive visual audit was performed, so none is claimed.
- **[P2] Generated procedural documentation diverges from behavior.** [documentation.rs:15](/Users/johannes/GIT/email-archiver/apps/desktop/src-tauri/src/documentation.rs:15) describes a 15-minute default, while Rust uses five minutes. Folder-exclusion language also disagrees with [sync.rs:517](/Users/johannes/GIT/email-archiver/crates/adapters/src/sync.rs:517), which currently hard-excludes no common folder names. Generate claims from actual persisted configuration and distinguish defaults from the user's selection. This is a software behavior/documentation finding, not an assessment of legal compliance.
- **[P3] Locked Rust builds fail at the reviewed commit.** `Cargo.lock` records `amberize` 0.2.2 while its manifest is 0.2.3. `cargo test --workspace --locked --offline` refuses to run without updating it. The release wizard refreshes the npm lockfile but not this workspace lockfile. Refresh it in the version-bump workflow and use `--locked` in CI/release checks.
- **[P2/P3] Release checks do not enforce important gates.** [release.yml:18](/Users/johannes/GIT/email-archiver/.github/workflows/release.yml:18) can build a draft release independently of passing tests; missing macOS signing credentials select an unsigned fallback. Make intentional unsigned/developer builds explicit and gate public release readiness on tests and required signing/notarization checks. This does not establish whether a current shipped app is unsigned. [ci.yml:61](/Users/johannes/GIT/email-archiver/.github/workflows/ci.yml:61) runs `cargo audit || true` without explicitly installing it, making the audit non-enforcing even when it is unavailable.
- **[P3] Landing social preview references a missing asset.** [landing/+layout.svelte:40](/Users/johannes/GIT/email-archiver/apps/landing/src/routes/+layout.svelte:40) points to `images/cover.png`, while the checked-in asset is `cover.webp`; the same applies to Twitter metadata. Correct the reference and verify deployed metadata.
- **[P3] Download-stat fetching breaks across API pages.** [fetch_download_stats.py:45](/Users/johannes/GIT/email-archiver/scripts/fetch_download_stats.py:45) passes concatenated paginated JSON arrays to one `json.loads`. Feeding two synthetic pages reproduced `JSONDecodeError: Extra data`. The deployment script stops on this failure. Collect pages into one JSON array or parse the page stream explicitly and test multi-page output.

## Verification performed

| Check | Result |
| --- | --- |
| Existing desktop tests | 19 passed across three files. |
| Desktop coverage | 46.45% statements/lines; 32.98% functions; 57.34% branches. Configuration tests principally use browser localStorage rather than native persistence. General Settings, Add Account, and Account Settings behavior have no functional coverage. |
| Desktop production build | Passed. Existing compiler warnings include initial-prop capture in SettingsPage and autofocus in ConfirmDialog. |
| Landing production build | Passed. Existing warning about nonreactive `popover_id_counter`. |
| Rust formatting | `cargo fmt --all -- --check` passed. |
| Rust lint | `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings` passed in the isolated copy, using the same lockfile version correction. |
| Rust existing tests | 63 passed: 6 desktop, 13 adapters, 44 storage, in an isolated copy after correcting only that copy's root-package version in Cargo.lock and supplying built frontend assets. |
| Targeted frontend reproductions | Eight passed, explicitly asserting current defective behavior: read failure, hidden save failure, false success status, date pagination, ineffective attachment filter, selection-free export, hidden reload, and preview race. |
| Targeted Rust reproductions | Four storage cases passed: missing DB recreation, changed raw MIME undetected by routine check, legitimate uncheckpointed ingestion flagged, and quick check accepting a broken chain. A fifth desktop case tested destructive same-path ZIP export. |
| Synthetic SQL/script checks | Current list query plan/timings inspected; multi-page release-stat parsing failure reproduced. |

The initial PATH selected Rust 1.70, which cannot read this version-4 lockfile. Rust/Cargo 1.93 from the installed Homebrew toolchain was used for current checks. No original application source, credentials, application configuration, login registration, or live archive was changed by this review. Temporary reproduction tests were confined to the isolated copy. This review adds only this report; concurrent release/packaging edits were preserved.

Limitations: no live IMAP/Google session, no shipped-installer signing examination, no OS reboot/login test, no Windows/Linux native execution, and no native RSS/energy measurement. Those are the next validation gates for the fixes, especially R03–R05, R08, R21–R23, and the webview lifecycle change.

## Recommended repair order

1. Fix R01/R02/R03: atomic durable configuration, open-existing semantics, and persistent platform credential storage. Centralize backend bootstrap and expose recoverable startup states.
2. Fix R13/R14 immediately: enforce selection and prevent exports from touching archive files. Add output-content and file-identity regressions.
3. Fix R09–R12 and R15: preserve completeness, validate actual contents, distinguish interrupted work, retain integrity failures, and export a consistent snapshot.
4. Fix R04–R08 and R25: durable login registration, safe plugin failure handling, honest sync outcomes, restored intervals, deadlines/cancellation, and correct retry cursors.
5. Fix browsing/account/OAuth behavior R16–R24, then adopt the ordered background-memory plan with measured native baselines.
6. Strengthen release gates and add end-to-end tests for restart, unavailable/moved archives, credential-store locks, failed background sync, canceled setup, and release-artifact login behavior.

Useful foundations to preserve: content-hash blob deduplication; atomic blob/location/event ingestion; read-only `BODY.PEEK[]` fetches; WAL and foreign keys; streaming fetch iteration; EML export one blob at a time; secret-redacting Debug implementations; and DOMPurify plus a sandboxed preview. These are meaningful protections, but the findings above show where their surrounding lifecycle and recovery logic does not yet deliver the promised behavior.
