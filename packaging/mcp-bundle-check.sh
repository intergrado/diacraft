#!/usr/bin/env bash
# Looks at an MCP bundle and runs what is in it, as a client that installs it would: unpacks it, reads the manifest, starts the server the way the manifest says, and asks it
# for `initialize` and `tools/list`. The release workflow's jobs run it on each bundle `mcp-bundle.sh` made, before anything is uploaded.
#
#   packaging/mcp-bundle-check.sh [--no-run] BUNDLE [EXPECTED_TAG]
#
#   BUNDLE        a `diacraft-mcp-<version>-<system>-<cpu>.mcpb` file.
#   EXPECTED_TAG  a release tag such as v0.2.2: the manifest must say its version (0.2.2).
#   --no-run      look at the files and start nothing: a bundle for another CPU than the machine's (the Intel program on an Apple silicon runner that has no Rosetta).
#
# The environment variable RUN_WITH is a command put before the program when it is run (`arch -x86_64`, for the Intel program on Apple silicon with Rosetta); it is split into
# words, so it holds a command and its options and no quoting.
#
# What it holds the bundle to: the file is named as a bundle is; it holds the manifest, the icon (a PNG), the program named by the manifest's `entry_point` (runnable) and the five
# licence files, and nothing else at its top; the manifest says the format's version, the name `diacraft`, a binary server whose command is its entry point, the one system the file
# name says, and the version of the tag (or the bundle's own); the program says that version; and the server answers `initialize` as DiaCraft's and lists exactly the tools the
# manifest declares (the manifest's list says `tools_generated: false`: it is the whole list).
#
# Reads the manifest by the layout of packaging/mcp/manifest.json (one key to a line, the tools one to a line, the name first): a test holds the template to it. Needs unzip (or
# PowerShell, which is what it uses on Windows) and the tools every runner has: sed, grep, sort, od, head. Uses bash 3.2 syntax (macOS).
set -euo pipefail

# Git Bash on Windows rewrites an argument that looks like a POSIX path before a Windows program sees it. The paths below are already in the form the programs take.
export MSYS2_ARG_CONV_EXCL='*'

die() {
    echo "mcp-bundle-check: FAILED: $*" >&2
    exit 1
}

no_run=no
if [ "${1:-}" = "--no-run" ]; then
    no_run=yes
    shift
fi
[ $# -ge 1 ] && [ $# -le 2 ] || die "usage: mcp-bundle-check.sh [--no-run] BUNDLE [EXPECTED_TAG]"
bundle=$1
expected_tag=${2:-}
[ -f "$bundle" ] || die "no such file: $bundle"
case $bundle in
*.mcpb) ;;
*) die "a bundle is a .mcpb file: $bundle" ;;
esac
bundle=$(cd "$(dirname "$bundle")" && pwd)/$(basename "$bundle")

# The system the name says, and the word the manifest says it with.
base=$(basename "$bundle" .mcpb)
case $base in
diacraft-mcp-*-linux-*) want_platform=linux ;;
diacraft-mcp-*-windows-*) want_platform=win32 ;;
diacraft-mcp-*-macos-*) want_platform=darwin ;;
*) die "the name of the bundle is not diacraft-mcp-<version>-<system>-<cpu>.mcpb: $base" ;;
esac
name_version=${base#diacraft-mcp-}
name_version=${name_version%-linux-*}
name_version=${name_version%-windows-*}
name_version=${name_version%-macos-*}

work=$(mktemp -d "${TMPDIR:-/tmp}/diacraft-mcp-check.XXXXXX")
# Windows cannot remove a folder that is the current one, so the trap leaves it first.
trap 'cd / && rm -rf "$work"' EXIT
unpacked=$work/unpacked
mkdir "$unpacked"
# On Windows the archive is unpacked by PowerShell, as the workflow's own step unpacks the portable zip: a program of Windows takes the paths of Windows, which `unzip` of Git Bash may not be given.
on_windows=no
case "$(uname -s)" in
MINGW* | MSYS* | CYGWIN*) on_windows=yes ;;
esac
if [ "$on_windows" = no ] && command -v unzip >/dev/null 2>&1; then
    unzip -q "$bundle" -d "$unpacked" || die "$bundle is not an archive unzip reads"
else
    # PowerShell reads an archive only under the name `.zip`.
    win() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s\n' "$1"; fi; }
    cp "$bundle" "$work/bundle.zip"
    pwsh -NoProfile -NonInteractive -Command "Expand-Archive -Path '$(win "$work/bundle.zip")' -DestinationPath '$(win "$unpacked")'" || die "$bundle is not an archive PowerShell reads"
