"""Mirrors calibre's `calibre.ebooks.metadata.fb2`: metadata and cover from
a FictionBook 2 file's `<description>`."""

import base64
import re

from lxml import etree

from calibre.ebooks.metadata import MetaInformation
from calibre.utils.date import parse_date
from calibre.utils.imghdr import what
from calibre.utils.localization import canonicalize_lang

NS = 'http://www.gribuser.ru/xml/fictionbook/2.0'
XLINK = 'http://www.w3.org/1999/xlink'


def _t(el):
    return (el.text or '').strip() if el is not None else ''


def _find(root, path):
    return root.find(
        path.replace('/', '/{%s}' % NS).replace('{%s}' % NS, '', 0)
        if False
        else '/'.join('{%s}%s' % (NS, p) for p in path.split('/'))
    )


def _findall(root, path):
    return root.findall('/'.join('{%s}%s' % (NS, p) for p in path.split('/')))


def get_metadata(stream):
    stream.seek(0)
    raw = stream.read()
    if raw.startswith(b'PK'):
        import io
        import zipfile

        with zipfile.ZipFile(io.BytesIO(raw)) as zf:
            names = [n for n in zf.namelist() if n.lower().endswith('.fb2')]
            raw = zf.read(names[0]) if names else b''
    root = etree.fromstring(raw, parser=etree.XMLParser(recover=True, resolve_entities=False, huge_tree=True))
    mi = MetaInformation(None, None)
    if root is None:
        return mi
    ti = _find(root, 'description/title-info')
    if ti is None:
        return mi
    title = _t(_find(ti, 'book-title'))
    if title:
        mi.title = title
    authors = []
    for a in _findall(ti, 'author'):
        parts = [_t(_find(a, 'first-name')), _t(_find(a, 'middle-name')), _t(_find(a, 'last-name'))]
        name = ' '.join(p for p in parts if p) or _t(_find(a, 'nickname'))
        if name:
            authors.append(name)
    if authors:
        mi.authors = authors
    tags = [_t(g) for g in _findall(ti, 'genre') if _t(g)]
    if tags:
        mi.tags = tags
    lang = _t(_find(ti, 'lang'))
    code = canonicalize_lang(lang) if lang else None
    if code:
        mi.languages = [code]
    ann = _find(ti, 'annotation')
    if ann is not None:
        text = etree.tostring(ann, method='text', encoding='unicode').strip()
        if text:
            mi.comments = '\n\n'.join(p.strip() for p in re.split(r'\n\s*\n', text) if p.strip())
    seq = _find(ti, 'sequence')
    if seq is not None and seq.get('name'):
        mi.series = seq.get('name')
        try:
            mi.series_index = float(seq.get('number') or 1)
        except ValueError:
            pass
    date = _find(ti, 'date')
    if date is not None:
        raw_date = date.get('value') or _t(date)
        if raw_date:
            try:
                mi.pubdate = parse_date(raw_date)
            except Exception:
                pass
    pi = _find(root, 'description/publish-info')
    if pi is not None:
        pub = _t(_find(pi, 'publisher'))
        if pub:
            mi.publisher = pub
        isbn = _t(_find(pi, 'isbn'))
        if isbn:
            mi.isbn = isbn
        year = _t(_find(pi, 'year'))
        if year and mi.is_null('pubdate'):
            try:
                mi.pubdate = parse_date(year)
            except Exception:
                pass
    cover = _find(ti, 'coverpage')
    if cover is not None:
        img = cover.find('{%s}image' % NS)
        href = img.get('{%s}href' % XLINK) if img is not None else None
        if href and href.startswith('#'):
            for binary in root.findall('{%s}binary' % NS):
                if binary.get('id') == href[1:]:
                    try:
                        data = base64.b64decode((binary.text or '').strip())
                        fmt = what(None, data)
                        if fmt:
                            mi.cover_data = (fmt, data)
                    except Exception:
                        pass
                    break
    return mi
