#!/bin/sh
# Regenerates flex-row-fit-oracle.json (device scale 1) and flex-row-fit-oracle-x2.json (scale 2) from flex-row-fit-oracle.html
# with headless Chrome.
exec sh "$(dirname "$0")/dsf-oracle.sh" flex-row-fit
