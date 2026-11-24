#!/usr/bin/env bash
# Installs the .msi on the machine this runs on, looks at what it did, runs the programs it installed, removes it, and looks at what it left. The
# release workflow and the CI workflow run it on a Windows runner; it is the installer's smoke test.
#
#   packaging/windows/check-msi.sh MSI [EXPECTED_TAG]
#
#   MSI           the installer made by packaging/windows/make-msi.sh.
#   EXPECTED_TAG  a release tag such as v0.2.0, as for packaging/smoke.sh.
#
# It installs for the person who runs it, with no window and no administrator rights asked (`msiexec /i ... /qn`), and checks:
#   * the folder %LocalAppData%\Programs\DiaCraft holds both programs and the five licence files, and packaging/smoke.sh passes on them there;
#   * the Start menu shortcut is there, and that folder is on the person's PATH;
#   * .diacraft, .vsdx and .odg list DiaCraft in "Open with" and none of them has DiaCraft as the program a double-click opens;
# then removes it (`msiexec /x ... /qn`) and checks that the folder, the shortcut, the PATH entry and every registry entry are gone. It stops at once if DiaCraft is installed
# already, so as not to remove a person's own. The installer's logs are printed when a step fails.
#
# Runs in Git Bash on Windows. Uses bash 3.2 syntax like the other scripts.
set -euo pipefail

script=check-msi.sh
# shellcheck source=packaging/common.sh
source "$(dirname "$0")/../common.sh"

[ $# -ge 1 ] && [ $# -le 2 ] || die "usage: check-msi.sh MSI [EXPECTED_TAG]"
msi=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
[ -f "$msi" ] || die "no such file: $1"
expected_tag=${2:-}

# Git Bash rewrites an argument that looks like a POSIX path (and /i of msiexec is one) before a Windows program sees it: the rewriting is off, and paths are given in the
# Windows form.
export MSYS2_ARG_CONV_EXCL='*'
win() { cygpath -w "$1"; }
[ -n "${LOCALAPPDATA:-}" ] && [ -n "${APPDATA:-}" ] || die "LOCALAPPDATA and APPDATA are not set: this runs on Windows"
install_dir=$(cygpath -u "$LOCALAPPDATA")/Programs/DiaCraft
shortcut="$(cygpath -u "$APPDATA")/Microsoft/Windows/Start Menu/Programs/DiaCraft.lnk" # brand-ok: the name of the folder Windows keeps the Start menu in
[ ! -e "$install_dir" ] || die "DiaCraft is installed already, in $install_dir: remove it first"

work=$(mktemp -d "${TMPDIR:-/tmp}/diacraft-msi-check.XXXXXX")
# Windows cannot remove a folder that is the current one, so the trap leaves it first.
trap 'cd / && rm -rf "$work"' EXIT
step() { echo "check-msi: $*"; }

# msiexec with a log of its own; the log is printed when it fails. 3010 is "done, and a restart is wanted", which this installer never asks for.
installer() { # installer LOGNAME ARGS...
    local log=$work/$1.log
    shift
    local code=0
    msiexec.exe "$@" /qn /norestart /l*v "$(win "$log")" || code=$?
    if [ "$code" -ne 0 ]; then
        echo "check-msi: msiexec $* exited with $code; the end of its log:" >&2
        # The log is UTF-16: the zero bytes between its letters are taken out to read it.
        tr -d '\000' <"$log" | tail -n 80 >&2 || true
        die "msiexec failed"
    fi
}

registry_has() { # registry_has KEY [VALUE]: the key (and its value) is in the registry
    if [ $# -eq 2 ]; then reg.exe query "$1" /v "$2" >/dev/null 2>&1; else reg.exe query "$1" >/dev/null 2>&1; fi
}

step "install"
installer install /i "$(win "$msi")"

step "the files"
for f in diacraft.exe diacraft-cli.exe LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md THIRD-PARTY-LICENSES.txt; do
    [ -s "$install_dir/$f" ] || die "the installer did not put $f in $install_dir"
done

step "the programs, where the installer put them"
# shellcheck disable=SC2086 # the tag is one word, or nothing
SMOKE_PACKAGE_DIR="$install_dir" bash "$root/packaging/smoke.sh" "$install_dir" $expected_tag

step "the Start menu shortcut"
[ -f "$shortcut" ] || die "no shortcut: $shortcut"

step "the PATH of the person"
user_path=$(reg.exe query 'HKCU\Environment' /v Path 2>/dev/null | tr -d '\r' | tr 'A-Z' 'a-z')
case $user_path in
*"$(cygpath -w "$install_dir" | tr 'A-Z' 'a-z')"*) ;;
*) die "the person's PATH does not hold $install_dir: $user_path" ;;
esac

step "Open with"
for ext in diacraft vsdx odg; do
    registry_has "HKCU\\Software\\Classes\\.$ext\\OpenWithProgids" "DiaCraft.$ext" || die ".$ext does not list DiaCraft in Open with"
    registry_has "HKCU\\Software\\Classes\\DiaCraft.$ext\\shell\\open\\command" || die "the program of DiaCraft.$ext has no command"
    # The type's own (Default) value is the program a double-click opens: it must not be DiaCraft's.
    default=$(reg.exe query "HKCU\\Software\\Classes\\.$ext" /ve 2>/dev/null | tr -d '\r' || true)
    case $default in
    *DiaCraft*) die ".$ext opens with DiaCraft by itself: $default" ;;
    esac
done

step "remove"
installer remove /x "$(win "$msi")"

step "what is left"
[ ! -e "$install_dir" ] || die "the folder is still there: $install_dir ($(ls "$install_dir" | tr '\n' ' '))"
[ ! -e "$shortcut" ] || die "the shortcut is still there: $shortcut"
user_path=$(reg.exe query 'HKCU\Environment' /v Path 2>/dev/null | tr -d '\r' | tr 'A-Z' 'a-z' || true)
case $user_path in
*"$(cygpath -w "$install_dir" | tr 'A-Z' 'a-z')"*) die "the person's PATH still holds $install_dir" ;;
esac
for ext in diacraft vsdx odg; do
    ! registry_has "HKCU\\Software\\Classes\\.$ext\\OpenWithProgids" "DiaCraft.$ext" || die ".$ext still lists DiaCraft in Open with"
    ! registry_has "HKCU\\Software\\Classes\\DiaCraft.$ext" || die "the program DiaCraft.$ext is still registered"
done
! registry_has 'HKCU\Software\Classes\Applications\diacraft.exe' || die "the program diacraft.exe is still registered"
! registry_has 'HKCU\Software\DiaCraft' || die "the installer's own keys are still in the registry"

echo "check-msi: ok"
