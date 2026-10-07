#!/usr/bin/env python3
"""Download the Lucide icons that calibre's built-in icon names map to
(crates/plugins/python/resources/images/calibre-icons.json) into
resources/images/lucide/, from the lucide-static package of the version
the mapping names, with its licence. Run after changing the mapping;
the SVGs are committed.

    python3 scripts/fetch-lucide-icons.py
"""

import json
import os
import sys
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
IMAGES = os.path.join(ROOT, 'crates', 'plugins', 'python', 'resources', 'images')
MAPPING = os.path.join(IMAGES, 'calibre-icons.json')
OUT = os.path.join(IMAGES, 'lucide')


def fetch(url):
    req = urllib.request.Request(url, headers={'User-Agent': 'magnitide-fetch-lucide-icons'})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def main():
    with open(MAPPING, encoding='utf-8') as f:
        mapping = json.load(f)
    version = mapping['lucide_static_version']
    base = f'https://unpkg.com/lucide-static@{version}'
    os.makedirs(OUT, exist_ok=True)
    wanted = sorted(set(mapping['icons'].values()))
    for name in wanted:
        dest = os.path.join(OUT, name + '.svg')
        if os.path.exists(dest):
            continue
        svg = fetch(f'{base}/icons/{name}.svg')
        if b'<svg' not in svg:
            print(f'not an SVG: {name}', file=sys.stderr)
            return 1
        with open(dest, 'wb') as f:
            f.write(svg)
        print('fetched', name)
    with open(os.path.join(OUT, 'LICENSE'), 'wb') as f:
        f.write(fetch(f'{base}/LICENSE'))
    stray = sorted(f[:-4] for f in os.listdir(OUT) if f.endswith('.svg') and f[:-4] not in wanted)
    for name in stray:
        os.remove(os.path.join(OUT, name + '.svg'))
        print('removed unused', name)
    print(f'{len(wanted)} icons in {os.path.relpath(OUT, ROOT)} (lucide-static {version})')
    return 0


if __name__ == '__main__':
    sys.exit(main())
