"""Mirrors calibre's `calibre.gui2.ui`: `get_gui()`, the main window. In
Magnitide the window is the host's stand-in (`magnitide_host.gui.window`),
which subclasses `Main` so `isinstance` checks hold."""

from qt.core import QMainWindow


class Main(QMainWindow):
    """calibre's main window class (the stand-in subclasses it)."""

    def __init__(self, *args, **kwargs):
        QMainWindow.__init__(self, *args, **kwargs)


_gui = None


def get_gui():
    return _gui


def set_gui(gui):
    global _gui
    _gui = gui
    get_gui.ans = gui


get_gui.ans = None
