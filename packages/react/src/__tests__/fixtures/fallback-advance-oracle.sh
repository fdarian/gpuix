#!/bin/sh
# Regenerates fallback-advance-oracle.json (device scale 1) and fallback-advance-oracle-x2.json (device scale 2).
exec sh "$(dirname "$0")/dsf-oracle.sh" fallback-advance
