# AppImage external update implementation

1. Configure the Linux release job's `UPDATE_INFORMATION` before the pinned Tauri action runs. Stable tags use `latest`; prerelease tags use `latest-pre`. Install `zsync` alongside the existing Linux packaging dependencies.
2. Add a read-only verifier for the embedded update channel and `.zsync` control file. Check the source filename/URL, payload size/hash and independently generated checksum table; use bounded reads and reject malformed ELF/control data.
3. Extend release metadata checks to generate the upload sidecar at an explicit adjacent path from the final signed AppImage, then run the verifier. After native Linux QA succeeds, upload sidecars only to a draft release. Keep signing and built-in updater behavior unchanged.
4. Add adversarial unit tests and a small hosted Linux packaging integration check using linuxdeploy and its AppImage plugin. Verify stable/prerelease metadata and actual generated sidecars without a local Rust build.
5. Update the release checklist, run targeted checks, inspect the final diff, commit and push. Confirm the hosted packaging check before reporting completion. No tag creation or publication is included.
