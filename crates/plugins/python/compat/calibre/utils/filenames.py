"""Mirrors calibre's `calibre.utils.filenames` (calibre 9.15): making file
names safe, shortening paths, atomic renames and the like."""

import errno
import os
import shutil
import stat
import time
from contextlib import suppress

from calibre import force_unicode, isbytestring, prints, sanitize_file_name
from calibre.constants import filesystem_encoding, ismacos, iswindows, preferred_encoding


def ascii_text(orig):
    """ASCII only: accents stripped, other scripts transliterated roughly."""
    from calibre.utils.localization import get_udc

    udc = get_udc()
    try:
        ascii = udc.decode(orig)
    except Exception:
        if isinstance(orig, str):
            orig = orig.encode('ascii', 'replace')
        ascii = orig.decode(preferred_encoding, 'replace')
    if isinstance(ascii, bytes):
        ascii = ascii.decode('ascii', 'replace')
    return ascii


def ascii_filename(orig, substitute='_'):
    if isinstance(substitute, bytes):
        substitute = substitute.decode(filesystem_encoding)
    orig = ascii_text(orig).replace('?', '_')
    ans = ''.join(x if ord(x) >= 32 else substitute for x in orig)
    return sanitize_file_name(ans, substitute=substitute)


def shorten_component(s, by_what):
    l = len(s)
    if l < by_what:
        return s
    l = (l - by_what) // 2
    if l <= 0:
        return s
    return s[:l] + s[-l:]


def limit_component_length(cname, max_length=250):
    if isinstance(cname, bytes):
        cname = cname.decode(filesystem_encoding)
    while len(cname.encode('utf-8')) > max_length:
        cname = cname[:-1]
    return cname


def shorten_components_to(length, components, last_has_extension=True, max_length=250):
    """Shorten path `components` so their joined length fits `length`,
    trimming the longest ones from the middle; the extension is kept."""
    components = [limit_component_length(cx, max_length) for cx in components]
    filepath = os.sep.join(components)
    extra = len(filepath) - (length - 1)
    if extra < 1:
        return components
    deltas = []
    for x in components:
        pct = len(x) / float(len(filepath))
        deltas.append(int(ceil(pct * extra)))
    ans = []

    for i, x in enumerate(components):
        delta = deltas[i]
        if delta > len(x):
            r = x[0] if x is components[-1] else ''
        else:
            if last_has_extension and x is components[-1]:
                b, e = os.path.splitext(x)
                if e == '.':
                    e = ''
                r = shorten_component(b, delta) + e
                if r.startswith('.'):
                    r = x[0] + r
            else:
                r = shorten_component(x, delta)
            r = r.strip()
        if not r:
            r = x.strip()[0] if x.strip() else 'x'
        ans.append(r)
    if len(os.sep.join(ans)) > length:
        return shorten_components_to(length, ans, last_has_extension)
    return ans


def ceil(x):
    import math

    return math.ceil(x)


def find_executable_in_path(name, path=None):
    if path is None:
        path = os.environ.get('PATH', '')
    exts = ('.exe', '.bat', '.cmd') if iswindows else ('',)
    path = path.split(os.pathsep)
    for x in path:
        for ext in exts:
            q = os.path.abspath(os.path.join(x, name + ext))
            if os.access(q, os.X_OK):
                return q


def is_case_sensitive(path):
    """Whether the file system at `path` (a folder) is case sensitive."""
    is_case_sensitive = False
    if not iswindows:
        name1, name2 = ('calibre_test_case_sensitivity.txt', 'calibre_TesT_CaSe_sensitiVitY.Txt')
        f1, f2 = os.path.join(path, name1), os.path.join(path, name2)
        if os.path.exists(f1):
            os.remove(f1)
        open(f1, 'w').close()
        is_case_sensitive = not os.path.exists(f2)
        os.remove(f1)
    return is_case_sensitive


def case_preserving_open_file(path, mode='wb', mkdir_mode=0o777):
    """Open `path`, creating folders along the way; on case-insensitive file
    systems existing folders with the same name in another case are reused."""
    path = os.path.abspath(path)
    sep = force_unicode(os.sep, 'ascii')
    if path.endswith(sep):
        path = path[:-1]
    if not path:
        raise ValueError('Path must not point to root')
    components = path.split(sep)
    if not components:
        raise ValueError('Invalid path: %r' % path)
    cpath = sep
    if iswindows:
        cpath = ''
    bdir = path if mode is None else os.path.dirname(path)
    if not os.path.exists(bdir):
        os.makedirs(bdir, mkdir_mode)
    for c in components[: -1 if mode is not None else None]:
        if not c:
            continue
        candidate = os.path.join(cpath, c)
        if not os.path.isdir(candidate):
            try:
                names = os.listdir(cpath or sep)
            except OSError:
                names = []
            for name in names:
                if name.lower() == c.lower():
                    candidate = os.path.join(cpath, name)
                    break
        cpath = candidate
    fpath = os.path.join(cpath, components[-1]) if mode is not None else cpath
    ans = None
    if mode is not None:
        with suppress(OSError):
            names = os.listdir(cpath)
            for name in names:
                if name.lower() == components[-1].lower():
                    fpath = os.path.join(cpath, name)
                    break
        ans = open(fpath, mode)
    return ans, fpath


