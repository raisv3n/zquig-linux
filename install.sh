#!/bin/sh
# Installs Zquig for the current user: copies zquig to ~/.local/bin and starts it in the tray.
# Starting it adds Zquig to the app menu and the file manager's "Open with" list. No root needed.
#   ./install.sh              install or update
#   ./install.sh --uninstall  remove Zquig, its menu entry and its login item
set -eu

bin="$HOME/.local/bin/zquig"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
config="${XDG_CONFIG_HOME:-$HOME/.config}"
autostart="$config/autostart/zquig.desktop"

# Stop a running copy (only this user's) and wait for it to exit, so the binary can be replaced.
if pkill -x -u "$(id -u)" zquig 2>/dev/null; then
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        pgrep -x -u "$(id -u)" zquig >/dev/null || break
        sleep 0.5
    done
fi

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$bin" "$data/applications/zquig.desktop" "$autostart"
    rm -rf "$data/zquig"
    update-desktop-database "$data/applications" 2>/dev/null || true
    echo "Zquig has been removed. Your settings are in $config/zquig if you want to delete them too."
    exit 0
fi

# Use the binary next to this script (the release archive), or the build output in the source tree.
here=$(dirname "$0")
if [ -x "$here/zquig" ] && [ ! -d "$here/zquig" ]; then
    source="$here/zquig"
elif [ -x "$here/target/release/zquig" ]; then
    source="$here/target/release/zquig"
else
    echo "zquig not found. Build it first with: cargo build --release" >&2
    exit 1
fi

mkdir -p "$(dirname "$bin")"
# Copy then rename, so a running binary is never overwritten in place.
cp "$source" "$bin.new"
chmod 755 "$bin.new"
mv -f "$bin.new" "$bin"

# Keep "Start at Login" pointing at the installed copy if it was turned on.
if [ -f "$autostart" ]; then
    sed -i "s|^Exec=.*|Exec=\"$bin\" --background|" "$autostart"
fi

command -v pdftoppm >/dev/null || echo "Note: opening PDFs needs poppler-utils (sudo apt install poppler-utils)."

# Starting it once also points the app menu and "Open with" entry at the installed copy.
nohup "$bin" --background >/dev/null 2>&1 &
echo "Zquig is installed in $bin and running in the tray. Find it in the app menu as Zquig."
