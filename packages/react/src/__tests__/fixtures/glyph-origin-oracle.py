"""Glyph origin oracle: the first and last rows that a label's ink and a chip text's ink are painted on, from a headless
Chrome screenshot, stdlib only.
usage: python3 -I glyph-origin-oracle.py <dump.html> <shot.png> <scale> <out.json>
A row is ink when a pixel of it is more than 64 off the background in some channel (the label: the page's #171717
over the label's box, 3 CSS px more above and below; the chip text: the chip's #525252 over the chip's inner rows, 8 CSS px
in from its sides). Rows are device pixel rows of the screenshot."""
import html
import json
import math
import re
import struct
import sys
import zlib

PAGE_W = 1920
PAGE_H = 400
PAGE_BG = (0x17, 0x17, 0x17)
CHIP_BG = (0x52, 0x52, 0x52)
THRESHOLD = 64


def read_png(path):
    data = open(path, 'rb').read()
    if data[:8] != b'\x89PNG\r\n\x1a\n':
        raise ValueError('not a PNG: ' + path)
    pos = 8
    idat = []
    ihdr = None
    while pos < len(data):
        length, = struct.unpack('>I', data[pos:pos + 4])
        kind = data[pos + 4:pos + 8]
        body = data[pos + 8:pos + 8 + length]
        pos += 12 + length
        if kind == b'IHDR':
            ihdr = struct.unpack('>IIBBBBB', body)
        elif kind == b'IDAT':
            idat.append(body)
        elif kind == b'IEND':
            break
    width, height, depth, ctype, _, _, interlace = ihdr
    channels = {0: 1, 2: 3, 4: 2, 6: 4}.get(ctype)
    if depth != 8 or channels not in (3, 4) or interlace != 0:
        raise ValueError('unsupported PNG: depth %d type %d interlace %d' % (depth, ctype, interlace))
    raw = zlib.decompress(b''.join(idat))
    stride = width * channels
    rows = []
    prev = bytearray(stride)
    for y in range(height):
        f = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        if f == 1:
            for i in range(channels, stride):
                line[i] = (line[i] + line[i - channels]) & 255
        elif f == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 255
        elif f == 3:
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                line[i] = (line[i] + ((left + prev[i]) >> 1)) & 255
        elif f == 4:
            for i in range(stride):
                a = line[i - channels] if i >= channels else 0
                b = prev[i]
                c = prev[i - channels] if i >= channels else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 255
        elif f != 0:
            raise ValueError('bad filter %d' % f)
        rows.append(line)
        prev = line
    return width, height, channels, rows


def ink_rows(rows, channels, x0, x1, y0, y1, bg):
    hit = []
    for y in range(y0, y1):
        line = rows[y]
        for x in range(x0, x1):
            at = x * channels
            if any(abs(line[at + c] - bg[c]) > THRESHOLD for c in range(3)):
                hit.append(y)
                break
    if not hit:
        raise ValueError('no ink in rows %d..%d' % (y0, y1))
    return [hit[0], hit[-1]]


def round_half_up(value):
    return math.floor(value + 0.5)


def main():
    dump_path, png_path, scale_arg, out_path = sys.argv[1:5]
    scale = int(scale_arg)
    text = open(dump_path, encoding='utf-8').read()
    body = re.search(r'<body[^>]*\sdata-rects="([^"]*)"', text)
    if body is None:
        raise ValueError('no data-rects on body in the dump')
    page = json.loads(html.unescape(body.group(1)))
    if page['fontsLoaded'] is not True:
        raise ValueError('Inter Variable did not load')
    width, height, channels, rows = read_png(png_path)
    if (width, height) != (PAGE_W * scale, PAGE_H * scale):
        raise ValueError('screenshot is %dx%d, expected %dx%d' % (width, height, PAGE_W * scale, PAGE_H * scale))
    cases = []
    for case in page['cases']:
        x, y, w, h = case['label']
        label_rows = ink_rows(rows, channels, round_half_up(x * scale), round_half_up((x + w) * scale),
                              round_half_up((y - 3) * scale), round_half_up((y + h + 3) * scale), PAGE_BG)
        x, y, w, h = case['chip']
        text_rows = ink_rows(rows, channels, round_half_up((x + 8) * scale), round_half_up((x + w - 8) * scale),
                             round_half_up(y * scale) + 1, round_half_up((y + h) * scale) - 1, CHIP_BG)
        case['labelRows'] = label_rows
        case['chipTextRows'] = text_rows
        cases.append(case)
    with open(out_path, 'w') as f:
        json.dump({'scale': scale, 'cases': cases}, f, separators=(',', ':'))
    print('wrote %s (%d cases)' % (out_path, len(cases)))


main()
