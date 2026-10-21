#!/bin/sh
# Regenerates fractional-lengths-oracle.json from fractional-lengths-oracle.html with headless Chrome at device scale 1 and
# 2: the probed boxes of every case, the painted edges (first/last device-pixel column and row of each box's colour in a
# screenshot) and the pixel runs along the scan lines of the cases that list them.
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
PAGE="file://$PWD/fractional-lengths-oracle.html"
WIDTH=${WIDTH:-2400}
HEIGHT=${HEIGHT:-2500}
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
for scale in 1 2; do
  "$CHROME" --headless --window-size=$WIDTH,$HEIGHT --force-device-scale-factor=$scale --hide-scrollbars \
    --allow-file-access-from-files --virtual-time-budget=3000 --dump-dom "$PAGE" 2>/dev/null | grep -o 'data-oracle="[^"]*"' \
    | python3 -I -c 'import html, re, sys; print(html.unescape(re.sub(r"^data-oracle=\"|\"$", "", sys.stdin.read().strip())))' > "$WORK/layout.$scale.json"
  "$CHROME" --headless --window-size=$WIDTH,$HEIGHT --force-device-scale-factor=$scale --hide-scrollbars \
    --screenshot="$WORK/shot.$scale.png" "$PAGE" >/dev/null 2>&1
done
python3 -I - "$WORK" fractional-lengths-oracle.json <<'PY'
import json, operator, struct, sys, zlib
from itertools import accumulate

work, target = sys.argv[1], sys.argv[2]

def read_png(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    pos, idat = 8, b""
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
    low = (255).__and__
    rows, previous = [], bytes(stride)
    for y in range(height):
        base = y * (stride + 1)
        filter_type, line = raw[base], raw[base + 1:base + 1 + stride]
        if filter_type == 0:
            current = bytes(line)
        elif filter_type == 1:
            current = bytearray(stride)
            for channel in range(channels):
                current[channel::channels] = bytes(map(low, accumulate(line[channel::channels])))
        elif filter_type == 2:
            current = bytes(map(low, map(operator.add, line, previous)))
        else:
            current = bytearray(line)
            for x in range(stride):
                left = current[x - channels] if x >= channels else 0
                up, up_left = previous[x], (previous[x - channels] if x >= channels else 0)
                if filter_type == 3:
                    current[x] = (current[x] + (left + up) // 2) & 255
                else:
                    p = left + up - up_left
                    pa, pb, pc = abs(p - left), abs(p - up), abs(p - up_left)
                    current[x] = (current[x] + (left if pa <= pb and pa <= pc else up if pb <= pc else up_left)) & 255
        rows.append(bytes(current))
        previous = rows[-1]
    return width, height, channels, rows

def find_aligned(row, needle, channels, last):
    index = row.rfind(needle) if last else row.find(needle)
    while index >= 0 and index % channels != 0:
        index = row.rfind(needle, 0, index + len(needle) - 1) if last else row.find(needle, index + 1)
    return index // channels

def painted_boxes(rows, channels, region, wanted):
    x0, y0, x1, y1 = region
    boxes = {}
    white = b"\xff" * (3 if channels == 3 else 3)
    for y in range(max(y0, 0), min(y1, len(rows))):
        row = rows[y]
        segment = row[x0 * channels:x1 * channels]
        present = set(zip(segment[0::channels], segment[1::channels], segment[2::channels]))
        for color in present:
            if color not in wanted:
                continue
            needle = bytes(color) + (b"\xff" if channels == 4 else b"")
            first = x0 + find_aligned(segment, needle, channels, False)
            last = x0 + find_aligned(segment, needle, channels, True)
            box = boxes.get(color)
            if box is None:
                boxes[color] = [first, y, last, y]
            else:
                box[0], box[2], box[3] = min(box[0], first), max(box[2], last), y
    return boxes

def runs(pixels):
    result = []
    for index, color in enumerate(pixels):
        if result and result[-1][0] == color:
            result[-1][2] = index + 1
        else:
            result.append([color, index, index + 1])
    return result

result = {}
for scale in (1, 2):
    layout = json.load(open(f"{work}/layout.{scale}.json"))
    assert layout["devicePixelRatio"] == scale, layout["devicePixelRatio"]
    assert layout["pageHeight"] * scale <= layout["innerHeight"] * scale, "page taller than the window: raise HEIGHT"
    width, height, channels, rows = read_png(f"{work}/shot.{scale}.png")
    assert (width, height) == (layout["innerWidth"] * scale, layout["innerHeight"] * scale), (width, height)
    for name, case in layout["cases"].items():
        entry = result.setdefault(name, {"html": case["html"], "paint": case["paint"]})
        if entry["html"] != case["html"]:
            entry["html" + str(scale)] = case["html"]
        data = {"boxes": case["elements"]}
        if case["paint"]:
            ox, oy = case["origin"]
            wanted = {}
            for element, colors in case["colors"].items():
                for kind, color in colors.items():
                    wanted[tuple(color)] = (element, kind)
            region = ((ox - 20) * scale, (oy - 20) * scale, (ox + 380) * scale, (oy + 180) * scale)
            painted = {}
            for color, (x0, y0, x1, y1) in painted_boxes(rows, channels, region, wanted).items():
                element, kind = wanted[color]
                painted.setdefault(element, {})[kind] = {"x0": x0 - ox * scale, "x1": x1 - ox * scale, "y0": y0 - oy * scale, "y1": y1 - oy * scale}
            data["painted"] = painted
            data["scans"] = []
            for axis, at, start, end in case["scans"]:
                if axis == "h":
                    pixels = [bytes(rows[(oy + at) * scale][x * channels:x * channels + 3]).hex() for x in range((ox + start) * scale, (ox + end) * scale)]
                else:
                    pixels = [bytes(rows[y][(ox + at) * scale * channels:(ox + at) * scale * channels + 3]).hex() for y in range((oy + start) * scale, (oy + end) * scale)]
                data["scans"].append({"axis": axis, "at": at, "start": start * scale, "runs": runs(pixels)})
        entry[str(scale)] = data
json.dump(result, open(target, "w"), separators=(",", ":"))
PY
