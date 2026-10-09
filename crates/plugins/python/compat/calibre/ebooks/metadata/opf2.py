# Derived from calibre's src/calibre/ebooks/metadata/opf2.py (calibre 9.15).
# Copyright: 2008, Kovid Goyal kovid@kovidgoyal.net. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.ebooks.metadata.opf2` (calibre 9.15): reading
and writing OPF 2 package files, with calibre's own `<meta>` extensions
(series, rating, timestamp, title sort, user metadata).

`OPF(stream)` parses a package; its attributes are the metadata; `smart_update`
writes a Metadata into it and `render()` serializes it. `metadata_to_opf(mi)`
makes a new package for a Metadata.
"""

import copy
import json
import os
import re
import uuid
from urllib.parse import unquote, urlparse

from lxml import etree

from calibre import guess_type, prepare_string_for_xml
from calibre.ebooks.metadata import MetaInformation, check_isbn, string_to_authors
from calibre.ebooks.metadata.book.base import Metadata
from calibre.utils.date import is_date_undefined, isoformat, parse_date
from calibre.utils.localization import canonicalize_lang

OPF_NS = 'http://www.idpf.org/2007/opf'
DC_NS = 'http://purl.org/dc/elements/1.1/'
OPF2_NSMAP = {
    'opf': OPF_NS,
    'dc': DC_NS,
    'dcterms': 'http://purl.org/dc/terms/',
    'xsi': 'http://www.w3.org/2001/XMLSchema-instance',
    'calibre': 'http://calibre.kovidgoyal.net/2009/metadata',
}
OPF1_NSMAP = {
    'dc': 'http://purl.org/dc/elements/1.0/',
    'oebpackage': 'http://openebook.org/namespaces/oeb-package/1.0/',
}

NON_EBOOK_EXTENSIONS = frozenset(
    [
        'jpg',
        'png',
        'gif',
        'jpeg',
        'bmp',
        'css',
        'js',
        'xml',
        'html',
        'htm',
        'xhtml',
        'xhtm',
        'svg',
        'otf',
        'ttf',
        'woff',
        'woff2',
        'ncx',
        'opf',
        'txt',
        'json',
        'mp3',
        'm4a',
        'mp4',
        'smil',
        'pls',
        'woff',
    ]
)


def dump_dict(cats):
    if not cats:
        cats = {}
    from calibre.utils.serialize import json_dumps

    return json_dumps(cats).decode('utf-8')


def dc(name):
    return '{%s}%s' % (DC_NS, name)


def opf(name):
    return '{%s}%s' % (OPF_NS, name)


def _text(el):
    return (el.text or '').strip() if el is not None else ''


class Resource:
    """A manifest item's reference: `href` relative to the OPF."""

    def __init__(self, href_or_path, basedir=os.getcwd(), is_path=True):
        self._href = None
        self._basedir = basedir
        self.path = None
        self.fragment = ''
        if is_path:
            self.path = os.path.abspath(os.path.join(basedir, href_or_path))
        else:
            url = urlparse(href_or_path)
            self.fragment = url.fragment
            self._href = unquote(url.path)
            if not urlparse(href_or_path).scheme:
                self.path = os.path.abspath(os.path.join(basedir, self._href.replace('/', os.sep)))

    def href(self, basedir=None):
        if basedir is None:
            basedir = self._basedir
        if self.path is None:
            return self._href
        rel = os.path.relpath(self.path, basedir).replace(os.sep, '/')
        if self.fragment:
            rel += '#' + self.fragment
        return rel

    def __repr__(self):
        return 'Resource(%s, %s)' % (repr(self.path), repr(self.href()))


class ManifestItem(Resource):
    def __init__(self, href, basedir=os.getcwd(), id=None, media_type=None):
        Resource.__init__(self, href, basedir, is_path=False)
        self.id = id or ''
        self.media_type = media_type or guess_type(href)[0] or 'application/octet-stream'

    @property
    def mime_type(self):
        return self.media_type

    def __str__(self):
        return f'<item id="{self.id}" href="{self.href()}" media-type="{self.media_type}" />'

    def __getitem__(self, index):
        if index == 0:
            return self.href()
        if index == 1:
            return self.media_type
        raise IndexError('%d out of bounds.' % index)


