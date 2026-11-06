#!/bin/sh
# Add Quill to this user's application menu and give it its icon.
# Run it from the unpacked folder. It writes only under ~/.local/share, and
# the menu entry points at the quill program where it is now, so move the
# folder first if you want it elsewhere. Wayland desktops (Ubuntu's default) find the
# window icon through this entry; there is no other way to give them one.
set -eu

here=$(cd "$(dirname "$0")" && pwd)

# In the unpacked package everything sits beside this script. In a source
# checkout the files are in tools/ and assets/, and the program is under
# target/; or name the program: install-linux.sh /path/to/quill
first() { for f in "$@"; do [ -e "$f" ] && { echo "$f"; return 0; }; done; return 1; }
[ -z "${1:-}" ] || [ -e "$1" ] || { echo "install-linux.sh: $1 does not exist" >&2; exit 1; }
program=$(first "${1:-}" "$here/quill" "$here/../target/release/quill" "$here/../target/debug/quill") ||
    { echo "install-linux.sh: cannot find the quill program (name it as an argument)" >&2; exit 1; }
program=$(cd "$(dirname "$program")" && pwd)/$(basename "$program")
icon=$(first "$here/quill.png" "$here/../assets/quill.png") ||
    { echo "install-linux.sh: cannot find quill.png" >&2; exit 1; }
entry=$(first "$here/quill.desktop") ||
    { echo "install-linux.sh: cannot find quill.desktop" >&2; exit 1; }

data="${XDG_DATA_HOME:-$HOME/.local/share}"
icons="$data/icons/hicolor/512x512/apps"
apps="$data/applications"
mkdir -p "$icons" "$apps"
cp "$icon" "$icons/quill.png"

# Exec needs the full path; the packaged file just says "quill".
sed "s|^Exec=.*|Exec=$program %f|" "$entry" > "$apps/quill.desktop"
chmod 644 "$apps/quill.desktop"

command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$data/icons/hicolor" || true
echo "Quill is in your application menu. Log out and in if the icon does not appear."
