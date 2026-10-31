#!/bin/sh
# Builds the test addon (its own crate, no GPUI) into the cache folder, when a source is newer than it.
set -e
here=$(cd "$(dirname "$0")" && pwd)
out="$HOME/.cache/gpuix-nativeview-e2e"
mkdir -p "$out"
addon="$out/nativeview_e2e_addon.node"
if [ ! -f "$addon" ] || [ -n "$(find "$here/addon/src" "$here/addon/Cargo.toml" "$here/addon/build.rs" -newer "$addon" -print -quit)" ]; then
  (cd "$here/addon" && cargo build --release)
  cp "$here/addon/target/release/libnativeview_e2e_addon.dylib" "$addon"
fi
