# Derived from calibre's src/calibre/ebooks/oeb/polish/container.py (calibre 9.15).
# Copyright: 2013, Kovid Goyal <kovid at kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.ebooks.oeb.polish.container` for EPUB: a
`Container` is the book unpacked in a temporary directory, with its OPF
manifest and spine, names (paths relative to the root) and hrefs, parsed
and raw access to every file, and `commit()` to write the EPUB back.
Not here: AZW3 containers, calibre's `tweak_mode`, and the pipeline-side
helpers (`clone_container` is a plain copy)."""

import os
import posixpath
import shutil
import uuid
import zipfile
from io import BytesIO
from urllib.parse import quote, unquote, urlparse

from lxml import etree

from calibre.ebooks.oeb.base import CSS_MIME, NCX_MIME, OEB_DOCS, OEB_STYLES, OPF, OPF2_NS, OPF_MIME, xml2str
from calibre.ebooks.oeb.parse_utils import NotHTML, parse_html
from calibre.ebooks.oeb.polish.utils import guess_type, parse_opf_version
from calibre.ptempfile import PersistentTemporaryDirectory

CONTAINER_NS = 'urn:oasis:names:tc:opendocument:xmlns:container'
OCF_NS = CONTAINER_NS
OPF_NAMESPACES = {'opf': OPF2_NS, 'dc': 'http://purl.org/dc/elements/1.1/'}


class InvalidBook(ValueError):
    pass


class DRMError(ValueError):
    pass


class InvalidEpub(InvalidBook):
    pass


class ObfuscationKeyMissing(InvalidEpub):
    pass


def abspath_to_name(path, root):
    return os.path.relpath(os.path.abspath(path), os.path.abspath(root)).replace(os.sep, '/')


def name_to_abspath(name, root):
    return os.path.abspath(os.path.join(root, *name.split('/')))


def href_to_name(href, root, base=None):
    base = root if base is None else os.path.dirname(name_to_abspath(base, root))
    try:
        purl = urlparse(href)
    except ValueError:
        return None
    if purl.scheme or not purl.path:
        return None
    href = unquote(purl.path)
    fullpath = os.path.join(base, *href.split('/'))
    return abspath_to_name(fullpath, root)


def name_to_href(name, root, base=None, quote_fn=quote):
    fullpath = name_to_abspath(name, root)
    basepath = root if base is None else os.path.dirname(name_to_abspath(base, root))
    path = os.path.relpath(fullpath, basepath).replace(os.sep, '/')
    return quote_fn(path)


class Container:
    book_type = 'oeb'
    SUPPORTS_TITLEPAGES = True
    SUPPORTS_FILENAMES = True

    def __init__(self, rootpath, opfpath, log=None, clone_data=None):
        self.root = os.path.abspath(os.path.realpath(rootpath))
        opfpath = os.path.realpath(opfpath)
        self.log = log
        self.parsed_cache = {}
        self.dirtied = set()
        self.name_path_map = {}
        self.mime_map = {}
        self.pretty_print = set()
        self.cloned = False
        self.tweak_mode = False
        for dirpath, _dirnames, filenames in os.walk(self.root):
            for f in filenames:
                path = os.path.join(dirpath, f)
                name = abspath_to_name(path, self.root)
                self.name_path_map[name] = path
                self.mime_map[name] = guess_type(path)
        self.opf_name = abspath_to_name(opfpath, self.root)
        self.opf_dir = posixpath.dirname(self.opf_name)
        self.mime_map[self.opf_name] = OPF_MIME
        for item in self.opf_xpath('//opf:manifest/opf:item[@href and @media-type]'):
            href = item.get('href')
            name = self.href_to_name(href, self.opf_name)
            if name in self.name_path_map:
                self.mime_map[name] = item.get('media-type')

    # -- names and hrefs --

    def abspath_to_name(self, fullpath, root=None):
        return abspath_to_name(fullpath, root or self.root)

    def name_to_abspath(self, name):
        return name_to_abspath(name, self.root)

    def exists(self, name):
        return os.path.exists(self.name_to_abspath(name))

    def href_to_name(self, href, base=None):
        return href_to_name(href, self.root, base=base)

    def name_to_href(self, name, base=None):
        return name_to_href(name, self.root, base=base)

    def relpath(self, path, base=None):
        base = base or self.root
        return os.path.relpath(path, base).replace(os.sep, '/')

    @property
    def names_that_need_not_be_manifested(self):
        return {self.opf_name, 'mimetype', 'META-INF/container.xml'}

    @property
    def names_that_must_not_be_removed(self):
        return {self.opf_name}

    @property
    def names_that_must_not_be_changed(self):
        return set()

    # -- the OPF --

    @property
    def opf(self):
        return self.parsed(self.opf_name)

    def opf_xpath(self, expr):
        return self.opf.xpath(expr, namespaces=OPF_NAMESPACES)

    @property
    def opf_version(self):
        return self.opf.get('version') or '2.0'

    @property
    def opf_version_parsed(self):
        return parse_opf_version(self.opf_version)

    @property
    def manifest_id_map(self):
        return {
            item.get('id'): self.href_to_name(item.get('href'), self.opf_name)
            for item in self.opf_xpath('//opf:manifest/opf:item[@href and @id]')
        }

    @property
    def manifest_type_map(self):
        ans = {}
        for item in self.opf_xpath('//opf:manifest/opf:item[@href and @media-type]'):
            ans.setdefault(item.get('media-type').lower(), []).append(
                self.href_to_name(item.get('href'), self.opf_name)
            )
        return ans

    def manifest_items_of_type(self, predicate):
        if isinstance(predicate, str):
            mt = predicate
            predicate = lambda x: x == mt  # noqa: E731
        elif isinstance(predicate, (set, frozenset, list, tuple)):
            mts = predicate
            predicate = lambda x: x in mts  # noqa: E731
        for name, mt in self.mime_map.items():
            if predicate(mt):
                yield name

    def manifest_items_with_property(self, property_name):
        for item in self.opf_xpath('//opf:manifest/opf:item[@href and @properties]'):
            if property_name in (item.get('properties') or '').split():
                yield self.href_to_name(item.get('href'), self.opf_name)

    @property
    def manifest_items(self):
        return self.opf_xpath('//opf:manifest/opf:item[@href]')

    @property
    def spine_items(self):
        for name, _linear in self.spine_names:
            yield self.name_path_map[name]

    @property
    def spine_names(self):
        for item, name, linear in self.spine_iter:
            yield name, linear

    @property
    def spine_iter(self):
        id_map = self.manifest_id_map
        for item in self.opf_xpath('//opf:spine/opf:itemref[@idref]'):
            name = id_map.get(item.get('idref'))
            if name is not None and name in self.name_path_map:
                yield item, name, (item.get('linear', 'yes') or 'yes').lower() == 'yes'

    def set_spine(self, spine_items):
        """`spine_items` is an iterable of (name, linear) tuples."""
        imap = {name: id_ for id_, name in self.manifest_id_map.items()}
        spine = self.opf_xpath('//opf:spine')[0]
        for item in list(spine):
            spine.remove(item)
        for name, linear in spine_items:
            item = etree.SubElement(spine, OPF('itemref'), idref=imap[name])
            if not linear:
                item.set('linear', 'no')
        self.dirty(self.opf_name)

    def insert_into_xml(self, parent, item, index=None):
        if index is None:
            parent.append(item)
        else:
            parent.insert(index, item)
        item.tail = parent.text if index == 0 else (parent[index - 1].tail if index else None)

    def generate_item(self, name, id_prefix=None, media_type=None, unique_href=True):
        """Add an item to the manifest with a unique id and href. Returns the name."""
        id_prefix = id_prefix or 'id'
        media_type = media_type or guess_type(name)
        if unique_href:
            base, ext = posixpath.splitext(name)
            n = 0
            while name in self.name_path_map:
                n += 1
                name = f'{base}_{n}{ext}'
        ids = {item.get('id') for item in self.opf_xpath('//*/@id')} | {
            x.get('id') for x in self.opf.iter('*') if x.get('id')
        }
        item_id = id_prefix
        n = 0
        while item_id in ids:
            n += 1
            item_id = f'{id_prefix}{n}'
        manifest = self.opf_xpath('//opf:manifest')[0]
        item = etree.SubElement(manifest, OPF('item'), id=item_id, href=self.name_to_href(name, self.opf_name))
        item.set('media-type', media_type)
        self.name_path_map[name] = self.name_to_abspath(name)
        self.mime_map[name] = media_type
        self.dirty(self.opf_name)
        return name

    def add_file(
        self, name, data, media_type=None, spine_index=None, modify_name_if_needed=False, process_manifest_item=None
    ):
        """Add a file with `data` (bytes); it joins the manifest (and the spine at `spine_index`)."""
        if name in self.name_path_map:
            if not modify_name_if_needed:
                raise ValueError(f'A file with the name {name} already exists')
            base, ext = posixpath.splitext(name)
            n = 0
            while name in self.name_path_map:
                n += 1
                name = f'{base}_{n}{ext}'
        path = self.name_to_abspath(name)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, 'wb') as f:
            f.write(data)
        media_type = media_type or guess_type(name)
        self.name_path_map[name] = path
        self.mime_map[name] = media_type
        if name in self.names_that_need_not_be_manifested:
            return name
        ids = {x.get('id') for x in self.opf.iter('*') if x.get('id')}
        item_id = posixpath.basename(name).replace('.', '_').replace(' ', '_') or 'id'
        base_id, n = item_id, 0
        while item_id in ids or not item_id[0].isalpha():
            n += 1
            item_id = f'id{n}' if not base_id[0].isalpha() else f'{base_id}_{n}'
        manifest = self.opf_xpath('//opf:manifest')[0]
        item = etree.SubElement(manifest, OPF('item'), id=item_id, href=self.name_to_href(name, self.opf_name))
        item.set('media-type', media_type)
        if process_manifest_item is not None:
            process_manifest_item(item)
        if spine_index is not None and media_type in OEB_DOCS:
            spine = self.opf_xpath('//opf:spine')[0]
            ref = etree.Element(OPF('itemref'), idref=item_id)
            self.insert_into_xml(spine, ref, index=spine_index)
        self.dirty(self.opf_name)
        return name

    def remove_item(self, name, remove_from_guide=True):
        """Remove a file from the book: its manifest item, spine and guide references, and the file."""
        href = None
        for item in self.opf_xpath('//opf:manifest/opf:item[@href]'):
            if self.href_to_name(item.get('href'), self.opf_name) == name:
                href = item.get('href')
                item_id = item.get('id')
                item.getparent().remove(item)
                for ref in self.opf_xpath(f'//opf:spine/opf:itemref[@idref="{item_id}"]'):
                    ref.getparent().remove(ref)
        if remove_from_guide and href is not None:
            for ref in self.opf_xpath('//opf:guide/opf:reference[@href]'):
                if self.href_to_name(ref.get('href'), self.opf_name) == name:
                    ref.getparent().remove(ref)
        path = self.name_path_map.pop(name, None)
        self.mime_map.pop(name, None)
        self.parsed_cache.pop(name, None)
        self.dirtied.discard(name)
        if path and os.path.exists(path):
            os.remove(path)
        self.dirty(self.opf_name)

    def rename(self, current_name, new_name):
        path = self.name_path_map.pop(current_name)
        new_path = self.name_to_abspath(new_name)
        os.makedirs(os.path.dirname(new_path), exist_ok=True)
        shutil.move(path, new_path)
        self.name_path_map[new_name] = new_path
        self.mime_map[new_name] = self.mime_map.pop(current_name)
        if current_name in self.parsed_cache:
            self.parsed_cache[new_name] = self.parsed_cache.pop(current_name)
        if current_name in self.dirtied:
            self.dirtied.discard(current_name)
            self.dirtied.add(new_name)
        for item in self.opf_xpath('//opf:manifest/opf:item[@href]'):
            if self.href_to_name(item.get('href'), self.opf_name) == current_name:
                item.set('href', self.name_to_href(new_name, self.opf_name))
        self.dirty(self.opf_name)

    # -- reading and writing --

    def open(self, name, mode='rb'):
        if name in self.dirtied:
            self.commit_item(name)
        path = self.name_to_abspath(name)
        if 'w' in mode or 'a' in mode:
            os.makedirs(os.path.dirname(path), exist_ok=True)
            self.name_path_map[name] = path
        return open(path, mode)

    def raw_data(self, name, decode=True, normalize_to_nfc=True):
        with self.open(name, 'rb') as f:
            ans = f.read()
        mime = self.mime_map.get(name, guess_type(name))
        if decode and (mime in OEB_STYLES or mime in OEB_DOCS or mime == 'text/plain' or mime.endswith('+xml')):
            try:
                return ans.decode('utf-8')
            except UnicodeDecodeError:
                return ans.decode('utf-8', 'replace')
        return ans

    def parse(self, path, mime):
        with open(path, 'rb') as f:
            raw = f.read()
        if mime in OEB_DOCS:
            try:
                return parse_html(raw, log=self.log)
            except NotHTML:
                return etree.fromstring(raw, parser=etree.XMLParser(recover=True, no_network=True))
        if mime in OEB_STYLES:
            return raw.decode('utf-8', 'replace')
        if mime == OPF_MIME or mime == NCX_MIME or mime.endswith('+xml') or mime.endswith('/xml'):
            return etree.fromstring(raw, parser=etree.XMLParser(recover=True, no_network=True, resolve_entities=False))
        return raw

    def parsed(self, name):
        """The file parsed: an lxml tree for XML, HTML and the OPF; text for CSS; bytes otherwise. Cached."""
        ans = self.parsed_cache.get(name)
        if ans is None:
            path = self.name_path_map[name]
            ans = self.parse(path, self.mime_map.get(name, guess_type(name)))
            self.parsed_cache[name] = ans
        return ans

    def replace(self, name, obj):
        """Replace the parsed object for `name` with `obj` (it is written on commit)."""
        self.parsed_cache[name] = obj
        self.dirty(name)

    def dirty(self, name):
        self.dirtied.add(name)

    def serialize_item(self, name):
        data = self.parsed(name)
        if isinstance(data, etree._Element):
            return xml2str(data, pretty_print=name in self.pretty_print)
        if isinstance(data, str):
            return data.encode('utf-8')
        return bytes(data)

    def commit_item(self, name, keep_parsed=False):
        if name not in self.parsed_cache:
            return
        data = self.serialize_item(name)
        self.dirtied.discard(name)
        if not keep_parsed:
            self.parsed_cache.pop(name)
        path = self.name_to_abspath(name)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, 'wb') as f:
            f.write(data)

    def flush_cache(self):
        for name in tuple(self.dirtied):
            self.commit_item(name)

    def commit(self, outpath=None, keep_parsed=False):
        for name in tuple(self.dirtied):
            self.commit_item(name, keep_parsed=keep_parsed)

    def compare_to(self, other):
        return None

    def filesize(self, name):
        return os.path.getsize(self.name_to_abspath(name))

    def __enter__(self):
        return self

    def __exit__(self, *a):
        pass

    def is_dir(self, name):
        return os.path.isdir(self.name_to_abspath(name))

    @property
    def guide_type_map(self):
        return {
            ref.get('type', '').lower(): self.href_to_name(ref.get('href'), self.opf_name)
            for ref in self.opf_xpath('//opf:guide/opf:reference[@href and @type]')
        }

    def book_metadata(self):
        from calibre.ebooks.metadata.opf2 import OPF as OPF2

        with self.open(self.opf_name) as f:
            return OPF2(f, os.path.dirname(self.name_path_map[self.opf_name]), populate_spine=False).to_book_metadata()

    @property
    def mi(self):
        return self.book_metadata()


class EpubContainer(Container):
    book_type = 'epub'
    META_INF = {
        'container.xml': True,
        'manifest.xml': False,
        'metadata.xml': False,
        'mimetype': False,
        'rights.xml': False,
        'signatures.xml': False,
        'encryption.xml': False,
    }

    def __init__(self, pathtoepub, log=None, clone_data=None, tdir=None):
        self.pathtoepub = os.path.abspath(pathtoepub)
        self.container_root = tdir or PersistentTemporaryDirectory('_epub_container')
        if not zipfile.is_zipfile(self.pathtoepub):
            raise InvalidEpub(f'{pathtoepub} is not a valid EPUB file (not a zip)')
        with zipfile.ZipFile(self.pathtoepub) as zf:
            for info in zf.infolist():
                name = info.filename
                if name.startswith('/') or '..' in name.split('/') or name.endswith('/'):
                    continue
                zf.extract(info, self.container_root)
        container_path = os.path.join(self.container_root, 'META-INF', 'container.xml')
        opf_path = None
        if os.path.exists(container_path):
            root = etree.parse(container_path).getroot()
            for rf in root.iter('{%s}rootfile' % CONTAINER_NS):
                if rf.get('full-path'):
                    opf_path = os.path.join(self.container_root, *unquote(rf.get('full-path')).split('/'))
                    break
        if opf_path is None or not os.path.exists(opf_path):
            for dirpath, _d, files in os.walk(self.container_root):
                for f in files:
                    if f.lower().endswith('.opf'):
                        opf_path = os.path.join(dirpath, f)
                        break
                if opf_path:
                    break
        if opf_path is None or not os.path.exists(opf_path):
            raise InvalidEpub('No OPF file found in the EPUB')
        self.encryption_meta = {}
        if os.path.exists(os.path.join(self.container_root, 'META-INF', 'encryption.xml')):
            raise DRMError('This EPUB is encrypted')
        Container.__init__(self, self.container_root, opf_path, log)
        self.name_path_map.pop('mimetype', None)
        self.mime_map.pop('mimetype', None)

    @property
    def names_that_need_not_be_manifested(self):
        return super().names_that_need_not_be_manifested | {'META-INF/' + x for x in self.META_INF}

    @property
    def names_that_must_not_be_removed(self):
        return super().names_that_must_not_be_removed | {'META-INF/container.xml'}

    def commit(self, outpath=None, keep_parsed=False):
        """Write the EPUB (to `outpath`, or over the original)."""
        super().commit(keep_parsed=keep_parsed)
        outpath = outpath or self.pathtoepub
        buf = BytesIO()
        with zipfile.ZipFile(buf, 'w') as zf:
            zf.writestr('mimetype', b'application/epub+zip', compress_type=zipfile.ZIP_STORED)
            for dirpath, _d, files in os.walk(self.root):
                for f in files:
                    path = os.path.join(dirpath, f)
                    name = abspath_to_name(path, self.root)
                    if name == 'mimetype':
                        continue
                    zf.write(path, name, compress_type=zipfile.ZIP_DEFLATED)
        with open(outpath, 'wb') as f:
            f.write(buf.getvalue())

    @property
    def path_to_ebook(self):
        return self.pathtoepub


def get_container(path, log=None, tdir=None, tweak_mode=False, ebook_cls=None):
    """The container for an EPUB (or KEPUB) file."""
    ext = path.rpartition('.')[-1].lower()
    if ext in ('epub', 'kepub') or zipfile.is_zipfile(path):
        return (ebook_cls or EpubContainer)(path, log=log, tdir=tdir)
    raise InvalidBook(f"{ext.upper()} books can't be edited in Magnitide (only EPUB)")


def clone_container(container, dest_dir, container_class=None):
    """A copy of the container's files in `dest_dir`, as its own container."""
    for name, path in container.name_path_map.items():
        dest = os.path.join(dest_dir, *name.split('/'))
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        shutil.copy2(path, dest)
    cls = container_class or Container
    if isinstance(container, EpubContainer):
        ans = EpubContainer.__new__(EpubContainer)
        ans.pathtoepub = container.pathtoepub
        ans.container_root = dest_dir
        Container.__init__(ans, dest_dir, os.path.join(dest_dir, *container.opf_name.split('/')), container.log)
        return ans
    return cls(dest_dir, os.path.join(dest_dir, *container.opf_name.split('/')), container.log)


def guess_mime(name):
    return guess_type(name)


__all__ = [
    'Container',
    'EpubContainer',
    'get_container',
    'clone_container',
    'InvalidBook',
    'InvalidEpub',
    'DRMError',
    'abspath_to_name',
    'name_to_abspath',
    'href_to_name',
    'name_to_href',
    'OEB_DOCS',
    'OEB_STYLES',
    'CSS_MIME',
    'OPF2_NS',
    'uuid',
]
