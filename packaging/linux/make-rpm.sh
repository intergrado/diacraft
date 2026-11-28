#!/usr/bin/env bash
# Makes the .rpm of DiaCraft for Fedora. The release workflow's Linux jobs run it in a Fedora container, after the .deb, by packaging/linux/rpm-in-fedora.sh.
#
#   packaging/linux/make-rpm.sh NAME OUT_DIR BIN_DIR
#
#   NAME     what goes in the file name: the release's version (0.2.4), or a label for a test build. Letters, digits, `.`, `_` and `-` only. A tag (v0.2.4) is named
#            as its version, with no `v` in the file name.
#   OUT_DIR  where the package is written; created when it is not there.
#   BIN_DIR  a folder holding the release programs `diacraft` and `diacraft-cli`.
#
# Writes OUT_DIR/diacraft-NAME-linux-CPU.rpm (CPU is x86_64 or aarch64, read from the programs, which is also the package's own architecture) and prints its path. The name follows
# the downloads' (diacraft-<version>-linux-<cpu>), and not Fedora's own (diacraft-<version>-<release>.<arch>.rpm): dnf and rpm read the fields inside, never the file name.
#
# What it installs (the spec is packaging/linux/diacraft.spec, which the script gives the macros it needs and the changelog entry):
#   /usr/bin/diacraft, /usr/bin/diacraft-cli
#   /usr/share/applications/app.diacraft.desktop, /usr/share/icons/hicolor/256x256/apps/app.diacraft.png
#   /usr/share/licenses/diacraft/   the five licence files (`%license`, where Fedora keeps them; the licence field says MIT OR Apache-2.0)
#   the shell completions of both programs for bash, zsh and fish (/usr/share/bash-completion/completions, /usr/share/zsh/site-functions, /usr/share/fish/vendor_completions.d)
#   and the manual pages (/usr/share/man/man1/diacraft*.1.gz). The command line writes them (`diacraft-cli completions SHELL --program NAME`, `diacraft-cli man PAGE`), so
#   they list what the build does. That needs a program that runs here and has those commands: a package for the other CPU than the machine's, and one of a build that has no
#   such commands, is made without them, and the script says so.
# The zsh completions go to site-functions, which Fedora's zsh reads: Debian's folder for them (/usr/share/zsh/vendor-completions) is none of Fedora's.
#
# What it needs: the programs link only the C library, the math library and libgcc_s, which rpm finds in them and turns into Requires itself (with the version of each symbol the
# build needs). They load the other libraries by name when they start, and the spec names those by soname, with no version (a Requires that Fedora's package of the library
# answers on every Fedora and for both CPUs). The script reads the names out of the programs and stops when one is no soname that a Requires or Recommends line of the spec
# names, so that a new dependency of the program cannot leave the package behind.
#
# The version field is the tag's: 0.2.4, and 0.2.4~rc1 for v0.2.4-rc1 (rpm sorts a ~ before the release, as Fedora's guidelines use it for a pre-release). A label that is no tag (a
# rehearsal's dev-<commit>) gives the workspace's version with the label after a ~, so that it sorts before the release it leads to. The release field is 1.
#
# Needs rpmbuild (from the package rpm-build, with redhat-rpm-config on a Fedora) and desktop-file-validate (desktop-file-utils), which the spec's %check runs on the launcher
# entry, and the tools every Linux has: od, tr, sed, awk, grep, gzip, install, find (and strip, where there is one). Uses bash 3.2 syntax like the other scripts.
#
# Environment (all optional): DIACRAFT_REPOSITORY (owner/name, for the package's home page), RPM_PACKAGER (the name and address in the changelog), and EXTRAS_CLI (the program to ask
# for the completions and the manual pages instead of the packaged diacraft-cli: for the tests of this script).
set -euo pipefail

script=make-rpm.sh
# shellcheck source=packaging/common.sh
source "$(dirname "$0")/../common.sh"

[ $# -eq 3 ] || die "usage: make-rpm.sh NAME OUT_DIR BIN_DIR"
name=$1
out=$2
bin=$3
check_name "$name"
name=$(file_version "$name")
[ -d "$bin" ] || die "no such folder: $bin"
bin=$(cd "$bin" && pwd)
command -v rpmbuild >/dev/null 2>&1 || die "rpmbuild is not here: this script runs on Fedora (or in a container of it: packaging/linux/rpm-in-fedora.sh)"
mkdir -p "$out"
out=$(cd "$out" && pwd)
set_repo_url

arch=$(programs_arch linux "$bin")
case $arch in
x86_64) rpm_arch=x86_64 ;;
*) rpm_arch=aarch64 ;;
esac
spec=$root/packaging/linux/diacraft.spec
[ -s "$spec" ] || die "missing $spec"

