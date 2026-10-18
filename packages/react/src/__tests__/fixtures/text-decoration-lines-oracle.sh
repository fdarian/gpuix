#!/bin/sh
# Regenerates text-decoration-lines-oracle.json from text-decoration-lines-oracle.html with headless Chrome: for every
# case, at device scale 1 and 2, the runs of red (and blue) pixels along x of each row of its band that has any.
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
PAGE="file://$PWD/text-decoration-lines-oracle.html"
"$CHROME" --headless --window-size=1280,800 --dump-dom "$PAGE" 2>/dev/null | grep -o 'data-oracle="[^"]*"' \
  | python3 -c 'import html, re, sys; print(html.unescape(re.sub(r"^data-oracle=\"|\"$", "", sys.stdin.read().strip())))' > /tmp/text-decoration-lines-oracle.layout.json
HEIGHT=$(python3 -c 'import json; print(json.load(open("/tmp/text-decoration-lines-oracle.layout.json"))["total"] + 20)')
for scale in 1 2; do
  "$CHROME" --headless --window-size=1280,$HEIGHT --force-device-scale-factor=$scale --hide-scrollbars \
    --screenshot=/tmp/text-decoration-lines-oracle.$scale.png "$PAGE" >/dev/null 2>&1
done
python3 - /tmp/text-decoration-lines-oracle.layout.json text-decoration-lines-oracle.json <<'PY'
import json, struct, sys, zlib

def read_png(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    pos, idat, width, height, depth, kind = 8, b"", 0, 0, 0, 0
    while pos < len(data):
        length, tag = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + length]
        if tag == b"IHDR":
            width, height, depth, kind = struct.unpack(">IIBB", body[:10])
        elif tag == b"IDAT":
            idat += body
        pos += 12 + length
    assert depth == 8 and kind in (2, 6), (depth, kind)
    channels = 4 if kind == 6 else 3
    raw, stride = zlib.decompress(idat), width * channels
    rows, previous = [], bytearray(stride)
    for y in range(height):
        base = y * (stride + 1)
        filter_type, line = raw[base], bytearray(raw[base + 1:base + 1 + stride])
        for x in range(stride):
            left = line[x - channels] if x >= channels else 0
            up, up_left = previous[x], (previous[x - channels] if x >= channels else 0)
            if filter_type == 1: line[x] = (line[x] + left) & 255
            elif filter_type == 2: line[x] = (line[x] + up) & 255
            elif filter_type == 3: line[x] = (line[x] + (left + up) // 2) & 255
            elif filter_type == 4:
                p = left + up - up_left
                pa, pb, pc = abs(p - left), abs(p - up), abs(p - up_left)
                line[x] = (line[x] + (left if pa <= pb and pa <= pc else up if pb <= pc else up_left)) & 255
        rows.append(line)
        previous = line
    return width, height, channels, rows

def runs_of(line, channels, width, test):
    found, start = [], None
    for x in range(width):
        on = test(line[x * channels], line[x * channels + 1], line[x * channels + 2])
        if on and start is None:
            start = x
        elif not on and start is not None:
            found.append([start, x])
            start = None
    if start is not None:
        found.append([start, width])
    return found

COLORS = (("red", lambda r, g, b: r > 200 and g < 120 and b < 120), ("blue", lambda r, g, b: b > 200 and r < 120 and g < 120))
layout = json.load(open(sys.argv[1]))
result = {"cases": {}}
for scale in (1, 2):
    width, height, channels, rows = read_png(f"/tmp/text-decoration-lines-oracle.{scale}.png")
    for name, case in layout["cases"].items():
        entry = result["cases"].setdefault(name, {"html": case["html"], "bandStyle": case["bandStyle"], "height": case["height"]})
        marks = {}
        for color, test in COLORS:
            previous = None
            for y in range(case["top"] * scale, (case["top"] + case["height"]) * scale):
                found = runs_of(rows[y], channels, 600 * scale, test)
                if not found:
                    previous = None
                    continue
                local = y - case["top"] * scale
                if previous is not None and previous[1] == found:
                    previous[0].append(local)
                else:
                    previous = [[local], found]
                    marks.setdefault(color, []).append(previous)
        entry[f"x{scale}"] = marks
json.dump(result, open(sys.argv[2], "w"), separators=(",", ":"))
PY
