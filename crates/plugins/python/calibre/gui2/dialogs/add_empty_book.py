"""Mirrors calibre's `calibre.gui2.dialogs.add_empty_book`: how many empty
books to add, by which authors (and series)."""

from calibre.ebooks.metadata import string_to_authors
from qt.core import QDialog, QDialogButtonBox, QFormLayout, QLineEdit, QSpinBox, QVBoxLayout


class AddEmptyBookDialog(QDialog):
    def __init__(self, parent, db, author, series=None, title=None, dup_title=None, dup_authors=None):
        QDialog.__init__(self, parent)
        self.db = db
        self.setWindowTitle(_('How many empty books?'))
        layout = QVBoxLayout(self)
        form = QFormLayout()
        layout.addLayout(form)
        self.qty_spinbox = QSpinBox(self)
        self.qty_spinbox.setRange(1, 10000)
        self.qty_spinbox.setValue(1)
        form.addRow(_('&Number of empty books to add:'), self.qty_spinbox)
        self.authors_combo = QLineEdit(self)
        self.authors_combo.setText(author or '')
        form.addRow(_('Set the author of the new books to:'), self.authors_combo)
        self.series_combo = QLineEdit(self)
        self.series_combo.setText(series or '')
        form.addRow(_('Set the series of the new books to:'), self.series_combo)
        self.title_edit = QLineEdit(self)
        self.title_edit.setText(title or '')
        form.addRow(_('Set the title of the new books to:'), self.title_edit)
        bb = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel, self)
        bb.accepted.connect(self.accept)
        bb.rejected.connect(self.reject)
        layout.addWidget(bb)
        self.resize(self.sizeHint())

    def exec_(self):
        return self.exec()

    @property
    def qty_to_add(self):
        return self.qty_spinbox.value()

    @property
    def selected_authors(self):
        return string_to_authors(self.authors_combo.text().strip()) or [_('Unknown')]

    @property
    def selected_series(self):
        return self.series_combo.text().strip() or None

    @property
    def selected_title(self):
        return self.title_edit.text().strip() or None

    @property
    def selected_format(self):
        return None
