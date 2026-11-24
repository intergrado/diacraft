#!/usr/bin/env bash
# A smoke test of the two release programs on the machine that built them: they start, say their version, and the command line
# makes a small drawing, saves it as .vsdx, converts it to .diacraft and back, and draws it as PNG, SVG and PDF. Nothing opens a window.
# The CI workflow's Windows and macOS jobs and the release workflow run it on the binaries they built.
#
#   packaging/smoke.sh BIN_DIR [EXPECTED_TAG]
#   packaging/smoke.sh --licences DIR
#
#   BIN_DIR       the folder holding diacraft and diacraft-cli (with .exe on Windows).
#   EXPECTED_TAG  a release tag such as v0.2.0: the programs must say its version (0.2.0) and the command line's app.version must say the build line of that tag, the version
#                 without the v (0.2.0: the build says a build of its tag by the plain version; a build of any other commit says more, apps/diacraft/build_info.rs).
#   --licences    only checks that DIR holds the licence files of a download (below), and stops. The macOS job uses it: its programs are in an app.
#
# The environment variable SMOKE_PACKAGE_DIR names the folder of an unpacked download: before anything runs, the licence files must be in it, each there
# and not empty (LICENSE-MIT, LICENSE-APACHE, NOTICE, ATTRIBUTION.md and THIRD-PARTY-LICENSES.txt, which must also look like the list of licences and not a stub).
# The release workflow sets it on the downloads it unpacks; it is not set on a bare build, which has no licence files beside it.
#
# Needs only bash, and the tools every runner has: head, od, grep, cmp, wc. Uses bash 3.2 syntax (macOS).
set -euo pipefail

# Git Bash on Windows rewrites an argument that looks like a POSIX path before a Windows program sees it. The paths below are already in the form
# the programs take (see `native`), so it is switched off. Other systems ignore the variable.
export MSYS2_ARG_CONV_EXCL='*'

die() {
    echo "smoke: FAILED: $*" >&2
    exit 1
}

# The licence files that every download holds: the two licences DiaCraft is offered under, its credits, the attributions of its pictures and fonts, and the
# licence texts of the crates it is built from.
check_licences() { # check_licences DIR
    [ -d "$1" ] || die "no such folder: $1"
    for f in LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md THIRD-PARTY-LICENSES.txt; do
        [ -f "$1/$f" ] || die "the download has no $f (in $1)"
        # `wc` pads its count with spaces on macOS.
        [ "$(wc -c <"$1/$f" | tr -d ' ')" -gt 0 ] || die "$f is empty (in $1)"
    done
    # The third-party file is the real one: its title, both programs and the one C library are named, and it is not a stub.
    third=$1/THIRD-PARTY-LICENSES.txt
    [ "$(wc -c <"$third" | tr -d ' ')" -gt 50000 ] || die "THIRD-PARTY-LICENSES.txt is only $(wc -c <"$third" | tr -d ' ') bytes: it is not the list of licences"
    for word in "DiaCraft: third-party licences" "diacraft-cli" "libmimalloc-sys"; do
        grep -q -- "$word" "$third" || die "THIRD-PARTY-LICENSES.txt does not say '$word'"
    done
    echo "smoke: licence files in $1: ok"
}

if [ "${1:-}" = "--licences" ]; then
    [ $# -eq 2 ] || die "usage: smoke.sh --licences DIR"
    check_licences "$2"
    exit 0
fi

[ $# -ge 1 ] && [ $# -le 2 ] || die "usage: smoke.sh BIN_DIR [EXPECTED_TAG] | --licences DIR"
bin=$(cd "$1" && pwd)
expected_tag=${2:-}
# The folder of an unpacked download, made absolute here as `bin` is: the script leaves the folder it was started in (below), and the workflows name this one by a path
# relative to it (`unpacked/diacraft-<tag>-linux-x86_64`).
package_dir=""
if [ -n "${SMOKE_PACKAGE_DIR:-}" ]; then
    [ -d "$SMOKE_PACKAGE_DIR" ] || die "no such folder: $SMOKE_PACKAGE_DIR"
    package_dir=$(cd "$SMOKE_PACKAGE_DIR" && pwd)
fi

exe=""
case "$(uname -s)" in
MINGW* | MSYS* | CYGWIN*) exe=".exe" ;;
esac
cli=$bin/diacraft-cli$exe
app=$bin/diacraft$exe
[ -f "$cli" ] || die "missing $cli"
[ -f "$app" ] || die "missing $app"

work=$(mktemp -d "${TMPDIR:-/tmp}/diacraft-smoke.XXXXXX")
# Windows cannot remove a folder that is the current one, so the trap leaves it first.
trap 'cd "$bin" && rm -rf "$work"' EXIT
cd "$work"

# A path as the program takes it: on Windows, C:/Users/... (forward slashes need no escaping in the JSON of a command's parameters).
native() {
    if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s\n' "$1"; fi
}

step() { echo "smoke: $*"; }

# 0. The licence files of an unpacked download.
if [ -n "$package_dir" ]; then
    step "the licence files of the download"
    check_licences "$package_dir"
fi

# 1. The versions.
step "diacraft-cli --version"
cli_version=$("$cli" --version) || die "diacraft-cli --version exited with an error"
echo "$cli_version"
case $cli_version in
"diacraft-cli "[0-9]*) ;;
*) die "unexpected output of diacraft-cli --version: '$cli_version'" ;;
esac

