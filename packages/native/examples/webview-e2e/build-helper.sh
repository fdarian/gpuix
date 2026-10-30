#!/bin/sh
# Builds the test-only native helper next to the other e2e artifacts, not into the repository.
set -e
here=$(cd "$(dirname "$0")" && pwd)
out="$HOME/.cache/gpuix-webview-e2e"
mkdir -p "$out"
if [ ! -f "$out/libe2e.dylib" ] || [ "$here/e2e_helper.m" -nt "$out/libe2e.dylib" ]; then
  clang -dynamiclib -fobjc-arc -framework AppKit -framework CoreGraphics -framework ImageIO \
    -o "$out/libe2e.dylib" "$here/e2e_helper.m"
fi
