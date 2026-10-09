# Derived from calibre's src/calibre/gui2/dnd.py (calibre 9.15).
# Copyright: 2011, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.gui2.dnd`: reading dropped files and images
from mime data."""

import os
import posixpath
from urllib.parse import unquote, urlparse

from qt.core import QImage, QPixmap, QUrl

IMAGE_EXTENSIONS = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp']


def image_extensions():
    return IMAGE_EXTENSIONS


def path_from_qurl(qurl, allow_remote=False):
    raw = bytes(qurl.toEncoded(QUrl.ComponentFormattingOption.FullyEncoded)).decode('utf-8', 'replace')
    ans = qurl.toLocalFile()
    if not ans and allow_remote:
        return raw
    return ans


def remote_urls_from_qurl(qurls, allow_remote=True):
    for qurl in qurls:
        if qurl.scheme() in {'http', 'https', 'ftp'} and qurl.path():
            yield bytes(qurl.toEncoded()).decode('utf-8'), posixpath.basename(qurl.path())


def dnd_has_image(md):
    return md.hasImage() or any(
        urlparse(u.toString()).path.rpartition('.')[-1].lower() in IMAGE_EXTENSIONS for u in md.urls()
    )


def dnd_has_extension(md, extensions, allow_all_extensions=False, allow_remote=False):
    if md.hasUrls():
        for u in md.urls():
            path = path_from_qurl(u, allow_remote=allow_remote)
            if path:
                ext = posixpath.splitext(path)[1][1:].lower()
                if allow_all_extensions or ext in extensions:
                    return True
    return False


def dnd_get_files(md, exts, allow_all_extensions=False, filter_exts=()):
    """Local paths (and remote URLs, as (url, name)) of dropped files with
    one of `exts`."""
    local_files, remote_files = [], []
    if md.hasUrls():
        for u in md.urls():
            path = u.toLocalFile()
            if path:
                ext = os.path.splitext(path)[1][1:].lower()
                if (allow_all_extensions or ext in exts) and ext not in filter_exts and os.path.exists(path):
                    local_files.append(path)
            elif u.scheme() in ('http', 'https', 'ftp'):
                name = posixpath.basename(unquote(u.path()))
                ext = posixpath.splitext(name)[1][1:].lower()
                if (allow_all_extensions or ext in exts) and ext not in filter_exts:
                    remote_files.append((bytes(u.toEncoded()).decode('utf-8'), name))
    return local_files, remote_files


def dnd_get_local_image_and_pixmap(md, image_exts=None):
    if md.hasImage():
        for x in md.formats():
            x = str(x)
            if x.startswith('image/'):
                cdata = bytes(md.data(x))
                pmap = QPixmap()
                pmap.loadFromData(cdata)
                if not pmap.isNull():
                    return pmap, cdata
    if md.hasFormat('application/octet-stream'):
        cdata = bytes(md.data('application/octet-stream'))
        pmap = QPixmap()
        pmap.loadFromData(cdata)
        if not pmap.isNull():
            return pmap, cdata
    if image_exts is None:
        image_exts = image_extensions()
    for u in md.urls():
        path = u.toLocalFile()
        if path and os.path.splitext(path)[1][1:].lower() in image_exts and os.path.exists(path):
            pmap = QPixmap()
            with open(path, 'rb') as f:
                cdata = f.read()
            pmap.loadFromData(cdata)
            if not pmap.isNull():
                return pmap, cdata
    return None, None


def dnd_get_image(md, image_exts=None):
    """(QPixmap, data) of a dropped image, or (None, None); a remote
    image URL gives (url, name)."""
    pmap, data = dnd_get_local_image_and_pixmap(md, image_exts)
    if pmap is not None:
        return pmap, data
    for url, name in remote_urls_from_qurl(md.urls()):
        if posixpath.splitext(name)[1][1:].lower() in (image_exts or image_extensions()):
            return url, name
    return None, None


class DownloadDialog:
    """calibre downloads dropped remote files through a dialog; here a
    plain download."""

    def __init__(self, url, fname, parent):
        self.url, self.fname = url, fname
        self.err = None
        self.tdir = None

    def start_download(self):
        from calibre import browser
        from calibre.ptempfile import PersistentTemporaryDirectory

        try:
            self.tdir = PersistentTemporaryDirectory('_dnd')
            with browser().open(self.url) as r, open(os.path.join(self.tdir, self.fname), 'wb') as f:
                f.write(r.read())
        except Exception as e:
            self.err = str(e)

    @property
    def path(self):
        return os.path.join(self.tdir, self.fname) if self.tdir and not self.err else None


__all__ = [
    'IMAGE_EXTENSIONS',
    'image_extensions',
    'dnd_has_image',
    'dnd_has_extension',
    'dnd_get_files',
    'dnd_get_image',
    'dnd_get_local_image_and_pixmap',
    'path_from_qurl',
    'remote_urls_from_qurl',
    'DownloadDialog',
    'QImage',
]
