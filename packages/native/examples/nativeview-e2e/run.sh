#!/bin/sh
# One command: builds the helpers and the test addon, opens the app's window and runs the checks inside
# it. Exit status: 0 every check passed, 1 a check failed, 2 the e2e could not run, 3 the machine would
# not let the window be looked at (not key, covered, screen locked or asleep): it prints "not checkable:
# <the condition>" and lists no failures; 124 is the hard limit of 600 seconds on the whole run. Needs a
# release addon (`bun run build` in packages/native) and the React package built (`bun run build` in
# packages/react).
#
# `E2E_ONLY=order,pointer` runs only those groups: order, geometry, overflow, pointer, focus, lifecycle,
# and close. Close is the last step: it closes the app's own window, which ends the app, and verdict.ts
# makes the checks about it from what the addon logged.
set -e
here=$(cd "$(dirname "$0")" && pwd)
sh "$here/build-helper.sh"
sh "$here/build-addon.sh"
out="$HOME/.cache/gpuix-nativeview-e2e"
rm -f "$out/close-state.json" "$out/addon.log"
cd "$here"
status=0
NATIVEVIEW_E2E_ADDON_LOG="$out/addon.log" timeout -k 10 600 bun e2e.tsx "$@" || status=$?
if [ -f "$out/close-state.json" ]; then
  APP_STATUS=$status exec bun verdict.ts
fi
exit $status
