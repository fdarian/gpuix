"""Transformed text oracle: per cell the ink, ink box and two sharpness measures of text Chrome paints under a transform,
from a headless Chrome screenshot, stdlib only.
usage: python3 -I transform-text-oracle.py <dump.html> <shot.png> <scale> <out.json>
Coverage c of a device pixel = |L - L_bg| / |L_fg - L_bg| (gamma-encoded Rec.709 luma, 0..1), as font-smoothing-oracle.py,
over the whole 260 x 150 CSS px cell. Per cell:
  ink    sum of c, in CSS px squared
  box    [x0, y0, x1, y1] in CSS px from the cell's corner: the pixels with c > 0.1
  solid  sum c^2 / sum c: how much of the ink is at full coverage (a resampled glyph loses it)
  edge   sum of the squared differences of c between neighbouring pixels (x and y) / sum c, in 1/device px: how steep
         the edges are (a resampled glyph loses it)"""
import html
import importlib.util
import json
import os
import re
import sys

PAGE_W = 1200
PAGE_H = 2060
CELL_W = 260
CELL_H = 150
BOX_THRESHOLD = 0.1

# The PNG reader and the luma of font-smoothing-oracle.py, loaded by path since -I leaves the script's directory off sys.path.
spec = importlib.util.spec_from_file_location('font_smoothing_oracle', os.path.join(os.path.dirname(os.path.abspath(__file__)), 'font-smoothing-oracle.py'))
shared = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shared)

POL = {'dark': ('#000000', '#ffffff'), 'light': ('#ffffff', '#000000')}


def coverage(rows, x0, y0, w, h, fg, bg):
    lbg = shared.luma(shared.hex_rgb(bg))
    span = abs(shared.luma(shared.hex_rgb(fg)) - lbg)
    grid = []
    for y in range(y0, y0 + h):
        line, ch = rows[y]
        grid.append([abs(shared.luma(line[x * ch:x * ch + 3]) - lbg) / span for x in range(x0, x0 + w)])
    return grid


def measures(grid, scale):
    total = 0.0
    squares = 0.0
    edge = 0.0
    x_min = y_min = 10 ** 9
    x_max = y_max = -1
    for y, line in enumerate(grid):
        below = grid[y + 1] if y + 1 < len(grid) else None
        for x, c in enumerate(line):
            if c <= 0.0:
                if x + 1 < len(line) and line[x + 1] > 0.0:
                    edge += line[x + 1] ** 2
                if below is not None and below[x] > 0.0:
                    edge += below[x] ** 2
                continue
            total += c
            squares += c * c
            if c > BOX_THRESHOLD:
                x_min, x_max = min(x_min, x), max(x_max, x)
                y_min, y_max = min(y_min, y), max(y_max, y)
            if x + 1 < len(line):
                edge += (line[x + 1] - c) ** 2
            if below is not None:
                edge += (below[x] - c) ** 2
    box = [round(x_min / scale, 2), round(y_min / scale, 2), round((x_max + 1) / scale, 2), round((y_max + 1) / scale, 2)] if x_max >= 0 else None
    return {
        'ink': round(total / (scale * scale), 2),
        'box': box,
        'solid': round(squares / total, 4) if total else 0,
        'edge': round(edge / total, 4) if total else 0,
    }


def main():
    dump_path, png_path, scale_arg, out_path = sys.argv[1:5]
    scale = int(scale_arg)
    text = open(dump_path, encoding='utf-8').read()
    body = re.search(r'<body[^>]*\sdata-samples="([^"]*)"', text)
    if body is None:
        raise ValueError('no data-samples on body in the dump')
    cells = json.loads(html.unescape(body.group(1)))
    result_tag = re.search(r'<script type="application/json" id="result">(.*?)</script>', text, re.S)
    if result_tag is None:
        raise ValueError('no result script in the dump')
    if json.loads(result_tag.group(1))['fontsLoaded'] is not True:
        raise ValueError('Inter Variable did not load')

    width, height, rows = shared.read_png(png_path)
    if (width, height) != (PAGE_W * scale, PAGE_H * scale):
        raise ValueError('screenshot is %dx%d, expected %dx%d' % (width, height, PAGE_W * scale, PAGE_H * scale))

    out_cells = []
    for c in cells:
        fg, bg = POL[c['polarity']]
        box = c['box']
        grid = coverage(rows, box['x'] * scale, box['y'] * scale, CELL_W * scale, CELL_H * scale, fg, bg)
        out_cells.append({
            'id': c['id'],
            'polarity': c['polarity'],
            'smoothing': c['smoothing'],
            'size': c['size'],
            'transform': c['transform'],
            **measures(grid, scale),
        })
    with open(out_path, 'w') as f:
        json.dump({'scale': scale, 'cells': out_cells}, f, indent=1)
        f.write('\n')
    print('wrote %s (%d cells)' % (out_path, len(out_cells)))


if __name__ == '__main__':
    main()
