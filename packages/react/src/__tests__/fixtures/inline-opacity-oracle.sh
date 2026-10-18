#!/bin/sh
# Regenerates inline-opacity-oracle.json from inline-opacity-oracle.html with headless Chrome: for each case the colours
# at the points the page names, at device scale 1.
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
PAGE="file://$PWD/inline-opacity-oracle.html"
"$CHROME" --headless --window-size=1280,800 --dump-dom "$PAGE" 2>/dev/null | grep -o 'data-oracle="[^"]*"' \
  | python3 -c 'import html, re, sys; print(html.unescape(re.sub(r"^data-oracle=\"|\"$", "", sys.stdin.read().strip())))' > /tmp/inline-opacity-oracle.layout.json
HEIGHT=$(python3 -c 'import json; print(json.load(open("/tmp/inline-opacity-oracle.layout.json"))["total"] + 20)')
"$CHROME" --headless --window-size=1280,$HEIGHT --force-device-scale-factor=1 --hide-scrollbars \
  --screenshot=/tmp/inline-opacity-oracle.png "$PAGE" >/dev/null 2>&1
python3 - /tmp/inline-opacity-oracle.layout.json inline-opacity-oracle.json <<'PY'
import json, struct, sys, zlib

def read_png(path):
    data = open(path, "rb").read()
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

layout = json.load(open(sys.argv[1]))
width, height, channels, rows = read_png("/tmp/inline-opacity-oracle.png")
pixel = lambda x, y: [rows[y][x * channels + c] for c in range(3)]
result = {"cases": {}}
for name, case in layout["cases"].items():
    colors = {point: pixel(*xy) for point, xy in case["points"].items()}
    result["cases"][name] = {"html": case["html"], "points": case["points"], "colors": colors}
json.dump(result, open(sys.argv[2], "w"), separators=(",", ":"))
PY
