#!/bin/sh
# Install Shard with `--setup --silent` into a scratch home, check what it put where, then remove
# it with `--uninstall --silent` and check it's gone. CI runs it on macOS and Linux.
#   scripts/test-setup.sh <shard binary, or Shard.app on macOS>
set -eu
target=${1:?usage: scripts/test-setup.sh <shard binary or Shard.app>}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
export HOME="$work/home" SHARD_DATA="$work/data" SHARD_INSTALL_DIR="$work/apps" SHARD_PORT=47119 \
	XDG_DATA_HOME="$work/home/.local/share" XDG_CONFIG_HOME="$work/home/.config"
mkdir -p "$HOME"
if [ -d "$target" ]; then exe="$target/Contents/MacOS/shard"; else exe="$target"; fi

"$exe" --setup --silent
if [ "$(uname)" = Darwin ]; then
	installed="$SHARD_INSTALL_DIR/Shard.app/Contents/MacOS/shard"
	login="$HOME/Library/LaunchAgents/dev.shard.app.plist"
	entry=""
else
	installed="$SHARD_INSTALL_DIR/shard"
	login="$XDG_CONFIG_HOME/autostart/shard.desktop"
	entry="$XDG_DATA_HOME/applications/shard.desktop"
	test -f "$entry" || { echo "no desktop entry at $entry"; exit 1; }
	test -f "$XDG_DATA_HOME/icons/hicolor/512x512/apps/shard.png" || { echo "no icon"; exit 1; }
fi
test -x "$installed" || { echo "nothing installed at $installed"; exit 1; }
test -f "$login" || { echo "no login entry at $login"; exit 1; }
"$installed" --version | grep -q "^shard " || { echo "the installed copy doesn't run"; exit 1; }
# Again over the top, as an update would.
"$exe" --setup --silent >/dev/null

"$installed" --uninstall --silent
test ! -e "$installed" || { echo "Shard is still at $installed"; exit 1; }
test ! -e "$login" || { echo "the login entry is still there"; exit 1; }
[ -z "$entry" ] || test ! -e "$entry" || { echo "the desktop entry is still there"; exit 1; }
echo "ok: setup installed Shard, installed it again over the top, and removed it"
