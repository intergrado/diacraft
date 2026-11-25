#!/usr/bin/env bash
# Makes one platform's MCP bundle of DiaCraft: the `.mcpb` file that an MCP directory or an app that installs MCP servers takes, and that
# the official MCP Registry's entry points at. A bundle is a zip with a `manifest.json` that says how to start the server, and the server's program. The server is the
# command line's `mcp` command (`diacraft-cli mcp`, docs/mcp.md), so a bundle holds that one program for one system and one CPU: the format names the system a bundle is for and has no
# word for the CPU, so there is a file for each of the six builds the downloads are made from and no one file for all. The release workflow's jobs run this script after
# `package.sh`, and `mcp-bundle-check.sh` after it.
#
#   packaging/mcp-bundle.sh PLATFORM NAME OUT_DIR BIN_DIR
#
#   PLATFORM  linux, windows or macos.
#   NAME      what goes in the file name and the manifest: the release's version (0.2.2), or a label for a test build. Letters, digits, `.`, `_` and `-` only. A tag (v0.2.2) is
#             named as its version, as the downloads are. The manifest says that version, or the workspace's when NAME is a label that is no version.
#   OUT_DIR   where the bundle is written; created when it is not there.
#   BIN_DIR   a folder holding the release binary `diacraft-cli` (with `.exe` on Windows). Only that program is bundled.
#
# Writes OUT_DIR/diacraft-mcp-NAME-SYSTEM-CPU.mcpb and prints its path as the last line. The name is the downloads' with `mcp` after `diacraft` (the registry wants "mcp" in
# the address of a bundle), and the CPU is written as the downloads write it (common.sh, `file_arch`): Linux x86_64 or aarch64, Windows x64 or arm64, macOS arm64 or x86_64. The
# CPU is read from the program's own header, never from the machine the script runs on.
#
# What is in a bundle (a test holds the list): `manifest.json` (packaging/mcp/manifest.json with the version, the system, the program and the repository filled in), `icon.png`,
# `server/diacraft-cli` (`.exe` on Windows), and the five licence files that every download holds.
#
# Environment (all optional):
#   MACOS_SIGN_IDENTITY  a Developer ID Application identity to sign the macOS program with, hardened runtime and timestamp. Without it the program is signed ad hoc, which is what an
#                        Apple silicon Mac needs to run it at all (docs/releasing.md, "Signing").
#
# Needs `zip` (or PowerShell 7 on a Windows runner, which has no `zip` in Git Bash). The macOS bundle needs `lipo` and `codesign`. Uses bash 3.2 syntax, because that is the bash of macOS.
set -euo pipefail

script=mcp-bundle.sh
# shellcheck source=packaging/common.sh
source "$(dirname "$0")/common.sh"

[ $# -eq 4 ] || die "usage: mcp-bundle.sh PLATFORM NAME OUT_DIR BIN_DIR"
platform=$1
name=$2
out=$3
bin=$4

check_name "$name"
name=$(file_version "$name")

# The system as the manifest says it (`compatibility.platforms`, the word of `process.platform`), and the program's file name.
exe=""
case $platform in
linux) manifest_platform=linux ;;
windows)
    manifest_platform=win32
    exe=".exe"
    ;;
macos) manifest_platform=darwin ;;
*) die "PLATFORM must be linux, windows or macos: '$platform'" ;;
esac
program="diacraft-cli$exe"

[ -d "$bin" ] || die "no such folder: $bin"
bin=$(cd "$bin" && pwd)
cli=$bin/$program
[ -f "$cli" ] || die "missing binary $cli"

# The CPU of the program, from its header. `set -e` does not reach into a command substitution, so each failure is passed on by hand.
case $platform in
macos)
    arch=$(lipo -archs "$cli") || exit 1
    case $arch in
    arm64 | x86_64) ;;
    *) die "expected one architecture in $cli, found: $arch" ;;
    esac
    ;;
*) arch=$(binary_arch "$platform" "$cli") || exit 1 ;;
esac
file_cpu=$(file_arch "$platform" "$arch")

# What the manifest says of the version: the release's, and for a label (a rehearsal's dev-<commit>, a test build's name) the workspace's, since a manifest wants a version.
case $name in
[0-9]*.[0-9]*.[0-9]*) manifest_version=$name ;;
*) manifest_version=$(workspace_version) ;;
esac
[ -n "$manifest_version" ] || die "cannot read the workspace's version from $root/Cargo.toml"
repo=$(workspace_repository)
case $repo in
https://github.com/?*/?*) ;;
*) die "[workspace.package] repository must be an address like https://github.com/<owner>/<name>: '$repo'" ;;
esac
case $repo in
*[!A-Za-z0-9._/:-]*) die "[workspace.package] repository must be an address like https://github.com/<owner>/<name>: '$repo'" ;;
esac

mkdir -p "$out"
out=$(cd "$out" && pwd)

stage=$(mktemp -d "${TMPDIR:-/tmp}/diacraft-mcp-bundle.XXXXXX")
trap 'rm -rf "$stage"' EXIT
dir=$stage/bundle
mkdir -p "$dir/server"

cp "$cli" "$dir/server/$program"
chmod 755 "$dir/server/$program"
if [ "$platform" = macos ]; then
    # Signing: ad hoc unless an identity is given (the program of the macOS download is signed the same way, `package.sh`).
    identity=${MACOS_SIGN_IDENTITY:--}
    if [ "$identity" != "-" ]; then
        codesign --force --sign "$identity" --options runtime --timestamp "$dir/server/$program"
    else
        codesign --force --sign - "$dir/server/$program"
    fi
fi

template=$root/packaging/mcp/manifest.json
[ -f "$template" ] || die "missing $template"
sed -e "s|@VERSION@|$manifest_version|g" -e "s|@PLATFORM@|$manifest_platform|g" -e "s|@PROGRAM@|$program|g" -e "s|@REPO_URL@|$repo|g" "$template" >"$dir/manifest.json"
if grep -q '@[A-Z_]*@' "$dir/manifest.json"; then
    die "the manifest has a word that nothing fills in: $(grep -o '@[A-Z_]*@' "$dir/manifest.json" | head -n 1)"
fi

icon=$root/assets/branding/diacraft-icon-256.png
[ -f "$icon" ] || die "missing $icon"
cp "$icon" "$dir/icon.png"
add_licences "$dir"
need "$dir/manifest.json" "$dir/icon.png" "$dir/server/$program"
need_licences "$dir"

archive=$out/diacraft-mcp-$name-$platform-$file_cpu.mcpb
rm -f "$archive"
# The manifest first, then the rest, in a fixed order. `zip -X` leaves out the owner and the times of the machine; the mode of the program stays, which a bundle needs on Linux and macOS.
if command -v zip >/dev/null 2>&1; then
    # shellcheck disable=SC2086 # LICENCE_FILES is a list of names
    (cd "$dir" && zip -qr -X "$archive" manifest.json icon.png server $LICENCE_FILES)
else
    # The runner's Git Bash has no zip: PowerShell 7 writes the archive, which it will do under the name `.zip` only (entry names with forward slashes), so it is renamed.
    win() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s\n' "$1"; fi; }
    pwsh -NoProfile -NonInteractive -Command "Compress-Archive -Path '$(win "$dir")\\*' -DestinationPath '$(win "$stage/bundle.zip")' -CompressionLevel Optimal"
    mv "$stage/bundle.zip" "$archive"
fi
[ -s "$archive" ] || die "the bundle was not written: $archive"
echo "$archive"
