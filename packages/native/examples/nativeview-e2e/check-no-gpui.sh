#!/bin/sh
# The addon is a library that knows nothing of GPUI: fails if its dependency tree or the libraries
# its .node links name gpui, gpuix or zed.
set -e
# A crate or library whose name starts with one of them. Left out: the first line of cargo tree (the
# crate's path) and the first two of otool -L (the file and its own install name, a path in target/), which
# are inside a checkout of gpuix.
names='(^|[^a-z])(gpui|gpuix|zed)'
here=$(cd "$(dirname "$0")" && pwd)
addon="$HOME/.cache/gpuix-nativeview-e2e/nativeview_e2e_addon.node"
[ -f "$addon" ] || sh "$here/build-addon.sh"

tree=$(cd "$here/addon" && cargo tree --manifest-path Cargo.toml)
echo "$ cargo tree (addon/)"
echo "$tree"
if echo "$tree" | tail -n +2 | grep -Eqi "$names"; then
  echo "FAIL: the addon's dependency tree names gpui, gpuix or zed" >&2
  exit 1
fi

linked=$(otool -L "$addon")
echo "$ otool -L $addon"
echo "$linked"
if echo "$linked" | tail -n +3 | grep -Eqi "$names"; then
  echo "FAIL: the addon links a gpui, gpuix or zed library" >&2
  exit 1
fi
echo "ok: no gpui, gpuix or zed in the dependency tree or the linked libraries"
