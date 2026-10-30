#!/bin/sh
# One command: builds the helper, opens the app's window and runs the checks inside it. Exit status:
# 0 every check passed, 1 a check failed, 2 the e2e could not run, 3 the machine would not let the
# window be looked at (not key, covered, screen locked or asleep): it prints "not checkable: <the
# condition>" and lists no failures. Needs a release addon (`bun run build` in packages/native) and the
# React package built (`bun run build` in packages/react).
set -e
here=$(cd "$(dirname "$0")" && pwd)
sh "$here/build-helper.sh"
out="$HOME/.cache/gpuix-webview-e2e"
rm -f "$out/present.log"
cd "$here"
GPUIX_PRESENT_LOG="$out/present.log" exec bun e2e.tsx "$@"
