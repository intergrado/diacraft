# shellcheck shell=bash disable=SC2034 # the variables this file sets (repo_url, cpu, glibc, distros) are read by the scripts that source it
# What the scripts that make the downloads share: packaging/package.sh (the archives, and the macOS disk image) and the installer scripts of
# packaging/linux/ and packaging/windows/. Each of them reads this file with `source`; it is not run on its own, and it changes no shell option.
#
# A script sets `script` (its own name, which an error message starts with) before it sources this file, and `set -euo pipefail` itself. It uses bash 3.2 syntax, because
# the bash of macOS is that.

die() {
    echo "${script:-packaging}: $*" >&2
    exit 1
}

# The repository's root: the folder above this one.
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# What goes in a file name: the release tag (v0.2.0), or a label for a test build.
check_name() { # check_name NAME
    case $1 in
    '' | *[!A-Za-z0-9._-]*) die "NAME must be letters, digits, '.', '_' and '-' only: '$1'" ;;
    esac
}

# The address of the repository a download came from, for the README and a package's home page: `repo_url`, from DIACRAFT_REPOSITORY (owner/name), or words when
# the variable is not set.
repo_url="the repository this download came from"
set_repo_url() {
    if [ -n "${DIACRAFT_REPOSITORY:-}" ]; then
        case $DIACRAFT_REPOSITORY in
        *[!A-Za-z0-9._/-]*) die "DIACRAFT_REPOSITORY must be owner/name: '$DIACRAFT_REPOSITORY'" ;;
        esac
        local repo_owner=${DIACRAFT_REPOSITORY%%/*}
        local repo_name=${DIACRAFT_REPOSITORY#*/}
        if [ -z "$repo_owner" ] || [ "$repo_owner" = "$DIACRAFT_REPOSITORY" ] || [ -z "$repo_name" ]; then
            die "DIACRAFT_REPOSITORY must be owner/name: '$DIACRAFT_REPOSITORY'"
        fi
        case $repo_name in
        */*) die "DIACRAFT_REPOSITORY must be owner/name: '$DIACRAFT_REPOSITORY'" ;;
        esac
        repo_url="https://github.com/$DIACRAFT_REPOSITORY"
    fi
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

# The version of DiaCraft in the workspace: the one source of it (`cargo xtask version`), which the release workflow holds equal to the tag.
workspace_version() {
    awk '/^\[/ { in_package = ($0 == "[workspace.package]") } in_package && /^version[ \t]*=/ { v = $0; sub(/^[^"]*"/, "", v); sub(/".*$/, "", v); print v; exit }' "$root/Cargo.toml"
}

# `version`: the plain MAJOR.MINOR.PATCH of a package that wants numbers only (a macOS bundle's, a Windows installer's): the tag's (v0.2.0, and v0.2.0-rc1 is 0.2.0), and for a
# name that holds none (a test build's label, a rehearsal's dev-<commit>) the workspace's own.
set_version_from_name() { # set_version_from_name NAME
    version=${1#v}
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
}

# The CPU a Linux or Windows program was built for, read from its own header, and not guessed from the machine that runs the script: x86_64 or arm64. The name of a
# download says what is in it, so a program of the other kind (or a file that is no program) stops the script.
hex_bytes() { # hex_bytes FILE OFFSET COUNT: COUNT bytes of FILE from OFFSET, as hex digits with no spaces
    od -An -tx1 -j "$2" -N "$3" "$1" | tr -d ' \n'
}
binary_arch() { # binary_arch linux|windows FILE
    local machine
    case $1 in
    linux)
        # An ELF file: the magic, 64-bit (class 2) and little endian (data 1), and the machine at offset 18: 0x3e is x86-64, 0xb7 is AArch64.
        [ "$(hex_bytes "$2" 0 6)" = "7f454c460201" ] || die "$2 is not a 64-bit little endian ELF program"
        machine=$(hex_bytes "$2" 18 2)
        case $machine in
        3e00) echo x86_64 ;;
        b700) echo arm64 ;;
        *) die "$2 is built for the ELF machine 0x$machine, which is neither x86-64 nor AArch64" ;;
        esac
        ;;
    windows)
        # A PE file: "MZ", the offset of the PE header in the four bytes at 0x3c (little endian), the signature "PE\0\0" there, and the machine after it: 0x8664 is
        # x64, 0xaa64 is ARM64.
        [ "$(hex_bytes "$2" 0 2)" = "4d5a" ] || die "$2 is not a Windows program"
        local o
        o=$(hex_bytes "$2" 60 4)
        local pe=$((0x${o:6:2}${o:4:2}${o:2:2}${o:0:2}))
        [ "$(hex_bytes "$2" "$pe" 4)" = "50450000" ] || die "$2 is not a Windows program"
        machine=$(hex_bytes "$2" "$((pe + 4))" 2)
        case $machine in
        6486) echo x86_64 ;;
        64aa) echo arm64 ;;
        *) die "$2 is built for the PE machine 0x$machine, which is neither x64 nor ARM64" ;;
        esac
        ;;
    *) die "binary_arch does not read $1 programs" ;;
    esac
}
# The architecture of the two programs of a folder, and the check that both are of one kind.
programs_arch() { # programs_arch linux|windows DIR
    local suffix=""
    if [ "$1" = windows ]; then
        suffix=".exe"
    fi
    local a b
    [ -f "$2/diacraft$suffix" ] || die "missing binary $2/diacraft$suffix"
    [ -f "$2/diacraft-cli$suffix" ] || die "missing binary $2/diacraft-cli$suffix"
    # `set -e` does not reach into a command substitution, so each failure is passed on by hand.
    a=$(binary_arch "$1" "$2/diacraft$suffix") || exit 1
    b=$(binary_arch "$1" "$2/diacraft-cli$suffix") || exit 1
    [ "$a" = "$b" ] || die "diacraft is built for $a and diacraft-cli for $b: one download holds one architecture"
    printf '%s\n' "$a"
}

# What a Linux download says of the CPU it is for, from the architecture (x86_64 or arm64): `cpu` (Intel or AMD, or ARM), `glibc` (the oldest C library the programs run
# on) and `distros` (distributions that have one that new). The release workflow builds both CPUs on the oldest Ubuntu image GitHub offers for each (22.04, and
# 22.04-arm), so that they run on any Linux with glibc 2.35 (release.yml, `linux`; the owner, 2026-11-24: ARM64 matches x64). The .deb's dependency on the C library and
# the README say it.
linux_cpu_words() { # linux_cpu_words ARCH
    case $1 in
    x86_64) cpu="Intel or AMD" ;;
    *) cpu="ARM" ;;
    esac
    glibc="2.35"
    distros="Ubuntu 22.04 and Debian 12"
}
