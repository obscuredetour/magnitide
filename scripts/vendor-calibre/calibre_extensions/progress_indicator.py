"""calibre_extensions.progress_indicator: the spinner and the Qt helpers
calibre implements in C++ (`set_menu_on_action`, `menu_for_action`,
`icon_from_name`, ...), in Python over PyQt6."""

import math

from PyQt6 import sip
from PyQt6.QtCore import QRectF, Qt, QTimer, pyqtSignal
from PyQt6.QtGui import QColor, QIcon, QPainter, QPen
from PyQt6.QtWidgets import QAction, QMenu, QWidget

_ICON_THEME = {'name': None}
_ICON_FACTORY = {'fn': None}


class SpinAnimator:
    def __init__(self, parent=None):
        self.angle = 0
        self.timer = QTimer(parent)
        self.timer.setInterval(20)

    def start(self):
        self.timer.start()

    def stop(self):
        self.timer.stop()

    def is_running(self):
        return self.timer.isActive()


class QProgressIndicator(QWidget):
    def __init__(self, parent=None, size=64, interval=10):
        QWidget.__init__(self, parent)
        self.display_size = size
        self.angle = 0
        self.timer = QTimer(self)
        self.timer.setInterval(max(1, interval))
        self.timer.timeout.connect(self._tick)
        self.setSizePolicy(self.sizePolicy().horizontalPolicy(), self.sizePolicy().verticalPolicy())

    def _tick(self):
        self.angle = (self.angle + 6) % 360
        self.update()

    def start(self):
        self.timer.start()

    startAnimation = start

    def stop(self):
        self.timer.stop()
        self.update()

    stopAnimation = stop

    def isAnimated(self):
        return self.timer.isActive()

    def setDisplaySize(self, size):
        self.display_size = size
        self.update()

    def sizeHint(self):
        from PyQt6.QtCore import QSize

        return QSize(self.display_size, self.display_size)

    def paintEvent(self, ev):
        if not self.isAnimated():
            return
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        side = min(self.width(), self.height())
        rect = QRectF(
            (self.width() - side) / 2 + side * 0.15, (self.height() - side) / 2 + side * 0.15, side * 0.7, side * 0.7
        )
        pen = QPen(self.palette().color(self.foregroundRole()))
        pen.setWidthF(max(2.0, side / 12))
        pen.setCapStyle(Qt.PenCapStyle.RoundCap)
        p.setPen(pen)
        p.drawArc(rect, int(-self.angle * 16), int(120 * 16))
        p.end()


def set_menu_on_action(action, menu):
    action.setMenu(menu)


def menu_for_action(action):
    return action.menu()


def set_image_allocation_limit(mb):
    from PyQt6.QtGui import QImageReader

    QImageReader.setAllocationLimit(mb)


def get_image_allocation_limit():
    from PyQt6.QtGui import QImageReader

    return QImageReader.allocationLimit()


def set_icon_theme(name, factory=None):
    _ICON_THEME['name'] = name
    _ICON_FACTORY['fn'] = factory


def icon_from_name(name):
    fn = _ICON_FACTORY['fn']
    if fn is not None:
        ans = fn(name)
        if ans is not None:
            return ans
    return QIcon.fromTheme(name) if QIcon.hasThemeIcon(name) else QIcon()


def contrast_ratio(a, b):
    def lum(c):
        c = QColor(c)
        vals = []
        for v in (c.redF(), c.greenF(), c.blueF()):
            vals.append(v / 12.92 if v <= 0.03928 else ((v + 0.055) / 1.055) ** 2.4)
        return 0.2126 * vals[0] + 0.7152 * vals[1] + 0.0722 * vals[2]

    la, lb = lum(a), lum(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


def utf16_slice(text, start, end):
    data = text.encode('utf-16-le')
    return data[start * 2 : end * 2].decode('utf-16-le', 'ignore')


def install_qt_translator(*a, **k):
    return False


def load_style(name):
    return False


def draw_snake_spinner(painter, rect, angle, light, dark):
    pass


__all__ = [
    'QProgressIndicator',
    'SpinAnimator',
    'set_menu_on_action',
    'menu_for_action',
    'icon_from_name',
    'set_icon_theme',
    'contrast_ratio',
    'utf16_slice',
    'install_qt_translator',
    'set_image_allocation_limit',
    'get_image_allocation_limit',
    'load_style',
    'draw_snake_spinner',
    'math',
    'sip',
    'QAction',
    'QMenu',
    'pyqtSignal',
]
