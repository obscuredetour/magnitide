# Derived from calibre's src/calibre/gui2/library/delegates.py (calibre 9.15).
# Copyright: 2010, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.gui2.library.delegates`: the item delegates
plugins subclass for their own tables (dates, ratings, text). The editors
are plain Qt ones; calibre's extras (completion, template editing) aren't
here."""

from calibre.utils.date import UNDEFINED_DATE, format_date, is_date_undefined, now, qt_to_dt
from qt.core import QDateTime, QDateTimeEdit, QDoubleSpinBox, QLineEdit, QSpinBox, QStyledItemDelegate, Qt


class DateDelegate(QStyledItemDelegate):
    def __init__(self, parent, tableview=None):
        QStyledItemDelegate.__init__(self, parent)
        self.table_widget = tableview
        self.format = 'dd MMM yyyy'

    def displayText(self, val, locale):
        if isinstance(val, QDateTime):
            val = qt_to_dt(val)
        if val is None or is_date_undefined(val):
            return ''
        try:
            return format_date(val, self.format)
        except Exception:
            return str(val)

    def createEditor(self, parent, option, index):
        qde = QDateTimeEdit(parent)
        qde.setDisplayFormat(self.format)
        qde.setMinimumDateTime(QDateTime(UNDEFINED_DATE.year, UNDEFINED_DATE.month, UNDEFINED_DATE.day, 0, 0, 0))
        qde.setSpecialValueText(_('Undefined'))
        qde.setCalendarPopup(True)
        return qde

    def setEditorData(self, editor, index):
        val = index.data(Qt.ItemDataRole.EditRole)
        if val is None or (not isinstance(val, QDateTime) and is_date_undefined(val)):
            editor.setDateTime(editor.minimumDateTime())
        elif isinstance(val, QDateTime):
            editor.setDateTime(val)
        else:
            editor.setDateTime(QDateTime(val))

    def setModelData(self, editor, model, index):
        val = editor.dateTime()
        if val == editor.minimumDateTime():
            val = None
        model.setData(index, val, Qt.ItemDataRole.EditRole)


class PubDateDelegate(DateDelegate):
    pass


class CcDateDelegate(DateDelegate):
    def set_format(self, fmt):
        self.format = fmt or 'dd MMM yyyy'


class RatingDelegate(QStyledItemDelegate):
    def __init__(self, parent, is_half_star=False):
        QStyledItemDelegate.__init__(self, parent)
        self.is_half_star = is_half_star

    def displayText(self, val, locale):
        try:
            val = float(val or 0)
        except Exception:
            return ''
        if val <= 0:
            return ''
        stars = val / 2 if val > 5 else val
        return '★' * int(stars) + ('½' if self.is_half_star and stars - int(stars) >= 0.5 else '')

    def createEditor(self, parent, option, index):
        sb = QDoubleSpinBox(parent) if self.is_half_star else QSpinBox(parent)
        sb.setRange(0, 5)
        if self.is_half_star:
            sb.setSingleStep(0.5)
        return sb


class TextDelegate(QStyledItemDelegate):
    def __init__(self, parent, *args, **kwargs):
        QStyledItemDelegate.__init__(self, parent)
        self.auto_complete_function = None

    def set_auto_complete_function(self, f):
        self.auto_complete_function = f

    def createEditor(self, parent, option, index):
        editor = QLineEdit(parent)
        if self.auto_complete_function is not None:
            try:
                from qt.core import QCompleter

                editor.setCompleter(QCompleter(list(self.auto_complete_function()), editor))
            except Exception:
                pass
        return editor


class CompleteDelegate(TextDelegate):
    def __init__(self, parent, sep=',', items_func_name=None, space_before_sep=False):
        TextDelegate.__init__(self, parent)
        self.sep = sep
        self.items_func_name = items_func_name
        self.space_before_sep = space_before_sep


class LanguagesDelegate(TextDelegate):
    pass


class SeriesDelegate(TextDelegate):
    pass


class CcTextDelegate(TextDelegate):
    pass


class CcSeriesDelegate(TextDelegate):
    pass


class CcLongTextDelegate(TextDelegate):
    pass


class CcMarkdownDelegate(TextDelegate):
    pass


class CcCommentsDelegate(TextDelegate):
    pass


class CcTemplateDelegate(TextDelegate):
    pass


class CcNumberDelegate(QStyledItemDelegate):
    def createEditor(self, parent, option, index):
        sb = QDoubleSpinBox(parent)
        sb.setRange(-1e9, 1e9)
        return sb


class CcBoolDelegate(QStyledItemDelegate):
    pass


class CcEnumDelegate(QStyledItemDelegate):
    pass


__all__ = [
    'DateDelegate',
    'PubDateDelegate',
    'CcDateDelegate',
    'RatingDelegate',
    'TextDelegate',
    'CompleteDelegate',
    'LanguagesDelegate',
    'SeriesDelegate',
    'CcTextDelegate',
    'CcSeriesDelegate',
    'CcLongTextDelegate',
    'CcMarkdownDelegate',
    'CcCommentsDelegate',
    'CcTemplateDelegate',
    'CcNumberDelegate',
    'CcBoolDelegate',
    'CcEnumDelegate',
    'now',
]