version=$(package_version "$name")
case $version in
[0-9]*) ;;
*) die "'$version' is not an RPM version: it does not start with a digit" ;;
esac
# The home page: the repository the build ran in, and the workspace's own public address when the build is no run of the workflow (a person's machine).
case $repo_url in
https://*) url=$repo_url ;;
*) url=$(workspace_repository) ;;
esac
case $url in
https://*) ;;
*) url=https://diacraft.dev ;;
esac

# The libraries the programs name that the spec does not cover. rpm gives the package the Requires for what the programs are linked with (these three), so every other name must be
# one a Requires or a Recommends line of the spec names. The names are read out of the bytes of the programs; a name can follow the text `lib64` with no byte between (the string
# table of a build on Fedora has `lib64libXi.so.6`), which is not part of it.
covered=$(grep -E '^(Requires|Recommends):' "$spec" | LC_ALL=C grep -o -E 'lib[A-Za-z0-9_+-]+\.so\.[0-9]+' | sort -u)
[ -n "$covered" ] || die "$spec names no library in its Requires and Recommends lines"
# The names are looked up in the text and not through `printf | grep -q`: under `pipefail` a `grep -q` that is done at its first match and leaves the pipe while `printf`
# still has lines to write kills the writer with a broken pipe, and the match becomes a failure (the script then said that a library the spec names was not named, once in a few
# dozen runs on a busy machine).
newline='
'
for program in diacraft diacraft-cli; do
    # shellcheck disable=SC2013 # a library's name has no space in it, so that it is read a word at a time
    for lib in $(LC_ALL=C grep -a -o -E 'lib[A-Za-z0-9_+-]+\.so\.[0-9]+' "$bin/$program" | sed -e 's/^lib64lib/lib/' | sort -u); do
        case $lib in
        libc.so.6 | libm.so.6 | libgcc_s.so.1) continue ;;
        esac
        case "$newline$covered$newline" in
        *"$newline$lib$newline"*) ;;
        *) die "$program names the library $lib, which no Requires or Recommends line of $spec names: add one, with the soname as the others have it" ;;
        esac
    done
done

stage=$(mktemp -d "${TMPDIR:-/tmp}/diacraft-rpm.XXXXXX")
trap 'rm -rf "$stage"' EXIT
# rpmbuild's build directory (`--build-in-place` builds where it is told to): the licence files, the spec, and `files/`, the tree the package puts under `/`.
build=$stage/build
files=$build/files
share=$files/usr/share
install -d -m 755 "$files/usr/bin" "$share/applications" "$share/icons/hicolor/256x256/apps"

install -m 755 "$bin/diacraft" "$bin/diacraft-cli" "$files/usr/bin/"
# The programs are stripped, as the .deb's are: the symbols are of no use to a person, and the program is a good deal smaller. The machine's `strip` reads the programs of its own CPU
# only (the release workflow builds each CPU's package on a runner of that CPU), so a package for the other CPU packages them as they are. A program of the machine's CPU that
# `strip` cannot read is no program, and stops the script.
machine_arch=$(uname -m)
case $machine_arch in
aarch64) machine_arch=arm64 ;;
esac
if [ "$arch" != "$machine_arch" ]; then
    echo "make-rpm.sh: the programs are for $arch and this machine is $machine_arch: they are packaged as they are, not stripped" >&2
elif command -v strip >/dev/null 2>&1; then
    for program in diacraft diacraft-cli; do
        strip --strip-unneeded "$files/usr/bin/$program" || die "strip could not read $program, which says it is a program of this machine's CPU ($arch)"
    done
else
    echo "make-rpm.sh: strip is not here: the programs in the package keep their symbols" >&2
fi

# The shell completions and the manual pages, which the command line writes. The program that writes them is the one just built, before it was stripped, and it runs only
# when it is the machine's own CPU's and has the commands: a program that answers `completions bash` with nothing (an error, or a program that is none) has none to write.
extras=""
cli=${EXTRAS_CLI:-$bin/diacraft-cli}
if [ "$arch" != "$machine_arch" ] && [ -z "${EXTRAS_CLI:-}" ]; then
    echo "make-rpm.sh: the programs are for $arch and this machine is $machine_arch: the package is made without shell completions and manual pages (they are written by running the program)" >&2
