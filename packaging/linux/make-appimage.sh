#!/usr/bin/env bash
# Makes the AppImage of DiaCraft, one file that runs on most Linux desktops without installing. The release workflow's Linux jobs run it, after the
# archive and the .deb.
#
#   packaging/linux/make-appimage.sh NAME OUT_DIR BIN_DIR
#
#   NAME     what goes in the file name: the release's version (0.2.0), or a label for a test build. Letters, digits, `.`, `_` and `-` only. A tag (v0.2.0) is named
#            as its version, with no `v` in the file name.
#   OUT_DIR  where the file is written; created when it is not there.
#   BIN_DIR  a folder holding the release programs `diacraft` and `diacraft-cli`.
#
# Environment (both required):
#   APPIMAGETOOL      the path of the `appimagetool` AppImage of the CPU the programs are built for. The release workflow downloads one version of it and checks its SHA-256.
#   APPIMAGE_RUNTIME  the path of the type 2 runtime file of the same CPU (`runtime-x86_64` or `runtime-aarch64`). appimagetool fetches a runtime from the internet when it
#                     is not given one, and the one it fetches changes from day to day: this one is pinned and checked too.
#
# Writes OUT_DIR/diacraft-NAME-linux-CPU.AppImage (CPU is x86_64 or aarch64, read from the programs) and prints its path. The name follows the downloads' (diacraft-<version>-
# linux-<cpu>), and not the AppImage convention (Name-Version-arch.AppImage): nothing reads the file name.
#
# What is in it (the AppDir): both programs under usr/bin, the launcher entry and the icon (at the top, as appimagetool wants them, and under usr/share, where a
# desktop looks), the licence files under usr/share/doc/diacraft, and AppRun, which starts the window program, or the command line:
#   ./diacraft-X.AppImage ARGS...             runs `diacraft ARGS...`
#   ./diacraft-X.AppImage --cli ARGS...       runs `diacraft-cli ARGS...`
#   a link named diacraft-cli to the file     runs `diacraft-cli` with its arguments
# Nothing else is bundled: the programs load the window system, Vulkan and OpenGL libraries from the machine they run on, as the archive's do (its README says which).
# An AppImage mounts itself through FUSE; where FUSE is not there, `--appimage-extract-and-run` runs it with no mount, as the release workflow does, and
# `--appimage-extract` unpacks it into a folder `squashfs-root`.
#
# Uses bash 3.2 syntax like the other scripts.
set -euo pipefail

script=make-appimage.sh
# shellcheck source=packaging/common.sh
source "$(dirname "$0")/../common.sh"

[ $# -eq 3 ] || die "usage: make-appimage.sh NAME OUT_DIR BIN_DIR"
name=$1
out=$2
bin=$3
check_name "$name"
name=$(file_version "$name")
[ -d "$bin" ] || die "no such folder: $bin"
bin=$(cd "$bin" && pwd)
[ -n "${APPIMAGETOOL:-}" ] && [ -f "$APPIMAGETOOL" ] || die "APPIMAGETOOL must be the path of appimagetool"
[ -n "${APPIMAGE_RUNTIME:-}" ] && [ -f "$APPIMAGE_RUNTIME" ] || die "APPIMAGE_RUNTIME must be the path of the type 2 runtime"
# Both made absolute: appimagetool is an AppImage that unpacks itself and runs from a folder of its own, where a path relative to this one means nothing.
appimagetool=$(cd "$(dirname "$APPIMAGETOOL")" && pwd)/$(basename "$APPIMAGETOOL")
appimage_runtime=$(cd "$(dirname "$APPIMAGE_RUNTIME")" && pwd)/$(basename "$APPIMAGE_RUNTIME")
mkdir -p "$out"
out=$(cd "$out" && pwd)

arch=$(programs_arch linux "$bin")
case $arch in
x86_64) appimage_arch=x86_64 ;;
*) appimage_arch=aarch64 ;;
esac
set_version_from_name "$name"

stage=$(mktemp -d "${TMPDIR:-/tmp}/diacraft-appimage.XXXXXX")
trap 'rm -rf "$stage"' EXIT
dir=$stage/AppDir
docs=$dir/usr/share/doc/diacraft
install -d -m 755 "$dir/usr/bin" "$dir/usr/share/applications" "$dir/usr/share/icons/hicolor/256x256/apps" "$docs"

install -m 755 "$bin/diacraft" "$bin/diacraft-cli" "$dir/usr/bin/"
# The entry as it is in the other downloads: `Exec=diacraft %F`, which AppRun finds in usr/bin. appimagetool wants one at the top of the AppDir, and the icon beside it,
# named as the entry's Icon (app.diacraft); the same two are put where a desktop's own tools look.
install -m 644 "$root/packaging/linux/app.diacraft.desktop" "$dir/app.diacraft.desktop"
install -m 644 "$root/packaging/linux/app.diacraft.desktop" "$dir/usr/share/applications/app.diacraft.desktop"
install -m 644 "$root/assets/branding/diacraft-icon-256.png" "$dir/app.diacraft.png"
install -m 644 "$root/assets/branding/diacraft-icon-256.png" "$dir/usr/share/icons/hicolor/256x256/apps/app.diacraft.png"
ln -s app.diacraft.png "$dir/.DirIcon"
add_licences "$docs"
chmod 644 "$docs"/*
need_licences "$docs"

cat >"$dir/AppRun" <<'APPRUN'
#!/bin/sh
# What the AppImage runs when it starts: the window program, or the command line when it is asked for (see packaging/linux/make-appimage.sh).
set -eu
here=$(dirname "$(readlink -f "$0")")
# The AppImage runtime sets ARGV0 to the name it was started by: a link named diacraft-cli starts the command line.
case "${ARGV0:-}" in
diacraft-cli | */diacraft-cli) exec "$here/usr/bin/diacraft-cli" "$@" ;;
esac
if [ "${1:-}" = "--cli" ]; then
    shift
    exec "$here/usr/bin/diacraft-cli" "$@"
fi
exec "$here/usr/bin/diacraft" "$@"
APPRUN
chmod 755 "$dir/AppRun"
need "$dir/AppRun" "$dir/app.diacraft.desktop" "$dir/app.diacraft.png" "$dir/usr/bin/diacraft" "$dir/usr/bin/diacraft-cli"

file=$out/diacraft-$name-linux-$(file_arch linux "$arch").AppImage
rm -f "$file"
# --appimage-extract-and-run: appimagetool is an AppImage itself, and a build machine has no FUSE to mount it. --no-appstream: the package has no AppStream data to check.
# VERSION names the version in the entry the file carries.
ARCH=$appimage_arch VERSION=$version "$appimagetool" --appimage-extract-and-run --no-appstream --runtime-file "$appimage_runtime" "$dir" "$file" >&2
[ -s "$file" ] || die "the AppImage was not written: $file"
chmod 755 "$file"
echo "$file"
