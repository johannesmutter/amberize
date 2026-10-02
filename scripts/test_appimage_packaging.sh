#!/usr/bin/env bash
# Exercise the same linuxdeploy output pipeline as Tauri without compiling Rust.
set -euo pipefail
repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
qa_root=$(mktemp -d)
trap 'rm -rf "$qa_root"' EXIT

curl --fail --location --retry 3 --max-time 120 \
  https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy/linuxdeploy-x86_64.AppImage \
  --output "$qa_root/linuxdeploy-x86_64.AppImage"
curl --fail --location --retry 3 --max-time 120 \
  https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage \
  --output "$qa_root/linuxdeploy-plugin-appimage.AppImage"
chmod 0755 "$qa_root/"*.AppImage
XDG_CACHE_HOME="$qa_root/cache" node "$repository_root/scripts/prepare_appimage_tools.mjs"

for channel in latest latest-pre; do
  appdir="$qa_root/$channel.AppDir"
  mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/1024x1024/apps"
  cc -x c -o "$appdir/usr/bin/Amberize" - <<'C'
int main(void) { return 0; }
C
  cp "$qa_root/cache/tauri/AppRun-x86_64" "$appdir/AppRun"
  cp "$repository_root/apps/desktop/src-tauri/icons/icon.png" "$appdir/usr/share/icons/hicolor/1024x1024/apps/Amberize.png"
  cat > "$appdir/usr/share/applications/Amberize.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Amberize
Exec=Amberize
Icon=Amberize
Categories=Office;
DESKTOP
  appimage="$qa_root/Amberize_0.0.0-$channel"_amd64.AppImage
  information="gh-releases-zsync|johannesmutter|amberize|$channel|Amberize_*_amd64.AppImage.zsync"
  UPDATE_INFORMATION="$information" OUTPUT="$appimage" ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 \
    "$qa_root/linuxdeploy-x86_64.AppImage" --appimage-extract-and-run --appdir "$appdir" --output appimage
  python3 "$repository_root/scripts/verify_appimage_update.py" "$appimage" --expected-update-information "$information"
  # The runtime and read-only ELF verifier must agree about what was embedded.
  test "$("$appimage" --appimage-updateinformation)" = "$information"
done
