#!/bin/sh
# Builds the webview e2e's helper (the capture and the events) and this e2e's own, next to the other e2e
# artifacts and not into the repository.
set -e
here=$(cd "$(dirname "$0")" && pwd)
sh "$here/../webview-e2e/build-helper.sh"
out="$HOME/.cache/gpuix-nativeview-e2e"
mkdir -p "$out"
if [ ! -f "$out/libnativeview_helper.dylib" ] || [ "$here/close_helper.m" -nt "$out/libnativeview_helper.dylib" ]; then
  clang -dynamiclib -fobjc-arc -framework AppKit -o "$out/libnativeview_helper.dylib" "$here/close_helper.m"
fi
