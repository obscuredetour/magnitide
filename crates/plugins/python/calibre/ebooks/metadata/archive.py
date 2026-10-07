"""Mirrors calibre's `calibre.ebooks.metadata.archive`: comic archives
(CBZ) and zips holding a single book."""

import io
import os
import posixpath
import re
import zipfile

from calibre.ebooks.metadata import MetaInformation
from calibre.ebooks.metadata.meta import metadata_from_filename
from calibre.utils.imghdr import what

ARCHIVE_IMAGE_EXTS = ('.png', '.jpg', '.jpeg', '.gif', '.webp')


def archive_type(stream):
    stream.seek(0)
    head = stream.read(8)
    stream.seek(0)
    if head.startswith(b'PK'):
        return 'zip'
    if head.startswith(b'Rar'):
        return 'rar'
    if head.startswith(b"7z\xbc\xaf'\x1c"):
        return '7z'
    return None


def is_comic(list_of_names):
    extensions = {posixpath.splitext(x)[1].lower() for x in list_of_names if '__MACOSX' not in x}
    extensions = {x for x in extensions if x and x != '.xml'}
    return len(extensions) > 0 and extensions.issubset(set(ARCHIVE_IMAGE_EXTS))


def _natural_key(name):
    return [int(t) if t.isdigit() else t.lower() for t in re.split(r'(\d+)', name)]


def get_comic_images(stream):
    with zipfile.ZipFile(stream) as zf:
        names = [
            n for n in zf.namelist() if posixpath.splitext(n)[1].lower() in ARCHIVE_IMAGE_EXTS and '__MACOSX' not in n
        ]
    return sorted(names, key=_natural_key)


def _comic_info(mi, raw):
    """ComicInfo.xml (the ComicRack schema) into `mi`."""
    from lxml import etree

    root = etree.fromstring(raw, parser=etree.XMLParser(recover=True, resolve_entities=False))
    if root is None:
        return

    def t(name):
        el = root.find(name)
        return (el.text or '').strip() if el is not None else ''

    if t('Title'):
        mi.title = t('Title')
    series = t('Series')
    if series:
        mi.series = series
        num = t('Number')
        try:
            mi.series_index = float(num) if num else 1.0
        except ValueError:
            pass
        if mi.is_null('title'):
            mi.title = f'{series} #{num}' if num else series
    writers = [w.strip() for w in re.split(r'[,;]', t('Writer')) if w.strip()]
    if writers:
        mi.authors = writers
    if t('Publisher'):
        mi.publisher = t('Publisher')
    if t('Summary'):
        mi.comments = t('Summary')
    genres = [g.strip() for g in t('Genre').split(',') if g.strip()]
    if genres:
        mi.tags = genres
    year, month, day = t('Year'), t('Month'), t('Day')
    if year:
        from calibre.utils.date import parse_only_date

        try:
            mi.pubdate = parse_only_date(f'{year}-{month or 1}-{day or 1}', assume_utc=True)
        except Exception:
            pass
    lang = t('LanguageISO')
    if lang:
        from calibre.utils.localization import canonicalize_lang

        code = canonicalize_lang(lang)
        if code:
            mi.languages = [code]


def get_comic_metadata(stream, stream_type, series_index='volume'):
    """Title/series from the file name or ComicInfo.xml, and the first page
    as the cover."""
    name = os.path.basename(getattr(stream, 'name', 'comic.cbz'))
    mi = metadata_from_filename(name)
    if stream_type != 'cbz':
        return mi
    stream.seek(0)
    with zipfile.ZipFile(stream) as zf:
        names = zf.namelist()
        info = next((n for n in names if posixpath.basename(n).lower() == 'comicinfo.xml'), None)
        if info:
            try:
                _comic_info(mi, zf.read(info))
            except Exception:
                pass
        images = get_comic_images(stream)
        if images:
            data = zf.read(images[0])
            fmt = what(None, data)
            if fmt:
                mi.cover_data = (fmt, data)
    return mi


def get_metadata(stream):
    """For a zip holding one book file: that book's metadata."""
    from calibre.ebooks import BOOK_EXTENSIONS
    from calibre.ebooks.metadata.meta import get_metadata as gm

    stream.seek(0)
    with zipfile.ZipFile(stream) as zf:
        names = [n for n in zf.namelist() if not n.endswith('/')]
        if is_comic(names):
            return get_comic_metadata(stream, 'cbz')
        books = [n for n in names if posixpath.splitext(n)[1][1:].lower() in BOOK_EXTENSIONS]
        if len(books) == 1:
            ext = posixpath.splitext(books[0])[1][1:].lower()
            inner = io.BytesIO(zf.read(books[0]))
            inner.name = posixpath.basename(books[0])
            return gm(inner, ext)
    return MetaInformation(os.path.splitext(os.path.basename(getattr(stream, 'name', 'book.zip')))[0], ['Unknown'])
