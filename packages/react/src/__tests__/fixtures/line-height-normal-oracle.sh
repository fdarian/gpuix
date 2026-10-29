#!/bin/sh
# Regenerates line-height-normal-oracle.json (device scale 1) and line-height-normal-oracle-x2.json (device scale 2).
exec sh "$(dirname "$0")/dsf-oracle.sh" line-height-normal
