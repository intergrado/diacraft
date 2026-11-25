#!/usr/bin/env bash
# Makes the .deb of DiaCraft for Debian, Ubuntu and what is built on them. The release workflow's Linux jobs run it, after the archive.
#
#   packaging/linux/make-deb.sh NAME OUT_DIR BIN_DIR
#
#   NAME     what goes in the file name: the release's version (0.2.0), or a label for a test build. Letters, digits, `.`, `_` and `-` only. A tag (v0.2.0) is named
#            as its version, with no `v` in the file name.
#   OUT_DIR  where the package is written; created when it is not there.
#   BIN_DIR  a folder holding the release programs `diacraft` and `diacraft-cli`.
#
# Writes OUT_DIR/diacraft-NAME-linux-CPU.deb (CPU is x86_64 or aarch64, read from the programs; the package's own architecture field says amd64 or arm64) and prints
# its path. The name follows the downloads' (diacraft-<version>-linux-<cpu>), and not Debian's own (diacraft_<version>_<arch>.deb): apt and dpkg read the fields inside,
# never the file name.
#
# What it installs:
#   /usr/bin/diacraft, /usr/bin/diacraft-cli
#   /usr/share/applications/app.diacraft.desktop, /usr/share/icons/hicolor/256x256/apps/app.diacraft.png
#   /usr/share/doc/diacraft/   the five licence files, and `copyright`, which says the licences in the form Debian reads
#
# What it needs: the programs load these libraries by name when they start, which a tool that reads a program's link line (`dpkg-shlibdeps`) does not see. `Depends` names
# the ones the program cannot start without (the C library, the keyboard library, a window system, a graphics API), and `Recommends` the rest. The list is below and is
# checked against the programs: a library name found in them that no line here covers stops the script, so a new dependency of the program cannot leave the package
# behind.
#
# The version field is the tag's: 0.2.0, and 0.2.0~rc1 for v0.2.0-rc1 (Debian sorts a ~ before the release). A label that is no tag (a rehearsal's dev-<commit>) gives the
# workspace's version with the label after a ~, so that it sorts before the release it leads to.
#
# Needs dpkg-deb (version 1.19 or newer), and the tools every Linux has: od, tr, sed, awk, du, md5sum, gzip, install (and strip, where there is one). Uses bash 3.2 syntax
# like the other scripts.
set -euo pipefail

script=make-deb.sh
# shellcheck source=packaging/common.sh
source "$(dirname "$0")/../common.sh"

[ $# -eq 3 ] || die "usage: make-deb.sh NAME OUT_DIR BIN_DIR"
name=$1
out=$2
bin=$3
check_name "$name"
name=$(file_version "$name")
[ -d "$bin" ] || die "no such folder: $bin"
bin=$(cd "$bin" && pwd)
command -v dpkg-deb >/dev/null 2>&1 || die "dpkg-deb is not here: this script runs on Debian or Ubuntu (or in a container of one)"
mkdir -p "$out"
out=$(cd "$out" && pwd)
set_repo_url

arch=$(programs_arch linux "$bin")
case $arch in
x86_64) deb_arch=amd64 ;;
*) deb_arch=arm64 ;;
esac
linux_cpu_words "$arch"

# The Debian version of the name.
deb_version() { # deb_version NAME
    local v=${1#v}
    case $v in
    [0-9]*.[0-9]*.[0-9]*)
        # A tag: the first hyphen starts a pre-release (0.2.0-rc1 is 0.2.0~rc1), a later one is a dot.
        v=$(printf '%s' "$v" | sed -e 's/-/~/' -e 's/-/./g')
        ;;
    *)
        v=$(workspace_version)
        v="${v%%-*}~$1"
        ;;
    esac
    # Letters, digits, and . + ~ only (a Debian version also allows : and -, which a name never needs).
    printf '%s' "$v" | tr -c 'A-Za-z0-9.+~' '.'
}
version=$(deb_version "$name")
case $version in
[0-9]*) ;;
*) die "'$version' is not a Debian version: it does not start with a digit" ;;
esac

