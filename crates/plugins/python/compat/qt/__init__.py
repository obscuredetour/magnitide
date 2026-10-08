"""Magnitide's stand-in for calibre's `qt` package: `qt.core` is every public
name of the PyQt6 modules calibre uses, in one namespace, as calibre's is.
`qt.webengine` isn't available (the runtime ships no WebEngine) and says so.

Older plugins import `PyQt5`; those imports are redirected to PyQt6 (the
names are the same for the most part; enums changed, which is on the plugin).
"""

import importlib
import importlib.abc
import importlib.machinery
import sys


class _PyQt5Redirect(importlib.abc.MetaPathFinder, importlib.abc.Loader):
    """`import PyQt5.QtWidgets` → `PyQt6.QtWidgets`; `PyQt5.Qt` → `qt.core`."""

    def find_spec(self, fullname, path=None, target=None):
        if fullname == 'PyQt5' or fullname.startswith('PyQt5.'):
            return importlib.machinery.ModuleSpec(fullname, self, is_package=fullname == 'PyQt5')
        return None

    def create_module(self, spec):
        # A module of our own that carries the real one's names: the import
        # machinery sets __name__ and __spec__ on what's returned, and
        # renaming PyQt6's own modules breaks sip.
        import types

        name = spec.name
        module = types.ModuleType(name)
        if name == 'PyQt5':
            module.__path__ = []
            return module
        sub = name.partition('.')[2]
        if sub in ('Qt', 'QtCore.Qt'):
            real = importlib.import_module('qt.core')
            real._load_all()
        else:
            real = importlib.import_module('PyQt6.' + sub)
        for key, value in vars(real).items():
            if not (key.startswith('__') and key.endswith('__')):
                module.__dict__[key] = value
        module.__getattr__ = lambda attr: getattr(real, attr)
        return module

    def exec_module(self, module):
        pass


if not any(isinstance(f, _PyQt5Redirect) for f in sys.meta_path):
    sys.meta_path.append(_PyQt5Redirect())
