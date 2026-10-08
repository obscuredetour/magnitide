"""Mirrors calibre's `calibre.constants` (calibre 9.15), for Magnitide's
plugin runtime.

The version is the calibre API level this compatibility package provides:
plugins compare their `minimum_calibre_version` against it. `MAGNITIDE`
tells a plugin it's running in Magnitide, not calibre.

Not provided: the C-extension loader (`plugins` is an empty mapping; there
are no `calibre_extensions`), `isfrozen` is always False, and there is no
portable mode.
"""

import locale
import os
import sys
from collections import namedtuple

__appname__ = 'calibre'
numeric_version = (9, 15, 0)
__version__ = '.'.join(map(str, numeric_version))
git_version = None
__author__ = 'Kovid Goyal <kovid@kovidgoyal.net>'

# Not calibre: Magnitide's compatibility package.
MAGNITIDE = True
magnitide_version = os.environ.get('MAGNITIDE_VERSION', '')

iswindows = sys.platform.startswith('win')
ismacos = isosx = sys.platform == 'darwin'
islinux = sys.platform.startswith('linux')
isfreebsd = sys.platform.startswith('freebsd')
isnetbsd = sys.platform.startswith('netbsd')
isopenbsd = sys.platform.startswith('openbsd')
isdragonflybsd = sys.platform.startswith('dragonfly')
isbsd = isfreebsd or isnetbsd or isopenbsd or isdragonflybsd
ishaiku = sys.platform == 'haiku1'
isunix = ismacos or islinux or isbsd or ishaiku
isportable = False
isfrozen = False
is64bit = sys.maxsize > (1 << 32)
isxp = isoldvista = False
ispy3 = True
iswebengine = False
is_running_from_develop = False
in_develop_mode = False

DEBUG = 'CALIBRE_DEBUG' in os.environ or 'MAGNITIDE_DEBUG' in os.environ


def debug():
    global DEBUG
    DEBUG = True


def is_debugging():
    return DEBUG


FAKE_PROTOCOL = 'calibre'
FAKE_HOST = 'calibre-internal.invalid'
VIEWER_APP_UID = 'com.calibre-ebook.viewer'
EDITOR_APP_UID = 'com.calibre-ebook.edit-book'
MAIN_APP_UID = 'com.calibre-ebook.main-gui'
SPECIAL_FIELDS = frozenset(('comments', 'identifiers'))
DEFAULT_THUMBNAIL_SIZE = 'small'
trash_name = '.caltrash'
notes_dir_name = '.calnotes'
backup_dir_name = '.calbackup'
data_dir_name = 'data'

filesystem_encoding = sys.getfilesystemencoding() or 'utf-8'
try:
    preferred_encoding = locale.getpreferredencoding() or 'utf-8'
except Exception:
    preferred_encoding = 'utf-8'
if preferred_encoding.lower() in ('ascii', 'ansi_x3.4-1968', 'us-ascii', 'cp0'):
    preferred_encoding = 'utf-8'

win32event = win32api = winerror = None
fcntl = None if iswindows else __import__('fcntl')

_osx_ver = None


def get_osx_version():
    global _osx_ver
    if _osx_ver is None:
        import platform

        try:
            _osx_ver = tuple(int(x) for x in platform.mac_ver()[0].split('.')[:3])
        except Exception:
            _osx_ver = (0, 0, 0)
        while len(_osx_ver) < 3:
            _osx_ver += (0,)
    return _osx_ver


def get_version():
    """The calibre API version this compatibility package provides."""
    v = __version__
    if numeric_version[-1] == 0:
        v = v[:-2]
    return v


def get_portable_base():
    return None


def get_windows_username():
    if iswindows:
        return os.environ.get('USERNAME')
    return None


def get_windows_temp_path():
    return None


def get_windows_user_locale_name():
    return None


def get_windows_number_formats():
    return None


# --- Folders -------------------------------------------------------------

CONFIG_DIR_MODE = 0o700


def _default_config_dir():
    if iswindows:
        base = os.environ.get('APPDATA') or os.path.expanduser('~')
        return os.path.join(base, 'calibre')
    if ismacos:
        return os.path.join(os.path.expanduser('~/Library/Preferences'), 'calibre')
    base = os.environ.get('XDG_CONFIG_HOME') or os.path.expanduser('~/.config')
    return os.path.join(base, 'calibre')


# Magnitide always sets this: plugin settings live in its own folder, never
# the user's calibre one.
config_dir = os.environ.get('CALIBRE_CONFIG_DIRECTORY') or _default_config_dir()
config_dir = os.path.abspath(config_dir)
try:
    os.makedirs(config_dir, mode=CONFIG_DIR_MODE, exist_ok=True)
except OSError:
    pass

_cache_dir = None


def cache_dir():
    global _cache_dir
    if _cache_dir is None:
        candidate = os.environ.get('CALIBRE_CACHE_DIRECTORY')
        if not candidate:
            candidate = os.path.join(config_dir, 'caches')
        _cache_dir = os.path.abspath(candidate)
        try:
            os.makedirs(_cache_dir, exist_ok=True)
        except OSError:
            pass
    return _cache_dir


# --- C extensions: none ----------------------------------------------------

plugins_loc = None


class _NoPlugins(dict):
    """calibre's `plugins`: name → (module, load error). Magnitide's runtime
    has no C extensions; every lookup reports that."""

    def __missing__(self, name):
        return (None, f"calibre's {name} extension isn't available in Magnitide's plugin runtime")

    def __contains__(self, name):
        return False


plugins = _NoPlugins()

# --- Things a few plugins poke at ----------------------------------------

dark_link_color = '#6cb4ee'
builtin_colors_light = {
    'yellow': '#ffeb6b',
    'green': '#c0ed72',
    'blue': '#add8ff',
    'red': '#ffb0ca',
    'purple': '#d9b2ff',
}
builtin_colors_dark = {
    'yellow': '#c18d16',
    'green': '#306f50',
    'blue': '#265589',
    'red': '#a23e5a',
    'purple': '#505088',
}
builtin_decorations = {
    'wavy': {'text-decoration-style': 'wavy', 'text-decoration-color': 'red', 'text-decoration-line': 'underline'},
    'strikeout': {'text-decoration-line': 'line-through', 'text-decoration-color': 'red'},
}

ExtensionsImporter = None
ExtensionsPackageLoader = None

_resources_location = None


def resources_location():
    """Where `P()` looks: set by the host (sys.resources_location), else the
    package's own resources folder."""
    global _resources_location
    if _resources_location is None:
        loc = getattr(sys, 'resources_location', None)
        if not loc:
            loc = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'resources')
        _resources_location = loc
    return _resources_location


Version = namedtuple('Version', 'major minor patch')


def calibre_version_tuple():
    return Version(*numeric_version)


__all__ = [n for n in dir(sys.modules[__name__]) if not n.startswith('_')]
