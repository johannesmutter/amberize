# Amberize 0.2.6 release evidence

## Confirmed defects

The AppImage catalog actually tested published 0.2.4. Its root-owned inner launcher had mode `0770`, preventing execution by an ordinary unrelated user. The 0.2.5 draft corrected this to `0755`, with unchanged pinned upstream launcher bytes and glibc references no higher than 2.35. Independent stored-SquashFS checks passed all 368 entries. Its exact native recovery runs intermittently timed out, so it was not published.

A separate cross-process regression confirmed that `Storage::open_existing` released the open archive's SQLite locks. The [unfixed-source CI run](https://github.com/johannesmutter/amberize/actions/runs/36937616560), at `e4f6f1e1244b30bbd139bb36d1fa4221a77c231c`, passed the initial contention probe and failed after reopening: the child obtained exclusive access and read the schema. This is consistent with [SQLite's documented POSIX descriptor-close hazard](https://sqlite.org/howtocorrupt.html#posix_advisory_locks_canceled_by_a_separate_thread_doing_close_). It establishes the lock defect; it does not independently establish that every previous startup timeout had that cause. The last 0.2.5 failure still passed post-stop full integrity on all 1,024 synthetic messages.

## Repair and required validation

Unix file identity now uses device/inode metadata without raw archive descriptors. SQLite validates existing archives through non-creating connections. Manual sync/reset and export destination comparisons use the same safe identity helper. Regression checks require continued contention after reopening, symlink/hardlink checks and both accepted/rejected export destination checks. Windows keeps its existing file-handle identity semantics.

The permanent release workflow checks actual stored AppImage modes, Ubuntu 22.04/glibc 2.35 compatibility, all five recovery cases and root-owned package execution under another UID. Full integrity and every synthetic MIME hash are required after those launches. The version 0.2.5 one-off diagnostic workflow has been removed; its hosted evidence remains available.

Release 0.2.6 remains pending fresh CI, package build, native recovery, independent signature verification and public deployment checks. No local production app, archive, credentials, Keychain settings or VM state was modified. Rust builds run in hosted CI. Existing manual platform/provider limitations from the 0.2.4 release remain applicable; this document does not claim a fully exhaustive product certification.
