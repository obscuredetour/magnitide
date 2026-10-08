"""Mirrors calibre's `calibre.ebooks.metadata.html`: metadata from an HTML
file's `<title>` and `<meta>` tags (Dublin Core names included)."""

import re

from calibre.ebooks.chardet import xml_to_unicode
from calibre.ebooks.metadata import MetaInformation, string_to_authors
from calibre.utils.date import parse_date


def get_metadata(stream):
    stream.seek(0)
    raw = stream.read()
    if isinstance(raw, bytes):
        raw = xml_to_unicode(raw, strip_encoding_pats=True, resolve_entities=True)[0]
    return get_metadata_(raw)


def _metas(src):
    ans = {}
    for m in re.finditer(r'<meta\s+([^>]*?)/?>', src, re.IGNORECASE | re.DOTALL):
        attrs = dict((k.lower(), v) for k, v in re.findall(r'([\w.:-]+)\s*=\s*["\']([^"\']*)["\']', m.group(1)))
        name = (attrs.get('name') or attrs.get('property') or '').lower()
        content = attrs.get('content')
        if name and content is not None:
            ans.setdefault(name, []).append(content.strip())
    return ans


def get_metadata_(src, encoding=None):
    mi = MetaInformation(None, None)
    metas = _metas(src)

    def first(*names):
        for n in names:
            for v in metas.get(n, []):
                if v:
                    return v
        return None

    def all_(*names):
        out = []
        for n in names:
            out.extend(v for v in metas.get(n, []) if v)
        return out

    title = first('dc.title', 'dcterms.title', 'og:title', 'title')
    if not title:
        m = re.search(r'<title[^>]*>(.*?)</title>', src, re.IGNORECASE | re.DOTALL)
        if m:
            title = re.sub(r'\s+', ' ', m.group(1)).strip()
    if title:
        mi.title = title
    authors = all_('author', 'dc.creator', 'dcterms.creator', 'dc.creator.aut')
    if authors:
        names = []
        for a in authors:
            names.extend(string_to_authors(a))
        if names:
            mi.authors = names
    publisher = first('publisher', 'dc.publisher', 'dcterms.publisher')
    if publisher:
        mi.publisher = publisher
    isbn = first('isbn', 'dc.identifier.isbn')
    if isbn:
        mi.isbn = isbn
    date = first('dc.date', 'dcterms.date', 'dcterms.issued', 'date')
    if date:
        try:
            mi.pubdate = parse_date(date)
        except Exception:
            pass
    comments = first('description', 'dc.description', 'dcterms.description', 'og:description')
    if comments:
        mi.comments = comments
    tags = all_('keywords', 'dc.subject', 'dcterms.subject')
    if tags:
        out = []
        for t in tags:
            out.extend(x.strip() for x in t.split(',') if x.strip())
        if out:
            mi.tags = out
    lang = first('dc.language', 'dcterms.language', 'language')
    if lang:
        from calibre.utils.localization import canonicalize_lang

        code = canonicalize_lang(lang)
        if code:
            mi.languages = [code]
    series = first('calibre.series', 'dc.series')
    if series:
        mi.series = series
        si = first('calibre.series_index', 'dc.series_index')
        if si:
            try:
                mi.series_index = float(si)
            except ValueError:
                pass
    return mi