step "diacraft --version"
app_version=$("$app" --version) || die "diacraft --version exited with an error"
if [ -z "$app_version" ] && [ -n "$exe" ]; then
    # On Windows the window program is built for the GUI subsystem, and a terminal does not always receive what it writes.
    echo "::warning::diacraft.exe --version printed nothing here (a window program on Windows): the exit status was checked, not the text"
else
    echo "$app_version"
    case $app_version in
    "diacraft "[0-9]*) ;;
    *) die "unexpected output of diacraft --version: '$app_version'" ;;
    esac
fi

if [ -n "$expected_tag" ]; then
    want=${expected_tag#v}
    [ "$cli_version" = "diacraft-cli $want" ] || die "diacraft-cli says '$cli_version', the tag $expected_tag wants 'diacraft-cli $want'"
    if [ -n "$app_version" ]; then
        [ "$app_version" = "diacraft $want" ] || die "diacraft says '$app_version', the tag $expected_tag wants 'diacraft $want'"
    fi
fi

# 2. The build's own identity: the build line the build says is the tag's version (without the v) when a tag is expected.
step "app.version"
identity=$("$cli" run --cmd file.new --cmd app.version | tail -n 1) || die "app.version failed"
echo "$identity"
case $identity in
*'"app":"diacraft"'*) ;;
*) die "app.version did not answer: '$identity'" ;;
esac
if [ -n "$expected_tag" ]; then
    case $identity in
    *"\"git\":\"$want\""*) ;;
    *) die "app.version does not say the build line $want of the tag $expected_tag: '$identity'" ;;
    esac
fi

# 3. A small drawing, saved as .vsdx by the command line itself.
step "make and save a drawing"
a_vsdx=$(native "$work/a.vsdx")
"$cli" run --cmd file.new \
    --cmd shape.drop --params '{"master":"Process","page":1,"x":2,"y":3}' \
    --cmd shape.drop --params '{"master":"Decision","page":1,"x":5,"y":3}' \
    --cmd connector.add --params '{"from":{"id":1},"to":{"id":2},"page":1}' \
    --cmd shape.set_text --params '{"id":1,"text":"Start"}' \
    --cmd file.save_as --params "{\"path\":\"$a_vsdx\"}" >run.out || die "building and saving the drawing failed"
[ -s a.vsdx ] || die "a.vsdx was not written"

# 4. .vsdx to .diacraft and back; the two .vsdx files hold the same content.
step "convert .vsdx to .diacraft and back"
"$cli" convert "$a_vsdx" --out "$(native "$work/a.diacraft")" >/dev/null || die "convert to .diacraft failed"
[ -s a.diacraft ] || die "a.diacraft was not written"
"$cli" convert "$(native "$work/a.diacraft")" --out "$(native "$work/b.vsdx")" >/dev/null || die "convert back to .vsdx failed"
[ -s b.vsdx ] || die "b.vsdx was not written"
same=$("$cli" compare --content "$a_vsdx" "$(native "$work/b.vsdx")") || die "compare --content failed"
case $same in
*'"same":true'*) ;;
*) die "the drawing changed on the trip through .diacraft: $same" ;;
esac

# 4b. A drawing read from a file, and the pictures. The machine's own fonts are read for the PNG; the bundled ones make the PDF.
step "export PNG, SVG and PDF"
"$cli" export "$a_vsdx" --out "$(native "$work/a.png")" >/dev/null || die "export to PNG failed"
"$cli" convert "$a_vsdx" --out "$(native "$work/a.svg")" >/dev/null || die "convert to SVG failed"
"$cli" convert "$a_vsdx" --out "$(native "$work/a.pdf")" --bundled-fonts >/dev/null || die "convert to PDF failed"

magic() { # magic FILE COUNT: the first COUNT bytes as hex
    head -c "$2" "$1" | od -An -tx1 | tr -d ' \n'
}
[ "$(magic a.png 8)" = "89504e470d0a1a0a" ] || die "a.png is not a PNG"
[ "$(magic a.pdf 5)" = "255044462d" ] || die "a.pdf is not a PDF"
[ "$(magic a.vsdx 2)" = "504b" ] || die "a.vsdx is not a zip package"
grep -q "<svg" a.svg || die "a.svg is not an SVG"
for f in a.png a.pdf a.svg a.diacraft; do
    # `wc` pads its count with spaces on macOS.
    [ "$(wc -c <"$f" | tr -d ' ')" -gt 500 ] || die "$f is almost empty"
done

# 5. The MCP server answers on its standard input and output.
step "mcp initialize"
reply=$(printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' |
    "$cli" mcp --headless) || die "diacraft-cli mcp failed"
case $reply in
*'"result"'*) ;;
*) die "the MCP server did not answer initialize: '$reply'" ;;
esac

echo "smoke: ok"
