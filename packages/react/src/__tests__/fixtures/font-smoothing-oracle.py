"""Font smoothing ink oracle: ink coverage per sample from a headless Chrome screenshot, stdlib only.
usage: python3 -I font-smoothing-oracle.py <dump.html> <shot.png> <scale> <out.json>
Ink = sum over the ink window of |L - L_bg| / |L_fg - L_bg| (gamma-encoded Rec.709 luma, 0..1), as r8-font-weight.py,
in device px; divided by scale^2 for CSS px^2. The ink window is the advance +-4 CSS px wide and one line tall."""
import html
import json
import math
import re
import struct
import sys
import zlib

POL = {'dark': ('#000000', '#ffffff'), 'light': ('#ffffff', '#000000'), 'dim': ('#a1a1a1', '#0a0a0a')}
PAGE_W = 1200
PAGE_H = 1160


def hex_rgb(hexs):
    return [int(hexs[i:i + 2], 16) for i in (1, 3, 5)]


def luma(rgb):
    return (0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]) / 255


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
    if depth != 8 or channels is None or interlace != 0:
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
        rows.append((line, channels))
        prev = line
    return width, height, rows


def main():
    dump_path, png_path, scale_arg, out_path = sys.argv[1:5]
    scale = int(scale_arg)
    text = open(dump_path, encoding='utf-8').read()
    body = re.search(r'<body[^>]*\sdata-samples="([^"]*)"', text)
    if body is None:
        raise ValueError('no data-samples on body in the dump')
    layout = json.loads(html.unescape(body.group(1)))
    result_tag = re.search(r'<script type="application/json" id="result">(.*?)</script>', text, re.S)
    if result_tag is None:
        raise ValueError('no result script in the dump')
    result = json.loads(result_tag.group(1))
    if result['fontsLoaded'] is not True:
        raise ValueError('Inter Variable did not load')
    advance = {s['id']: s['advance'] for s in result['samples']}

    width, height, rows = read_png(png_path)
    if (width, height) != (PAGE_W * scale, PAGE_H * scale):
        raise ValueError('screenshot is %dx%d, expected %dx%d' % (width, height, PAGE_W * scale, PAGE_H * scale))

    out_samples = []
    for s in layout:
        fg, bg = POL[s['polarity']]
        box = s['box']
        adv = advance[s['id']]
        x0 = box['x'] - 4
        x1 = math.floor(box['x'] + adv + 4)
        y0 = box['y']
        y1 = box['y'] + box['h']
        window = (x0 * scale, x1 * scale, y0 * scale, y1 * scale)
        out_samples.append({
            'id': s['id'],
            'smoothing': s['smoothing'],
            'polarity': s['polarity'],
            'size': s['size'],
            'wght': s['wght'],
            'opsz': s['opsz'],
            'box': box,
            'ink': round(ink_rgb(rows, window, fg, bg, scale), 2),
            'advance': advance[s['id']],
            'inkWindow': {'x0': x0, 'x1': x1, 'y0': y0, 'y1': y1},
        })
    out = {
        'scale': scale,
        'samples': out_samples,
    }
    with open(out_path, 'w') as f:
        json.dump(out, f, indent=1)
        f.write('\n')
    print('wrote %s (%d samples)' % (out_path, len(out_samples)))


def ink_rgb(rows, window, fg, bg, scale):
    x0, x1, y0, y1 = window
    lbg = luma(hex_rgb(bg))
    span = abs(luma(hex_rgb(fg)) - lbg)
    total = 0.0
    for y in range(y0, y1):
        line, ch = rows[y]
        for x in range(x0, x1):
            i = x * ch
            total += abs(luma(line[i:i + 3]) - lbg)
    return total / span / (scale * scale)


if __name__ == '__main__':
    main()
