#!/usr/bin/env bash
# Fetches the pinned upstream CodeMirror 5 source into upstream/codemirror5 (gitignored), then
# verifies it with tools/verify-reference.sh. Four cm-modes runmode tests read input files from it.
#
#   tools/fetch-upstream.sh            # clone CM5_UPSTREAM_URL at CM5_PIN, or check an existing clone
#   CM5_UPSTREAM_URL_OVERRIDE=<url> tools/fetch-upstream.sh   # another host; the same pin and digests apply
#
# It never changes an existing clone: a clone at another commit, or with changes, is refused (exit 3).
# The fetched source is a reference. Do not edit it to make a test pass.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/.." && pwd)
# shellcheck disable=SC1091
. "$HERE/reference.lock"
URL=${CM5_UPSTREAM_URL_OVERRIDE:-$CM5_UPSTREAM_URL}
DEST="$ROOT/$CM5_LIVE_DIR"

if [ ! -e "$DEST" ]; then
   TMP="$DEST.fetch-$$"
   mkdir -p "$(dirname "$DEST")"
   git init -q "$TMP" || exit 3
   git -C "$TMP" remote add origin "$URL"
   if ! git -C "$TMP" fetch -q --depth 1 origin "$CM5_PIN"; then
      echo "fetch-upstream: cannot fetch $CM5_PIN from $URL" >&2; rm -rf -- "$TMP"; exit 3
   fi
   git -C "$TMP" -c advice.detachedHead=false checkout -q FETCH_HEAD || { rm -rf -- "$TMP"; exit 3; }
   mv -- "$TMP" "$DEST"
fi

exec "$HERE/verify-reference.sh"
