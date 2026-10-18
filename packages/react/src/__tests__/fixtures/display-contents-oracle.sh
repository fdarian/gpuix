#!/bin/sh
# Regenerates display-contents-oracle.json from display-contents-oracle.html with headless Chrome over CDP.
set -e
cd "$(dirname "$0")"
bun display-contents-oracle.ts > display-contents-oracle.json