class Manifest(list):
    def item(self, id):
        for i in self:
            if i.id == id:
                return i

    def path_for_id(self, id):
        for i in self:
            if i.id == id:
                return i.path

    def type_for_id(self, id):
        for i in self:
            if i.id == id:
                return i.media_type

    def id_for_path(self, path):
        path = os.path.normpath(os.path.abspath(path))
        for i in self:
            if i.path and os.path.normpath(os.path.abspath(i.path)) == path:
                return i.id

    def path_for_href(self, href):
        for i in self:
            if i.href() == href:
                return i.path

    def remove(self, item):
        list.remove(self, item)

    def remove_duplicate_item(self, path):
        pass

    def next_id(self):
        n = len(self) + 1
        ids = {i.id for i in self}
        while 'id%d' % n in ids:
            n += 1
        return 'id%d' % n


class Spine(list):
    class Item(Resource):
        def __init__(self, idfunc, *args, **kwargs):
            Resource.__init__(self, *args, **kwargs)
            self.is_linear = True
            self.id = idfunc(self.path)
            self.idref = None

    def __init__(self, manifest):
        list.__init__(self)
        self.manifest = manifest

    def items(self):
        return [i.path for i in self]

    def linear_items(self):
        return [i.path for i in self if i.is_linear]


class Guide(list):
    class Reference(Resource):
        def __init__(self, href, basedir):
            Resource.__init__(self, href, basedir, is_path=False)
            self.type = ''
            self.title = ''

        def __repr__(self):
            return f'<reference type="{self.type}" href="{self.href()}" title="{self.title}" />'

    def set_cover(self, path):
        for ref in list(self):
            if ref.type.lower() == 'cover':
                self.remove(ref)
        r = Guide.Reference(path, os.path.dirname(path))
        r.type = 'cover'
        r.path = path
        self.append(r)


