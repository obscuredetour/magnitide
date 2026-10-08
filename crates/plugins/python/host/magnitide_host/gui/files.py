"""Book files: the shadow links to the library's files; a link becomes a
private copy before calibre hands out its path or writes to it, so a plugin
can't change the user's files. Only inside shadow libraries."""

import os
import shutil
import sys

shadow_roots = set()


def materialize(path):
    if not path or not os.path.islink(path):
        return
    absolute = os.path.abspath(path)
    if not any(absolute.startswith(root + os.sep) for root in shadow_roots):
        return
    target = os.path.realpath(path)
    tmp = path + '.magnitide-copy'
    _clone(target, tmp)
    os.replace(tmp, path)


def _clone(src, dst):
    """Copy-on-write where the disk can (APFS, Btrfs, XFS); else a plain copy."""
    if sys.platform == 'darwin':
        import ctypes
        import ctypes.util

        libc = ctypes.CDLL(ctypes.util.find_library('c'), use_errno=True)
        if libc.clonefile(os.fsencode(src), os.fsencode(dst), 0) == 0:
            return
    elif sys.platform.startswith('linux'):
        import fcntl

        try:
            with open(src, 'rb') as s, open(dst, 'wb') as d:
                fcntl.ioctl(d.fileno(), 0x40049409, s.fileno())  # FICLONE
            return
        except OSError:
            pass
    shutil.copy2(src, dst)


_installed = False


def install_file_guards():
    """Wrap the database's file operations so a linked file is materialized
    before calibre reads it for a plugin or writes to it."""
    global _installed
    if _installed:
        return
    _installed = True
    from calibre.db.backend import DB
    from calibre.db.cache import Cache

    def wrap(cls, name, before=None, after=None):
        original = getattr(cls, name)

        def wrapper(self, *args, **kwargs):
            if before:
                before(self, *args, **kwargs)
            result = original(self, *args, **kwargs)
            if after:
                after(result)
            return result

        wrapper.__name__ = name
        wrapper.__wrapped__ = original
        setattr(cls, name, wrapper)

    def book_files(backend, path, fmt=None):
        folder = os.path.join(backend.library_path, path)
        try:
            names = os.listdir(folder)
        except OSError:
            return []
        suffix = ('.' + fmt.lower()) if fmt else None
        return [os.path.join(folder, n) for n in names if suffix is None or n.lower().endswith(suffix)]

    # The path calibre hands a plugin, which may write to it.
    wrap(Cache, 'format_abspath', after=materialize)

    # calibre writing into files itself.
    def apply_to_format(self, book_id, path, fname, fmt, *a, **k):
        materialize(os.path.join(self.library_path, path, fname + '.' + fmt.lower()))

    def add_format(self, book_id, fmt, stream, title, author, path, *a, **k):
        for f in book_files(self, path, fmt):
            materialize(f)

    def set_cover(self, book_id, path, *a, **k):
        materialize(os.path.join(self.library_path, path, 'cover.jpg'))

    def copy_format_to(self, book_id, fmt, fname, path, *a, use_hardlink=False, **k):
        if use_hardlink:
            for f in book_files(self, path, fmt):
                materialize(f)

    def copy_cover_to(self, path, *a, use_hardlink=False, **k):
        if use_hardlink:
            materialize(os.path.join(self.library_path, path, 'cover.jpg'))

    wrap(DB, 'apply_to_format', before=apply_to_format)
    wrap(DB, 'add_format', before=add_format)
    wrap(DB, 'set_cover', before=set_cover)
    wrap(DB, 'copy_format_to', before=copy_format_to)
    wrap(DB, 'copy_cover_to', before=copy_cover_to)
