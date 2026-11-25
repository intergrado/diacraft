#!/usr/bin/env bash
# Stages one platform's DiaCraft download and archives it. The release workflow and the Windows and macOS jobs of the CI workflow
# run this one script, so a package that the release would make is made on every push too.
#
#   packaging/package.sh PLATFORM NAME OUT_DIR BIN_DIR...
#
#   PLATFORM  linux, windows or macos.
#   NAME      what goes in the file name: the release's version (0.2.0), or a label for a test build. Letters, digits, `.`, `_` and `-` only. A tag (v0.2.0) is
#             named as its version: a launcher wants the version to start with a digit, so no file name has the tag's `v`.
#   OUT_DIR   where the archive is written; created when it is not there.
#   BIN_DIR   a folder holding the release binaries `diacraft` and `diacraft-cli` (with `.exe` on Windows): one folder, of one architecture. There is no
#             universal macOS download (the owner, 2026-11-25): each architecture's programs are packaged on their own.
#
# Writes (the last line printed is the archive's path; macOS also writes a disk image, and prints its path before it):
#   Linux    diacraft-NAME-linux-CPU.tar.gz      a folder holding both programs, the launcher entry, the icon, an install script, the licences (LICENSE-MIT,
#                                                LICENSE-APACHE), NOTICE, ATTRIBUTION.md, THIRD-PARTY-LICENSES.txt, a README
#   Windows  diacraft-NAME-windows-CPU-portable.zip   a folder holding both .exe files, the same licence files, a README (the folder is diacraft-NAME-windows-CPU)
#   macOS    diacraft-NAME-macos-CPU.zip         a folder holding DiaCraft.app (with its icon, and the licence files in its Resources), diacraft-cli, the licence
#                                                files, a README
#            diacraft-NAME-macos-CPU.dmg         a disk image of that same folder, with a link to Applications beside the app
#            CPU is read from the header of the program (never from the machine the script runs on, which may be a cross build's), and written as a file name has it
#            (common.sh, `file_arch`): Linux x86_64 or aarch64, Windows x64 or arm64, macOS x86_64 or arm64.
#
# Environment (all optional):
#   DIACRAFT_REPOSITORY  owner/name of the GitHub repository, for the link in the README.
#   MACOS_SIGN_IDENTITY  a Developer ID Application identity to sign with, hardened runtime and timestamp. Without it the app and the CLI are
#                        signed ad hoc, which is what an arm64 Mac needs to run them at all, and macOS still asks the person to confirm the first
#                        opening (docs/releasing.md, "Signing").
#
# The script uses bash 3.2 syntax, because that is the bash of macOS. What it shares with the installer scripts (the checks of a name, the licence files, the
# architecture read from a program's header) is in packaging/common.sh.
set -euo pipefail

script=package.sh
# shellcheck source=packaging/common.sh
source "$(dirname "$0")/common.sh"