class OPF:
    """An OPF 2 package: metadata as attributes, plus `manifest`, `spine`,
    `guide`. Dates are time-zone-aware datetimes; `languages` are three-
    letter codes; identifiers are a dict."""

    def __init__(
        self,
        stream,
        basedir=os.getcwd(),
        unquote_urls=True,
        populate_spine=True,
        try_to_guess_cover=False,
        preparsed_opf=None,
        read_toc=True,
    ):
        self.basedir = self.base_dir = basedir
        self.path_to_html_toc = self.html_toc_fragment = None
        if preparsed_opf is not None:
            self.root = preparsed_opf
        else:
            raw = stream.read() if hasattr(stream, 'read') else stream
            if isinstance(raw, str):
                raw = raw.encode('utf-8')
            raw = raw.lstrip()
            parser = etree.XMLParser(recover=True, resolve_entities=False, no_network=True)
            self.root = etree.fromstring(raw, parser=parser)
            if self.root is None:
                raise ValueError('Not an OPF file')
        self.tree = self.root.getroottree() if hasattr(self.root, 'getroottree') else None
        self.package_version = (
            float(self.root.get('version', '2.0').split('.')[0])
            if self.root.get('version', '2.0')[:1].isdigit()
            else 2.0
        )
        self.metadata = self._find_or_create('metadata')
        self.manifest = Manifest()
        for item in self.root.iterfind('.//' + opf('item')):
            href = item.get('href')
            if not href:
                continue
            if unquote_urls:
                href = unquote(href)
            self.manifest.append(ManifestItem(href, basedir, id=item.get('id'), media_type=item.get('media-type')))
        self.spine = Spine(self.manifest)
        if populate_spine:
            for itemref in self.root.iterfind('.//' + opf('itemref')):
                path = self.manifest.path_for_id(itemref.get('idref'))
                if path:
                    s = Spine.Item(lambda p: itemref.get('idref'), path, basedir, is_path=True)
                    s.idref = itemref.get('idref')
                    s.is_linear = itemref.get('linear', 'yes') != 'no'
                    self.spine.append(s)
        self.guide = Guide()
        for ref in self.root.iterfind('.//' + opf('reference')):
            href = ref.get('href')
            if not href:
                continue
            r = Guide.Reference(unquote(href) if unquote_urls else href, basedir)
            r.type = ref.get('type', '')
            r.title = ref.get('title', '')
            self.guide.append(r)
        self.try_to_guess_cover = try_to_guess_cover

    # -- helpers --

    def _find_or_create(self, name):
        el = self.root.find(opf(name))
        if el is None:
            el = self.root.find(name)
        if el is None:
            el = etree.SubElement(self.root, opf(name), nsmap={'dc': DC_NS, 'opf': OPF_NS})
            el.tail = '\n'
        return el

    def _dc(self, name):
        return [
            e
            for e in self.metadata
            if isinstance(e.tag, str)
            and (e.tag == dc(name) or e.tag.endswith('}' + name) and 'dc' in e.tag or e.tag == name)
        ]

    def _meta(self, name):
        for e in self.metadata.findall(opf('meta')) + self.metadata.findall('meta'):
            if e.get('name') == name:
                return e
        return None

    def _meta_content(self, name):
        e = self._meta(name)
        return e.get('content') if e is not None else None

    def _set_meta(self, name, content):
        e = self._meta(name)
        if content is None or content == '':
            if e is not None:
                self.metadata.remove(e)
            return
        if e is None:
            e = etree.SubElement(self.metadata, opf('meta'))
            e.tail = '\n'
            e.set('name', name)
        e.set('content', str(content))

    def _set_dc(self, name, values, attrs=None):
        for e in self._dc(name):
            self.metadata.remove(e)
        if values is None:
            return
        if isinstance(values, str):
            values = [values]
        for i, v in enumerate(values):
            e = etree.SubElement(self.metadata, dc(name))
            e.tail = '\n'
            e.text = v
            if attrs:
                a = attrs[i] if isinstance(attrs, list) else attrs
                for k, val in (a or {}).items():
                    e.set(k, val)

    # -- metadata attributes --

    @property
    def title(self):
        for e in self._dc('title'):
            t = _text(e)
            if t:
                return t
        return None

    @title.setter
    def title(self, val):
        self._set_dc('title', val)

    @property
    def title_sort(self):
        return self._meta_content('calibre:title_sort')

    @title_sort.setter
    def title_sort(self, val):
        self._set_meta('calibre:title_sort', val)

    @property
    def authors(self):
        ans = []
        for e in self._dc('creator'):
            role = e.get(opf('role')) or e.get('role')
            if role and role != 'aut':
                continue
            t = _text(e)
            if t:
                ans.extend(string_to_authors(t))
        return ans

    @authors.setter
    def authors(self, val):
        for e in self._dc('creator'):
            role = e.get(opf('role')) or e.get('role')
            if not role or role == 'aut':
                self.metadata.remove(e)
        for a in val or []:
            e = etree.SubElement(self.metadata, dc('creator'))
            e.tail = '\n'
            e.text = a
            e.set(opf('role'), 'aut')

    @property
    def author_sort(self):
        for e in self._dc('creator'):
            role = e.get(opf('role')) or e.get('role')
            if role and role != 'aut':
                continue
            fa = e.get(opf('file-as')) or e.get('file-as')
            if fa:
                return fa
        return None

    @author_sort.setter
    def author_sort(self, val):
        for e in self._dc('creator'):
            role = e.get(opf('role')) or e.get('role')
            if not role or role == 'aut':
                if val:
                    e.set(opf('file-as'), val)
                else:
                    e.attrib.pop(opf('file-as'), None)
                break

    @property
    def book_producer(self):
        for e in self._dc('contributor'):
            if (e.get(opf('role')) or e.get('role')) == 'bkp':
                return _text(e) or None
        return None

    @book_producer.setter
    def book_producer(self, val):
        for e in self._dc('contributor'):
            if (e.get(opf('role')) or e.get('role')) == 'bkp':
                self.metadata.remove(e)
        if val:
            e = etree.SubElement(self.metadata, dc('contributor'))
            e.tail = '\n'
            e.text = val
            e.set(opf('role'), 'bkp')

    @property
    def publisher(self):
        for e in self._dc('publisher'):
            if _text(e):
                return _text(e)
        return None

    @publisher.setter
    def publisher(self, val):
        self._set_dc('publisher', val)

    @property
    def comments(self):
        for e in self._dc('description'):
            if _text(e):
                return _text(e)
        return None

    @comments.setter
    def comments(self, val):
        self._set_dc('description', val)

    @property
    def rights(self):
        for e in self._dc('rights'):
            if _text(e):
                return _text(e)
        return None

    @rights.setter
    def rights(self, val):
        self._set_dc('rights', val)

    @property
    def tags(self):
        return [_text(e) for e in self._dc('subject') if _text(e)]

    @tags.setter
    def tags(self, val):
        self._set_dc('subject', list(val or []))

    @property
    def languages(self):
        ans = []
        for e in self._dc('language'):
            t = _text(e)
            if t:
                code = canonicalize_lang(t) or t
                if code not in ans:
                    ans.append(code)
        return ans

    @languages.setter
    def languages(self, val):
        self._set_dc('language', list(val or []))

    @property
    def language(self):
        langs = self.languages
        return langs[0] if langs else None

    @language.setter
    def language(self, val):
        self.languages = [val] if val else []

    @property
    def raw_languages(self):
        return [_text(e) for e in self._dc('language') if _text(e)]

    def get_identifiers(self):
        ans = {}
        for e in self._dc('identifier'):
            scheme = (e.get(opf('scheme')) or e.get('scheme') or '').lower().strip()
            val = _text(e)
            if not val:
                continue
            if not scheme:
                if ':' in val and not val.lower().startswith(('urn:uuid', 'uuid:')):
                    scheme, _, val = val.partition(':')
                    scheme = scheme.lower().strip()
                    if scheme == 'urn':
                        scheme, _, val = val.partition(':')
                        scheme = scheme.lower().strip()
                else:
                    continue
            if scheme in ('uuid', 'calibre'):
                continue
            if scheme == 'isbn':
                val = check_isbn(val) or val.replace('urn:isbn:', '')
            ans[scheme] = val.strip()
        return ans

    def set_identifiers(self, identifiers):
        identifiers = {k.lower(): v for k, v in (identifiers or {}).items() if k and v}
        for e in list(self._dc('identifier')):
            scheme = (e.get(opf('scheme')) or e.get('scheme') or '').lower()
            if e.get('id') == self.root.get('unique-identifier') or scheme in ('uuid', 'calibre'):
                continue
            self.metadata.remove(e)
        for k, v in identifiers.items():
            e = etree.SubElement(self.metadata, dc('identifier'))
            e.tail = '\n'
            e.set(opf('scheme'), k.upper())
            e.text = v

    @property
    def identifiers(self):
        return self.get_identifiers()

    @identifiers.setter
    def identifiers(self, val):
        self.set_identifiers(val)

    @property
    def isbn(self):
        return self.get_identifiers().get('isbn')

    @isbn.setter
    def isbn(self, val):
        ids = self.get_identifiers()
        if val:
            ids['isbn'] = val
        else:
            ids.pop('isbn', None)
        self.set_identifiers(ids)

    @property
    def application_id(self):
        for e in self._dc('identifier'):
            if (e.get(opf('scheme')) or e.get('scheme') or '').lower() == 'calibre':
                return _text(e) or None
        return None

    @application_id.setter
    def application_id(self, val):
        for e in list(self._dc('identifier')):
            if (e.get(opf('scheme')) or e.get('scheme') or '').lower() == 'calibre':
                self.metadata.remove(e)
        if val is not None:
            e = etree.SubElement(self.metadata, dc('identifier'))
            e.tail = '\n'
            e.set(opf('scheme'), 'calibre')
            e.text = str(val)

    @property
    def uuid(self):
        for e in self._dc('identifier'):
            scheme = (e.get(opf('scheme')) or e.get('scheme') or '').lower()
            val = _text(e)
            if scheme == 'uuid':
                return val.replace('urn:uuid:', '')
            if val.lower().startswith('urn:uuid:'):
                return val[9:]
        return None

    @uuid.setter
    def uuid(self, val):
        for e in list(self._dc('identifier')):
            scheme = (e.get(opf('scheme')) or e.get('scheme') or '').lower()
            if scheme == 'uuid':
                self.metadata.remove(e)
        if val:
            e = etree.SubElement(self.metadata, dc('identifier'))
            e.tail = '\n'
            e.set(opf('scheme'), 'uuid')
            e.set('id', 'uuid_id')
            e.text = str(val)
            if not self.root.get('unique-identifier'):
                self.root.set('unique-identifier', 'uuid_id')

    @property
    def pubdate(self):
        for e in self._dc('date'):
            event = e.get(opf('event')) or e.get('event')
            if event and event != 'publication':
                continue
            t = _text(e)
            if t:
                try:
                    return parse_date(t)
                except Exception:
                    continue
        return None

    @pubdate.setter
    def pubdate(self, val):
        for e in list(self._dc('date')):
            event = e.get(opf('event')) or e.get('event')
            if not event or event == 'publication':
                self.metadata.remove(e)
        if val is not None and not is_date_undefined(val):
            e = etree.SubElement(self.metadata, dc('date'))
            e.tail = '\n'
            e.text = isoformat(val)

    @property
    def timestamp(self):
        t = self._meta_content('calibre:timestamp')
        if t:
            try:
                return parse_date(t)
            except Exception:
                pass
        for e in self._dc('date'):
            if (e.get(opf('event')) or e.get('event')) == 'modification' and _text(e):
                try:
                    return parse_date(_text(e))
                except Exception:
                    pass
        return None

    @timestamp.setter
    def timestamp(self, val):
        self._set_meta('calibre:timestamp', isoformat(val) if val is not None and not is_date_undefined(val) else None)

    @property
    def series(self):
        return self._meta_content('calibre:series')

    @series.setter
    def series(self, val):
        self._set_meta('calibre:series', val)

    @property
    def series_index(self):
        v = self._meta_content('calibre:series_index')
        try:
            return float(v) if v is not None else None
        except Exception:
            return None

    @series_index.setter
    def series_index(self, val):
        self._set_meta('calibre:series_index', None if val is None else ('%.2f' % float(val)))

    @property
    def rating(self):
        v = self._meta_content('calibre:rating')
        try:
            return float(v) if v is not None else None
        except Exception:
            return None

    @rating.setter
    def rating(self, val):
        self._set_meta('calibre:rating', None if val is None else ('%.2f' % float(val)))

    @property
    def publication_type(self):
        return self._meta_content('calibre:publication_type')

    @publication_type.setter
    def publication_type(self, val):
        self._set_meta('calibre:publication_type', val)

    @property
    def author_link_map(self):
        v = self._meta_content('calibre:author_link_map')
        if v:
            try:
                return json.loads(v)
            except Exception:
                pass
        return {}

    @author_link_map.setter
    def author_link_map(self, val):
        self._set_meta('calibre:author_link_map', dump_dict(val) if val else None)

    @property
    def user_categories(self):
        v = self._meta_content('calibre:user_categories')
        if v:
            try:
                return json.loads(v)
            except Exception:
                pass
        return {}

    @user_categories.setter
    def user_categories(self, val):
        self._set_meta('calibre:user_categories', dump_dict(val) if val else None)

    @property
    def cover(self):
        """The cover image's path (from the guide, or the `cover` meta)."""
        for r in self.guide:
            if r.type.lower() == 'cover' and r.path:
                return r.path
        cid = self._meta_content('cover')
        if cid:
            p = self.manifest.path_for_id(cid)
            if p:
                return p
        for item in self.manifest:
            if 'cover-image' in (item.media_type or '') or item.id in ('cover', 'cover-image'):
                if (item.media_type or '').startswith('image/'):
                    return item.path
        return None

    @cover.setter
    def cover(self, path):
        self.guide.set_cover(path)

    @property
    def raster_cover(self):
        cid = self._meta_content('cover')
        if cid:
            item = self.manifest.item(cid)
            if item is not None and (item.media_type or '').startswith('image/'):
                return item.href()
        for item in self.manifest:
            if item.id in ('cover', 'cover-image') and (item.media_type or '').startswith('image/'):
                return item.href()
        return None

    def get_user_metadata(self):
        """Custom columns stored as `calibre:user_metadata:#name` metas."""
        ans = {}
        for e in self.metadata.findall(opf('meta')) + self.metadata.findall('meta'):
            name = e.get('name') or ''
            if name.startswith('calibre:user_metadata:'):
                key = name[len('calibre:user_metadata:') :]
                try:
                    from calibre.utils.config_base import from_json

                    v = json.loads(e.get('content') or '{}', object_hook=from_json)
                    if isinstance(v, dict):
                        ans[key] = v
                except Exception:
                    pass
        return ans

    def set_user_metadata(self, um):
        for e in list(self.metadata.findall(opf('meta')) + self.metadata.findall('meta')):
            if (e.get('name') or '').startswith('calibre:user_metadata:'):
                self.metadata.remove(e)
        from calibre.utils.config_base import to_json

        for key, fm in (um or {}).items():
            e = etree.SubElement(self.metadata, opf('meta'))
            e.tail = '\n'
            e.set('name', 'calibre:user_metadata:' + key)
            e.set('content', json.dumps(fm, default=to_json))

    # -- Metadata in and out --

    def to_book_metadata(self):
        mi = Metadata(None)
        for attr in (
            'title',
            'authors',
            'author_sort',
            'title_sort',
            'publisher',
            'isbn',
            'languages',
            'pubdate',
            'timestamp',
            'comments',
            'tags',
            'series',
            'series_index',
            'rating',
            'uuid',
            'application_id',
            'publication_type',
            'book_producer',
            'rights',
            'author_link_map',
            'user_categories',
        ):
            val = getattr(self, attr, None)
            if val is not None and val != [] and val != {}:
                setattr(mi, attr, val)
        mi.set_identifiers(self.get_identifiers())
        um = self.get_user_metadata()
        if um:
            mi.set_all_user_metadata(um)
        if self.cover:
            mi.cover = self.cover
        return mi

    def smart_update(self, mi, replace_metadata=False, apply_null=False):
        """Write `mi` into the package: fields `mi` has set, or every field
        when `apply_null`/`replace_metadata`."""
        for attr in (
            'title',
            'authors',
            'author_sort',
            'title_sort',
            'publisher',
            'series',
            'series_index',
            'rating',
            'isbn',
            'tags',
            'comments',
            'languages',
            'pubdate',
            'timestamp',
            'publication_type',
            'book_producer',
            'rights',
            'uuid',
            'author_link_map',
            'user_categories',
        ):
            val = getattr(mi, attr, None)
            is_null = mi.is_null(attr) if hasattr(mi, 'is_null') else not val
            if is_null and not (apply_null or replace_metadata):
                continue
            if attr == 'series_index' and mi.is_null('series') and not apply_null:
                continue
            if attr in ('title', 'authors') and is_null:
                continue
            try:
                setattr(self, attr, None if is_null else val)
            except Exception:
                pass
        if hasattr(mi, 'get_identifiers'):
            ids = mi.get_identifiers()
            if ids or apply_null or replace_metadata:
                current = {} if (apply_null or replace_metadata) else self.get_identifiers()
                current.update(ids)
                self.set_identifiers(current)
        if hasattr(mi, 'get_all_user_metadata'):
            um = mi.get_all_user_metadata(make_copy=True)
            if um:
                self.set_user_metadata(um)
        if getattr(mi, 'application_id', None) is not None:
            self.application_id = mi.application_id

    def render(self, encoding='utf-8'):
        """The package as bytes."""
        for e in self.metadata:
            if e.tail is None:
                e.tail = '\n'
        return etree.tostring(self.root, encoding=encoding, xml_declaration=True, pretty_print=False)

    def unquote_urls(self):
        pass

    @property
    def toc(self):
        return None

    def get_text(self, elem):
        return _text(elem)

    def create_manifest_item(self, href, media_type, append=False):
        item = ManifestItem(href, self.basedir, id=self.manifest.next_id(), media_type=media_type)
        if append:
            self.manifest.append(item)
        return item

    def __iter__(self):
        return iter(self.manifest)


