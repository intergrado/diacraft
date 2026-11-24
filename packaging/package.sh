#!/usr/bin/env bash
# Stages one platform's DiaCraft download and archives it. The release workflow and the Windows and macOS jobs of the CI workflow
# run this one script, so a package that the release would make is made on every push too.
#
#   packaging/package.sh PLATFORM NAME OUT_DIR BIN_DIR...
#
#   PLATFORM  linux, windows or macos.
#   NAME      what goes in the file name: the release tag (v0.2.0), or a label for a test build. Letters, digits, `.`, `_` and `-` only.
#   OUT_DIR   where the archive is written; created when it is not there.
#   BIN_DIR   a folder holding the release binaries `diacraft` and `diacraft-cli` (with `.exe` on Windows). macOS takes one folder for each
#             architecture and joins them into universal binaries with lipo; the other platforms take one.
#
# Writes (the last line printed is the archive's path):
#   Linux    diacraft-NAME-linux-ARCH.tar.gz     a folder holding both programs, the launcher entry, the icon, an install script, the licences (LICENSE-MIT,
#                                                LICENSE-APACHE), NOTICE, ATTRIBUTION.md, THIRD-PARTY-LICENSES.txt, a README
#   Windows  diacraft-NAME-windows-ARCH.zip      a folder holding both .exe files, the same licence files, a README
#   macOS    diacraft-NAME-macos-ARCH.zip        a folder holding DiaCraft.app (with its icon, and the licence files in its Resources), diacraft-cli, the licence
#                                                files, a README
#            ARCH is universal for two or more BIN_DIRs, else the architecture of the binary (arm64 or x86_64).
#
# Environment (all optional):
#   DIACRAFT_REPOSITORY  owner/name of the GitHub repository, for the link in the README.
#   MACOS_SIGN_IDENTITY  a Developer ID Application identity to sign with, hardened runtime and timestamp. Without it the app and the CLI are
#                        signed ad hoc, which is what an arm64 Mac needs to run them at all, and macOS still asks the person to confirm the first
#                        opening (docs/releasing.md, "Signing").
#
# The script uses bash 3.2 syntax, because that is the bash of macOS.
set -euo pipefail

die() {
    echo "package.sh: $*" >&2
    exit 1
}

[ $# -ge 4 ] || die "usage: package.sh PLATFORM NAME OUT_DIR BIN_DIR..."
platform=$1
name=$2
out=$3
shift 3

case $name in
'' | *[!A-Za-z0-9._-]*) die "NAME must be letters, digits, '.', '_' and '-' only: '$name'" ;;
esac

root=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$out"
out=$(cd "$out" && pwd)

# The folders of binaries, made absolute.
bin_dirs=()
for d in "$@"; do
    [ -d "$d" ] || die "no such folder: $d"
    bin_dirs+=("$(cd "$d" && pwd)")
done

stage=$(mktemp -d "${TMPDIR:-/tmp}/diacraft-package.XXXXXX")
trap 'rm -rf "$stage"' EXIT

