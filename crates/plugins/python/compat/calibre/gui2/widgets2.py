"""Mirrors the widgets plugins borrow from calibre's `calibre.gui2.widgets2`
(calibre 9.15): `Dialog` (a dialog with saved geometry), history line
edits, colour and tool buttons, a flow layout, an HTML display."""

from calibre.gui2 import gprefs
from qt.core import (
    QColor,
    QColorDialog,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QFontMetrics,
    QIcon,
    QLayout,
    QLineEdit,
    QMenu,
    QPainter,
    QPixmap,
    QPlainTextEdit,
    QPoint,
    QPushButton,
    QRect,
    QSize,
    QSpinBox,
    QStyle,
    Qt,
    QTextBrowser,
    QToolButton,
    QWidget,
    pyqtSignal,
)


class Dialog(QDialog):
    """calibre's base dialog: `name` keys its saved geometry; subclasses
    implement `setup_ui()` and have `self.bb` (the button box)."""

    def __init__(
        self,
        title,
        name,
        parent=None,
        default_buttons=QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel,
        prefs=gprefs,
    ):
        QDialog.__init__(self, parent)
        self.prefs_for_persistence = prefs
        self.setWindowTitle(title)
        self.name = name
        self.bb = QDialogButtonBox(default_buttons)
        self.bb.accepted.connect(self.accept)
        self.bb.rejected.connect(self.reject)
        self.setup_ui()
        self.resize(self.sizeHint())
        geom = self.prefs_for_persistence.get(name + '-geometry', None)
        if geom is not None:
            try:
                self.restoreGeometry(bytes(geom))
            except Exception:
                pass
        if hasattr(self, 'splitter'):
            state = self.prefs_for_persistence.get(name + '-splitter-state', None)
            if state is not None:
                try:
                    self.splitter.restoreState(bytes(state))
                except Exception:
                    pass

    def accept(self):
        self.prefs_for_persistence.set(self.name + '-geometry', bytearray(self.saveGeometry()))
        if hasattr(self, 'splitter'):
            self.prefs_for_persistence.set(self.name + '-splitter-state', bytearray(self.splitter.saveState()))
        QDialog.accept(self)

    def reject(self):
        self.prefs_for_persistence.set(self.name + '-geometry', bytearray(self.saveGeometry()))
        if hasattr(self, 'splitter'):
            self.prefs_for_persistence.set(self.name + '-splitter-state', bytearray(self.splitter.saveState()))
        QDialog.reject(self)

    def setup_ui(self):
        raise NotImplementedError('You must implement this method in Dialog subclasses')


class HistoryMixin:
    max_history_items = None
    min_history_entry_length = 3

    def __init__(self, *args, **kwargs):
        pass

    @property
    def store_name(self):
        return 'lineedit_history_' + self._name

    def initialize(self, name):
        self._name = name
        self.history = self.prefs.get(self.store_name, [])
        self.set_separator(None) if hasattr(self, 'set_separator') else None
        self.update_items_cache(self.history)
        self.setText('')
        try:
            self.editingFinished.connect(self.save_history)
        except AttributeError:
            self.lineEdit().editingFinished.connect(self.save_history)

    def save_history(self):
        ct = str(self.text())
        if len(ct) >= self.min_history_entry_length:
            try:
                self.history.remove(ct)
            except ValueError:
                pass
            self.history.insert(0, ct)
            if self.max_history_items is not None:
                del self.history[self.max_history_items :]
            self.prefs.set(self.store_name, self.history)
            self.update_items_cache(self.history)

    def clear_history(self):
        self.history = []
        self.prefs.set(self.store_name, self.history)
        self.update_items_cache(self.history)

    @property
    def prefs(self):
        return gprefs

    def update_items_cache(self, items):
        from qt.core import QCompleter

        self._completer = QCompleter(list(items), self)
        self._completer.setCaseSensitivity(Qt.CaseSensitivity.CaseInsensitive)
        self.setCompleter(self._completer)


class HistoryLineEdit2(HistoryMixin, QLineEdit):
    def __init__(self, parent=None, max_history_items=None, min_history_entry_length=3):
        QLineEdit.__init__(self, parent)
        self.max_history_items = max_history_items
        self.min_history_entry_length = min_history_entry_length


