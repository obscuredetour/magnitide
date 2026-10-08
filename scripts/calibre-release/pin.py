#!/usr/bin/env python3
"""Pin the calibre release Magnitide downloads as a plugin host.

    python3 scripts/calibre-release/pin.py 9.15.0        # write calibre.json
    python3 scripts/calibre-release/pin.py --rust-table   # print CALIBRE_RELEASES for calibre.rs

Sizes come from calibre's GitHub release, checksums from the SHA-512
files calibre publishes at https://calibre-ebook.com/signatures/, so
nothing large is downloaded here. Paste the table into
crates/plugins/src/calibre.rs and commit calibre.json with it.
"""

import json
import os
import sys
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
JSON = os.path.join(HERE, 'calibre.json')
TARGETS = {
    'aarch64-apple-darwin': 'calibre-{v}.dmg',
    'x86_64-apple-darwin': 'calibre-{v}.dmg',
    'x86_64-pc-windows-msvc': 'calibre-64bit-{v}.msi',
    'x86_64-unknown-linux-gnu': 'calibre-{v}-x86_64.txz',
    'aarch64-unknown-linux-gnu': 'calibre-{v}-arm64.txz',
}


def get(url):
    req = urllib.request.Request(url, headers={'User-Agent': 'magnitide-calibre-pin', 'Accept': 'application/vnd.github+json'})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def pin(version):
    release = json.loads(get(f'https://api.github.com/repos/kovidgoyal/calibre/releases/tags/v{version}'))
    sizes = {a['name']: a['size'] for a in release['assets']}
    files = {}
    for target, pattern in TARGETS.items():
        name = pattern.format(v=version)
        if name not in sizes:
            raise SystemExit(f'{name} is not in the GitHub release v{version}')
        sha512 = get(f'https://calibre-ebook.com/signatures/{name}.sha512').decode().split()[0]
        if len(sha512) != 128:
            raise SystemExit(f'no SHA-512 published for {name}')
        files[target] = {'file': name, 'size': sizes[name], 'sha512': sha512}
    with open(JSON, encoding='utf-8') as f:
        data = json.load(f)
    data['version'] = version
    data['files'] = files
    with open(JSON, 'w', encoding='utf-8') as f:
        json.dump(data, f, indent=2)
        f.write('\n')
    print(f'pinned calibre {version} ({len(files)} targets) in {os.path.relpath(JSON)}')


def rust_table():
    with open(JSON, encoding='utf-8') as f:
        data = json.load(f)
    print(f'pub const CALIBRE_VERSION: &str = "{data["version"]}";')
    print('pub const CALIBRE_RELEASES: &[CalibreRelease] = &[')
    for target, f in sorted(data['files'].items()):
        print('    CalibreRelease {')
        print(f'        target: "{target}",')
        print(f'        file: "{f["file"]}",')
        print(f'        sha512: "{f["sha512"]}",')
        print(f'        size: {f["size"]},')
        print('    },')
    print('];')


def main():
    args = sys.argv[1:]
    if args == ['--rust-table']:
        rust_table()
    elif len(args) == 1:
        pin(args[0])
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == '__main__':
    sys.exit(main())
