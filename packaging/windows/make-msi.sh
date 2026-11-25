#!/usr/bin/env bash
# Makes the Windows installer of DiaCraft, an .msi, with WiX. The release workflow's Windows jobs run it, after the zip; so does the CI workflow's.
#
#   packaging/windows/make-msi.sh NAME OUT_DIR BIN_DIR
#
#   NAME     what goes in the file name: the release's version (0.2.0), or a label for a test build. Letters, digits, `.`, `_` and `-` only. A tag (v0.2.0) is named
#            as its version, with no `v` in the file name.
#   OUT_DIR  where the installer is written; created when it is not there.
#   BIN_DIR  a folder holding the release programs `diacraft.exe` and `diacraft-cli.exe`.
#
# Writes OUT_DIR/diacraft-NAME-windows-CPU.msi (CPU is x64 or arm64, read from the programs, as WiX says them) and prints its path. The name follows the
# downloads' (diacraft-<version>-windows-<cpu>), which also says what CPU it is for.
#
# What the installer does is written in packaging/windows/diacraft.wxs: per user, into %LocalAppData%\Programs\DiaCraft, a Start menu shortcut, the folder on the person's
# PATH, "Open with" for .diacraft, .vsdx and .odg and never the default program. The installer's version is the plain MAJOR.MINOR.PATCH of the name (v0.2.0-rc1 is 0.2.0:
# an installer's version has no suffix), and for a name that holds none (a rehearsal's dev-<commit>) the workspace's own. The application's icon is made into an .ico
# here, from the PNG that is in the repository (an icon file is an asset, and there is one picture): a PNG inside an .ico is what Windows reads since Vista.
#
# Needs `wix` (WiX 5, which the workflows install by version and checksum) on the PATH. The environment variable DIACRAFT_WIX names another program for it, which the
# tests use. It is not called WIX: GitHub's Windows images set WIX to the folder of the WiX 3 toolset. Runs in Git Bash on Windows (or any bash, with a stand-in for WiX).
# Uses bash 3.2 syntax like the other scripts.
set -euo pipefail

script=make-msi.sh
# shellcheck source=packaging/common.sh
source "$(dirname "$0")/../common.sh"

[ $# -eq 3 ] || die "usage: make-msi.sh NAME OUT_DIR BIN_DIR"
name=$1
out=$2
bin=$3
check_name "$name"
name=$(file_version "$name")
[ -d "$bin" ] || die "no such folder: $bin"
bin=$(cd "$bin" && pwd)
wix=${DIACRAFT_WIX:-wix}
command -v "$wix" >/dev/null 2>&1 || die "wix is not here: install WiX 5 (dotnet tool install wix --version 5.0.2)"
mkdir -p "$out"
out=$(cd "$out" && pwd)
set_repo_url

arch=$(programs_arch windows "$bin")
case $arch in
x86_64) wix_arch=x64 ;;
*) wix_arch=arm64 ;;
esac
set_version_from_name "$name"

# Git Bash rewrites an argument that looks like a POSIX path before a Windows program sees it; WiX is given paths in the Windows form, and the rewriting is off.
export MSYS2_ARG_CONV_EXCL='*'
win() { # win PATH: the path as Windows writes it, where there is cygpath to say so
    if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s\n' "$1"; fi
}

stage=$(mktemp -d "${TMPDIR:-/tmp}/diacraft-msi.XXXXXX")
trap 'rm -rf "$stage"' EXIT

# The .ico: one 256 by 256 image, which is the PNG itself. An .ico is a header of 6 bytes (reserved, type 1 for an icon, 1 image), an entry of 16 (width and height 0
# for 256, no palette, 1 plane, 32 bits, the image's size, and where it starts: 22), and the image.
le32() { # le32 NUMBER: the four bytes of a number, least significant first, as a printf format
    printf '\\x%02x\\x%02x\\x%02x\\x%02x' $(($1 & 255)) $((($1 >> 8) & 255)) $((($1 >> 16) & 255)) $((($1 >> 24) & 255))
}
png=$root/assets/branding/diacraft-icon-256.png
[ -f "$png" ] || die "missing $png"
[ "$(hex_bytes "$png" 0 8)" = "89504e470d0a1a0a" ] || die "$png is not a PNG"
[ "$(hex_bytes "$png" 16 8)" = "0000010000000100" ] || die "$png is not 256 by 256 pixels"
png_size=$(wc -c <"$png" | tr -d ' ')
{
    printf '\x00\x00\x01\x00\x01\x00'
    printf '\x00\x00\x00\x00\x01\x00\x20\x00'
    # shellcheck disable=SC2059 # the format is made by le32
    printf "$(le32 "$png_size")"
    # shellcheck disable=SC2059
    printf "$(le32 22)"
    cat "$png"
} >"$stage/DiaCraft.ico"
need "$stage/DiaCraft.ico"

defines=(-d "Version=$version" -d "BinDir=$(win "$bin")" -d "RepoRoot=$(win "$root")" -d "IconFile=$(win "$stage/DiaCraft.ico")")
case $repo_url in
https://*) defines+=(-d "HomePage=$repo_url") ;;
esac

msi=$out/diacraft-$name-windows-$(file_arch windows "$arch").msi
rm -f "$msi"
# -pdbtype none: no .wixpdb is written beside the installer. The installer's checks (ICE) run, which is what -sval would turn off.
"$wix" build -nologo -arch "$wix_arch" -pdbtype none "${defines[@]}" -o "$(win "$msi")" "$(win "$root/packaging/windows/diacraft.wxs")" >&2
[ -s "$msi" ] || die "the installer was not written: $msi"
echo "$msi"
