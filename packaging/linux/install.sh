#!/bin/sh
# Installs DiaCraft for the current user from the folder this script is in: the two programs in ~/.local/bin, the launcher entry and the icon
# under ~/.local/share, so that DiaCraft shows in the application menu, and the licences under ~/.local/share/doc/diacraft. Needs no administrator rights.
#
#   ./install.sh              install
#   ./install.sh --uninstall  remove what an install put there
#
# PREFIX (default ~/.local) is the folder the files go under.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
prefix=${PREFIX:-$HOME/.local}
id=app.diacraft
bin=$prefix/bin
entry=$prefix/share/applications/$id.desktop
icon=$prefix/share/icons/hicolor/256x256/apps/$id.png
docs=$prefix/share/doc/diacraft
# The licence files of the download: the two licences DiaCraft is offered under, the credits and attributions, and the licences of the crates it is built from.
licences="LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md THIRD-PARTY-LICENSES.txt"

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$bin/diacraft" "$bin/diacraft-cli" "$entry" "$icon"
    for f in $licences; do
        rm -f "$docs/$f"
    done
    # Only if it is empty: a folder that holds something else stays.
    rmdir "$docs" 2>/dev/null || true
    echo "DiaCraft removed from $prefix"
    exit 0
fi
if [ $# -ne 0 ]; then
    echo "usage: install.sh [--uninstall]" >&2
    exit 2
fi

for f in diacraft diacraft-cli "$id.desktop" "$id.png" $licences; do
    if [ ! -f "$here/$f" ]; then
        echo "install.sh: $here/$f is missing: run this script from the unpacked folder" >&2
        exit 1
    fi
done

mkdir -p "$bin" "$prefix/share/applications" "$prefix/share/icons/hicolor/256x256/apps" "$docs"
cp "$here/diacraft" "$here/diacraft-cli" "$bin/"
chmod 755 "$bin/diacraft" "$bin/diacraft-cli"
cp "$here/$id.png" "$icon"
for f in $licences; do
    cp "$here/$f" "$docs/$f"
    chmod 644 "$docs/$f"
done
# The entry starts the program by the full path it was installed to, so it works when ~/.local/bin is not on the desktop's PATH.
sed "s|^Exec=diacraft |Exec=\"$bin/diacraft\" |" "$here/$id.desktop" >"$entry"
chmod 644 "$entry" "$icon"

# Refresh the desktop's caches when it has the tools; the menu entry also shows up without them, at the next login.
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$prefix/share/applications" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "DiaCraft installed in $prefix"
case ":$PATH:" in
*":$bin:"*) ;;
*) echo "Add $bin to your PATH to run diacraft and diacraft-cli from a terminal." ;;
esac
