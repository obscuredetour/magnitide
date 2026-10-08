"""Mirrors the small helpers of calibre's `calibre.ebooks.oeb.polish.utils`."""

import re
from collections import namedtuple

OPF_VERSION = namedtuple('OPFVersion', 'major minor patch')


def parse_opf_version(raw):
    parts = [int(x) for x in re.findall(r'\d+', raw or '2.0')][:3]
    while len(parts) < 3:
        parts.append(0)
    return OPF_VERSION(*parts)


def guess_type(x):
    import mimetypes

    from calibre.ebooks.oeb.base import OPENTYPE_MIME, XHTML_MIME

    ext = x.rpartition('.')[-1].lower()
    extra = {
        'xhtml': XHTML_MIME,
        'html': XHTML_MIME,
        'htm': XHTML_MIME,
        'ncx': 'application/x-dtbncx+xml',
        'opf': 'application/oebps-package+xml',
        'otf': OPENTYPE_MIME,
        'ttf': 'application/x-font-ttf',
        'woff': 'font/woff',
        'woff2': 'font/woff2',
        'svg': 'image/svg+xml',
        'webp': 'image/webp',
    }
    if ext in extra:
        return extra[ext]
    return mimetypes.guess_type(x)[0] or 'application/octet-stream'


class PositionFinder:
    def __init__(self, raw):
        self.newlines = [m.start() for m in re.finditer(r'\n', raw)]

    def __call__(self, pos):
        import bisect

        line = bisect.bisect_right(self.newlines, pos)
        col = pos - (self.newlines[line - 1] if line else -1)
        return line + 1, col


def apply_func_to_match_groups(match, func=str.upper, handle_entities=lambda x: x):
    return func(match.group())


def lead_text(top_elem, num_words=10):
    text = ''.join(top_elem.itertext())
    return ' '.join(text.split()[:num_words])


__all__ = ['OPF_VERSION', 'parse_opf_version', 'guess_type', 'PositionFinder', 'lead_text']
