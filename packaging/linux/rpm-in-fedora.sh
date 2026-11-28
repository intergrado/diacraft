#!/usr/bin/env bash
# Runs the two scripts of the .rpm that need a Fedora, in a container of the latest one. The release workflow's Linux jobs run it on a runner of each CPU, so the
# container is of that CPU too; on a Fedora machine it runs the same way (with podman).
#
#   packaging/linux/rpm-in-fedora.sh make NAME OUT_DIR BIN_DIR
#   packaging/linux/rpm-in-fedora.sh check RPM [EXPECTED_TAG]
#
#   make   installs rpmbuild and its macros in the container (rpm-build, redhat-rpm-config, binutils, desktop-file-utils), and runs packaging/linux/make-rpm.sh on BIN_DIR, with the
#          arguments it takes. The package is built by the rpmbuild of the Fedora it is for: its macros, its dependency generators and its tools judge the package, and
#          no other system's rpm does. It lands in OUT_DIR, owned by whoever owns that folder.
#   check  installs the package with dnf in a fresh container, runs the programs and removes it again, as packaging/linux/check-rpm.sh does.
#
# Environment:
#   FEDORA_IMAGE      (required) the image: docker.io/library/fedora@sha256:<digest of the index>, which the release workflow names (release.yml, `linux`): the Fedora project's own image
#                     of the newest release, pinned by the digest that covers both CPUs. There is no default, so that a run which lost the variable stops and does not take a tag that moves.
#                     On a machine of your own, docker.io/library/fedora:latest is the newest Fedora, unpinned (the script says so). The package does not depend on the Fedora it is made
#                     on (the programs are built against glibc 2.35 and load their window libraries by name), so another release does not change what the script does.
#   CONTAINER_ENGINE  (optional) docker or podman; the first one the machine has when it is not set.
# Other variables the script reads (DIACRAFT_REPOSITORY, RPM_PACKAGER) are handed to the container. Uses bash 3.2 syntax like the other scripts.
set -euo pipefail

script=rpm-in-fedora.sh
# shellcheck source=packaging/common.sh
source "$(dirname "$0")/../common.sh"

image=${FEDORA_IMAGE:-}
[ -n "$image" ] || die "FEDORA_IMAGE is not set: the release workflow gives it the digest of the Fedora image (release.yml); on a machine of your own, docker.io/library/fedora:latest is the newest Fedora, unpinned"
case $image in
*@sha256:????????????????????????????????????????????????????????????????) ;;
*) echo "rpm-in-fedora.sh: the image $image is not pinned by a digest, so it is whatever that name is today" >&2 ;;
esac
engine=${CONTAINER_ENGINE:-}
if [ -z "$engine" ]; then
    for candidate in docker podman; do
        if command -v "$candidate" >/dev/null 2>&1; then
            engine=$candidate
            break
        fi
    done
fi
[ -n "$engine" ] || die "neither docker nor podman is here: a container of Fedora is needed"
command -v "$engine" >/dev/null 2>&1 || die "$engine is not here"

[ $# -ge 1 ] || die "usage: rpm-in-fedora.sh make NAME OUT_DIR BIN_DIR | check RPM [EXPECTED_TAG]"
mode=$1
shift

# `--security-opt label=disable`: a machine with SELinux (a Fedora) would otherwise refuse the container the folders it is given; one without ignores it. The repository is given
# read only, and so are the programs and the package: the container writes only the package it makes.
common_options=(--rm --security-opt label=disable --volume "$root:/work:ro")
env_options=()
for variable in DIACRAFT_REPOSITORY RPM_PACKAGER; do
    if [ -n "${!variable:-}" ]; then
        env_options+=(--env "$variable")
    fi
done

case $mode in
make)
    [ $# -eq 3 ] || die "usage: rpm-in-fedora.sh make NAME OUT_DIR BIN_DIR"
    name=$1
    check_name "$name"
    [ -d "$3" ] || die "no such folder: $3"
    mkdir -p "$2"
    out=$(cd "$2" && pwd)
    bin=$(cd "$3" && pwd)
    # The commands the container runs (dnf's own output goes to the standard error, so that the standard output is the package's path and nothing else). The package's own file is chowned to the owner of the output folder as the container sees it: root where the engine maps the user to root
    # (podman without root), the runner's user where it does not (docker), so that the files of a workflow's folder stay the runner's.
    # shellcheck disable=SC2016 # the commands are the container's to expand: $1 is the name it is given
    inside='
set -euo pipefail
attempt=1
until dnf -y --setopt=install_weak_deps=False install rpm-build redhat-rpm-config binutils desktop-file-utils >&2; do
    [ "$attempt" -lt 3 ] || { echo "rpm-in-fedora.sh: dnf could not install the build tools" >&2; exit 1; }
    attempt=$((attempt + 1))
    sleep 5
done
# What the package is made with, in the log: the image is pinned and the packages are what the repositories of Fedora hold that day (an older build is gone from them, so a pin would stop the job).
{ cat /etc/fedora-release; rpm -q rpm rpm-build redhat-rpm-config binutils desktop-file-utils; } >&2
rpm=$(bash /work/packaging/linux/make-rpm.sh "$1" /out /programs | tail -n 1)
chown --reference=/out "$rpm"
echo "$rpm"'
    path=$("$engine" run "${common_options[@]}" ${env_options[@]+"${env_options[@]}"} --volume "$bin:/programs:ro" --volume "$out:/out" "$image" bash -c "$inside" bash "$name" | tail -n 1)
    # The path the container gave is inside it: the file is in OUT_DIR under the same name.
    file=$out/$(basename "$path")
    [ -s "$file" ] || die "the package was not written: $file"
    echo "$file"
    ;;
check)
    [ $# -ge 1 ] && [ $# -le 2 ] || die "usage: rpm-in-fedora.sh check RPM [EXPECTED_TAG]"
    [ -f "$1" ] || die "no such file: $1"
    dir=$(cd "$(dirname "$1")" && pwd)
    file=$(basename "$1")
    "$engine" run "${common_options[@]}" --volume "$dir:/rpm:ro" "$image" bash /work/packaging/linux/check-rpm.sh "/rpm/$file" ${2:+"$2"}
    ;;
*)
    die "the first argument is make or check: '$mode'"
    ;;
esac
