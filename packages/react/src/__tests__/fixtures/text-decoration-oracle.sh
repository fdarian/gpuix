#!/bin/sh
# Regenerates text-decoration-oracle.json from text-decoration-oracle.html with headless Chrome: the rows of red
# (and blue) underline pixels of every `data-seg` stretch, in CSS pixels, at device scale 1 and 2.
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
PAGE="file://$PWD/text-decoration-oracle.html"
HEIGHT=${HEIGHT:-12000}
"$CHROME" --headless --window-size=1280,800 --dump-dom "$PAGE" 2>/dev/null | grep -o 'data-oracle="[^"]*"' \
  | python3 -c 'import html, re, sys; print(html.unescape(re.sub(r"^data-oracle=\"|\"$", "", sys.stdin.read().strip())))' > /tmp/text-decoration-oracle.layout.json
for scale in 1 2; do
  "$CHROME" --headless --window-size=1280,$HEIGHT --force-device-scale-factor=$scale --hide-scrollbars \
    --screenshot=/tmp/text-decoration-oracle.$scale.png "$PAGE" >/dev/null 2>&1
done
python3 - /tmp/text-decoration-oracle.layout.json text-decoration-oracle.json <<'PY'
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

layout = json.load(open(sys.argv[1]))
result = {"cases": {}}
for scale in (1, 2):
    width, height, channels, rows = read_png(f"/tmp/text-decoration-oracle.{scale}.png")
    for name, case in layout["cases"].items():
        entry = result["cases"].setdefault(name, {"html": case["html"], "bandStyle": case["bandStyle"], "segs": {}})
        for seg, (left, top, right, bottom) in case["segs"].items():
            x0, x1 = int((left + 1) * scale), int((right - 1) * scale)
            y0, y1 = int(case["top"] * scale), int((case["top"] + 60) * scale)
            marks = {}
            for y in range(y0, y1):
                line = rows[y]
                for color, test in (("red", lambda r, g, b: r > 200 and g < 120 and b < 120), ("blue", lambda r, g, b: b > 200 and r < 120 and g < 120)):
                    count = sum(1 for x in range(x0, x1) if test(line[x * channels], line[x * channels + 1], line[x * channels + 2]))
                    if count >= 3:
                        marks.setdefault(color, []).append(y - y0)
            entry["segs"].setdefault(seg, {"rect": [left, top - case["top"], right, bottom - case["top"]]})[f"x{scale}"] = marks
json.dump(result, open(sys.argv[2], "w"), separators=(",", ":"))
PY
