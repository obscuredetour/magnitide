"""Mirrors calibre's `calibre.gui2.dialogs.choose_format`: pick one of a
book's formats."""

from qt.core import QDialog, QDialogButtonBox, QLabel, QListWidget, QListWidgetItem, QVBoxLayout


class ChooseFormatDialog(QDialog):
    def __init__(self, window, msg, formats, show_open_with=False):
        QDialog.__init__(self, window)
        self.setWindowTitle('Choose format')
        l = QVBoxLayout(self)
        self.msg = QLabel(msg, self)
        l.addWidget(self.msg)
        self.formats = QListWidget(self)
        for fmt in formats:
            QListWidgetItem(fmt.upper(), self.formats)
        self.formats.setCurrentRow(0)
        self.formats.itemDoubleClicked.connect(lambda item: self.accept())
        l.addWidget(self.formats)
        self.bb = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel, self)
        self.bb.accepted.connect(self.accept)
        self.bb.rejected.connect(self.reject)
        l.addWidget(self.bb)
        self._formats = list(formats)
        self._format = None

    def accept(self):
        row = self.formats.currentRow()
        if 0 <= row < len(self._formats):
            self._format = self._formats[row]
        QDialog.accept(self)

    def format(self):
        return self._format

    @property
    def open_with_format(self):
        return None