class OPFCreator(Metadata):
    """Builds a new OPF for a Metadata (`render(opf_stream, ncx_stream=None)`)."""

    def __init__(self, base_path, other):
        Metadata.__init__(self, title='', other=other)
        self.base_path = os.path.abspath(base_path)
        self.page_progression_direction = None
        self.primary_writing_mode = None
        if self.application_id is None:
            self.application_id = str(uuid.uuid4())
        if not isinstance(self.toc, Manifest):
            self.toc = None
        if not self.authors:
            self.authors = [_('Unknown')]
        if self.guide is None:
            self.guide = Guide()
        if self.cover:
            self.guide.set_cover(self.cover)
        self.manifest = self.manifest if isinstance(self.manifest, Manifest) else Manifest()
        self.spine = Spine(self.manifest)

    def create_manifest(self, entries):
        """`entries` is a list of (path, mime-type) tuples."""
        entries = [(os.path.join(self.base_path, p) if not os.path.isabs(p) else p, mt) for p, mt in entries]
        m = Manifest()
        for p, mt in entries:
            rel = os.path.relpath(p, self.base_path).replace(os.sep, '/')
            m.append(ManifestItem(rel, self.base_path, id=m.next_id(), media_type=mt))
        self.manifest = m

    def create_manifest_from_files_in(self, files_and_dirs, exclude=lambda x: False):
        entries = []

        def dodir(dir):
            for spec in os.walk(dir):
                root, files = spec[0], spec[-1]
                for name in files:
                    path = os.path.join(root, name)
                    if os.path.isfile(path) and not exclude(path):
                        entries.append((path, None))

        for i in files_and_dirs:
            if os.path.isdir(i):
                dodir(i)
            else:
                entries.append((i, None))
        self.create_manifest([(p, mt or guess_type(p)[0] or 'application/octet-stream') for p, mt in entries])

    def create_spine(self, entries):
        """`entries` is a list of paths."""
        self.spine = Spine(self.manifest)
        for e in entries:
            p = os.path.join(self.base_path, e) if not os.path.isabs(e) else e
            idref = self.manifest.id_for_path(p)
            if idref is None:
                self.manifest.append(
                    ManifestItem(
                        os.path.relpath(p, self.base_path).replace(os.sep, '/'),
                        self.base_path,
                        id=self.manifest.next_id(),
                    )
                )
                idref = self.manifest.id_for_path(p)
            item = Spine.Item(lambda path: idref, p, self.base_path, is_path=True)
            item.idref = idref
            self.spine.append(item)

    def set_toc(self, toc):
        self.toc = toc

    def create_guide(self, guide_element):
        self.guide = guide_element

    def render(self, opf_stream=None, ncx_stream=None, ncx_manifest_entry=None, encoding=None, process_guide=None):
        if encoding is None:
            encoding = 'utf-8'
        toc = getattr(self, 'toc', None)
        if self.manifest:
            self.manifest.set_basedir(self.base_path) if hasattr(self.manifest, 'set_basedir') else None
            if ncx_manifest_entry is not None and toc is not None:
                if not os.path.isabs(ncx_manifest_entry):
                    ncx_manifest_entry = os.path.join(self.base_path, ncx_manifest_entry)
                remove = [i for i in self.manifest if i.id == 'ncx']
                for item in remove:
                    self.manifest.remove(item)
                self.manifest.append(
                    ManifestItem(
                        os.path.relpath(ncx_manifest_entry, self.base_path).replace(os.sep, '/'),
                        self.base_path,
                        id='ncx',
                        media_type='application/x-dtbncx+xml',
                    )
                )
        root = etree.fromstring(metadata_to_opf(self, as_string=True))
        manifest = root.find(opf('manifest'))
        if manifest is None:
            manifest = etree.SubElement(root, opf('manifest'))
            manifest.tail = '\n'
        for item in self.manifest:
            e = etree.SubElement(manifest, opf('item'))
            e.tail = '\n'
            e.set('id', item.id)
            e.set('href', item.href(self.base_path))
            e.set('media-type', item.media_type)
        spine = root.find(opf('spine'))
        if spine is None:
            spine = etree.SubElement(root, opf('spine'))
            spine.tail = '\n'
        if toc is not None and any(i.id == 'ncx' for i in self.manifest):
            spine.set('toc', 'ncx')
        for item in self.spine:
            e = etree.SubElement(spine, opf('itemref'))
            e.tail = '\n'
            e.set('idref', item.idref)
            if not item.is_linear:
                e.set('linear', 'no')
        if self.guide:
            guide = root.find(opf('guide'))
            if guide is None:
                guide = etree.SubElement(root, opf('guide'))
                guide.tail = '\n'
            for ref in self.guide:
                e = etree.SubElement(guide, opf('reference'))
                e.tail = '\n'
                e.set('type', ref.type)
                e.set('title', ref.title or '')
                e.set('href', ref.href(self.base_path))
        raw = etree.tostring(root, encoding=encoding, xml_declaration=True, pretty_print=True)
        if opf_stream is not None:
            opf_stream.write(raw)
        if toc is not None and ncx_stream is not None:
            toc.render(ncx_stream, self.application_id)
        return raw


