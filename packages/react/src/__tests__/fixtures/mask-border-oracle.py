"""Samples the screenshot of mask-border-oracle.html: reads the case list on stdin, the PNG at argv[1]."""
import html, json, re, struct, sys, zlib


def read_png(path):
    data = open(path, 'rb').read()
    position, idat, width, height, kind = 8, b'', 0, 0, 0
    while position < len(data):
        length, = struct.unpack('>I', data[position:position + 4])
        chunk, body = data[position + 4:position + 8], data[position + 8:position + 8 + length]
        position += 12 + length
        if chunk == b'IHDR':
            width, height, depth, kind = struct.unpack('>IIBB', body[:10])
            assert depth == 8
        elif chunk == b'IDAT':
            idat += body
    raw = zlib.decompress(idat)
    bytes_per_pixel = {2: 3, 6: 4}[kind]
    stride = width * bytes_per_pixel
    rows, previous, offset = [], bytearray(stride), 0
    for _ in range(height):
        filter_type = raw[offset]
        line = bytearray(raw[offset + 1:offset + 1 + stride])
        offset += 1 + stride
        for i in range(stride):
            a = line[i - bytes_per_pixel] if i >= bytes_per_pixel else 0
            b = previous[i]
            c = previous[i - bytes_per_pixel] if i >= bytes_per_pixel else 0
            if filter_type == 1:
                line[i] = (line[i] + a) & 255
            elif filter_type == 2:
                line[i] = (line[i] + b) & 255
            elif filter_type == 3:
                line[i] = (line[i] + ((a + b) >> 1)) & 255
            elif filter_type == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                predictor = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + predictor) & 255
        rows.append(line)
        previous = line
    return width, height, bytes_per_pixel, rows


width, height, bytes_per_pixel, rows = read_png(sys.argv[1])
text = html.unescape(re.sub(r'^data-oracle="|"$', '', sys.stdin.read().strip()))
cases = json.loads(text)
for index, case in enumerate(cases):
    left, top = (index % 5) * 200, (index // 5) * 200
    samples = ''
    for block_y in range(0, 200, 5):
        for block_x in range(0, 200, 5):
            total = sum(255 - rows[top + y][(left + x) * bytes_per_pixel + 1]
                        for y in range(block_y, block_y + 5) for x in range(block_x, block_x + 5))
            samples += '%02x' % round(total / 25)
    case['samples'] = samples
    if not re.search('repeat|round|space|fill', case['name']):
        continue
    alpha = lambda x, y: 255 - rows[top + y][(left + x) * bytes_per_pixel + 1]

    def band(along, fixed):
        # The smallest and largest alpha of the five pixels across the line at every pixel along it.
        low, high = '', ''
        for position in range(200):
            values = [along(position, fixed + across) for across in range(-2, 3)]
            low += '%02x' % min(values)
            high += '%02x' % max(values)
        return {'low': low, 'high': high}

    case['lines'] = {
        'row 57': band(lambda x, y: alpha(x, y), 57),
        'row 107': band(lambda x, y: alpha(x, y), 107),
        'column 57': band(lambda y, x: alpha(x, y), 57),
        'column 107': band(lambda y, x: alpha(x, y), 107),
    }
print(json.dumps(cases, separators=(',', ':')))
