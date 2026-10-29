#!/bin/sh
# Regenerates cjk-spacing-oracle.json (device scale 1) and cjk-spacing-oracle-x2.json (device scale 2).
exec sh "$(dirname "$0")/dsf-oracle.sh" cjk-spacing
