#!/bin/sh
# Regenerates motion-default-oracle.json from Motion's own animateTarget. MOTION_DOM is the motion-dom/dist/es directory of an
# installed motion-dom 14.1.0.
set -e
cd "$(dirname "$0")"
bun motion-default-oracle.mjs > motion-default-oracle.json
