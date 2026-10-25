"""Transformed backdrop-filter oracle: samples of a headless Chrome screenshot, stdlib only.
usage: python3 -I transform-backdrop-oracle.py <dump.html> <shot.png> <out.json>
Per cell of transform-backdrop-oracle.html:
  points  the inverted fraction (1 - red / 255) of the pixel each point falls in
  lines   the red channel of the pixels along a line, as hex pairs, and `sigma`: the standard deviation of the Gaussian-blurred
          step at `edge` (in pixels from the line's start) that fits the line best, found by a grid search"""
import html
import importlib.util
import json
import math
import os
import re
import sys

PAGE_W = 1600
PAGE_H = 1650

# The PNG reader of font-smoothing-oracle.py, loaded by path since -I leaves the script's directory off sys.path.
spec = importlib.util.spec_from_file_location('font_smoothing_oracle', os.path.join(os.path.dirname(os.path.abspath(__file__)), 'font-smoothing-oracle.py'))
shared = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shared)


def red(rows, x, y):
    line, channels = rows[y]
    return line[x * channels]


def fit_sigma(values, edge):
    low, high = values[0], values[-1]
    if abs(high - low) < 64:
        raise ValueError('no step in the line')
    target = [(v - low) / (high - low) for v in values]
    best, best_error = None, None
    for step in range(5, 400):
        sigma = step / 20
        error = 0.0
        for i, t in enumerate(target):
            model = 0.5 * (1 + math.erf((i + 0.5 - edge) / (sigma * math.sqrt(2))))
            error += (model - t) ** 2
        if best_error is None or error < best_error:
            best, best_error = sigma, error
    return best


def main():
    dump_path, png_path, out_path = sys.argv[1:4]
    text = open(dump_path, encoding='utf-8').read()
    body = re.search(r'<body[^>]*\sdata-oracle="([^"]*)"', text)
    if body is None:
        raise ValueError('no data-oracle on body in the dump')
    cells = json.loads(html.unescape(body.group(1)))
    width, height, rows = shared.read_png(png_path)
    if (width, height) != (PAGE_W, PAGE_H):
        raise ValueError('screenshot is %dx%d, expected %dx%d' % (width, height, PAGE_W, PAGE_H))

    out = []
    for cell in cells:
        left, top = cell['box']['x'], cell['box']['y']
        entry = {'id': cell['id'], 'kind': cell['kind'], 'matrix': [round(v, 4) for v in cell['matrix']]}
        if cell['points']:
            entry['points'] = {
                p['name']: round(1 - red(rows, left + math.floor(p['x']), top + math.floor(p['y'])) / 255, 3)
                for p in cell['points']
            }
        if cell['lines']:
            entry['lines'] = {}
            for line in cell['lines']:
                values = []
                for i in range(line['count']):
                    x = line['x'] + (i if line['axis'] == 'x' else 0)
                    y = line['y'] + (i if line['axis'] == 'y' else 0)
                    values.append(red(rows, left + x, top + y))
                entry['lines'][line['name']] = {
                    'start': [line['x'], line['y']],
                    'axis': line['axis'],
                    'values': ''.join('%02x' % v for v in values),
                    'sigma': fit_sigma(values, line['edge']),
                }
        out.append(entry)
    with open(out_path, 'w') as f:
        json.dump(out, f, separators=(',', ':'))
        f.write('\n')
    print('wrote %s (%d cells)' % (out_path, len(out)))


if __name__ == '__main__':
    main()
