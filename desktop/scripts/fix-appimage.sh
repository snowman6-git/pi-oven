#!/usr/bin/env bash
# After `cargo tauri build`: make the AppImage run as a native Wayland client on other distributions.
#
# linuxdeploy copies the build machine's libwayland into the AppImage, and its GTK hook then forces GDK_BACKEND=x11
# because that copy crashes next to another system's Mesa (tauri-apps/tauri#8541). Without the copy the system's
# libwayland is used (every desktop with Wayland has it), so the forced X11 can go too: the app decides at start
# (desktop/src/lib.rs, linux_env) and a GDK_BACKEND the person sets passes through.
#
# usage: desktop/scripts/fix-appimage.sh   (rebuilds target/release/bundle/appimage/pi-oven_<ver>_amd64.AppImage)
set -euo pipefail
cd "$(dirname "$0")/.."
dir=target/release/bundle/appimage
appdir=$dir/pi-oven.AppDir
[ -d "$appdir" ] || { echo "no $appdir: run cargo tauri build first" >&2; exit 1; }

rm -f "$appdir"/usr/lib/libwayland-client.so* "$appdir"/usr/lib/libwayland-cursor.so* \
      "$appdir"/usr/lib/libwayland-egl.so* "$appdir"/usr/lib/libwayland-server.so*
sed -i 's/^export GDK_BACKEND=x11.*$/# GDK_BACKEND is chosen by the app at start (see desktop\/src\/lib.rs)/' "$appdir"/apprun-hooks/linuxdeploy-plugin-gtk.sh
grep -q '^export GDK_BACKEND' "$appdir"/apprun-hooks/linuxdeploy-plugin-gtk.sh && { echo "GDK_BACKEND line still there" >&2; exit 1; }

ver=$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' tauri.conf.json | head -1)
out=$dir/pi-oven_${ver}_amd64.AppImage
rm -f "$out"
tool=${LINUXDEPLOY_PLUGIN_APPIMAGE:-$HOME/.cache/tauri/linuxdeploy-plugin-appimage.AppImage}
( cd "$dir" && ARCH=x86_64 LDAI_OUTPUT="$(basename "$out")" "$tool" --appimage-extract-and-run --appdir pi-oven.AppDir >/dev/null )
echo "$out"
