#!/usr/bin/env python3
"""Generate crates/plugins/python/resources/localization/iso639.json from the
iso-codes project's data (LGPL 2.1; https://salsa.debian.org/iso-codes-team/iso-codes).

Usage: gen-iso-data.py [path-to-iso-codes-checkout]   (downloads otherwise)

The file holds what calibre's language functions need: three-letter
(terminology) codes to English names, two-letter codes to three-letter ones,
and bibliographic codes to terminology ones.
"""

import json
import os
import sys
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, '..', '..', 'crates', 'plugins', 'python', 'resources', 'localization', 'iso639.json')
BASE = 'https://salsa.debian.org/iso-codes-team/iso-codes/-/raw/main/data/'


def load(name, src):
    if src:
        with open(os.path.join(src, 'data', name), 'rb') as f:
            return json.load(f)
    with urllib.request.urlopen(BASE + name) as r:
        return json.load(r)


def main():
    src = sys.argv[1] if len(sys.argv) > 1 else None
    iso3 = load('iso_639-3.json', src)['639-3']
    iso2 = load('iso_639-2.json', src)['639-2']
    by_3, two_to_3, bib_to_term = {}, {}, {}
    for e in iso3:
        code = e['alpha_3']
        by_3[code] = e['name']
        if e.get('alpha_2'):
            two_to_3[e['alpha_2']] = code
    for e in iso2:
        term = e['alpha_3']
        if term not in by_3:
            by_3[term] = e['name']
        if e.get('alpha_2'):
            two_to_3.setdefault(e['alpha_2'], term)
        if e.get('bibliographic'):
            bib_to_term[e['bibliographic']] = term
    by_3.setdefault('und', 'Undetermined')
    data = {'by_3': dict(sorted(by_3.items())), '2_to_3': dict(sorted(two_to_3.items())),
            'bib_to_term': dict(sorted(bib_to_term.items())),
            'source': 'iso-codes (https://salsa.debian.org/iso-codes-team/iso-codes), LGPL-2.1'}
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, 'w', encoding='utf-8') as f:
        json.dump(data, f, ensure_ascii=False, separators=(',', ':'))
        f.write('\n')
    print(f'{len(by_3)} languages, {len(two_to_3)} two-letter codes, {len(bib_to_term)} bibliographic codes → {os.path.normpath(OUT)}')


if __name__ == '__main__':
    main()
