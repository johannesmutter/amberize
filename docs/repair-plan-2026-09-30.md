# Amberize reliability and efficiency repairs

The user approved implementing all tasks in the 2026-09-30 codebase review and asked that each issue first be double-checked. Existing unrelated release/packaging edits must be preserved. Changes apply to local code and tests; no release publication, live archive mutation, credential changes, or OS login-registration changes are part of local validation.

## Design

Use backend-owned durable configuration and explicit archive creation/opening. Startup restores the archive and interval without depending on a webview, preserves recoverable errors, and runs without optional autostart support. Native visibility and process ownership are explicit. Login registration is refreshed only when already enabled. Archive selection completes only after validation and durable save.

Retain atomic blob/location/event ingestion and SHA-256 deduplication. Reuse bounded SQLite connections during storage work, add indexed canonical sort/attachment metadata, and return paginated server-filtered results. Integrity results distinguish checked content, chain, schema, and checkpoint state; legitimate incomplete sync tails are recognized without masking real tampering. Failed scopes remain failed until verified successfully. Audit exports use one consistent snapshot, stream metadata, enforce selection, and reject archive-file aliases before creating output. Exports have operation-scoped cancellation with a serialized publication barrier. Root reuse is guarded by local blob-write revisions and SQLite data_version inside IMMEDIATE transactions; full content audits remain uncached.

Use platform-specific persistent credential stores. IMAP stages and streams have deadlines, cancellation, and durable safe progress. Oversized messages must remain visible unresolved coverage rather than silent success. Carry UIDVALIDITY through error outcomes. Google setup has an explicit configurable-client route and cross-platform browser opening, and canceling an operation prevents account commit and cleans up credentials.

The frontend consumes backend state rather than interpreting failed reads as empty data. Correct filtering/pagination, preserve selection independently of loaded pages, discard stale previews, and suspend hidden-window data work. Restore/recreate views from durable backend state before attempting native webview destruction. External-content consent governs CSP directly, CID attachment placement is explicit, and controls/dialogs have accessible names and focus behavior.

Release tooling refreshes Cargo.lock, runs locked tests/lints/audit, validates required signing and OAuth configuration policy, and correctly handles paginated release statistics and landing assets. Generated documentation reflects actual configuration and excludes unsupported guarantees.

## Execution and acceptance

Spec review refinements: incomplete tails require one-to-one blob/event correspondence and a matching reconstructed checkpointed set; integrity scopes need explicit freshness and must share a snapshot; commands and sync share bounded per-file connection ownership; exports snapshot documentation and publish only after rechecking file identity; selected exports omit unrelated audit metadata; OAuth cancel versus commit uses one serialized operation state; destroyed webviews require a ready/action-delivery mechanism. Native destruction will follow the independently tested suspend-on-hide stage. Local hash chains do not prove protection against an entire coordinated rewrite without an external anchor.

- [x] Revalidate all findings and record confirmed, narrowed, or rejected claims.
- [x] Storage: explicit open/create, bounded connection reuse, integrity/checkpoint semantics, filters/indexes/cursors, retained accounts, output safety, snapshot streaming, regression tests.
- [x] Desktop backend: atomic configuration, bootstrap/recovery, sync status/interval/deadlines, safe native startup/login and instance ownership, lifecycle tests.
- [x] Adapters: persistent credentials, bounded IMAP, oversized-message visibility, UIDVALIDITY progress, cancellable cross-platform OAuth, tests.
- [x] Frontend: recovery/save errors, honest status, backend filters/bidirectional cache, selection export, preview/hide races, OAuth setup/cancel, image policy/CID, accessibility, tests.
- [x] Release/docs/landing: preserve existing edits, enforce validation gates, refresh locks, correct generated claims and paginated statistics, tests.
- [x] Run formatting, Clippy, workspace tests, frontend tests/builds and focused synthetic/native checks; document any native/provider/OS checks that cannot be performed locally.

Tests must assert corrected behavior with synthetic archives and deterministic error injection. Existing live archives are never used as fixtures. Native startup/login, platform credential persistence, provider authorization, and process-tree memory savings need explicit evidence before claiming them verified. The final status must account for every review task, including narrowed findings and incomplete validation.

Final accounting: all numbered defect repairs are implemented and locally validated to the limits in `docs/repair-results-2026-09-30.md`. Performance repairs include safe reuse of unchanged roots and cancellable atomic exports. Changed blob sets retain the existing O(N) proof calculation; a future versioned incremental proof format is an optional deeper redesign. Release-platform/provider/RSS validation remains outstanding. Checked items above describe the completed repair pass, not release sign-off.
