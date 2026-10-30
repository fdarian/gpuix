"""Translated box oracle: where Chrome paints the four vertical edges of a box that is moved by a translation, from a
headless Chrome screenshot, stdlib only.
usage: python3 -I transform-snap-oracle.py <dump.html> <shot.png> <scale> <out.json>
A case is a row of the page (see transform-snap-oracle.html). Its pixels are read along the middle of the box, as luma
v in 0..1 (Rec.709, gamma-encoded) over the grey page, the black border or background and the white background. An edge
between a region of value a on its left and b on its right sits at x0 + sum((v(x) - b) / (a - b)) over the pixels x from x0,
the first pixel of the transition, to the last, in CSS px; the transition is the run of pixels that are neither a nor b
exactly, so a hard edge is the pixel boundary and a covered pixel moves it by its coverage. Edges, left to right:
  fill    left (page -> black), right (black -> page)
  border  outer left (page -> black), inner left (black -> page), inner right (page -> black), outer right (black -> page)
  both    inner left (black -> white), inner right (white -> black)"""
import html
import importlib.util
import json
import os
import re
import sys

PAGE_W = 1280
PAGE_H = 5900
BOX_H = 12
BOX_TOP = 2

spec = importlib.util.spec_from_file_location('font_smoothing_oracle', os.path.join(os.path.dirname(os.path.abspath(__file__)), 'font-smoothing-oracle.py'))
shared = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shared)

GREY = shared.luma([0x80, 0x80, 0x80])
BLACK = 0.0
WHITE = 1.0


def profile(rows, y, x0, x1):
    line, channels = rows[y]
    return [shared.luma(line[x * channels:x * channels + 3]) for x in range(x0, x1)]


def edge(values, start, a, b, tolerance=1.5 / 255):
    """The edge between regions a (left) and b (right) in `values`, in pixels from index 0: `start` is a pixel in a."""
    pos = start
    while pos < len(values) and abs(values[pos] - a) <= tolerance:
        pos += 1
    first = pos
    total = 0.0
    while pos < len(values) and abs(values[pos] - b) > tolerance:
        total += (values[pos] - b) / (a - b)
        pos += 1
    return first + total, pos


def edges(kind, values):
    if kind == 'fill':
        left, at = edge(values, 0, GREY, BLACK)
        right, at = edge(values, at, BLACK, GREY)
        return [left, right]
    if kind == 'border':
        found = []
        at = 0
        for a, b in ((GREY, BLACK), (BLACK, GREY), (GREY, BLACK), (BLACK, GREY)):
            position, at = edge(values, at, a, b)
            found.append(position)
        return found
    _, at = edge(values, 0, GREY, BLACK)
    inner_left, at = edge(values, at, BLACK, WHITE)
    inner_right, at = edge(values, at, WHITE, BLACK)
    return [inner_left, inner_right]


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
    out = []
    for c in cases:
        nominal = c['origin'] + c['translate']
        # Look from 4 px before the nominal left edge to 30 px after it, in device px.
        lo = int((nominal - 4) * scale)
        hi = int((nominal + 30) * scale)
        y = int((c['y'] + BOX_TOP + BOX_H / 2) * scale)
        if hi > width:
            raise ValueError('case %s reaches %d of %d' % (c['id'], hi, width))
        if lo < 0:
            raise ValueError('case %s starts at %d' % (c['id'], lo))
        values = profile(rows, y, lo, hi)
        found = [round((lo + e) / scale, 3) for e in edges(c['type'], values)]
        out.append({k: c[k] for k in ('id', 'type', 'kind', 'parentLeft', 'left', 'translate', 'x', 'y')} | {'edges': found})
    with open(out_path, 'w') as f:
        json.dump({'scale': scale, 'cases': out}, f, separators=(',', ':'))
    print('wrote %s (%d cases)' % (out_path, len(out)))


if __name__ == '__main__':
    main()
