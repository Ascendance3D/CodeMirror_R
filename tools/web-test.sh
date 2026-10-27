#!/usr/bin/env bash
# Build the cm-web browser adapter and run its runtime tests:
#   1. Node (development host): the L-B scenarios through the adapter API.
#   2. Chromium, Firefox and WebKit (Playwright): the same scenarios in a page, then
#      real keyboard/mouse/wheel/touch/clipboard input, compared with CM5 when a
#      built CM5 tree is given.
#
#   CM5_PLAYWRIGHT_FROM=<dir with node_modules/playwright>/ tools/web-test.sh <out dir> [<built cm5 dir>] [browsers...]
#
# <built cm5 dir> holds lib/codemirror.js (a rollup build of the pinned tree, made in
# a disposable copy as for the C0 suite), lib/codemirror.css and mode/. Without it
# the browser checks run without the CM5 comparison. Browsers default to chromium,
# firefox, webkit and the emulated chromium-mobile (Pixel 7) and webkit-mobile
# (iPhone 15) profiles.
# A missing tool is reported as UNAVAILABLE with exit status 2; nothing is skipped
# silently.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:?usage: tools/web-test.sh <out dir> [<built cm5 dir>] [browsers...]}
shift
cm5=""
if [[ $# -gt 0 && -d "$1" ]]; then cm5=$1; shift; fi
browsers=("$@")
[[ ${#browsers[@]} -eq 0 ]] && browsers=(chromium firefox webkit chromium-mobile webkit-mobile)

unavailable() { echo "UNAVAILABLE: $*" >&2; exit 2; }
command -v node >/dev/null || unavailable "node not found"
command -v wasm-bindgen >/dev/null || unavailable "wasm-bindgen not found"
rustup target list --installed | grep -qx wasm32-unknown-unknown || unavailable "rustup target wasm32-unknown-unknown not installed"
[[ -n "${CM5_PLAYWRIGHT_FROM:-}" ]] || unavailable "CM5_PLAYWRIGHT_FROM is not set (Playwright)"

mkdir -p "$out"
out=$(cd "$out" && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$out/target}
export CARGO_BUILD_BUILD_DIR=${CARGO_BUILD_BUILD_DIR:-$out/build}
(cd "$root" && cargo build --offline --locked --release -p cm-web --target wasm32-unknown-unknown)
wasm-bindgen --target web --out-dir "$out/pkg" "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/cm_web.wasm"

status=0
node "$root/crates/cm-web/tests/web/node-run.mjs" "$out/pkg" > "$out/node-run.json" || status=1
echo "node: $(node -e 'const r=require(process.argv[1]);console.log(r.ok?"PASS":"FAIL", r.steps+" steps", r.label)' "$out/node-run.json")"
args=(--pkg "$out/pkg" --out "$out")
[[ -n "$cm5" ]] && args+=(--cm5 "$cm5")
node "$root/crates/cm-web/tests/web/browser-run.mjs" "${args[@]}" "${browsers[@]}" || status=1
exit $status
