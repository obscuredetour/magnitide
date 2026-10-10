"""Mirrors calibre's `qt.core` (calibre 9.15): one module holding every public
name of PyQt6's QtCore, QtGui, QtWidgets, QtNetwork, QtPrintSupport, QtSvg,
QtSvgWidgets, QtOpenGL and QtOpenGLWidgets, plus `sip`.

Names resolve on first use (`__getattr__`), so `from qt.core import QDialog`
loads only QtWidgets, and a module Estuary doesn't ship fails only when a
name from it is asked for.
"""

import enum
import importlib

_MODULES = (
    'QtCore',
    'QtGui',
    'QtWidgets',
    'QtNetwork',
    'QtPrintSupport',
    'QtSvg',
    'QtSvgWidgets',
    'QtOpenGL',
    'QtOpenGLWidgets',
    'QtDBus',
)
_loaded = {}
_names = {}


def _pyqt5_compat(m):
    """Names plugins written for calibre 5 (PyQt5) still use:

    - an enum member reached from its class (`QToolButton.MenuButtonPopup`,
      `Qt.AlignCenter`), where PyQt6 wants `QToolButton.ToolButtonPopupMode.
      MenuButtonPopup`: each class gets its enums' members as attributes
      too, where the name is free;
    - `exec_()` on dialogs, menus, applications, event loops and threads,
      which PyQt6 spells `exec()`;
    - `QFontMetrics.width()`, now `horizontalAdvance()`.
    """
    for name in dir(m):
        if name.startswith('_'):
            continue
        cls = getattr(m, name, None)
        if not isinstance(cls, type):
            continue
        try:
            attrs = dict(vars(cls))
        except TypeError:
            continue
        for attr, value in attrs.items():
            if isinstance(value, type) and issubclass(value, enum.Enum):
                for member_name, member in value.__members__.items():
                    if not hasattr(cls, member_name):
                        try:
                            setattr(cls, member_name, member)
                        except TypeError, AttributeError:
                            break
        if 'exec' in attrs and not hasattr(cls, 'exec_'):
            try:
                cls.exec_ = attrs['exec']  # the raw descriptor: via the class it comes back bound to the class
            except TypeError, AttributeError:
                pass
        if name in ('QFontMetrics', 'QFontMetricsF') and not hasattr(cls, 'width'):
            try:
                cls.width = attrs.get('horizontalAdvance', cls.horizontalAdvance)
            except TypeError, AttributeError:
                pass


def _load(mod):
    if mod in _loaded:
        return _loaded[mod]
    try:
        m = importlib.import_module('PyQt6.' + mod)
    except ImportError:
        _loaded[mod] = None
        return None
    _loaded[mod] = m
    _pyqt5_compat(m)
    g = globals()
    for name in dir(m):
        if not name.startswith('_') and name not in g:
            g[name] = getattr(m, name)
            _names[name] = mod
    return m


def _load_all():
    for mod in _MODULES:
        _load(mod)


def __getattr__(name):
    if name.startswith('__'):
        raise AttributeError(name)
    g = globals()
    for mod in _MODULES:
        if mod in _loaded:
            continue
        _load(mod)
        if name in g:
            return g[name]
    if name == 'sip':
        from PyQt6 import sip

        g['sip'] = sip
        return sip
    raise AttributeError(f"qt.core has no {name!r} (it isn't in the Qt modules Estuary ships)")


def __dir__():
    _load_all()
    return sorted(k for k in globals() if not k.startswith('_'))


# The modules every plugin needs are loaded up front, so `from qt.core import *`
# and attribute checks see them.
for _m in ('QtCore', 'QtGui', 'QtWidgets'):
    _load(_m)
del _m

# calibre adds a few conveniences to Qt's classes; the ones plugins rely on.
if 'QIcon' in globals() and not hasattr(globals()['QIcon'], 'ic'):

    def _ic(name):
        from calibre.gui2 import icon_from_name

        return icon_from_name(name)

    globals()['QIcon'].ic = staticmethod(_ic)
