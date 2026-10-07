"""Mirrors the widgets plugins borrow from calibre's `calibre.gui2.widgets`
(calibre 9.15): line edits with history and completion, an image view, and
a few small ones."""

from calibre.gui2 import gprefs
from qt.core import (
    QApplication,
    QComboBox,
    QFont,
    QIcon,
    QLabel,
    QLineEdit,
    QListWidget,
    QPixmap,
    QSize,
    QSizePolicy,
    QSplitter,
    Qt,
    QToolButton,
    QWidget,
    pyqtSignal,
)

history = gprefs


class LineEditECM:
    """Mixin: a line edit with a case-changing context menu (as calibre's)."""

    def contextMenuEvent(self, event):
        menu = self.createStandardContextMenu()
        menu.addSeparator()
        menu.addAction('Upper case', lambda: self.setText(self.text().upper()))
        menu.addAction('Lower case', lambda: self.setText(self.text().lower()))
        menu.addAction('Title case', lambda: self.setText(self.text().title()))
        menu.exec(event.globalPos())


class EnLineEdit(LineEditECM, QLineEdit):
    """A QLineEdit with calibre's case-changing context menu."""

    pass


class ItemsCompleter:
    pass


class CompleteLineEdit(EnLineEdit):
    """A line edit completing on a list of items (comma-separated values)."""

    def __init__(self, parent=None, complete_items=[], sep=',', space_before_sep=False):
        EnLineEdit.__init__(self, parent)
        self.separator = sep
        self.space_before_sep = space_before_sep
        self.update_items_cache(complete_items)

    def update_items_cache(self, complete_items):
        from qt.core import QCompleter

        self.all_items = list(complete_items)
        self.completer = QCompleter(self.all_items, self)
        self.completer.setCaseSensitivity(Qt.CaseSensitivity.CaseInsensitive)
        self.setCompleter(self.completer)

    def set_separator(self, sep):
        self.separator = sep

    def set_space_before_sep(self, space_before):
        self.space_before_sep = space_before


class EnComboBox(QComboBox):
    """An editable combo box with calibre's line edit."""

    def __init__(self, *args):
        QComboBox.__init__(self, *args)
        self.setLineEdit(EnLineEdit(self))
        self.setAutoCompletionCaseSensitivity(Qt.CaseSensitivity.CaseSensitive) if hasattr(
            self, 'setAutoCompletionCaseSensitivity'
        ) else None
        self.setMinimumContentsLength(20)

    def text(self):
        return str(self.currentText())

    def setText(self, text):
        idx = self.findText(text, Qt.MatchFlag.MatchFixedString | Qt.MatchFlag.MatchCaseSensitive)
        if idx == -1:
            self.insertItem(0, text)
            idx = 0
        self.setCurrentIndex(idx)


class HistoryLineEdit(QComboBox):
    """An editable combo box remembering what was typed (`initialize(name)`
    loads the history, `save_history()` stores it)."""

    lost_focus = pyqtSignal()

    def __init__(self, *args):
        QComboBox.__init__(self, *args)
        self.setEditable(True)
        self.setInsertPolicy(QComboBox.InsertPolicy.NoInsert)
        self.setSizeAdjustPolicy(QComboBox.SizeAdjustPolicy.AdjustToMinimumContentsLengthWithIcon)
        self.setMinimumContentsLength(20)
        self.store_name = None

    @property
    def store_name(self):
        return self._store_name

    @store_name.setter
    def store_name(self, val):
        self._store_name = val

    def initialize(self, name):
        self.store_name = 'lineedit_history_' + name
        self.history = history.get(self.store_name, [])
        self.setCurrentIndex(-1)
        self.clear()
        self.addItems(self.history)
        self.setEditText('')
        self.lineEdit().editingFinished.connect(self.save_history)

    def save_history(self):
        items = []
        ct = str(self.currentText())
        if ct:
            items.append(ct)
        for i in range(self.count()):
            item = str(self.itemText(i))
            if item not in items:
                items.append(item)
        self.blockSignals(True)
        self.clear()
        self.addItems(items)
        self.setEditText(ct)
        self.blockSignals(False)
        history.set(self.store_name, items)

    def setText(self, t):
        self.setEditText(t)
        self.lineEdit().setCursorPosition(0)

    def text(self):
        return self.currentText()

    def focusOutEvent(self, e):
        QComboBox.focusOutEvent(self, e)
        self.lost_focus.emit()


class ComboBoxWithHelp(QComboBox):
    """A combo box showing help text when nothing is selected."""

    def __init__(self, parent=None):
        QComboBox.__init__(self, parent)
        self.currentIndexChanged.connect(self.index_changed)
        self.help_text = ''
        self.state_set = False

    def initialize(self):
        if not self.state_set:
            self.help_text = self.itemText(0) if self.count() else ''
        self.state_set = True

    def index_changed(self, index):
        self.initialize()

    def setHelpText(self, text):
        self.help_text = text

    def showPopup(self):
        self.initialize()
        QComboBox.showPopup(self)

    def hidePopup(self):
        QComboBox.hidePopup(self)