def metadata_to_opf(mi, as_string=True, default_lang=None):
    """A new OPF 2 package (bytes) holding `mi`'s metadata, as calibre writes
    into its `metadata.opf` files."""
    from calibre.utils.date import utcnow

    root = etree.Element(opf('package'), nsmap={'opf': OPF_NS, None: OPF_NS} if False else {None: OPF_NS})
    root.set('version', '2.0')
    root.set('unique-identifier', 'uuid_id')
    md = etree.SubElement(
        root,
        opf('metadata'),
        nsmap={
            'dc': DC_NS,
            'opf': OPF_NS,
            'calibre': OPF2_NSMAP['calibre'],
            'xsi': OPF2_NSMAP['xsi'],
            'dcterms': OPF2_NSMAP['dcterms'],
        },
    )
    md.text = '\n'
    md.tail = '\n'
    o = OPF(None, preparsed_opf=root, populate_spine=False)
    o.uuid = getattr(mi, 'uuid', None) or str(uuid.uuid4())
    if getattr(mi, 'application_id', None) is not None:
        o.application_id = mi.application_id
    o.title = mi.title or _('Unknown')
    o.authors = mi.authors or [_('Unknown')]
    if not mi.is_null('author_sort'):
        o.author_sort = mi.author_sort
    if not mi.is_null('title_sort'):
        o.title_sort = mi.title_sort
    o.book_producer = (
        getattr(mi, 'book_producer', None)
        or 'calibre (%s) [https://calibre-ebook.com]'
        % __import__('calibre.constants', fromlist=['__version__']).__version__
    )
    if mi.publisher:
        o.publisher = mi.publisher
    if mi.comments:
        o.comments = mi.comments
    if mi.tags:
        o.tags = mi.tags
    if mi.rights:
        o.rights = mi.rights
    langs = [x for x in (mi.languages or []) if x and x != 'und']
    o.languages = langs or ([default_lang] if default_lang else ['und'])
    if mi.pubdate is not None and not is_date_undefined(mi.pubdate):
        o.pubdate = mi.pubdate
    ts = getattr(mi, 'timestamp', None)
    if ts is None or is_date_undefined(ts):
        ts = utcnow()
    o.timestamp = ts
    if mi.series:
        o.series = mi.series
        o.series_index = mi.series_index if mi.series_index is not None else 1.0
    if mi.rating is not None and mi.rating > 0:
        o.rating = mi.rating
    if getattr(mi, 'publication_type', None):
        o.publication_type = mi.publication_type
    ids = mi.get_identifiers() if hasattr(mi, 'get_identifiers') else {}
    if ids:
        o.set_identifiers(ids)
    if getattr(mi, 'author_link_map', None):
        o.author_link_map = mi.author_link_map
    if getattr(mi, 'user_categories', None):
        o.user_categories = mi.user_categories
    um = mi.get_all_user_metadata(make_copy=True) if hasattr(mi, 'get_all_user_metadata') else {}
    if um:
        o.set_user_metadata(um)
    if getattr(mi, 'cover', None):
        guide = etree.SubElement(root, opf('guide'))
        guide.tail = '\n'
        ref = etree.SubElement(guide, opf('reference'))
        ref.set('type', 'cover')
        ref.set('title', 'Cover')
        ref.set('href', os.path.basename(mi.cover))
    raw = etree.tostring(root, encoding='utf-8', xml_declaration=True, pretty_print=True)
    return raw if as_string else root


def suite():
    return None


__all__ = [
    'OPF',
    'OPFCreator',
    'Manifest',
    'ManifestItem',
    'Spine',
    'Guide',
    'Resource',
    'metadata_to_opf',
    'OPF2_NSMAP',
    'OPF1_NSMAP',
    'DC_NS',
    'OPF_NS',
    'dump_dict',
    'copy',
    'MetaInformation',
    'prepare_string_for_xml',
    're',
]
