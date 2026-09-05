#!/usr/bin/env python3
"""Compare retained actual-GPU artifacts; stdlib only, no exact portable hash.

Pass native evidence.json followed by downloaded browser evidence JSON(s).
Tolerance: RGB mean absolute error <= 2/255; <= 1% of pixels may differ by
more than 12/255 in any RGB channel. Alpha must be opaque. Geometry and HUD
probes are separate assertions, so a globally small image error cannot hide them.
"""
import base64
import json
from pathlib import Path
import struct
import sys
import zlib

WIDTH, HEIGHT = 960, 540
MAX_BYTES = 40 * 1024 * 1024


def rgba(data):
    assert data.startswith(b'\x89PNG\r\n\x1a\n'), 'not PNG'
    offset, compressed, dimensions = 8, bytearray(), None
    while offset < len(data):
        length = struct.unpack('>I', data[offset:offset + 4])[0]
        kind = data[offset + 4:offset + 8]
        payload = data[offset + 8:offset + 8 + length]
        if kind == b'IHDR':
            dimensions = struct.unpack('>IIBBBBB', payload)
        if kind == b'IDAT':
            compressed.extend(payload)
        offset += length + 12
    assert dimensions == (WIDTH, HEIGHT, 8, 6, 0, 0, 0), dimensions
    decoder = zlib.decompressobj()
    raw = decoder.decompress(compressed, (WIDTH * 4 + 1) * HEIGHT + 1)
    assert decoder.eof and not decoder.unconsumed_tail
    stride = WIDTH * 4
    assert len(raw) == (stride + 1) * HEIGHT
    rows, previous = [], bytearray(stride)
    for y in range(HEIGHT):
        start = y * (stride + 1)
        filtering, row = raw[start], bytearray(raw[start + 1:start + 1 + stride])
        assert filtering in range(5)
        for x in range(stride):
            a, b, c = (row[x - 4] if x >= 4 else 0), previous[x], (previous[x - 4] if x >= 4 else 0)
            if filtering == 1: predictor = a
            elif filtering == 2: predictor = b
            elif filtering == 3: predictor = (a + b) // 2
            elif filtering == 4:
                p = a + b - c
                distances = (abs(p - a), abs(p - b), abs(p - c))
                predictor = (a, b, c)[distances.index(min(distances))]
            else: predictor = 0
            row[x] = (row[x] + predictor) & 255
        rows.append(row)
        previous = row
    pixels = b''.join(rows)
    assert all(a == 255 for a in pixels[3::4]), 'nonopaque composed frame'
    return pixels


def load(path):
    assert path.stat().st_size <= MAX_BYTES
    document = json.loads(path.read_text())
    assert document.get('status', 'passed') == 'passed', document.get('error')
    images = {}
    for name, capture in document['captures'].items():
        artifact = capture['artifact']
        if artifact.startswith('data:image/png;base64,'):
            raw = base64.b64decode(artifact.split(',', 1)[1], validate=True)
            # Persist browser PNGs next to the downloaded evidence for visual review.
            destination = path.parent / f'{path.stem}-{name}.png'
            assert name in {'initial', 'initial-repeat', 'win', 'replay', 'reset', 'suspended', 'depth-behind', 'depth-front', 'projection-far', 'after-cancel', 'after-error', 'after-pending-mutation'}
            destination.write_bytes(raw)
        else:
            assert Path(artifact).name == artifact, 'capture must be adjacent local PNG'
            raw = (path.parent / artifact).read_bytes()
        images[name] = rgba(raw)
    return document, images


def cyan_bounds(pixels):
    points = [(i // 4 % WIDTH, i // 4 // WIDTH) for i in range(0, len(pixels), 4)
              if pixels[i + 1] > 95 and pixels[i + 2] > 110 and pixels[i] * 2 < pixels[i + 1]]
    if not points:
        return {'pixels': 0, 'width': 0, 'height': 0}
    xs, ys = zip(*points)
    return {'pixels': len(points), 'width': max(xs) - min(xs) + 1,
            'height': max(ys) - min(ys) + 1, 'center': [(min(xs)+max(xs))/2, (min(ys)+max(ys))/2]}


def geometry(images):
    probes = {name: cyan_bounds(images[name]) for name in ['initial', 'depth-behind', 'depth-front', 'projection-far']}
    assert probes['depth-front']['pixels'] > 100, probes
    assert probes['depth-behind']['pixels'] < probes['depth-front']['pixels'] * .1, probes
    assert probes['initial']['width'] > probes['projection-far']['width'] * 1.15, probes
    assert probes['initial']['height'] > probes['projection-far']['height'] * 1.15, probes
    # Warm ECS text appears in the top-left region, separate from the 3D room.
    hud = sum(1 for y in range(4, 30) for x in range(4, 320)
              if (p := images['initial'][(y * WIDTH + x) * 4:(y * WIDTH + x) * 4 + 3])[0] > 200 and p[1] > 180 and p[2] < 220)
    assert hud > 25, hud
    return {'cyan': probes, 'hud_pixels': hud}


def main(paths):
    loaded = [(path, *load(path)) for path in paths]
    report = {'tolerances': {'mean_absolute_rgb': 2, 'pixel_channel_threshold': 12, 'fraction_over_threshold': .01}, 'geometry': {}, 'comparisons': []}
    for path, document, images in loaded:
        report['geometry'][str(path)] = geometry(images)
        for name in ['initial', 'win']:
            capture = document['captures'][name]
            assert capture['state']['session_tick'] == (44 if name == 'win' else 0)
            assert capture['state']['collected'] == (3 if name == 'win' else 0)
    native = loaded[0][2]
    for path, _, images in loaded[1:]:
        for name in ['initial', 'win', 'depth-behind', 'depth-front', 'projection-far']:
            differences = [abs(a - b) for i, (a, b) in enumerate(zip(native[name], images[name])) if i % 4 != 3]
            mean = sum(differences) / len(differences)
            fraction = sum(max(differences[i:i + 3]) > 12 for i in range(0, len(differences), 3)) / (WIDTH * HEIGHT)
            assert mean <= 2 and fraction <= .01, (path, name, mean, fraction)
            report['comparisons'].append({'path': str(path), 'capture': name, 'mean_absolute_rgb': mean, 'fraction_over_threshold': fraction})
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    assert len(sys.argv) >= 2, 'pass native evidence.json and optional downloaded browser JSON(s)'
    main([Path(value).resolve() for value in sys.argv[1:]])
