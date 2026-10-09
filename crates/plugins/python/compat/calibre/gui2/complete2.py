# Derived from calibre's src/calibre/gui2/complete2.py (calibre 9.15).
# Copyright: 2012, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.gui2.complete2`: `EditWithComplete`, the
combo box with completion over a list of items (tags, authors, …) that
plugins put in their dialogs, and `LineEdit`."""

from qt.core import QComboBox, QCompleter, QLineEdit, Qt, pyqtSignal


class LineEdit(QLineEdit):
    """A line edit completing on items, with a separator for lists."""

    item_selected = pyqtSignal(object)

    def __init__(self, parent=None, completer_widget=None, sort_func=None, strip_completion_entries=True):
        QLineEdit.__init__(self, parent)
        self.sep = ','
        self.space_before_sep = False
        self.add_separator = True
        self.all_items = []
        self._completer = QCompleter([], self)
        self._completer.setCaseSensitivity(Qt.CaseSensitivity.CaseInsensitive)
        self._completer.setFilterMode(Qt.MatchFlag.MatchContains)
        self._completer.setWidget(self)
        self._completer.activated.connect(self._complete)
        self.textEdited.connect(self._edited)

    def update_items_cache(self, complete_items):
        from qt.core import QStringListModel

        self.all_items = list(complete_items)
        self._completer.setModel(QStringListModel(self.all_items))

    def set_separator(self, sep):
        self.sep = sep

    def set_space_before_sep(self, space_before):
        self.space_before_sep = space_before

    def set_add_separator(self, what):
        self.add_separator = bool(what)

    def _current_prefix(self):
        text = self.text()
        if not self.sep:
            return text, 0
        pos = self.cursorPosition()
        before = text[:pos]
        start = before.rfind(self.sep)
        return before[start + 1 :].lstrip(), (start + 1 if start >= 0 else 0)

    def _edited(self, text):
        prefix, _ = self._current_prefix()
        if prefix:
            self._completer.setCompletionPrefix(prefix)
            self._completer.complete()
        else:
            self._completer.popup().hide()

    def _complete(self, item):
        text = self.text()
        prefix, start = self._current_prefix()
        pos = self.cursorPosition()
        before = text[:start]
        if before and self.sep and not before.endswith(' ') and self.space_before_sep:
            before = before
        after = text[pos:]
        new = before + (' ' if before and not before.endswith(' ') and self.sep else '') + item
        if self.sep and self.add_separator:
            new += (' ' if self.space_before_sep else '') + self.sep + ' '
        self.setText(new + after)
        self.setCursorPosition(len(new))
        self.item_selected.emit(item)


class EditWithComplete(QComboBox):
    """An editable combo box whose line edit completes on items;
    `update_items_cache(items)` sets them, `set_separator` the list
    separator (None for a single value)."""

    item_selected = pyqtSignal(object)

    def __init__(self, *args, **kwargs):
        QComboBox.__init__(self, *args)
        self.setEditable(True)
        self.setInsertPolicy(QComboBox.InsertPolicy.NoInsert)
        self.setMinimumContentsLength(20)
        self.lineEdit().item_selected = self.item_selected if False else None
        self._line = LineEdit(self)
        self.setLineEdit(self._line)
        self._line.item_selected.connect(self.item_selected)
        self.setSizeAdjustPolicy(QComboBox.SizeAdjustPolicy.AdjustToMinimumContentsLengthWithIcon)

    def update_items_cache(self, complete_items):
        self._line.update_items_cache(complete_items)
        self.blockSignals(True)
        text = self.currentText()
        self.clear()
        self.addItems(sorted(complete_items, key=lambda x: x.lower()))
        self.setEditText(text)
        self.blockSignals(False)

    def set_separator(self, sep):
        self._line.set_separator(sep)

    def set_space_before_sep(self, space_before):
        self._line.set_space_before_sep(space_before)

    def set_add_separator(self, what):
        self._line.set_add_separator(what)

    def set_elide_mode(self, mode):
        pass

    @property
    def all_items(self):
        return self._line.all_items

    @all_items.setter
    def all_items(self, items):
        self.update_items_cache(items)

    def show_initial_value(self, what):
        self.setText(what)

    def setText(self, val):
        self.setEditText(val or '')
        self.lineEdit().setCursorPosition(0)

    def text(self):
        return str(self.lineEdit().text())

    def selectAll(self):
        self.lineEdit().selectAll()

    def setCursorPosition(self, *args):
        self.lineEdit().setCursorPosition(*args)

    def set_clear_button_enabled(self, enabled=True):
        self.lineEdit().setClearButtonEnabled(enabled)

    @property
    def textChanged(self):
        return self.lineEdit().textChanged

    def clear(self):
        self.lineEdit().setText('')
        QComboBox.clear(self)

    def clear_edit_text(self):
        self.lineEdit().setText('')

    def set_focus(self):
        self.lineEdit().setFocus(Qt.FocusReason.OtherFocusReason)


__all__ = ['EditWithComplete', 'LineEdit']
