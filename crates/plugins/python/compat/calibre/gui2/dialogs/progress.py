"""Mirrors calibre's `calibre.gui2.dialogs.progress`: `ProgressDialog` and
`BlockingBusy`."""

from qt.core import (
    QDialog,
    QDialogButtonBox,
    QIcon,
    QLabel,
    QProgressBar,
    QSize,
    Qt,
    QVBoxLayout,
    pyqtSignal,
)


class ProgressDialog(QDialog):
    canceled_signal = pyqtSignal()

    def __init__(self, title, msg='', min=0, max=99, parent=None, cancelable=True, icon=None):
        QDialog.__init__(self, parent)
        if icon is None:
            icon = QIcon.ic('lt.png')
        elif isinstance(icon, str):
            icon = QIcon.ic(icon)
        self.setWindowTitle(title)
        self.setWindowIcon(icon)
        l = QVBoxLayout(self)
        self.title = QLabel(f'<b>{title}</b>', self)
        l.addWidget(self.title)
        self.message = QLabel(msg, self)
        self.message.setWordWrap(True)
        l.addWidget(self.message)
        self.bar = QProgressBar(self)
        self.bar.setRange(min, max)
        self.bar.setValue(min)
        l.addWidget(self.bar)
        self.button_box = QDialogButtonBox(QDialogButtonBox.StandardButton.Abort, self)
        self.button_box.rejected.connect(self._canceled)
        l.addWidget(self.button_box)
        self.setModal(True)
        self.setWindowModality(Qt.WindowModality.ApplicationModal)
        self.canceled = False
        if not cancelable:
            self.button_box.setVisible(False)
        self.cancelable = cancelable
        self.resize(self.sizeHint().expandedTo(QSize(400, 100)))

    def set_msg(self, msg=''):
        self.message.setText(msg)

    def set_value(self, val):
        self.bar.setValue(val)

    @property
    def value(self):
        return self.bar.value()

    @value.setter
    def value(self, val):
        self.bar.setValue(val)

    def set_min(self, min):
        self.bar.setMinimum(min)

    def set_max(self, max):
        self.bar.setMaximum(max)

    @property
    def max(self):
        return self.bar.maximum()

    @max.setter
    def max(self, val):
        self.bar.setMaximum(val)

    @property
    def min(self):
        return self.bar.minimum()

    @min.setter
    def min(self, val):
        self.bar.setMinimum(val)

    @property
    def msg(self):
        return self.message.text()

    @msg.setter
    def msg(self, val):
        self.message.setText(val)

    def _canceled(self, *args):
        self.canceled = True
        self.button_box.setDisabled(True)
        self.title.setText('Aborting...')
        self.canceled_signal.emit()

    def reject(self):
        if not self.cancelable:
            return
        QDialog.reject(self)

    def keyPressEvent(self, ev):
        if ev.key() == Qt.Key.Key_Escape:
            if self.cancelable:
                self._canceled()
        else:
            QDialog.keyPressEvent(self, ev)


class BlockingBusy(QDialog):
    def __init__(self, msg, parent=None, window_title='Working'):
        QDialog.__init__(self, parent)
        self._layout = QVBoxLayout()
        self.setLayout(self._layout)
        self.msg = QLabel(msg)
        self._layout.addWidget(self.msg, 0, Qt.AlignmentFlag.AlignHCenter)
        self.setWindowTitle(window_title)
        self.resize(self.sizeHint())
        self.setModal(True)

    def start(self):
        self.show()

    def stop(self):
        self.hide()

    def accept(self):
        self.stop()
        return QDialog.accept(self)

    def reject(self):
        pass  # Cannot cancel this dialog


__all__ = ['ProgressDialog', 'BlockingBusy']