# What the package needs. `Depends`: the C library at the version the programs were built against, the library that runs the compiler's own unwinding, the keyboard
# library (both window systems use it), libxcb (the program loads it to talk to an X server: `x11rb`, with its `dl-libxcb` feature, in both CPUs' builds; a scan of the
# bytes of the x86_64 build does not show the name, which the compiler wrote into the code a few bytes at a time, and the ARM64 build's does), a window system (Wayland
# or X11) and a graphics API (Vulkan, or OpenGL through EGL). `Recommends`: what completes them, the
# graphics drivers, and the desktop's portal that the Open and Save dialogs use. The packages are named the same on Debian and Ubuntu, for both CPUs.
depends="libc6 (>= $glibc), libgcc-s1, libxkbcommon0, libxcb1, libwayland-client0 | libx11-6, libvulkan1 | libegl1"
recommends="libxkbcommon-x11-0, libx11-xcb1, libxcursor1, libxi6, libwayland-egl1, libdbus-1-3, mesa-vulkan-drivers | libgl1-mesa-dri, xdg-desktop-portal"

# The Debian package that holds a library the programs name, or nothing for a library this script does not know.
package_of_library() { # package_of_library SONAME
    case $1 in
    libc.so.6 | libm.so.6) echo libc6 ;;
    libgcc_s.so.1) echo libgcc-s1 ;;
    libxkbcommon.so.0) echo libxkbcommon0 ;;
    libxkbcommon-x11.so.0) echo libxkbcommon-x11-0 ;;
    libwayland-client.so.0) echo libwayland-client0 ;;
    libwayland-egl.so.1) echo libwayland-egl1 ;;
    libX11.so.6) echo libx11-6 ;;
    libX11-xcb.so.1) echo libx11-xcb1 ;;
    libxcb.so.1) echo libxcb1 ;;
    libXcursor.so.1) echo libxcursor1 ;;
    libXi.so.6) echo libxi6 ;;
    libEGL.so.1) echo libegl1 ;;
    libvulkan.so.1) echo libvulkan1 ;;
    libdbus-1.so.3) echo libdbus-1-3 ;;
    *) return 1 ;;
    esac
}
named=$(printf '%s %s' "$depends" "$recommends" | tr -c 'a-z0-9.+-' ' ')
for program in diacraft diacraft-cli; do
    for lib in $(LC_ALL=C grep -a -o -E 'lib[A-Za-z0-9_+-]+\.so\.[0-9]+' "$bin/$program" | sort -u); do
        pkg=$(package_of_library "$lib") || die "$program names the library $lib, which this script does not know: add it to package_of_library, and to Depends or Recommends"
        case " $named " in
        *" $pkg "*) ;;
        *) die "$program names the library $lib, of the package $pkg, which neither Depends nor Recommends names" ;;
        esac
    done
done

stage=$(mktemp -d "${TMPDIR:-/tmp}/diacraft-deb.XXXXXX")
trap 'rm -rf "$stage"' EXIT
pkg=$stage/pkg
docs=$pkg/usr/share/doc/diacraft
install -d -m 755 "$pkg/DEBIAN" "$pkg/usr/bin" "$pkg/usr/share/applications" "$pkg/usr/share/icons/hicolor/256x256/apps" "$docs"

install -m 755 "$bin/diacraft" "$bin/diacraft-cli" "$pkg/usr/bin/"
# Debian packages hold stripped programs: the symbols are of no use to a person, and the program is a good deal smaller. A machine without `strip` makes a larger
# package that works as well, and so does a package for the other CPU than the machine's: the machine's `strip` reads the programs of its own CPU only (the release
# workflow builds each CPU's package on a runner of that CPU). A program of the machine's CPU that `strip` cannot read is no program, and stops the script.
machine_arch=$(uname -m)
case $machine_arch in
aarch64) machine_arch=arm64 ;;
esac
if [ "$arch" != "$machine_arch" ]; then
    echo "make-deb.sh: the programs are for $arch and this machine is $machine_arch: they are packaged as they are, not stripped" >&2