fi

# The files: the manifest, the icon, the program's folder and the licences, and nothing else at the top.
licences="LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md THIRD-PARTY-LICENSES.txt"
for f in manifest.json icon.png $licences; do
    [ -f "$unpacked/$f" ] || die "the bundle has no $f"
    [ -s "$unpacked/$f" ] || die "$f is empty"
done
[ -d "$unpacked/server" ] || die "the bundle has no server folder"
[ "$(head -c 8 "$unpacked/icon.png" | od -An -tx1 | tr -d ' \n')" = "89504e470d0a1a0a" ] || die "icon.png is not a PNG picture"
for entry in $(ls -A "$unpacked"); do
    case " manifest.json icon.png server $licences " in
    *" $entry "*) ;;
    *) die "the bundle has a file the format does not use: $entry" ;;
    esac
done

# The manifest, read by its layout: `"key": "value"` and `"key": ["a", "b"]` on a line each.
manifest=$unpacked/manifest.json
text_of() { sed -n "s/^ *\"$1\": \"\(.*\)\",\{0,1\}\$/\1/p" "$manifest" | head -n 1; }
list_of() { sed -n "s/^ *\"$1\": \[\(.*\)\],\{0,1\}\$/\1/p" "$manifest" | head -n 1 | tr -d '" '; }
[ "$(text_of manifest_version)" = "0.3" ] || die "manifest_version is '$(text_of manifest_version)', not 0.3"
[ "$(text_of name)" = "diacraft" ] || die "name is '$(text_of name)', not diacraft"
grep -q '^    "type": "binary",\{0,1\}$' "$manifest" || die "the server's type is not binary"
version=$(text_of version)
[ -n "$version" ] || die "the manifest has no version"
if [ -n "$expected_tag" ] && [ "$version" != "${expected_tag#v}" ]; then
    die "the manifest says version $version, the tag $expected_tag wants ${expected_tag#v}"
fi
case $name_version in
[0-9]*.[0-9]*.[0-9]*) [ "$version" = "$name_version" ] || die "the file is named for version $name_version and the manifest says $version" ;;
esac
platforms=$(list_of platforms)
[ "$platforms" = "$want_platform" ] || die "the manifest is for '$platforms' and the file name says $want_platform"
entry_point=$(text_of entry_point)
case $entry_point in
server/diacraft-cli | server/diacraft-cli.exe) ;;
*) die "entry_point is '$entry_point', not the program in server/" ;;
esac
[ "$(text_of command)" = "\${__dirname}/$entry_point" ] || die "the command is '$(text_of command)', not \${__dirname}/$entry_point"
program=$unpacked/$entry_point
[ -f "$program" ] || die "the entry point $entry_point is not in the bundle"
[ -x "$program" ] || die "the entry point $entry_point is not runnable (its mode is lost)"
args=$(list_of args | tr ',' ' ')
[ -n "$args" ] || die "the manifest gives the server no arguments: it would start the command line with none"

if [ "$no_run" = yes ]; then
    echo "mcp-bundle-check: ok (the program was not run: --no-run): $(basename "$bundle")"
    exit 0
fi

# The program says the version of the manifest.
said=$(${RUN_WITH:-} "$program" --version) || die "$entry_point --version failed"
[ "$said" = "diacraft-cli $version" ] || die "the program says '$said', the manifest says $version"

# The server, started as the manifest says, answers two requests on its standard input and ends when it is closed.
replies=$(printf '%s\n%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"bundle-check","version":"0"}}}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | ${RUN_WITH:-} "$program" $args 2>"$work/server.err") || die "the server did not run: $(head -c 300 "$work/server.err")"
first=$(printf '%s\n' "$replies" | sed -n 1p)
second=$(printf '%s\n' "$replies" | sed -n 2p)
case $first in
*'"result"'*'"serverInfo"'*'"name":"diacraft"'*) ;;
*) die "the server did not answer initialize as DiaCraft's: '$(printf '%s' "$first" | head -c 300)'" ;;
esac
declared=$(sed -n 's/^ *{ *"name": "\([a-z_]*\)".*$/\1/p' "$manifest" | sort | tr '\n' ' ')
served=$(printf '%s' "$second" | grep -o '"name":"[a-z_]*"' | sed 's/"name":"\(.*\)"/\1/' | sort | tr '\n' ' ' || true)
[ -n "$declared" ] || die "the manifest declares no tool"
[ "$declared" = "$served" ] || die "the tools differ: the manifest declares [${declared% }] and the server lists [${served% }]"

echo "mcp-bundle-check: ok: $(basename "$bundle"): version $version, tools ${declared% }"
