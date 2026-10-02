# Release and Update Checklist

This checklist covers the end-to-end flow for DMG installation and Tauri auto-updates.

## Validate the candidate before publication

Keep the release as a draft until the cross-platform validation, package installation/recovery, provider sync, actual login/reboot, and older-version staging update gates pass. The current evidence and remaining blockers are recorded in [the QA results](qa-results-2026-09-30.md); the isolated procedure is in [the QA plan](qa-plan-2026-09-30.md). A green unit-test run or an updater signature check alone does not establish those native behaviors.

Use an MSI-compatible numeric prerelease version, such as `0.2.4-6`, consistently across the candidate's Tauri, Cargo, npm, and lockfile metadata. An `rc.6` suffix is rejected by the MSI bundler unless a separate valid WiX installer version is configured. The working production version should remain unchanged during candidate preparation.

Pass Cargo's lockfile flag after Tauri's separator: `npx tauri build --target TARGET --bundles BUNDLES -- --locked`. Keep the JavaScript Tauri API and plugin versions compatible with the locked Rust packages. Inspect the draft's exact tag, commit, installer hashes, and signatures before testing it; retain native evidence from the release workflow.

Before offering shared Google sign-in to the public, check the owning OAuth project’s Audience, Branding and Verification Center. Confirm **Amberize** in both the intended and published branding, resolve the reported **Ambermail** identity, and verify the requested restricted scopes or document an applicable exemption. An External/Testing project’s Gmail refresh tokens expire after seven days. The current IMAP flow requires full-mail access; a narrower read-only scope requires a Gmail API integration and still has restricted-scope verification requirements. Keep personal-client QA distinct from public onboarding readiness. [Google branding](https://support.google.com/cloud/answer/15549049), [restricted-scope verification](https://developers.google.com/identity/protocols/oauth2/production-readiness/restricted-scope-verification), and [refresh-token expiration](https://developers.google.com/identity/protocols/oauth2#expiration) describe these gates.

On October 1, project `ambermail` (`439963303263`) was confirmed External/In production, and Amberize branding was verified and published. The project still declares no scopes although the actual IMAP app requests full-mail permission, so permission verification is incomplete. The corrected public privacy/imprint pages are deployed. Before submitting public permission review, settle the Gmail API read-only integration choice, declare the scopes actually requested, complete provider and existing-account migration tests, and prepare an accurate demonstration of the implemented flow. Do not describe branding approval as Gmail permission approval. The current build has no supplied shared Google client defaults; public-client configuration and its release artifacts still need preparation after the flow is finalized.

## 0) Use automation scripts (recommended)

From repo root:

1. Run the interactive release wizard:
   - `./scripts/release_wizard.py`
2. Follow prompts to:
   - bump version in all release metadata files
   - update `apps/desktop/package-lock.json`
   - optionally commit, push branch, create and push tag
3. After publishing the GitHub Release, run:
   - `./scripts/release_verify.py`
4. Confirm all checks print `PASS`.

## 1) Prepare signing key and updater config

1. Generate a signing key once:
   - `cd apps/desktop`
   - `npx tauri signer generate --write-keys "src-tauri/amberize.key"`
2. Keep `apps/desktop/src-tauri/amberize.key` private and out of version control.
3. Set GitHub secrets for the release workflow:
   - `TAURI_SIGNING_PRIVATE_KEY` (file content of `amberize.key`)
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (if used)
4. Ensure `apps/desktop/src-tauri/tauri.conf.json` contains the matching public key and updater endpoint.
5. Ensure your updater endpoint is publicly accessible (for GitHub Releases this requires a public repository).

## 2) Trigger release pipeline

1. Bump app version in one step:
   - preferred: `./scripts/release_wizard.py`
   - manual fallback: update `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`, `apps/desktop/package.json` and refresh `apps/desktop/package-lock.json`
2. Create and push a tag:
   - `git tag vX.Y.Z`
   - `git push origin vX.Y.Z`
3. Confirm the Release workflow starts:
   - GitHub repo → **Actions** tab
   - Left sidebar → **Release**
   - Click the run for tag `vX.Y.Z`
4. Wait for `.github/workflows/release.yml` to finish (all matrix jobs).
   - The Linux job extracts each AppImage and checks metadata links, readability/traversal for unrelated users, and executable `AppRun` plus `AppRun.wrapped`. It also checks bundled glibc references against the Ubuntu 22.04 baseline and starts a root-owned extracted package under a separate user. Do not publish if these checks fail.
5. Confirm the **draft** GitHub Release has assets:
   - GitHub repo → **Releases** → open `vX.Y.Z`
   - Assets should include installers and updater files, e.g.:
     - macOS: `*.dmg`, `*.app.tar.gz`, `*.app.tar.gz.sig`
     - Windows: `*.msi` (or other configured installer)
     - Linux: `*.deb`, `*.AppImage`, matching `*.AppImage.zsync`
     - updater: `latest.json`

## 3) Publish and verify updater metadata

1. Open the GitHub draft release and publish it.
2. Verify the updater endpoint resolves:
   - `https://github.com/johannesmutter/amberize/releases/latest/download/latest.json`
3. Confirm `latest.json` references artifacts for the current version and includes signatures.

### AppImage catalog retest

The AppImage catalog reported a broken `.DirIcon` in `v0.2.3`: it linked to an absolute path on the GitHub build runner. Tauri CLI 2.11.4 fixes this by creating relative metadata links; keep the desktop CLI dependency at 2.11.4 or newer and commit its lockfile. See the [upstream fix](https://github.com/tauri-apps/tauri/pull/15596).

Its later test of `v0.2.4` exposed root-owned `AppRun.wrapped` with mode `0770`. The `beforeBundleCommand` now prepares Tauri's pinned upstream launcher with mode `0755`; this runs before signing and uploading, including local package builds. A build-user-only execution check is insufficient because it can retain access that a mounted AppImage denies to ordinary users.

For permission checks, extract with `scripts/extract_appimage.py` and `unsquashfs`, which restores stored SquashFS modes. The runtime's `--appimage-extract` creates directories with private modes and therefore cannot establish whether the package's stored directories are accessible to unrelated users. Never chmod an extracted fixture to make a defective package pass.

New Linux releases embed external update information through linuxdeploy's `UPDATE_INFORMATION` before Tauri signs the AppImage. Stable tags use `gh-releases-zsync|johannesmutter|amberize|latest|Amberize_*_amd64.AppImage.zsync`; prerelease tags use `latest-pre`. Because appimagetool writes its sidecar in its working directory, the release job explicitly generates the upload sidecar beside the final signed AppImage. The release verifier checks the embedded ELF metadata, source filename/URL, payload size/SHA-1 and independently regenerated zsync checksum table. After native QA succeeds, the sidecar is uploaded beside its matching AppImage to the draft release. Confirm both assets are present before publication. Published 0.2.7 has no external update metadata; this change takes effect with the next newly built release. Never retrofit metadata into an already signed/published AppImage.

After publishing a new release with the corrected AppImage, comment `/retest` on [AppImage catalog PR #8381](https://github.com/AppImage/appimage.github.io/pull/8381).

## Troubleshooting: Release exists but only has "Source code" assets

If the GitHub Release page shows only:
- "Source code (zip)"
- "Source code (tar.gz)"

then the Release workflow either **did not run**, **failed**, or **built without bundling artifacts**.

### A) Verify the Release workflow run

1. GitHub repo → **Actions** → **Release**
2. Click the run for your tag (e.g. `v0.1.1`).
3. Check the matrix jobs:
   - If there is **no run at all**:
     - ensure the tag was pushed: `git push origin v0.1.1`
     - ensure `.github/workflows/release.yml` exists on the commit pointed to by the tag
   - If there is a run but it **failed**:
     - open the failed job and scroll to the **bottom error**

### B) Common failure causes

- **No artifacts were found**
  - Symptom in logs: `##[error]No artifacts were found.`
  - Fix: ensure Tauri bundling is enabled for the target (the workflow should pass `--bundles ...`).

- **Missing signing private key**
  - Symptom in logs: `A public key has been found, but no private key. Make sure to set TAURI_SIGNING_PRIVATE_KEY`
  - Fix: GitHub repo → **Settings** → **Secrets and variables** → **Actions**
    - `TAURI_SIGNING_PRIVATE_KEY`: paste the full contents of `apps/desktop/src-tauri/amberize.key`
    - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: set if your key is password protected

### C) Re-run the release after fixing

Because the workflow runs on tag push, you must either:
- create a new tag (recommended): `v0.1.2`, or
- delete and recreate the broken tag + Release.

## 4) Clean-machine DMG test (manual)

Use a clean macOS user profile or VM:

1. Download the latest `.dmg` from Releases.
2. Open DMG and drag Amberize to `Applications`.
3. Launch Amberize.
   - If macOS shows **“Amberize is damaged and can’t be opened”**, the build is not properly **signed + notarized** for distribution.
   - Stop the distribution test and fix the signature/notarization failure; removing quarantine does not validate the release.
4. Complete smoke checks:
   - choose archive location
   - add/open account data
   - open Settings
   - run sync once
   - app can close to tray and reopen

## 5) macOS signing + notarization (required for public downloads)

Without notarization, many users on newer macOS versions will see:
**“App is damaged and can’t be opened. You should move it to the Trash.”**

### A) Create GitHub secrets (repo → Settings → Secrets and variables → Actions)

- `APPLE_CERTIFICATE`: base64 of your exported `.p12` (Developer ID Application)
- `APPLE_CERTIFICATE_PASSWORD`: password used when exporting the `.p12`
- `APPLE_ID`: your Apple ID email
- `APPLE_PASSWORD`: an **app-specific password** (recommended) for notarization
- `APPLE_TEAM_ID`: your Apple Developer Team ID

### B) Create an Apple app-specific password

1. Apple ID account → **Sign-In and Security** → **App-Specific Passwords**
2. Create one for notarization and save it

### C) Trigger a new tag

After adding the secrets, create a new tag (or delete/recreate the old one) to re-run the Release workflow.

### D) Troubleshooting

Assess the DMG container as well as its app. A notarized app inside a signed DMG does not establish that the distributed DMG is notarized. The release workflow now runs `scripts/notarize_macos_dmgs.py`, requires Apple's `Accepted` result, staples and validates the DMG ticket, and runs `spctl --assess --type open --context context:primary-signature --verbose=2` before replacing the draft asset. It refuses this replacement on a published release. Apple recommends notarizing the outermost distributed container in [Packaging Mac software for distribution](https://developer.apple.com/documentation/xcode/packaging-mac-software-for-distribution). App notarization also remains necessary for the separately distributed updater payload.

The release workflow authenticates with `scripts/check_notarization_auth.py` before compiling the macOS application. Its artifact contains status codes, formatting flags, and fixed failure categories, without credential values or submission history. An HTTP 401 with all fields present and no surrounding whitespace means Apple rejected the credential combination; recompiling does not repair it. Verify that `APPLE_ID` is the account owning the app-specific password and `APPLE_TEAM_ID` is the correct developer team, then create a fresh app-specific password and update `APPLE_PASSWORD` directly in GitHub Actions secrets. Never paste it into logs or chat. [Apple documents creating app-specific passwords and their automatic revocation when the main account password changes](https://support.apple.com/en-us/102654).

If the report contains HTTP 403 with `agreement_required`, Apple says a required developer agreement is missing or expired. The Account Holder must review pending agreements and confirm active membership for the signing team in [Apple Developer](https://developer.apple.com/account). Apple documents the access restriction from an unaccepted updated license agreement in [Resolving access issues](https://developer.apple.com/help/account/access/resolving-access-issues). The report does not identify which specific agreement is outstanding. If it contains `team_access_denied`, verify the configured team ID and the Apple account's membership in that team. A generic `authorization_denied` report cannot establish either specific cause. Repeat the authentication check after resolving the reported account requirement, then rerun the macOS builds.

An existing working notarization Keychain profile can be used for a local notarization operation. App Store Connect API authentication is also supported through `APPLE_API_ISSUER`, `APPLE_API_KEY`, and `APPLE_API_KEY_PATH`; its private key must be securely provisioned on the runner. Neither alternative removes the notarization requirement. See [Apple's authentication migration guide](https://developer.apple.com/documentation/technotes/tn3147-migrating-to-the-latest-notarization-tool) and [Tauri's current signing configuration](https://v2.tauri.app/distribute/sign/macos/#notarization). Keep the candidate as a draft while repairing authentication.

If the macOS jobs fail with:

- `failed to decode certificate`
- `failed to run command base64 --decode: failed to decode certificate`

then `APPLE_CERTIFICATE` is **not a valid base64-encoded `.p12`**.

Fix:

1. Re-export your **Developer ID Application** certificate as a `.p12` (including the private key).
2. Encode it as a single-line base64 string:
   - `openssl base64 -A -in "/path/to/DeveloperID.p12" | pbcopy`
3. GitHub repo → **Settings** → **Secrets and variables** → **Actions** → update `APPLE_CERTIFICATE` by pasting from clipboard.
4. Re-run the failed jobs for the tag.

## 6) Auto-update verification

Before publication, use an isolated older installation and a separate staging endpoint. Verify that the updater detects the candidate, authenticates and installs its artifact, restarts into the expected version, and restores the same archive, credentials, and launch-at-login choice. Compare every synthetic message's MIME hash before and after the update, and exercise an interrupted download and rejected signature. Keep the production updater endpoint on the published stable release throughout candidate testing.

An offline Minisign verification confirms the signed artifact bytes and rejects a modified artifact. It does not confirm that the older app can install the update or restart successfully. Record this distinction explicitly when the staging update environment is unavailable.

1. Install an older build (`vX.Y.(Z-1)`).
2. Publish a newer release (`vX.Y.Z`).
3. In the old app build, trigger **Check for Updates** from menu.
4. Confirm:
   - update banner appears
   - install/restart succeeds
   - app relaunches on new version
