"""Mirrors calibre's `calibre.ebooks.metadata.epub` (calibre 9.15): reading
metadata (and the cover) from an EPUB, and writing metadata back into its
OPF, keeping every other file in the zip as it is."""

import io
import os
import posixpath
import re
import zipfile
from urllib.parse import unquote

from lxml import etree

from calibre.ebooks.metadata.opf2 import OPF
from calibre.utils.imghdr import what
from calibre.utils.zipfile import safe_replace

CONTAINER_NS = 'urn:oasis:names:tc:opendocument:xmlns:container'


class EPubException(Exception):
    pass


class OCFException(EPubException):
    pass


class ContainerException(OCFException):
    pass


def _opf_path(zf):
    try:
        container = zf.read('META-INF/container.xml')
    except KeyError:
        raise ContainerException('Not an EPUB: no META-INF/container.xml')
    root = etree.fromstring(container, parser=etree.XMLParser(recover=True, resolve_entities=False))
    for rf in root.iter('{%s}rootfile' % CONTAINER_NS):
        path = rf.get('full-path')
        if path:
            return unquote(path)
    for rf in root.iter():
        if isinstance(rf.tag, str) and rf.tag.endswith('rootfile') and rf.get('full-path'):
            return unquote(rf.get('full-path'))
    raise ContainerException('No rootfile in META-INF/container.xml')


def _resolve(opf_path, href):
    base = posixpath.dirname(opf_path)
    return posixpath.normpath(posixpath.join(base, unquote(href))) if base else posixpath.normpath(unquote(href))


def _cover_href(opf, opf_root):
    """The cover image's href inside the zip, from EPUB 3 properties, the
    `cover` meta, or a cover-named manifest item."""
    for item in opf_root.iterfind('.//{%s}item' % 'http://www.idpf.org/2007/opf'):
        props = (item.get('properties') or '').split()
        if 'cover-image' in props:
            return item.get('href')
    cid = None
    for meta in opf_root.iterfind('.//{%s}meta' % 'http://www.idpf.org/2007/opf'):
        if meta.get('name') == 'cover':
            cid = meta.get('content')
            break
    items = list(opf_root.iterfind('.//{%s}item' % 'http://www.idpf.org/2007/opf'))
    if cid:
        for item in items:
            if item.get('id') == cid:
                if (item.get('media-type') or '').startswith('image/'):
                    return item.get('href')
                # The meta points at an HTML cover page: look for its image.
                break
    for item in items:
        if item.get('id', '').lower() in ('cover', 'cover-image', 'coverimage') and (
            item.get('media-type') or ''
        ).startswith('image/'):
            return item.get('href')
    for item in items:
        href = item.get('href') or ''
        if (item.get('media-type') or '').startswith('image/') and 'cover' in posixpath.basename(href).lower():
            return href
    return None


def get_cover(zf, opf_path, opf_root):
    href = _cover_href(opf, opf_root) if False else _cover_href(None, opf_root)
    if not href:
        return None
    path = _resolve(opf_path, href)
    try:
        data = zf.read(path)
    except KeyError:
        return None
    fmt = what(None, data)
    if fmt is None:
        return None
    return fmt, data


def get_quick_metadata(stream):
    return get_metadata(stream, extract_cover=False)


def get_metadata(stream, extract_cover=True):
    """The metadata of the EPUB in `stream`, with its cover as `cover_data`."""
    stream.seek(0)
    with zipfile.ZipFile(stream) as zf:
        opf_path = _opf_path(zf)
        raw = zf.read(opf_path)
        opf = OPF(io.BytesIO(raw), populate_spine=False)
        mi = opf.to_book_metadata()
        mi.cover = None
        if extract_cover:
            try:
                cover = get_cover(zf, opf_path, opf.root)
            except Exception:
                cover = None
            if cover:
                mi.cover_data = cover
    return mi


def set_metadata(stream, mi, apply_null=False, update_timestamp=False, force_identifiers=False, add_missing_cover=True):
    """Write `mi` into the EPUB's OPF (and replace its cover image when
    `mi.cover_data` is set), leaving every other entry untouched."""
    stream.seek(0)
    with zipfile.ZipFile(stream) as zf:
        opf_path = _opf_path(zf)
        raw = zf.read(opf_path)
        opf = OPF(io.BytesIO(raw), populate_spine=False)
        cover_path = None
        href = _cover_href(None, opf.root)
        if href:
            cover_path = _resolve(opf_path, href)
    opf.smart_update(mi, apply_null=apply_null)
    if update_timestamp:
        from calibre.utils.date import utcnow

        opf.timestamp = utcnow()
    if force_identifiers and hasattr(mi, 'get_identifiers'):
        opf.set_identifiers(mi.get_identifiers())
    new_opf = opf.render()
    replacements = {}
    cover_data = getattr(mi, 'cover_data', (None, None))
    if cover_path and cover_data and cover_data[1]:
        replacements[cover_path] = io.BytesIO(cover_data[1])
    stream.seek(0)
    safe_replace(stream, opf_path, io.BytesIO(new_opf), extra_replacements=replacements)


__all__ = [
    'get_metadata',
    'get_quick_metadata',
    'set_metadata',
    'get_cover',
    'EPubException',
    'OCFException',
    'ContainerException',
    'os',
    're',
]


class Encryption:
    """What an EPUB's META-INF/encryption.xml says is encrypted: font
    obfuscation is fine; anything else is DRM (`is_encrypted`)."""

    OBFUSCATION_ALGORITHMS = {'http://ns.adobe.com/pdf/enc#RC', 'http://www.idpf.org/2008/embedding'}

    def __init__(self, raw):
        from lxml import etree

        self.entries = {}
        self.encrypted_files = set()
        if not raw:
            return
        try:
            root = etree.fromstring(raw)
        except Exception:
            return
        for em in root.iter('{http://www.w3.org/2001/04/xmlenc#}EncryptedData'):
            algorithm = None
            for m in em.iter('{http://www.w3.org/2001/04/xmlenc#}EncryptionMethod'):
                algorithm = m.get('Algorithm')
            for cr in em.iter('{http://www.w3.org/2001/04/xmlenc#}CipherReference'):
                uri = cr.get('URI')
                if uri:
                    self.entries[uri] = algorithm
                    if algorithm not in self.OBFUSCATION_ALGORITHMS:
                        self.encrypted_files.add(uri)

    def is_encrypted(self, uri):
        return uri in self.encrypted_files

    def __bool__(self):
        return bool(self.encrypted_files)


def encryption_of(zf):
    try:
        raw = zf.read('META-INF/encryption.xml')
    except KeyError:
        raw = b''
    return Encryption(raw)
