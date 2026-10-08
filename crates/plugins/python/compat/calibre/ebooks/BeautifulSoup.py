"""Mirrors calibre's `calibre.ebooks.BeautifulSoup`: Beautiful Soup 4 with
calibre's defaults (the lxml parser, and `BeautifulStoneSoup` for XML)."""

from bs4 import BeautifulSoup as _soup  # noqa: F401
from bs4 import (
    CData,
    Comment,
    Declaration,
    NavigableString,
    ProcessingInstruction,
    Tag,
    __version__,  # noqa: F401
)
from bs4.element import SoupStrainer  # noqa: F401


def parse_html(markup):
    if isinstance(markup, bytes):
        from calibre.ebooks.chardet import xml_to_unicode

        markup = xml_to_unicode(markup, strip_encoding_pats=True, resolve_entities=True)[0]
    return markup


def BeautifulSoup(markup='', *a, **kw):
    kw.setdefault('features', 'lxml')
    return _soup(parse_html(markup), *a, **kw)


def BeautifulStoneSoup(markup='', *a, **kw):
    kw.setdefault('features', 'xml')
    return _soup(parse_html(markup), *a, **kw)


def prettify(soup):
    return soup.prettify()


__all__ = [
    'BeautifulSoup',
    'BeautifulStoneSoup',
    'CData',
    'Comment',
    'Declaration',
    'NavigableString',
    'ProcessingInstruction',
    'Tag',
    'SoupStrainer',
    'parse_html',
    'prettify',
]
