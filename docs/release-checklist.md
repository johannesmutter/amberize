# Release and Update Checklist

This checklist covers the end-to-end flow for DMG installation and Tauri auto-updates.

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
5. Confirm the **draft** GitHub Release has assets:
   - GitHub repo → **Releases** → open `vX.Y.Z`
   - Assets should include installers and updater files, e.g.:
     - macOS: `*.dmg`, `*.app.tar.gz`, `*.app.tar.gz.sig`
     - Windows: `*.msi` (or other configured installer)
     - Linux: `*.deb`, `*.AppImage`
     - updater: `latest.json`

## 3) Publish and verify updater metadata

1. Open the GitHub draft release and publish it.
2. Verify the updater endpoint resolves:
   - `https://github.com/johannesmutter/amberize/releases/latest/download/latest.json`
3. Confirm `latest.json` references artifacts for the current version and includes signatures.

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
   - Temporary local-only workaround:
     - `xattr -dr com.apple.quarantine "/Applications/Amberize.app"`
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

1. Install an older build (`vX.Y.(Z-1)`).
2. Publish a newer release (`vX.Y.Z`).
3. In the old app build, trigger **Check for Updates** from menu.
4. Confirm:
   - update banner appears
   - install/restart succeeds
   - app relaunches on new version
