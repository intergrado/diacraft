#!/usr/bin/env bash
# Installs the three tools that build the web app: trunk, wasm-bindgen and binaryen's wasm-opt, each from its release archive checked against a
# SHA-256 written in the workflow, on a Linux x86_64 runner. The `site` job of release.yml and the job of deploy-site.yml run this one script, so that the two build the website with
# the same tools.
#
#   packaging/install-site-tools.sh
#
# Environment (all required):
#   TRUNK_VERSION, TRUNK_SHA256                 trunk's version, and the SHA-256 of its archive
#   WASM_BINDGEN_VERSION, WASM_BINDGEN_SHA256   the same for wasm-bindgen (its version is that of the `wasm-bindgen` crate in Cargo.lock)
#   BINARYEN_VERSION, BINARYEN_SHA256           the same for binaryen (the version Trunk.toml names)
#   RUNNER_TEMP                                 a folder to keep the tools and their archives in
#   GITHUB_PATH                                 the file the workflow reads to add a folder to the PATH of the steps after this one
#
# To move a version, change the version and its checksum in both workflows (a test holds them equal) and the lock or Trunk.toml it follows (another test holds that).
set -euo pipefail

: "${TRUNK_VERSION:?}" "${TRUNK_SHA256:?}" "${WASM_BINDGEN_VERSION:?}" "${WASM_BINDGEN_SHA256:?}" "${BINARYEN_VERSION:?}" "${BINARYEN_SHA256:?}" "${RUNNER_TEMP:?}" "${GITHUB_PATH:?}"

bin="$RUNNER_TEMP/tools/bin"
downloads="$RUNNER_TEMP/tools/downloads"
mkdir -p "$bin" "$downloads"
fetch() { # fetch <url> <sha256> <file>
    curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
        --retry 5 --retry-all-errors --output "$3" "$1"
    echo "$2  $3" | sha256sum --check --strict -
}
trunk="trunk-x86_64-unknown-linux-gnu.tar.gz"
fetch "https://github.com/trunk-rs/trunk/releases/download/v$TRUNK_VERSION/$trunk" "$TRUNK_SHA256" "$downloads/$trunk"
tar -xzf "$downloads/$trunk" -C "$bin" trunk
bindgen="wasm-bindgen-$WASM_BINDGEN_VERSION-x86_64-unknown-linux-musl"
fetch "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/$WASM_BINDGEN_VERSION/$bindgen.tar.gz" "$WASM_BINDGEN_SHA256" "$downloads/$bindgen.tar.gz"
tar -xzf "$downloads/$bindgen.tar.gz" -C "$bin" --strip-components=1 "$bindgen/wasm-bindgen"
binaryen="binaryen-$BINARYEN_VERSION-x86_64-linux"
fetch "https://github.com/WebAssembly/binaryen/releases/download/$BINARYEN_VERSION/$binaryen.tar.gz" "$BINARYEN_SHA256" "$downloads/$binaryen.tar.gz"
tar -xzf "$downloads/$binaryen.tar.gz" -C "$bin" --strip-components=2 "binaryen-$BINARYEN_VERSION/bin/wasm-opt"
echo "$bin" >>"$GITHUB_PATH"
# The version each tool reports is the one asked for: trunk uses a program of the PATH only at exactly the version it expects.
reported=$("$bin/trunk" --version)
[ "$reported" = "trunk $TRUNK_VERSION" ] || { echo "::error::trunk reports: $reported"; exit 1; }
reported=$("$bin/wasm-bindgen" --version)
[ "$reported" = "wasm-bindgen $WASM_BINDGEN_VERSION" ] || { echo "::error::wasm-bindgen reports: $reported"; exit 1; }
reported=$("$bin/wasm-opt" --version)
case $reported in "wasm-opt version "*"($BINARYEN_VERSION)") ;; *) echo "::error::wasm-opt reports: $reported"; exit 1 ;; esac
