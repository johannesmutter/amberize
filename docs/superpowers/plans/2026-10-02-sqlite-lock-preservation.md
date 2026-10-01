# Archive lock repair and release

1. Reproduce lock loss with a fresh subprocess on the current source in hosted CI, preserving the failure log.
2. Replace Unix raw-file identity with device/inode metadata. Remove independent header reads and retain SQLite/schema validation and non-creating opens. Reuse this comparison for desktop sync/reset and protected export destinations.
3. Require the same lock regression to pass after reopening, symlink/hardlink comparison and output validation. Run existing storage, app, UI, helper and dependency checks through CI.
4. Build version 0.2.7 with the AppImage permission/glibc repairs, the archive-lock repair and validation before write settings. Test all native recovery cases against the exact packages, including the root-owned AppImage under a separate UID. Independently verify signatures, all 1,024 fixture hashes and full integrity.
5. Publish the verified release, refresh and deploy the download site, and verify public updater/download links. Obtain explicit permission for the external catalog `/retest` comment, then record its actual result. Preserve original local applications, accounts, archive data, Keychain settings and VM state.
