"""Mirrors calibre's `EbookIterator` for EPUB (and KEPUB) files: the book is
unpacked into a temporary directory, `spine` lists its content files in
reading order (each a `SpineItem` str with a character count), `opf` and
`mi` carry its metadata. Other formats need calibre's conversion input
plugins and aren't supported."""

import os
import re
import shutil
import zipfile
from urllib.parse import unquote

from calibre.ebooks.metadata.opf2 import OPF
from calibre.ptempfile import PersistentTemporaryDirectory

TAG_RE = re.compile(r'<[^>]+>')


class SpineItem(str):
    def __new__(
        cls,
        path,
        mime_type=None,
        read_anchor_map=True,
        run_char_count=True,
        from_epub=False,
        read_links=True,
        idref=None,
    ):
        self = str.__new__(cls, path)
        self.mime_type = mime_type or 'application/xhtml+xml'
        self.idref = idref
        self.character_count = 0
        self.anchor_map = {}
        self.all_links = set()
        self.start_page = -1
        self.pages = -1
        self.max_page = -1
        self.is_single_page = None
        self.page_names = None
        self.index_in_book = None
        if run_char_count:
            try:
                with open(path, 'rb') as f:
                    raw = f.read().decode('utf-8', 'replace')
                body = re.split(r'(?i)<body[^>]*>', raw, maxsplit=1)
                text = TAG_RE.sub('', body[1] if len(body) > 1 else raw)
                self.character_count = len(re.sub(r'\s+', ' ', text))
            except Exception:
                self.character_count = 0
        return self


class EbookIterator:
    CHARACTERS_PER_PAGE = 1000

    def __init__(self, pathtoebook, log=None, copy_bookmarks_to_file=True, use_tdir_in_cache=False):
        self.pathtoebook = os.path.abspath(pathtoebook)
        self.log = log
        self.copy_bookmarks_to_file = copy_bookmarks_to_file
        self.use_tdir_in_cache = use_tdir_in_cache
        self.book_format = os.path.splitext(pathtoebook)[1][1:].upper()
        self._tdir = None
        self.spine = []
        self.opf = None
        self.mi = None
        self.toc = None
        self.pathtoopf = None
        self.base = None
        self.bookmarks = []
        self.config = {}

    def __enter__(
        self,
        processed=False,
        only_input_plugin=False,
        run_char_count=True,
        read_anchor_map=True,
        view_kepub=False,
        read_links=True,
    ):
        if not zipfile.is_zipfile(self.pathtoebook):
            raise ValueError(
                f"{self.book_format} books need calibre's conversion input plugins, which Magnitide doesn't have"
            )
        self._tdir = PersistentTemporaryDirectory('_ebook_iter')
        self.base = os.path.realpath(self._tdir)
        with zipfile.ZipFile(self.pathtoebook) as zf:
            for info in zf.infolist():
                name = info.filename
                if name.startswith('/') or '..' in name.split('/'):
                    continue
                zf.extract(info, self.base)
        container = os.path.join(self.base, 'META-INF', 'container.xml')
        opf_path = None
        if os.path.exists(container):
            from lxml import etree

            root = etree.parse(container).getroot()
            for rf in root.iter('{urn:oasis:names:tc:opendocument:xmlns:container}rootfile'):
                if rf.get('full-path'):
                    opf_path = os.path.join(self.base, *rf.get('full-path').split('/'))
                    break
        if opf_path is None:
            for dirpath, _, files in os.walk(self.base):
                for f in files:
                    if f.lower().endswith('.opf'):
                        opf_path = os.path.join(dirpath, f)
                        break
                if opf_path:
                    break
        if opf_path is None or not os.path.exists(opf_path):
            raise ValueError('No OPF file found in the book')
        self.pathtoopf = opf_path
        with open(opf_path, 'rb') as f:
            self.opf = OPF(f, os.path.dirname(opf_path))
        self.mi = self.opf.to_book_metadata()
        self.language = self.opf.language
        self.spine = []
        for i, item in enumerate(self.opf.spine):
            path = item.path if hasattr(item, 'path') else getattr(item, 'href', None)
            if path is None:
                continue
            path = unquote(path) if not os.path.exists(path) else path
            if not os.path.exists(path):
                continue
            si = SpineItem(
                path,
                mime_type=getattr(item, 'media_type', None),
                read_anchor_map=read_anchor_map,
                run_char_count=run_char_count,
                from_epub=True,
                read_links=read_links,
                idref=getattr(item, 'id', None),
            )
            si.index_in_book = i
            self.spine.append(si)
        # Page numbers at calibre's rate, for position bookmarks.
        page = 1
        for si in self.spine:
            si.start_page = page
            si.pages = max(1, (si.character_count + self.CHARACTERS_PER_PAGE - 1) // self.CHARACTERS_PER_PAGE)
            si.max_page = page + si.pages - 1
            page = si.max_page + 1
        try:
            self.toc = self.opf.toc
        except Exception:
            self.toc = None
        return self

    def __exit__(self, *args):
        if self._tdir and os.path.exists(self._tdir):
            shutil.rmtree(self._tdir, ignore_errors=True)
        self._tdir = None

    def search(self, text, index, backwards=False):
        from calibre.ebooks.oeb.polish.container import get_container  # noqa: F401

        pages = self.spine[index:] if not backwards else self.spine[: index + 1][::-1]
        for i, path in enumerate(pages):
            with open(path, 'rb') as f:
                raw = f.read().decode('utf-8', 'replace')
            if text.lower() in TAG_RE.sub('', raw).lower():
                return self.spine.index(path)
        return None