elif command -v strip >/dev/null 2>&1; then
    for program in diacraft diacraft-cli; do
        strip --strip-unneeded "$pkg/usr/bin/$program" || die "strip could not read $program, which says it is a program of this machine's CPU ($arch)"
    done
else
    echo "make-deb.sh: strip is not here: the programs in the package keep their symbols" >&2
fi
# The launcher entry as it is in the download: it starts `diacraft` by name, which is on the PATH of a system install.
install -m 644 "$root/packaging/linux/app.diacraft.desktop" "$pkg/usr/share/applications/app.diacraft.desktop"
install -m 644 "$root/assets/branding/diacraft-icon-256.png" "$pkg/usr/share/icons/hicolor/256x256/apps/app.diacraft.png"
add_licences "$docs"
chmod 644 "$docs"/*
need_licences "$docs"

cat >"$docs/copyright" <<COPYRIGHT
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: DiaCraft
Source: $repo_url

Files: *
Copyright: 2026 the DiaCraft contributors
License: MIT or Apache-2.0
 DiaCraft is offered under the MIT licence or the Apache licence 2.0, at your
 option. The texts are in /usr/share/doc/diacraft/LICENSE-MIT and
 /usr/share/doc/diacraft/LICENSE-APACHE. The credits are in NOTICE and the
 attributions of the pictures and fonts in ATTRIBUTION.md, in the same folder;
 the licences of the libraries the programs are built from are in
 THIRD-PARTY-LICENSES.txt.
COPYRIGHT
chmod 644 "$docs/copyright"
# The changelog every Debian package holds: for this package, the release's own page says what changed.
changelog_date=$(date -u -R)
{
    echo "diacraft ($version) unstable; urgency=medium"
    echo
    echo "  * DiaCraft $name."
    case $repo_url in
    https://*) echo "    What changed is in the notes of the release, at $repo_url/releases" ;;
    *) echo "    What changed is in the notes of the release." ;;
    esac
    echo
    echo " -- ${DEB_MAINTAINER:-DiaCraft <noreply@diacraft.app>}  $changelog_date"
} | gzip -9n >"$docs/changelog.gz"
chmod 644 "$docs/changelog.gz"

# The sums of every file, for `debsums` and `dpkg --verify`.
(cd "$pkg" && find usr -type f -print0 | sort -z | xargs -0 md5sum >DEBIAN/md5sums)
chmod 644 "$pkg/DEBIAN/md5sums"
installed_size=$(du -sk "$pkg/usr" | awk '{ print $1 }')
{
    echo "Package: diacraft"
    echo "Version: $version"
    echo "Architecture: $deb_arch"
    echo "Maintainer: ${DEB_MAINTAINER:-DiaCraft <noreply@diacraft.app>}"
    echo "Installed-Size: $installed_size"
    echo "Depends: $depends"
    echo "Recommends: $recommends"
    echo "Section: graphics"
    echo "Priority: optional"
    case $repo_url in https://*) echo "Homepage: $repo_url" ;; esac
    echo "Description: diagram editor"
    echo " DiaCraft is a free, open-source diagram editor. It opens and saves"
    echo " drawings in the .diacraft, .vsdx and .odg formats, and draws them as PNG,"
    echo " SVG and PDF."
    echo " ."
    echo " The package holds the drawing application, diacraft, and the command"
    echo " line, diacraft-cli, which converts and exports drawings, runs commands"
    echo " and serves MCP."
} >"$pkg/DEBIAN/control"
chmod 644 "$pkg/DEBIAN/control"

deb=$out/diacraft-$name-linux-$(file_arch linux "$arch").deb
rm -f "$deb"
# Owned by root in the package whoever builds it; xz, which every dpkg since Debian 8 reads (a newer compression is not read by an older apt).
dpkg-deb --root-owner-group -Zxz --build "$pkg" "$deb" >/dev/null
[ -s "$deb" ] || die "the package was not written: $deb"
echo "$deb"