# The README of the platform, with the release's name and the repository's address filled in.
repo_url="the repository this download came from"
if [ -n "${DIACRAFT_REPOSITORY:-}" ]; then
    case $DIACRAFT_REPOSITORY in
    *[!A-Za-z0-9._/-]*) die "DIACRAFT_REPOSITORY must be owner/name: '$DIACRAFT_REPOSITORY'" ;;
    esac
    repo_owner=${DIACRAFT_REPOSITORY%%/*}
    repo_name=${DIACRAFT_REPOSITORY#*/}
    if [ -z "$repo_owner" ] || [ "$repo_owner" = "$DIACRAFT_REPOSITORY" ] || [ -z "$repo_name" ]; then
        die "DIACRAFT_REPOSITORY must be owner/name: '$DIACRAFT_REPOSITORY'"
    fi
    case $repo_name in
    */*) die "DIACRAFT_REPOSITORY must be owner/name: '$DIACRAFT_REPOSITORY'" ;;
    esac
    repo_url="https://github.com/$DIACRAFT_REPOSITORY"
fi
render_readme() { # render_readme SOURCE DEST
    [ -f "$1" ] || die "missing $1"
    sed -e "s|@NAME@|$name|g" -e "s|@REPO_URL@|$repo_url|g" "$1" >"$2"
}

# The licences and the notices that go in every package: the two licences DiaCraft is offered under, the credits, the attributions of the art and the fonts, and the
# licence texts of the third-party crates the programs are built from (made by `cargo xtask licenses`, kept in the repository, checked by `licenses --check`).
LICENCE_FILES="LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md THIRD-PARTY-LICENSES.txt"
add_licences() { # add_licences DIR
    for f in $LICENCE_FILES; do
        [ -s "$root/$f" ] || die "missing or empty $root/$f"
        cp "$root/$f" "$1/$f"
    done
}

# The binary of a given name in the first folder; Windows programs end in .exe.
exe_suffix=""
if [ "$platform" = windows ]; then
    exe_suffix=".exe"
fi
find_bin() { # find_bin NAME [DIR]
    local p="${2:-${bin_dirs[0]}}/$1$exe_suffix"
    [ -f "$p" ] || die "missing binary $p"
    printf '%s\n' "$p"
}

need() { # need FILE...: each is there and is not empty
    for f in "$@"; do
        [ -s "$f" ] || die "the package is missing $f, or it is empty"
    done
}
need_licences() { # need_licences DIR
    for f in $LICENCE_FILES; do
        need "$1/$f"
    done
}

# The version of the macOS bundle: the tag's (v0.2.0), and for a name that holds none (a test build's label, a rehearsal's dev-<commit>) the workspace's own, which is
# the one source of DiaCraft's version (`cargo xtask version`) and which the release workflow holds equal to the tag.
workspace_version() {
    awk '/^\[/ { in_package = ($0 == "[workspace.package]") } in_package && /^version[ \t]*=/ { v = $0; sub(/^[^"]*"/, "", v); sub(/".*$/, "", v); print v; exit }' "$root/Cargo.toml"
}
version=${name#v}
version=${version%%-*}
case $version in
[0-9]*.[0-9]*.[0-9]*) ;;
*)
    version=$(workspace_version)
    version=${version%%-*}
    case $version in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) version=0.0.0 ;;
    esac
    ;;
esac

case $platform in
linux)
    [ ${#bin_dirs[@]} -eq 1 ] || die "linux takes one BIN_DIR"
    arch=$(uname -m)
    base="diacraft-$name-linux-$arch"
    dir=$stage/$base
    mkdir -p "$dir"
    for b in diacraft diacraft-cli; do
        cp "$(find_bin "$b")" "$dir/$b"
        chmod 755 "$dir/$b"
    done
    cp "$root/packaging/linux/app.diacraft.desktop" "$dir/"
    cp "$root/assets/branding/diacraft-icon-256.png" "$dir/app.diacraft.png"
    cp "$root/packaging/linux/install.sh" "$dir/install.sh"
    chmod 755 "$dir/install.sh"
    render_readme "$root/packaging/linux/README.txt" "$dir/README.txt"
    add_licences "$dir"
    need "$dir/diacraft" "$dir/diacraft-cli" "$dir/app.diacraft.desktop" "$dir/app.diacraft.png" "$dir/install.sh" \
        "$dir/README.txt"
    need_licences "$dir"
    archive=$out/$base.tar.gz
    # A fixed order, no owner in the tar entries, and no name or time in the gzip header.
    (cd "$stage" && tar --sort=name --owner=0 --group=0 --numeric-owner -cf - "$base") | gzip -9n >"$archive"
    ;;

windows)
    [ ${#bin_dirs[@]} -eq 1 ] || die "windows takes one BIN_DIR"
    arch=x86_64
    base="diacraft-$name-windows-$arch"
    dir=$stage/$base
    mkdir -p "$dir"
    for b in diacraft diacraft-cli; do
        cp "$(find_bin "$b")" "$dir/$b.exe"
    done
    render_readme "$root/packaging/windows/README.txt" "$dir/README.txt"
    add_licences "$dir"
    need "$dir/diacraft.exe" "$dir/diacraft-cli.exe" "$dir/README.txt"
    need_licences "$dir"
    archive=$out/$base.zip
    rm -f "$archive"
    if command -v zip >/dev/null 2>&1; then
        (cd "$stage" && zip -qr -X "$archive" "$base")
    else
        # The runner's Git Bash has no zip: PowerShell 7 writes the archive (with forward slashes in the entry names).
        win() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s\n' "$1"; fi; }
        pwsh -NoProfile -NonInteractive -Command "Compress-Archive -Path '$(win "$dir")' -DestinationPath '$(win "$archive")' -CompressionLevel Optimal"
    fi
    ;;

macos)
    [ ${#bin_dirs[@]} -ge 1 ] || die "macos takes a BIN_DIR for each architecture"
    base_bins=$(find_bin diacraft)
    if [ ${#bin_dirs[@]} -ge 2 ]; then
        arch=universal
    else
        arch=$(lipo -archs "$base_bins")
        case $arch in
        arm64 | x86_64) ;;
        *) die "expected one architecture in $base_bins, found: $arch" ;;
        esac
    fi
    base="diacraft-$name-macos-$arch"
    dir=$stage/$base
    app=$dir/DiaCraft.app
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

    # One architecture: the binary as it is. Several: one universal binary joined from them.
    join_bin() { # join_bin NAME DEST
        if [ ${#bin_dirs[@]} -eq 1 ]; then
            cp "$(find_bin "$1")" "$2"
        else
            inputs=()
            for d in "${bin_dirs[@]}"; do inputs+=("$(find_bin "$1" "$d")"); done
            lipo -create -output "$2" "${inputs[@]}"
        fi
        chmod 755 "$2"
    }
    # The program in the app is named as the app, DiaCraft, the CFBundleExecutable of Info.plist: the app menu names its items after the
    # program's file name ("About DiaCraft", "Hide DiaCraft", "Quit DiaCraft"). The command line keeps its name.
    join_bin diacraft "$app/Contents/MacOS/DiaCraft"
    join_bin diacraft-cli "$dir/diacraft-cli"

    sed -e "s|@VERSION@|$version|g" "$root/packaging/macos/Info.plist" >"$app/Contents/Info.plist"

    # The icon: an .icns made from the original 1254-pixel icon, with the tools macOS has. Not kept in the repository (an icon file is an asset).
    iconset=$stage/DiaCraft.iconset
    mkdir -p "$iconset"
    master=$root/assets/branding/diacraft-icon-master.png
    [ -f "$master" ] || die "missing $master"
    for spec in 16:icon_16x16 32:icon_16x16@2x 32:icon_32x32 64:icon_32x32@2x 128:icon_128x128 256:icon_128x128@2x \
        256:icon_256x256 512:icon_256x256@2x 512:icon_512x512 1024:icon_512x512@2x; do
        sips -z "${spec%%:*}" "${spec%%:*}" "$master" --out "$iconset/${spec#*:}.png" >/dev/null
    done
    iconutil -c icns "$iconset" -o "$app/Contents/Resources/DiaCraft.icns"

    # Signing: ad hoc unless an identity is given. An arm64 program must carry a signature to run at all, and joining slices with lipo drops the linker's.
    identity=${MACOS_SIGN_IDENTITY:--}
    sign_options=()
    if [ "$identity" != "-" ]; then
        sign_options=(--options runtime --timestamp)
    fi
    sign() { # sign PATH
        if [ ${#sign_options[@]} -gt 0 ]; then
            codesign --force --sign "$identity" "${sign_options[@]}" "$1"
        else
            codesign --force --sign "$identity" "$1"
        fi
    }
    # The licences go inside the app too, in its Resources, so that they stay with the program when the app is dragged to the Applications folder alone.
    # They are put there before the app is signed: the signature covers the resources.
    add_licences "$app/Contents/Resources"
    sign "$dir/diacraft-cli"
    sign "$app/Contents/MacOS/DiaCraft"
    sign "$app"

    render_readme "$root/packaging/macos/README.txt" "$dir/README.txt"
    add_licences "$dir"
    need "$app/Contents/MacOS/DiaCraft" "$app/Contents/Info.plist" "$app/Contents/Resources/DiaCraft.icns" "$dir/diacraft-cli" "$dir/README.txt"
    need_licences "$dir"
    need_licences "$app/Contents/Resources"
    archive=$out/$base.zip
    rm -f "$archive"
    # ditto keeps what a bundle needs (permissions, signatures, extended attributes) and puts the folder in the archive as its top level.
    ditto -c -k --keepParent "$dir" "$archive"
    ;;

*)
    die "PLATFORM must be linux, windows or macos: '$platform'"
    ;;
esac

[ -s "$archive" ] || die "the archive was not written: $archive"
echo "$archive"
