"""Text under a clip in a transformed box, ink per case from a headless Chrome screenshot, stdlib only.
usage: python3 -I clip-in-transform-oracle.py <dump.html> <shot.png> <scale> <out.json>
A case is one 420 x 260 cell of clip-in-transform-oracle.html. Its ink is the number of device pixels in the cell where the
red of the text is over the grey of the card or the white of the page: R - max(G, B) over 60, so an antialiased edge
counts as the part of it that is red."""
import html
import importlib.util
import json
import os
import re
import sys

PAGE_W = 1260
PAGE_H = 7020
CELL_W = 420
CELL_H = 260

spec = importlib.util.spec_from_file_location('font_smoothing_oracle', os.path.join(os.path.dirname(os.path.abspath(__file__)), 'font-smoothing-oracle.py'))
shared = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shared)


def ink(rows, x0, y0, scale):
    count = 0
    for y in range(int(y0 * scale), int((y0 + CELL_H) * scale)):
        line, channels = rows[y]
        for x in range(int(x0 * scale), int((x0 + CELL_W) * scale)):
            r, g, b = line[x * channels], line[x * channels + 1], line[x * channels + 2]
            if r - max(g, b) > 60:
                count += 1
    return count


def main():
    dump_path, png_path, scale_arg, out_path = sys.argv[1:5]
    scale = int(scale_arg)
    text = open(dump_path, encoding='utf-8').read()
    body = re.search(r'<body[^>]*\sdata-cases="([^"]*)"', text)
    if body is None:
        raise ValueError('no data-cases on body in the dump')
    cases = json.loads(html.unescape(body.group(1)))
    width, height, rows = shared.read_png(png_path)
    if (width, height) != (PAGE_W * scale, PAGE_H * scale):
        raise ValueError('screenshot is %dx%d, expected %dx%d' % (width, height, PAGE_W * scale, PAGE_H * scale))
    out = [c | {'ink': ink(rows, c['x'], c['y'], scale)} for c in cases]
    with open(out_path, 'w') as f:
        json.dump({'scale': scale, 'cases': out}, f, separators=(',', ':'))
    print('wrote %s (%d cases)' % (out_path, len(out)))


if __name__ == '__main__':
    main()
