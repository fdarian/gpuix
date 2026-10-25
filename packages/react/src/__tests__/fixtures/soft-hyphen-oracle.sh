#!/bin/sh
# Regenerates soft-hyphen-oracle.json (device scale 1) and soft-hyphen-oracle-x2.json (device scale 2) from
# soft-hyphen-oracle.html with headless Chrome, which is what `sh dsf-oracle.sh soft-hyphen` does.
set -e
cd "$(dirname "$0")"
exec sh dsf-oracle.sh soft-hyphen