class HistoryComboBox(QComboBox):
    def __init__(self, parent=None, strip_completion_entries=True):
        QComboBox.__init__(self, parent)
        self.setEditable(True)
        self.setInsertPolicy(QComboBox.InsertPolicy.NoInsert)
        self.strip_completion_entries = strip_completion_entries

    def initialize(self, name):
        self._name = name
        self.history = gprefs.get('lineedit_history_' + name, [])
        self.clear()
        self.addItems(self.history)
        self.setEditText('')
        self.lineEdit().editingFinished.connect(self.save_history)

    def save_history(self):
        ct = str(self.currentText())
        if ct and ct not in self.history:
            self.history.insert(0, ct)
            gprefs.set('lineedit_history_' + self._name, self.history)
            self.blockSignals(True)
            self.clear()
            self.addItems(self.history)
            self.setEditText(ct)
            self.blockSignals(False)

    def text(self):
        return self.currentText()

    def setText(self, t):
        self.setEditText(t)

    def update_items_cache(self, items):
        pass

    def set_separator(self, s):
        pass


class ColorButton(QPushButton):
    """A button showing a colour that opens a colour dialog; `color` is
    the colour's name ('#rrggbb')."""

    color_changed = pyqtSignal(object)

    def __init__(self, initial_color=None, parent=None, choose_text=None):
        QPushButton.__init__(self, parent)
        self._color = None
        self.choose_text = choose_text or 'Choose &color'
        self.color = initial_color
        self.clicked.connect(self.choose_color)

    @property
    def color(self):
        return self._color

    @color.setter
    def color(self, val):
        val = str(val or '')
        col = QColor(val)
        orig = self._color
        if col.isValid():
            self._color = val
            self.setText(val)
            p = QPixmap(self.iconSize())
            p.fill(col)
            self.setIcon(QIcon(p))
        else:
            self._color = None
            self.setText(self.choose_text)
            self.setIcon(QIcon())
        if orig != col:
            self.color_changed.emit(self._color)

    def choose_color(self):
        col = QColorDialog.getColor(QColor(self._color or Qt.GlobalColor.white), self, 'Choose a color')
        if col.isValid():
            self.color = str(col.name())


class RightClickButton(QToolButton):
    def mousePressEvent(self, ev):
        if ev.button() == Qt.MouseButton.RightButton and self.menu() is not None:
            self.showMenu()
            ev.accept()
            return
        return QToolButton.mousePressEvent(self, ev)


class CenteredToolButton(RightClickButton):
    def __init__(self, icon, text, parent=None):
        RightClickButton.__init__(self, parent)
        self.setText(text)
        self.setIcon(icon if isinstance(icon, QIcon) else QIcon.ic(icon))
        self.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextBesideIcon)


class FlowLayout(QLayout):
    """A layout that wraps its widgets like words in a paragraph."""

    def __init__(self, parent=None):
        QLayout.__init__(self, parent)
        self.items = []

    def addItem(self, item):
        self.items.append(item)

    def itemAt(self, idx):
        try:
            return self.items[idx]
        except IndexError:
            pass

    def takeAt(self, idx):
        try:
            return self.items.pop(idx)
        except IndexError:
            pass

    def count(self):
        return len(self.items)

    def expandingDirections(self):
        return Qt.Orientation(0)

    def hasHeightForWidth(self):
        return True

    def heightForWidth(self, width):
        return self.do_layout(QRect(0, 0, width, 0), True)

    def setGeometry(self, rect):
        QLayout.setGeometry(self, rect)
        self.do_layout(rect, False)

    def sizeHint(self):
        return self.minimumSize()

    def minimumSize(self):
        size = QSize()
        for item in self.items:
            size = size.expandedTo(item.minimumSize())
        left, top, right, bottom = self.getContentsMargins()
        return size + QSize(left + right, top + bottom)

    def smart_spacing(self, horizontal=True):
        p = self.parent()
        if p is None:
            return -1
        if p.isWidgetType():
            which = (
                QStyle.PixelMetric.PM_LayoutHorizontalSpacing
                if horizontal
                else QStyle.PixelMetric.PM_LayoutVerticalSpacing
            )
            return p.style().pixelMetric(which, None, p)
        return p.spacing()

    def do_layout(self, rect, test_only=False):
        left, top, right, bottom = self.getContentsMargins()
        erect = rect.adjusted(left, top, -right, -bottom)
        x, y = erect.x(), erect.y()
        line_height = 0

        def layout_spacing(wid, horizontal=True):
            ans = self.smart_spacing(horizontal)
            if ans != -1:
                return ans
            if wid is None:
                return 0
            which = (
                QStyle.PixelMetric.PM_LayoutHorizontalSpacing
                if horizontal
                else QStyle.PixelMetric.PM_LayoutVerticalSpacing
            )
            return wid.style().pixelMetric(which, None, wid)

        lines, current_line = [], []
        gmap = {}
        for item in self.items:
            isz, wid = item.sizeHint(), item.widget()
            hs, vs = layout_spacing(wid), layout_spacing(wid, False)
            next_x = x + isz.width() + hs
            if next_x - hs > erect.right() and line_height > 0:
                x = erect.x()
                y = y + line_height + vs
                next_x = x + isz.width() + hs
                lines.append(current_line)
                current_line = []
                line_height = 0
            if not test_only:
                gmap[item] = x, y, isz
            current_line.append(item)
            x = next_x
            line_height = max(line_height, isz.height())
        lines.append(current_line)
        if not test_only:
            for line in lines:
                for item in line:
                    x, y, isz = gmap[item]
                    item.setGeometry(QRect(QPoint(x, y), isz))
        return y + line_height - rect.y() + bottom