class ImageView(QWidget):
    """Shows a pixmap scaled to fit; `set_pixmap`/`pixmap`, with a
    `changed` signal when one is dropped on it."""

    changed = pyqtSignal()
    cover_changed = pyqtSignal(object)
    BORDER_WIDTH = 1

    def __init__(self, parent=None, show_size_pref_name=None, default_show_size=False):
        QWidget.__init__(self, parent)
        self._pixmap = QPixmap()
        self.setMinimumSize(QSize(150, 200))
        self.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Expanding)
        self.draw_border = True
        self.show_size = False
        self.setAcceptDrops(True)

    def setPixmap(self, pixmap):
        if not isinstance(pixmap, QPixmap):
            raise TypeError('Must use a QPixmap')
        self._pixmap = pixmap
        self.updateGeometry()
        self.update()

    set_pixmap = setPixmap

    def pixmap(self):
        return self._pixmap

    def sizeHint(self):
        if self._pixmap.isNull():
            return self.minimumSize()
        return self._pixmap.size()

    def paintEvent(self, event):
        from qt.core import QPainter

        QWidget.paintEvent(self, event)
        pmap = self._pixmap
        if pmap.isNull():
            return
        w, h = pmap.width(), pmap.height()
        cw, ch = self.rect().width(), self.rect().height()
        scaled, nw, nh = __import__('calibre', fromlist=['fit_image']).fit_image(w, h, cw, ch)
        if scaled:
            pmap = pmap.scaled(
                int(nw * pmap.devicePixelRatio()),
                int(nh * pmap.devicePixelRatio()),
                Qt.AspectRatioMode.IgnoreAspectRatio,
                Qt.TransformationMode.SmoothTransformation,
            )
        w, h = int(pmap.width() / pmap.devicePixelRatio()), int(pmap.height() / pmap.devicePixelRatio())
        x = int(abs(cw - w) / 2)
        y = int(abs(ch - h) / 2)
        target = self.rect().adjusted(x, y, -x, -y)
        p = QPainter(self)
        p.setRenderHints(QPainter.RenderHint.Antialiasing | QPainter.RenderHint.SmoothPixmapTransform)
        p.drawPixmap(target, pmap)
        if self.draw_border:
            pen = p.pen()
            pen.setWidth(self.BORDER_WIDTH)
            p.setPen(pen)
            p.drawRect(target)
        p.end()

    def dragEnterEvent(self, event):
        md = event.mimeData()
        if md.hasImage() or md.hasUrls():
            event.acceptProposedAction()

    def dropEvent(self, event):
        from calibre.gui2.dnd import dnd_get_image

        x, y = dnd_get_image(event.mimeData())
        if x is not None:
            self.setPixmap(x)
            self.changed.emit()
            self.cover_changed.emit(bytes(__import__('calibre.gui2', fromlist=['pixmap_to_data']).pixmap_to_data(x)))
            event.accept()


class Splitter(QSplitter):
    def __init__(
        self,
        name,
        label,
        icon,
        initial_show=True,
        initial_side_size=120,
        connect_button=True,
        orientation=Qt.Orientation.Horizontal,
        side_index=0,
        parent=None,
        shortcut=None,
        hide_handle_on_single_panel=True,
    ):
        QSplitter.__init__(self, parent)
        self.name = name
        self.setOrientation(orientation)
        self.side_index = side_index

    def save_state(self):
        pass

    def restore_state(self):
        pass


class BasicList(QListWidget):
    def add_item(self, text, user_data=None, emphasize=False):
        from qt.core import QListWidgetItem

        item = QListWidgetItem(text, self)
        item.setData(Qt.ItemDataRole.UserRole, user_data)
        if emphasize:
            f = item.font()
            f.setBold(True)
            item.setFont(f)
        return item

    def remove_selected_items(self, *args):
        for item in self.selectedItems():
            self.takeItem(self.row(item))

    def items(self):
        for i in range(self.count()):
            yield self.item(i)


class LayoutButton(QToolButton):
    def __init__(self, icon, text, splitter=None, parent=None, shortcut=None):
        QToolButton.__init__(self, parent)
        self.label = text
        self.setIcon(QIcon.ic(icon) if isinstance(icon, str) else icon)
        self.setCheckable(True)
        self.setText(text)


class Dialog(QWidget):
    pass


def setup_status_tip(w, text):
    w.setStatusTip(text)
    w.setToolTip(text)


__all__ = [
    'EnLineEdit',
    'EnComboBox',
    'HistoryLineEdit',
    'CompleteLineEdit',
    'ComboBoxWithHelp',
    'ImageView',
    'Splitter',
    'BasicList',
    'LayoutButton',
    'LineEditECM',
    'history',
    'setup_status_tip',
    'QApplication',
    'QFont',
    'QLabel',
]
