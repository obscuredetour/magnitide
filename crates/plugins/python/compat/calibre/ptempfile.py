# Derived from calibre's src/calibre/ptempfile.py (calibre 9.15).
# Copyright: 2008, Kovid Goyal <kovid at kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.ptempfile` (calibre 9.15): temporary files and
folders under one per-process base folder, removed when the process ends.
`PersistentTemporaryFile` and `PersistentTemporaryDirectory` outlive their
Python objects (plugins hand their paths around), until the base folder goes.
"""

import atexit
import os
import shutil
import tempfile

from calibre.constants import __appname__, __version__

_base_dir = None


def cleanup(path):
    try:
        if os.path.isdir(path):
            shutil.rmtree(path, ignore_errors=True)
        elif os.path.exists(path):
            os.remove(path)
    except OSError:
        pass


def app_prefix(prefix):
    return f'{__appname__}_{__version__}_{prefix}'


def remove_dir(x):
    shutil.rmtree(x, ignore_errors=True)


def reset_base_dir():
    global _base_dir
    _base_dir = None
    base_dir()


def base_dir():
    """The folder this process's temporary files live in. calibre honours
    CALIBRE_TEMP_DIR; Magnitide sets it for workers so they share a folder."""
    global _base_dir
    if _base_dir is not None and not os.path.exists(_base_dir):
        _base_dir = None
    if _base_dir is None:
        td = os.environ.get('CALIBRE_WORKER_TEMP_DIR')
        if td is not None:
            try:
                from polyglot.binary import from_hex_bytes

                td = from_hex_bytes(td).decode('utf-8')
            except Exception:
                td = None
        if td and os.path.exists(td):
            _base_dir = td
        else:
            parent = os.environ.get('CALIBRE_TEMP_DIR') or None
            if parent and not os.path.isdir(parent):
                parent = None
            _base_dir = tempfile.mkdtemp(prefix=f'{__appname__}_{__version__}_tmp_', dir=parent)
            atexit.register(remove_dir, _base_dir)
    return _base_dir


def force_unicode(x):
    if isinstance(x, bytes):
        x = x.decode('utf-8', 'replace')
    return x


def _make_file(suffix, prefix, base):
    suffix, prefix = force_unicode(suffix), force_unicode(prefix)
    return tempfile.mkstemp(suffix, prefix, dir=base)


def _make_dir(suffix, prefix, base):
    suffix, prefix = force_unicode(suffix), force_unicode(prefix)
    return tempfile.mkdtemp(suffix, prefix, base)


class PersistentTemporaryFile:
    """A file (under base_dir) that stays when closed; `.name` is its path.
    Removed when the process ends, or by `os.remove`."""

    def __init__(self, suffix='', prefix='', dir=None, mode='w+b'):
        if prefix is None:
            prefix = ''
        if dir is None:
            dir = base_dir()
        fd, name = _make_file(suffix, prefix, dir)
        self._file = os.fdopen(fd, mode)
        self._name = name
        self._fd = fd
        atexit.register(cleanup, name)

    @property
    def name(self):
        return self._name

    def __getattr__(self, name):
        return getattr(self._file, name)

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()
        return False

    def __del__(self):
        try:
            self.close()
        except Exception:
            pass


def PersistentTemporaryDirectory(suffix='', prefix='', dir=None):
    """A folder that stays until the process ends; returns its path."""
    if dir is None:
        dir = base_dir()
    tdir = _make_dir(suffix, prefix, dir)
    atexit.register(remove_dir, tdir)
    return tdir


class TemporaryDirectory:
    """`with TemporaryDirectory() as tdir:` — removed on exit unless `keep`."""

    def __init__(self, suffix='', prefix='', dir=None, keep=False):
        if prefix is None:
            prefix = ''
        if suffix is None:
            suffix = ''
        self.suffix = suffix
        self.prefix = prefix
        if dir is None:
            dir = base_dir()
        self.dir = dir
        self.keep = keep

    def __enter__(self):
        self.tdir = _make_dir(self.suffix, self.prefix, self.dir)
        return self.tdir

    def __exit__(self, *args):
        if not self.keep and os.path.exists(self.tdir):
            remove_dir(self.tdir)
        return False


class TemporaryFile:
    """`with TemporaryFile(suffix) as path:` — a path to a closed file, removed on exit."""

    def __init__(self, suffix='', prefix='', dir=None, mode='r+b'):
        if prefix is None:
            prefix = ''
        if suffix is None:
            suffix = ''
        if dir is None:
            dir = base_dir()
        self.prefix, self.suffix, self.dir, self.mode = prefix, suffix, dir, mode
        self._file = None

    def __enter__(self):
        fd, name = _make_file(self.suffix, self.prefix, self.dir)
        self._file = name
        os.close(fd)
        return name

    def __exit__(self, *args):
        cleanup(self._file)
        return False


class SpooledTemporaryFile(tempfile.SpooledTemporaryFile):
    def __init__(self, max_size=0, suffix='', prefix='', dir=None, mode='w+b', bufsize=-1):
        if prefix is None:
            prefix = ''
        if suffix is None:
            suffix = ''
        if dir is None:
            dir = base_dir()
        tempfile.SpooledTemporaryFile.__init__(
            self, max_size=max_size, suffix=suffix, prefix=prefix, dir=dir, mode=mode
        )

    def truncate(self, *args):
        # The stdlib's forgets to return the new size.
        self._file.truncate(*args)
        return self._file.tell()


def better_mktemp(*args, **kwargs):
    fd, path = tempfile.mkstemp(*args, **kwargs)
    os.close(fd)
    return path
