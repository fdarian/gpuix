#!/bin/sh
# One command: builds the helper, opens the app's window and runs the checks inside it. The exit
# status is non-zero when a check fails. Needs a release addon (`bun run build` in packages/native)
# and the React package built (`bun run build` in packages/react).
set -e
here=$(cd "$(dirname "$0")" && pwd)
sh "$here/build-helper.sh"
out="$HOME/.cache/gpuix-round29/e2e"
rm -f "$out/present.log"
cd "$here"
GPUIX_PRESENT_LOG="$out/present.log" exec bun e2e.tsx "$@"
