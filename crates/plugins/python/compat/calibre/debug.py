"""Mirrors the helpers of calibre's `calibre.debug` plugins call."""

import sys


def print_basic_debug_info(out=None):
    import platform

    from calibre.constants import __version__, numeric_version

    if out is None:
        out = sys.stdout
    print(f'calibre {__version__} (API {".".join(map(str, numeric_version))}) in Magnitide', file=out)
    print(f'Python {platform.python_version()} on {platform.platform()}', file=out)
    try:
        from qt.core import QT_VERSION_STR

        print(f'Qt {QT_VERSION_STR}', file=out)
    except Exception:
        pass


def run_calibre_debug(*args, **kw):
    raise RuntimeError('calibre-debug is not available in Magnitide')


__all__ = ['print_basic_debug_info', 'run_calibre_debug']