def samefile_windows(src, dst):
    return os.path.normcase(os.path.abspath(src)) == os.path.normcase(os.path.abspath(dst))


def samefile(src, dst):
    """Whether two paths are the same file (handles missing files)."""
    if iswindows:
        return samefile_windows(src, dst)
    if hasattr(os.path, 'samefile'):
        try:
            return os.path.samefile(src, dst)
        except OSError:
            pass
    return os.path.normcase(os.path.abspath(src)) == os.path.normcase(os.path.abspath(dst))


def hardlink_file(src, dest):
    os.link(src, dest)


def nlinks_file(path):
    return os.stat(path).st_nlink


def atomic_rename(oldpath, newpath):
    """Rename, replacing `newpath` if it exists; retries on Windows."""
    if iswindows:
        for i in range(10):
            try:
                os.replace(oldpath, newpath)
                break
            except Exception:
                if i > 8:
                    raise
                time.sleep(1)
    else:
        os.replace(oldpath, newpath)


def remove_dir_if_empty(path, ignore_metadata_caches=False):
    """Remove `path` if it holds nothing (or only OS metadata files, when asked)."""
    try:
        os.rmdir(path)
    except OSError as e:
        if e.errno == errno.ENOTEMPTY or len(os.listdir(path)) > 0:
            if ignore_metadata_caches:
                try:
                    found = False
                    for x in os.listdir(path):
                        if x.lower() in {'.ds_store', 'thumbs.db'}:
                            found = True
                            x = os.path.join(path, x)
                            if os.path.isdir(x):
                                shutil.rmtree(x)
                            else:
                                os.remove(x)
                except Exception:
                    found = False
                if found:
                    remove_dir_if_empty(path)
            return
        raise


def copyfile(src, dest):
    shutil.copyfile(src, dest)


def copyfile_using_links(path, dest, dest_is_dir=True, use_hardlink=False):
    if dest_is_dir:
        dest = os.path.join(dest, os.path.basename(path))
    if use_hardlink:
        with suppress(OSError):
            os.link(path, dest)
            return
    shutil.copyfile(path, dest)


def copytree_using_links(path, dest, dest_is_parent=True, use_hardlink=False):
    if dest_is_parent:
        dest = os.path.join(dest, os.path.basename(path))
    os.makedirs(dest, exist_ok=True)
    for dirpath, dirnames, filenames in os.walk(path):
        base = os.path.relpath(dirpath, path)
        dbase = os.path.join(dest, base) if base != '.' else dest
        for d in dirnames:
            os.makedirs(os.path.join(dbase, d), exist_ok=True)
        for f in filenames:
            copyfile_using_links(os.path.join(dirpath, f), dbase, use_hardlink=use_hardlink)


def retry_on_fail(func, *args, count=10, sleep_time=0.2):
    for i in range(count):
        try:
            return func(*args)
        except OSError:
            if i > count - 2:
                raise
            time.sleep(sleep_time)


def format_permissions(st_mode):
    return stat.filemode(st_mode)


def get_long_path_name(path):
    return path


def make_long_path_useable(path, threshold=200):
    return path


def expanduser(path):
    return os.path.expanduser(path)


def rmtree(path, **kw):
    shutil.rmtree(path, **kw)


def windows_get_fileid(path):
    return None


def windows_hardlink(src, dest):
    os.link(src, dest)


def windows_fast_hardlink(src, dest):
    os.link(src, dest)


class WindowsAtomicFolderMove:
    """calibre's helper for moving a folder's files on Windows; here a plain
    copy-then-delete."""

    def __init__(self, path):
        self.path = path

    def copy_path_to(self, path, dest):
        shutil.copyfile(path, dest)

    def release_file(self, path):
        pass

    def close_handles(self):
        pass

    def delete_originals(self):
        for x in os.listdir(self.path):
            with suppress(OSError):
                os.remove(os.path.join(self.path, x))


def get_hardlink_function(src, dest):
    return hardlink_file


def is_fat_filesystem(path):
    return False


def max_path_len(fs_path):
    return 255


def nlinks(path):
    return os.stat(path).st_nlink


def copy_tree(src, dest, transform_destination_filename=lambda src, dest: dest, delete_source=False):
    os.makedirs(dest, exist_ok=True)
    for dirpath, dirnames, filenames in os.walk(src):
        rel = os.path.relpath(dirpath, src)
        d = dest if rel == '.' else os.path.join(dest, rel)
        os.makedirs(d, exist_ok=True)
        for f in filenames:
            s = os.path.join(dirpath, f)
            t = transform_destination_filename(s, os.path.join(d, f))
            shutil.copy2(s, t)
    if delete_source:
        shutil.rmtree(src, ignore_errors=True)


def copy_files(src_to_dest_map, delete_source=False):
    for src, dest in src_to_dest_map.items():
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        shutil.copy2(src, dest)
    if delete_source:
        for src in src_to_dest_map:
            with suppress(OSError):
                os.remove(src)


__all__ = [n for n in dir() if not n.startswith('_')]
del isbytestring, prints, ismacos
