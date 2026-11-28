#!/usr/bin/env bash
# Installs the .rpm with dnf on the machine this runs on, looks at what it did, runs the programs it installed, removes it, and looks at what it left. The release workflow
# runs it in a Fedora container (packaging/linux/rpm-in-fedora.sh check); it is the package's smoke test, as check-msi.sh is the installer's.
#
#   packaging/linux/check-rpm.sh RPM [EXPECTED_TAG]
#
#   RPM           the package made by packaging/linux/make-rpm.sh.
#   EXPECTED_TAG  a release tag such as v0.2.4, as for packaging/smoke.sh.
#
# It runs as root on a Fedora with dnf and no DiaCraft (a container; it stops at once if DiaCraft is installed already, so as not to remove a person's own), and checks:
#   * the package's architecture is the machine's, its licence field is MIT OR Apache-2.0, and it has no script of its own (Fedora's file triggers refresh the desktop database and
#     the icon cache);
#   * `dnf install` takes it (with the documentation, which Fedora's container image leaves out by default) with every Requires answered by Fedora's own packages (weak dependencies are left out: those are drivers and portals that nothing here runs), and
#     `rpm --verify` finds every file as the package made it;
#   * both programs, the launcher entry, the icon and the five licence files are where Fedora looks, the entry passes `desktop-file-validate`, and every library the programs link
#     is there (`ldd`), and packaging/smoke.sh passes on the programs in /usr/bin with the licence files of /usr/share/licenses/diacraft;
# then removes it and checks that none of its files, nor the package, is left. Uses bash 3.2 syntax like the other scripts.
set -euo pipefail

script=check-rpm.sh
# shellcheck source=packaging/common.sh
source "$(dirname "$0")/../common.sh"

[ $# -ge 1 ] && [ $# -le 2 ] || die "usage: check-rpm.sh RPM [EXPECTED_TAG]"
rpm_file=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
[ -f "$rpm_file" ] || die "no such file: $1"
expected_tag=${2:-}
[ "$(id -u)" = 0 ] || die "this installs a package: run it as root, in a container of Fedora (packaging/linux/rpm-in-fedora.sh check)"
command -v dnf >/dev/null 2>&1 || die "dnf is not here: this runs on Fedora"
command -v rpm >/dev/null 2>&1 || die "rpm is not here: this runs on Fedora"
if rpm -q diacraft >/dev/null 2>&1; then
    die "DiaCraft is installed already: remove it first"
fi
step() { echo "check-rpm: $*"; }
# What the package is tried on, in the log: the image is pinned and the packages are what Fedora's repositories hold that day.
{ cat /etc/fedora-release 2>/dev/null || true; rpm -q rpm; dnf --version | head -n 1; } || true

# dnf, asked again twice when it fails: a mirror that is slow or has just changed is not the package's fault. Fedora's container image sets `tsflags=nodocs`, which leaves out every file
# rpm calls documentation (the manual pages among them): a person's Fedora installs them, so the flag is emptied here.
dnf_retry() { # dnf_retry ARGS...
    local attempt=1
    until dnf -y --setopt=install_weak_deps=False --setopt=tsflags= "$@"; do
        [ "$attempt" -lt 3 ] || die "dnf $* failed"
        attempt=$((attempt + 1))
        sleep 5
    done
}

step "the package"
rpm -qpi "$rpm_file"
machine=$(uname -m)
[ "$(rpm -qp --qf '%{ARCH}' "$rpm_file")" = "$machine" ] || die "the package is for $(rpm -qp --qf '%{ARCH}' "$rpm_file") and this machine is $machine"
[ "$(rpm -qp --qf '%{LICENSE}' "$rpm_file")" = "MIT OR Apache-2.0" ] || die "the package's licence is '$(rpm -qp --qf '%{LICENSE}' "$rpm_file")', not MIT OR Apache-2.0"
[ -z "$(rpm -qp --scripts "$rpm_file")" ] || die "the package has a script of its own, which Fedora's file triggers make unneeded"

# desktop-file-validate is the tool that the spec's own check ran at build time; here it looks at the file that was installed.
step "dnf install"
dnf_retry install desktop-file-utils
dnf_retry install "$rpm_file"
rpm -q diacraft
step "rpm --verify"
rpm --verify diacraft || die "rpm --verify found a file that is not as the package made it"

step "the files"
# Every file the package lists is installed (`rpm --verify` leaves out what a transaction was told to skip, which is why the flag above is emptied).
for f in $(rpm -ql diacraft); do
    [ -e "$f" ] || die "the package lists $f, and it is not installed"
done
for f in /usr/bin/diacraft /usr/bin/diacraft-cli; do
    [ -x "$f" ] || die "$f is not there, or not a program"
done
for f in /usr/share/applications/app.diacraft.desktop /usr/share/icons/hicolor/256x256/apps/app.diacraft.png; do
    [ -s "$f" ] || die "$f is not there"
done
licences=/usr/share/licenses/diacraft
need_licences "$licences"
desktop-file-validate /usr/share/applications/app.diacraft.desktop || die "desktop-file-validate does not accept the launcher entry"
# The files rpm calls licence files are the five, and no others.
listed=$(rpm -qL diacraft | sort | tr '\n' ' ')
want=$(for f in $LICENCE_FILES; do echo "$licences/$f"; done | sort | tr '\n' ' ')
[ "$listed" = "$want" ] || die "the licence files of the package are '$listed', not '$want'"

step "ldd"
for program in /usr/bin/diacraft /usr/bin/diacraft-cli; do
    if ldd "$program" | grep 'not found'; then
        die "$program links a library that the package's Requires do not bring"
    fi
done

step "packaging/smoke.sh on the installed programs"
SMOKE_PACKAGE_DIR=$licences bash "$root/packaging/smoke.sh" /usr/bin ${expected_tag:+"$expected_tag"}

step "dnf remove"
# Everything the package listed (its files, and the folder of its licences, which no other package owns), remembered before it goes.
installed=$(rpm -ql diacraft)
dnf_retry remove diacraft
if rpm -q diacraft >/dev/null 2>&1; then
    die "the package is still installed after dnf remove"
fi
for f in $installed; do
    if [ -e "$f" ] || [ -L "$f" ]; then
        die "the package left $f behind when it was removed"
    fi
done
echo "check-rpm: ok"
