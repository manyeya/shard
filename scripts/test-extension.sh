#!/bin/sh
# End-to-end check of the browser extension: headless Chrome for Testing (branded Chrome ignores
# unpacked extensions) clicks a cookie-protected download, and Shard must fetch it with that cookie.
# Needs agent-browser, python3, sqlite3, and CHROME set to the Chrome for Testing binary.
set -eu
: "${CHROME:?set CHROME to the Chrome for Testing binary}"
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'agent-browser close --all >/dev/null 2>&1 || true; kill $shard $site 2>/dev/null || true; rm -rf "$work"' EXIT
cargo build --release --manifest-path "$root/Cargo.toml"
mkdir -p "$work/data" "$work/dl"
sqlite3 "$work/data/shard.db" "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL); INSERT INTO settings VALUES ('dir', '$work/dl');"
python3 -u "$root/scripts/fixture_server.py" "$work/expected.bin" > "$work/site.log" 2>&1 & site=$!
SHARD_DATA="$work/data" "$root/target/release/shard" serve > "$work/shard.log" 2>&1 & shard=$!
for _ in $(seq 50); do curl -s http://127.0.0.1:47110/ping | grep -q shard && curl -s -o /dev/null http://127.0.0.1:8765/ && break; sleep 0.2; done

export AGENT_BROWSER_SESSION=shard-extension-test
agent-browser --executable-path "$CHROME" --extension "$root/extension" open http://127.0.0.1:8765/ >/dev/null
agent-browser click "#dl" >/dev/null
for _ in $(seq 60); do [ -f "$work/dl/Quarterly Report.bin" ] && break; sleep 0.5; done

cmp "$work/dl/Quarterly Report.bin" "$work/expected.bin"
grep -q 'range=bytes=0-0 cookie=True' "$work/site.log" || { echo "Shard didn't send the browser's cookie"; cat "$work/site.log"; exit 1; }
echo "ok: the extension handed the download over and Shard fetched it with the browser's cookie"