elif [ -n "$("$cli" completions bash --program diacraft-cli 2>/dev/null || true)" ]; then
    extras=1
    install -d -m 755 "$share/bash-completion/completions" "$share/zsh/site-functions" "$share/fish/vendor_completions.d" "$share/man/man1"
    for program in diacraft-cli diacraft; do
        "$cli" completions bash --program "$program" >"$share/bash-completion/completions/$program" || die "diacraft-cli could not write the bash completions of $program"
        "$cli" completions zsh --program "$program" >"$share/zsh/site-functions/_$program" || die "diacraft-cli could not write the zsh completions of $program"
        "$cli" completions fish --program "$program" >"$share/fish/vendor_completions.d/$program.fish" || die "diacraft-cli could not write the fish completions of $program"
        need "$share/bash-completion/completions/$program" "$share/zsh/site-functions/_$program" "$share/fish/vendor_completions.d/$program.fish"
    done
    pages=$("$cli" man --list) || die "diacraft-cli could not list its manual pages"
    [ -n "$pages" ] || die "diacraft-cli lists no manual page"
    for page in $pages; do
        case $page in
        diacraft*) ;;
        *) die "the manual page '$page' is not named diacraft*, which the spec's file list (the %files section) has for them" ;;
        esac
        "$cli" man "$page" | gzip -9n >"$share/man/man1/$page.1.gz" || die "diacraft-cli could not write the manual page $page"
        need "$share/man/man1/$page.1.gz"
    done
    chmod 644 "$share"/bash-completion/completions/* "$share"/zsh/site-functions/* "$share"/fish/vendor_completions.d/* "$share"/man/man1/*
else
    echo "make-rpm.sh: this diacraft-cli has no completions and manual pages to write: the package is made without them" >&2
fi

# The launcher entry as it is in the download: it starts `diacraft` by name, which is on the PATH of a system install.
install -m 644 "$root/packaging/linux/app.diacraft.desktop" "$share/applications/app.diacraft.desktop"
install -m 644 "$root/assets/branding/diacraft-icon-256.png" "$share/icons/hicolor/256x256/apps/app.diacraft.png"
add_licences "$build"
chmod 644 "$build"/LICENSE-MIT "$build"/LICENSE-APACHE "$build"/NOTICE "$build"/ATTRIBUTION.md "$build"/THIRD-PARTY-LICENSES.txt
need_licences "$build"

# The spec, and the one changelog entry every rpm holds: for this package, the release's own page says what changed.
cp "$spec" "$build/diacraft.spec"
changelog_date=$(LC_ALL=C date -u '+%a %b %d %Y')
{
    echo "* $changelog_date ${RPM_PACKAGER:-DiaCraft <noreply@diacraft.app>} - $version-1"
    echo "- DiaCraft $name."
    case $repo_url in
    https://*) echo "  What changed is in the notes of the release, at $repo_url/releases" ;;
    *) echo "  What changed is in the notes of the release." ;;
    esac
} >>"$build/diacraft.spec"

define_extras=()
if [ -n "$extras" ]; then
    define_extras=(--define "diacraft_extras 1")
fi
log=$stage/rpmbuild.log
# The file name rpm gives the package is the same whatever the machine's macros say, and it is the only package: no source package (-bb), and no debuginfo (the spec).
if ! (cd "$build" && rpmbuild -bb --build-in-place --target "$rpm_arch-linux" \
    --define "_topdir $stage/top" --define "_rpmdir $stage/rpms" --define "_rpmfilename %%{NAME}-%%{VERSION}-%%{RELEASE}.%%{ARCH}.rpm" \
    --define "diacraft_version $version" --define "diacraft_url $url" ${define_extras[@]+"${define_extras[@]}"} diacraft.spec >"$log" 2>&1); then
    cat "$log" >&2
    die "rpmbuild failed"
fi
built=$(find "$stage/rpms" -type f -name '*.rpm')
[ -n "$built" ] && [ "$(printf '%s\n' "$built" | wc -l | tr -d ' ')" = 1 ] || {
    cat "$log" >&2
    die "rpmbuild did not write exactly one package: $built"
}

rpm_file=$out/diacraft-$name-linux-$(file_arch linux "$arch").rpm
rm -f "$rpm_file"
cp "$built" "$rpm_file"
[ -s "$rpm_file" ] || die "the package was not written: $rpm_file"
echo "$rpm_file"