class HTMLDisplay(QTextBrowser):
    anchor_clicked = pyqtSignal(object)
    notes_resource_scheme = ''

    def __init__(self, parent=None, save_resources_in_document=True):
        QTextBrowser.__init__(self, parent)
        self.setOpenLinks(False)
        self.anchorClicked.connect(self.on_anchor_clicked)
        self.setOpenExternalLinks(False)

    def on_anchor_clicked(self, qurl):
        if not qurl.scheme() and qurl.hasFragment() and qurl.toString().startswith('#'):
            self.scrollToAnchor(qurl.fragment())
            return
        self.anchor_clicked.emit(qurl)

    def set_html(self, html):
        self.setHtml(html)


class RatingEditor(QSpinBox):
    """A spin box for a rating (0–5 stars, half stars when allowed)."""

    def __init__(self, parent=None, is_half_star=False):
        QSpinBox.__init__(self, parent)
        self.is_half_star = is_half_star
        self.setRange(0, 10 if is_half_star else 5)
        self.setSuffix(' stars')

    @property
    def rating_value(self):
        """The rating out of 10, as the database stores it."""
        v = self.value()
        return v if self.is_half_star else v * 2

    @rating_value.setter
    def rating_value(self, val):
        val = int(val or 0)
        self.setValue(val if self.is_half_star else val // 2)

    @property
    def current_val(self):
        return self.rating_value

    @current_val.setter
    def current_val(self, val):
        self.rating_value = val

    def set_rating(self, val):
        self.rating_value = val


class Separator(QWidget):
    def __init__(self, parent, widget_for_height=None):
        QWidget.__init__(self, parent)
        self.setFixedWidth(1)
        from qt.core import QFrame

        self.frame = QFrame(self)
        self.frame.setFrameShape(QFrame.Shape.VLine)


class PlainTextEdit(QPlainTextEdit):
    pass


def to_plain_text(self):
    """Plain text of a QTextEdit, with Unicode paragraph separators as newlines."""
    return self.toPlainText().replace(' ', '\n')


def access_key(k):
    """The & accelerator shown in text."""
    if k:
        return '&' + k
    return ''


def ensure_valid_rating(val):
    return max(0, min(10, int(val or 0)))


def rating_value(v):
    return ensure_valid_rating(v)


__all__ = [
    'Dialog',
    'HistoryLineEdit2',
    'HistoryComboBox',
    'HistoryMixin',
    'ColorButton',
    'RightClickButton',
    'CenteredToolButton',
    'FlowLayout',
    'HTMLDisplay',
    'RatingEditor',
    'Separator',
    'PlainTextEdit',
    'to_plain_text',
    'access_key',
    'ensure_valid_rating',
    'rating_value',
    'QFontMetrics',
    'QMenu',
    'QPainter',
]
