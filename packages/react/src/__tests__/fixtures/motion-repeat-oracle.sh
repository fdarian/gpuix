#!/bin/sh
# Regenerates motion-repeat-oracle.json from Motion's own animation code. MOTION_DOM is the motion-dom/dist/es
# directory of an installed motion-dom (12.x).
set -e
cd "$(dirname "$0")"
bun motion-repeat-oracle.mjs > motion-repeat-oracle.json
