#!/usr/bin/env bash
# Verifies the CodeMirror 5.65.21 reference against tools/reference.lock (the CM5 half of the
# X_ITE_R reference gate, tools/verify-candidate.sh at ffe080b1).
#
#   tools/verify-reference.sh [--lock FILE]
#
# Checks the vendored tree oracle/cm5-build by version and content digest. When the pinned
# upstream clone (CM5_LIVE_DIR) exists, its HEAD, tree and content digest must equal the lock.
# Run it on an unbuilt tree: a CM5 rollup build writes oracle/cm5-build/lib/codemirror.js, which
# is inside the digest scope. Exit 0 accepted, 2 usage, 3 refused.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/.." && pwd)
LOCK="$HERE/reference.lock"
if [ "${1:-}" = "--lock" ]; then LOCK=${2:?--lock needs a file}; shift 2; fi
[ $# -eq 0 ] || { echo "verify-reference: unknown argument $1" >&2; exit 2; }
[ -f "$LOCK" ] || { echo "verify-reference: no lock $LOCK" >&2; exit 2; }
# shellcheck disable=SC1090
. "$LOCK"
: "${CM5_PIN:?}" "${CM5_VERSION:?}" "${CM5_REF_DIR:?}" "${CM5_REF_DIGEST:?}" "${CM5_REF_FILES:?}" "${CM5_VERSION_FILE:?}"
DIGEST="$HERE/reference-digest.py"
P=()

json_version () { python3 -I -B -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$1"; }

CM_DIR="$ROOT/$CM5_REF_DIR"
if [ ! -d "$CM_DIR" ]; then
   P+=("CodeMirror reference tree is missing: $CM5_REF_DIR")
else
   if [ -f "$ROOT/$CM5_VERSION_FILE" ]; then
      v=$(json_version "$ROOT/$CM5_VERSION_FILE")
      [ "$v" = "$CM5_VERSION" ] || P+=("CodeMirror version is '$v', lock says $CM5_VERSION")
   else
      P+=("CodeMirror version file is missing: $CM5_VERSION_FILE")
   fi
   args=(); for s in $CM5_SUBS; do args+=(--sub "$s"); done
   got=$(python3 -I -B "$DIGEST" "$CM_DIR" "${args[@]}")
   [ "$got" = "$CM5_REF_FILES $CM5_REF_DIGEST" ] || \
      P+=("CodeMirror source digest is '$got', lock says '$CM5_REF_FILES $CM5_REF_DIGEST'")
fi

LIVE="not present (not checked)"
if [ -n "${CM5_LIVE_DIR:-}" ] && [ -e "$ROOT/$CM5_LIVE_DIR/.git" ]; then
   L="$ROOT/$CM5_LIVE_DIR"
   head=$(git -C "$L" rev-parse HEAD 2>/dev/null)
   tree=$(git -C "$L" rev-parse 'HEAD^{tree}' 2>/dev/null)
   dirty=$(git -C "$L" status --porcelain 2>/dev/null | wc -l)
   dig=$(python3 -I -B "$DIGEST" "$L")
   LIVE="$head"
   [ "$head" = "$CM5_PIN" ] || P+=("upstream clone $CM5_LIVE_DIR is at '${head:0:8}', lock says ${CM5_PIN:0:8}")
   [ "$tree" = "${CM5_UPSTREAM_TREE:?CM5_UPSTREAM_TREE missing from lock}" ] || P+=("upstream tree is $tree, lock says $CM5_UPSTREAM_TREE")
   [ "$dirty" = 0 ] || P+=("upstream clone has $dirty changed or untracked paths")
   [ "$dig" = "${CM5_UPSTREAM_DIGEST:?CM5_UPSTREAM_DIGEST missing from lock}" ] || P+=("upstream digest is '$dig', lock says '$CM5_UPSTREAM_DIGEST'")
fi

echo "reference"
echo "  root               $ROOT"
echo "  CodeMirror         $CM5_PIN (version $CM5_VERSION, $CM5_REF_FILES files)"
echo "  upstream clone     $LIVE"
if [ "${#P[@]}" -gt 0 ]; then
   for p in "${P[@]}"; do echo "  reference problem  $p"; done
   echo "  result             reference REFUSED"
   exit 3
fi
echo "  result             reference ACCEPTED"
