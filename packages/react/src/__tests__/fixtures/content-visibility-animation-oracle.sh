#!/bin/sh
# Regenerates content-visibility-animation-oracle.json from content-visibility-animation-oracle.html with headless Chrome
# (playwright, through uv; the python script launches the cached chromium_headless_shell, see its ROOT, and CHROME=<path>
# picks another binary): the scenes a test plays, each in a Chrome of its own, as {"chrome": version, "scenes": {"<scene>:<mode>": {"log": [...]}}}.
# The script also records a devtools trace of each scene; it is not kept, since no test reads it.
set -eu
cd "$(dirname "$0")"
SCENES="q1_run q1_run_read q1_load q2_run q2_load q2_class q3_before q3_inside q3_inside_mid"
NAMES=""
for scene in $SCENES; do NAMES="$NAMES $scene:hidden $scene:auto"; done
OUT=$(mktemp)
trap 'rm -f "$OUT"' EXIT
export OUT
rm -f "$OUT"
# shellcheck disable=SC2086
uv run --quiet --with playwright python content-visibility-animation-oracle.py $NAMES
python3 -I - "$OUT" content-visibility-animation-oracle.json <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))
for scene in data["scenes"].values():
    scene.pop("trace", None)
json.dump(data, open(sys.argv[2], "w"), separators=(",", ":"))
PY