[ $# -ge 4 ] || die "usage: package.sh PLATFORM NAME OUT_DIR BIN_DIR..."
platform=$1
name=$2
out=$3
shift 3

check_name "$name"
name=$(file_version "$name")

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

# The README of the platform, with the release's name and the repository's address filled in, and what it says of the CPU its download is for: @ARCH@ (x86_64 or
# arm64), @FILEARCH@ (the same CPU as the names of the files say it: aarch64, x64), @CPU@ (Intel or AMD, or ARM; for macOS Apple silicon and Intel, Apple silicon, or Intel), for macOS @CHIP@ (the Macs the download runs on), and for Linux @GLIBC@ (the oldest
# C library the programs run on) and @DISTROS@ (distributions that have one that new). A platform sets those once it has read the architecture of the programs.
set_repo_url
arch=""
file_cpu=""
cpu=""
glibc=""
distros=""
chip=""
render_readme() { # render_readme SOURCE DEST
    [ -f "$1" ] || die "missing $1"
    sed -e "s|@NAME@|$name|g" -e "s|@REPO_URL@|$repo_url|g" -e "s|@ARCH@|$arch|g" -e "s|@FILEARCH@|$file_cpu|g" -e "s|@CPU@|$cpu|g" -e "s|@CHIP@|$chip|g" -e "s|@GLIBC@|$glibc|g" \
        -e "s|@DISTROS@|$distros|g" "$1" >"$2"
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

# The version of the macOS bundle: the tag's (v0.2.0), and for a name that holds none (a test build's label, a rehearsal's dev-<commit>) the workspace's own.
set_version_from_name "$name"

case $platform in
linux)
    [ ${#bin_dirs[@]} -eq 1 ] || die "linux takes one BIN_DIR"
    arch=$(programs_arch linux "${bin_dirs[0]}")
    file_cpu=$(file_arch linux "$arch")
    linux_cpu_words "$arch"
    base="diacraft-$name-linux-$file_cpu"
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
    arch=$(programs_arch windows "${bin_dirs[0]}")
    file_cpu=$(file_arch windows "$arch")
    case $arch in
    x86_64) cpu="Intel or AMD" ;;
    *) cpu="ARM" ;;
    esac
    base="diacraft-$name-windows-$file_cpu"
    dir=$stage/$base
    mkdir -p "$dir"
    for b in diacraft diacraft-cli; do
        cp "$(find_bin "$b")" "$dir/$b.exe"
    done
    render_readme "$root/packaging/windows/README.txt" "$dir/README.txt"
    add_licences "$dir"
    need "$dir/diacraft.exe" "$dir/diacraft-cli.exe" "$dir/README.txt"
    need_licences "$dir"
    # The zip holds the whole program in a folder, and needs no installer, so its name says `portable` (the installer, the .msi, has the name without it).
    archive=$out/$base-portable.zip
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
    [ ${#bin_dirs[@]} -eq 1 ] || die "macos takes one BIN_DIR: a download is for one architecture"
    base_bins=$(find_bin diacraft)
    arch=$(lipo -archs "$base_bins")
    case $arch in
    arm64 | x86_64) ;;
    *) die "expected one architecture in $base_bins, found: $arch" ;;
    esac
    # What the README says of the Macs the download is for.
    case $arch in
    arm64)
        cpu="Apple silicon"
        chip="a Mac with an Apple silicon chip (M1 or later)"
        ;;
    *)
        cpu="Intel"
        chip="a Mac with an Intel chip"
        ;;
    esac
    file_cpu=$(file_arch macos "$arch")
    base="diacraft-$name-macos-$file_cpu"
    dir=$stage/$base
    app=$dir/DiaCraft.app
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

    # The program as it is, one architecture.
    join_bin() { # join_bin NAME DEST
        cp "$(find_bin "$1")" "$2"
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

    # Signing: ad hoc unless an identity is given. An arm64 program must carry a signature to run at all.
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

    # The disk image: the same folder, with a link to Applications beside the app, so that the app is dragged onto it. ditto copies what a bundle needs
    # (permissions, the signature, extended attributes). The image is a compressed (UDZO) HFS+ volume, which every macOS the app runs on mounts. A person who opens it sees
    # the app, the link, the command line, the licences and the README, in the order Finder gives them: there is no background picture, which a Finder script would draw.
    dmg=$out/$base.dmg
    dmg_root=$stage/dmg-root
    mkdir -p "$dmg_root"
    ditto "$dir" "$dmg_root"
    ln -s /Applications "$dmg_root/Applications"
    rm -f "$dmg"
    attempt=1
    # hdiutil sometimes answers "Resource busy" on a runner that has just made another image: it is asked again, twice at most.
    until hdiutil create -quiet -volname DiaCraft -fs HFS+ -format UDZO -srcfolder "$dmg_root" -ov "$dmg"; do
        [ "$attempt" -lt 3 ] || die "hdiutil could not make the disk image"
        attempt=$((attempt + 1))
        sleep 5
    done
    if [ "$identity" != "-" ]; then
        # A signed image is the one that can be notarised and stapled (docs/releasing.md, "Signing"): ad hoc, it has nothing to add to the signed app inside.
        codesign --force --sign "$identity" --timestamp "$dmg"
    fi
    [ -s "$dmg" ] || die "the disk image was not written: $dmg"
    ;;

*)
    die "PLATFORM must be linux, windows or macos: '$platform'"
    ;;
esac

[ -s "$archive" ] || die "the archive was not written: $archive"
# macOS makes a disk image too, which is printed first: the last line is always the archive.
if [ -n "${dmg:-}" ]; then
    echo "$dmg"
fi
echo "$archive"
