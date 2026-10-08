"""Mirrors the name of calibre's device-deletion dialog; Magnitide manages
devices itself, so there is never a connected calibre device to match."""

from qt.core import QDialog, QDialogButtonBox, QLabel, QVBoxLayout


class DeleteMatchingFromDeviceDialog(QDialog):
    def __init__(self, parent, items):
        QDialog.__init__(self, parent)
        self.setWindowTitle(_('Delete from device'))
        self.result = []
        layout = QVBoxLayout(self)
        layout.addWidget(QLabel(_('No calibre device is connected: Magnitide manages devices itself.'), self))
        bb = QDialogButtonBox(QDialogButtonBox.StandardButton.Close, self)
        bb.rejected.connect(self.reject)
        bb.accepted.connect(self.reject)
        layout.addWidget(bb)

    def exec_(self):
        return self.exec()
