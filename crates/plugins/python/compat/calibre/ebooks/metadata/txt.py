"""Mirrors calibre's `calibre.ebooks.metadata.txt`: metadata from a text
file's first lines (a Markdown `# Title`, or `Title:` / `Author:` headers)."""

import re

from calibre.ebooks.metadata import MetaInformation, string_to_authors


def get_metadata(stream, extract_cover=True):
    stream.seek(0)
    raw = stream.read(4096)
    if isinstance(raw, bytes):
        from calibre.ebooks.chardet import xml_to_unicode

        raw = xml_to_unicode(raw)[0]
    mi = MetaInformation(None, None)
    lines = [l.strip() for l in raw.splitlines()[:40]]
    for line in lines:
        if not line:
            continue
        m = re.match(r'^#\s+(.+)$', line)
        if m and mi.is_null('title'):
            mi.title = m.group(1).strip()
            continue
        m = re.match(r'^(title|author|authors|publisher|tags|series)\s*:\s*(.+)$', line, re.IGNORECASE)
        if m:
            key, val = m.group(1).lower(), m.group(2).strip()
            if key == 'title' and mi.is_null('title'):
                mi.title = val
            elif key in ('author', 'authors'):
                mi.authors = string_to_authors(val) or mi.authors
            elif key == 'publisher':
                mi.publisher = val
            elif key == 'tags':
                mi.tags = [t.strip() for t in val.split(',') if t.strip()]
            elif key == 'series':
                mi.series = val
    return mi


def set_metadata(stream, mi):
    """Text files hold no metadata fields; nothing is written."""
    return
