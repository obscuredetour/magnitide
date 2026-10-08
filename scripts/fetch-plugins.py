#!/usr/bin/env python3
"""Download the real calibre plugins Magnitide is checked against
(tests/plugins/manifest.json) into target/plugin-corpus/, verifying each
against its sha256. Already-downloaded files that match are kept. Exits 0
with a note when offline, so a test run without network just skips the
corpus (crates/plugins/tests/corpus.rs needs MAGNITIDE_PLUGIN_CORPUS anyway).

    python3 scripts/fetch-plugins.py [--check]

`--check` only reports which files are present and valid.
"""

import hashlib
import json
import os
import sys
import urllib.error
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MANIFEST = os.path.join(ROOT, 'tests', 'plugins', 'manifest.json')
OUT = os.path.join(ROOT, 'target', 'plugin-corpus')


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def main():
    check = '--check' in sys.argv[1:]
    with open(MANIFEST, encoding='utf-8') as f:
        plugins = json.load(f)['plugins']
    os.makedirs(OUT, exist_ok=True)
    missing = 0
    for p in plugins:
        dest = os.path.join(OUT, p['file'])
        if os.path.exists(dest) and sha256(dest) == p['sha256']:
            print(f'ok       {p["file"]}')
            continue
        if check:
            print(f'missing  {p["file"]}')
            missing += 1
            continue
        tmp = dest + '.part'
        try:
            req = urllib.request.Request(p['url'], headers={'User-Agent': 'magnitide-fetch-plugins'})
            with urllib.request.urlopen(req, timeout=60) as r, open(tmp, 'wb') as f:
                while chunk := r.read(1 << 20):
                    f.write(chunk)
        except (urllib.error.URLError, OSError) as e:
            print(f'offline? {p["file"]}: {e}')
            if os.path.exists(tmp):
                os.remove(tmp)
            missing += 1
            continue
        got = sha256(tmp)
        if got != p['sha256']:
            os.remove(tmp)
            print(f'BAD HASH {p["file"]}: {got}', file=sys.stderr)
            return 1
        os.replace(tmp, dest)
        print(f'fetched  {p["file"]}')
    if missing:
        print(f'{missing} of {len(plugins)} not available; the corpus test skips them')
    return 0


if __name__ == '__main__':
    sys.exit(main())
