"""GUI plugins (calibre's "interface actions").

They need Qt, which lives on the main thread: it starts the first time a GUI
plugin is used (metadata sources never need it). Each plugin gets a
stand-in for calibre's main window (`window.py`) with the parts plugins use,
on a shadow copy of the library that Magnitide writes in calibre's format
(see crates/plugins/src/shadow.rs). What the plugin changes there, Magnitide
reads back once the plugin is idle and applies through its own edit path.
"""

import os
import sys
import threading
import traceback
from threading import Event

from magnitide_host import rpc
from magnitide_host.rpc import event
from magnitide_host.rpc import qt as _qt

_qt_ready = Event()
_qt_lock = threading.Lock()

# The stand-in window, once built (window.py), and what's going on in it.
state = {'running': 0, 'plugin': None, 'watching': False, 'changed': False, 'window': None}


def main_window():
    return state['window']


def ensure_qt():
    with _qt_lock:
        if not _qt_ready.is_set() and 'starting' not in _qt:
            _qt['starting'] = True
            rpc.main_calls.put(_run_qt)
    _qt_ready.wait()


def qt_running():
    return 'invoker' in _qt


def _run_qt():
    """On the main thread: start Qt and run its event loop until shutdown."""
    from calibre.gui2 import Application, gprefs
    from magnitide_host.gui.macos import macos_accessory
    from qt.core import QObject, Qt, pyqtSignal

    class Invoker(QObject):
        call = pyqtSignal(object)

        def __init__(self):
            QObject.__init__(self)
            self.call.connect(self.run, type=Qt.ConnectionType.QueuedConnection)

        def run(self, fn):
            fn()

    # The native look, not calibre's own style.
    gprefs['ui_style'] = 'system'
    gprefs['color_palette'] = 'system'
    # On macOS, Qt would make the host a regular app (a Dock icon, and the
    # focus taken from Magnitide as it starts); an accessory app stays out
    # of the way, as calibre does when it runs headless.
    os.environ['QT_MAC_DISABLE_FOREGROUND_APPLICATION_TRANSFORM'] = '1'
    macos_accessory()
    app = Application(['magnitide-plugins'])
    app.setQuitOnLastWindowClosed(False)
    # An exception in a plugin's slot must not abort the process (PyQt's
    # default); report it and carry on, as calibre does.
    sys.excepthook = _excepthook
    _raise_plugin_windows(app)
    _qt['app'], _qt['invoker'] = app, Invoker()
    _qt_ready.set()
    while not rpc.main_calls.empty():
        fn = rpc.main_calls.get_nowait()
        if fn is not None:
            _qt['invoker'].call.emit(fn)
    app.exec()


def _raise_plugin_windows(app):
    """Bring each window a plugin opens (its dialogs) to the front."""
    from magnitide_host.gui.macos import bring_to_front
    from qt.core import QEvent, QObject

    class Raiser(QObject):
        def eventFilter(self, obj, ev):
            if ev.type() == QEvent.Type.Show and obj.isWidgetType() and obj.isWindow() and obj is not state['window']:
                if not obj.inherits('QMenu') and not obj.inherits('QToolTip'):
                    bring_to_front(obj)
            return False

    _qt['raiser'] = Raiser(app)
    app.installEventFilter(_qt['raiser'])


def _excepthook(kind, value, tb):
    print(''.join(traceback.format_exception(kind, value, tb)), file=sys.stderr, flush=True)
    event('gui-error', plugin=state.get('plugin'), message=f'{kind.__name__}: {value}'.strip())
