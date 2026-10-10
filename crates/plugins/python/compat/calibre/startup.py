# Derived from calibre's src/calibre/startup.py (calibre 9.15).
# Copyright: 2008, Kovid Goyal kovid@kovidgoyal.net. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.startup` (calibre 9.15): what calibre does
before anything else runs, so plugins find the built-in names calibre gives
every module: `_` and `ngettext` (translation), `P` and `I` (resource paths),
`icu_lower`/`icu_upper`/`icu_title`, `connect_lambda`, `lopen` and
`dynamic_property`.

Importing `calibre` runs `initialize_calibre()` once.
"""

import builtins
import sys

_initialized = False


def _passthrough(s):
    return s


builtins.__dict__.setdefault('_', _passthrough)
builtins.__dict__.setdefault('__', _passthrough)


def dynamic_property(func):
    """calibre's old way of declaring a property from a function returning
    `property(**locals())`."""
    return func(None)


builtins.__dict__['dynamic_property'] = dynamic_property


def connect_lambda(bound_signal, self, func, **kw):
    """Connect `func(self, *args)` to a signal without a strong reference to
    `self` (as calibre's widgets do in their lambdas)."""
    import weakref

    r = weakref.ref(self)
    del self
    num_args = func.__code__.co_argcount - 1
    if num_args < 0:
        raise TypeError('lambda must take at least one argument')

    def slot(*args):
        ctx = r()
        if ctx is not None:
            if len(args) != num_args:
                args = args[:num_args]
            func(ctx, *args)

    bound_signal.connect(slot, **kw)


def initialize_calibre():
    global _initialized
    if _initialized:
        return
    _initialized = True

    # Translations: identity unless a plugin loads its own (`load_translations`).
    from calibre.utils.localization import set_translators

    set_translators()

    # P() and I(): resource paths.
    from calibre.utils import resources  # noqa: F401

    # Case changes that respect Unicode (calibre's ICU; ours is pure Python).
    from calibre.utils.icu import lower as icu_lower
    from calibre.utils.icu import title_case
    from calibre.utils.icu import upper as icu_upper

    builtins.__dict__['icu_lower'] = icu_lower
    builtins.__dict__['icu_upper'] = icu_upper
    builtins.__dict__['icu_title'] = title_case
    builtins.__dict__['lopen'] = open  # calibre's long-path-aware open on Windows
    builtins.__dict__['connect_lambda'] = connect_lambda

    # The temp folder calibre code expects (`calibre.ptempfile`).
    from calibre.ptempfile import base_dir

    base_dir()

    # More open files than the default, as calibre asks for.
    if not sys.platform.startswith('win'):
        try:
            import resource

            soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
            if soft < 4096:
                resource.setrlimit(resource.RLIMIT_NOFILE, (min(4096, hard), hard))
        except Exception:
            pass


def get_debug_executable(headless=False, exe_name='calibre-debug'):
    """Where calibre's own tools would be: here, Estuary's interpreter
    running Magnitide's worker, which answers `calibre-debug -c` and `-e`."""
    import os

    worker = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'magnitide_host', 'run-calibre-worker.py')
    return [sys.executable, '-I', os.path.abspath(worker), exe_name]


def get_calibre_gui_command():
    raise RuntimeError("Magnitide hosts calibre plugins; there's no calibre window to launch")


def launch_calibre_gui(*args, **kwargs):
    get_calibre_gui_command()
