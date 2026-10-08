"""Mirrors calibre's `calibre.gui2.proceed`: the question calibre shows when
a job ends with something to decide ("Proceed?" with a log). In calibre it
is an overlay inside the main window; here, a standalone dialog with the
same signature (`gui.proceed_question(...)`)."""

from calibre.gui2 import gprefs
from qt.core import (
    QCheckBox,
    QDialog,
    QDialogButtonBox,
    QHBoxLayout,
    QIcon,
    QLabel,
    QPlainTextEdit,
    QPushButton,
    QSize,
    Qt,
    QVBoxLayout,
)


class ProceedQuestion(QDialog):
    def __init__(
        self,
        parent,
        callback,
        payload,
        html_log,
        log_viewer_title,
        title,
        msg,
        det_msg='',
        show_copy_button=False,
        cancel_callback=None,
        log_is_file=False,
        checkbox_msg=None,
        checkbox_checked=False,
        action_callback=None,
        action_label=None,
        action_icon=None,
        show_det=False,
        show_ok=False,
        icon=None,
    ):
        QDialog.__init__(self, parent)
        self.setWindowTitle(title)
        self.callback, self.cancel_callback, self.action_callback = callback, cancel_callback, action_callback
        self.payload = payload
        self.html_log, self.log_viewer_title, self.log_is_file = html_log, log_viewer_title, log_is_file
        self.setModal(False)
        v = QVBoxLayout(self)
        h = QHBoxLayout()
        v.addLayout(h)
        il = QLabel(self)
        il.setPixmap((icon if isinstance(icon, QIcon) else QIcon.ic(icon or 'dialog_question.png')).pixmap(48, 48))
        h.addWidget(il)
        self.msg = QLabel(msg, self)
        self.msg.setWordWrap(True)
        self.msg.setOpenExternalLinks(True)
        h.addWidget(self.msg, 10)
        self.checkbox = QCheckBox(checkbox_msg or '', self)
        self.checkbox.setChecked(bool(checkbox_checked))
        self.checkbox.setVisible(bool(checkbox_msg))
        v.addWidget(self.checkbox)
        self.det = QPlainTextEdit(self)
        self.det.setReadOnly(True)
        self.det.setPlainText(det_msg or '')
        self.det.setVisible(bool(det_msg) and show_det)
        v.addWidget(self.det)
        self.bb = QDialogButtonBox(self)
        self.bb.accepted.connect(self.accept)
        self.bb.rejected.connect(self.reject)
        if show_ok:
            self.bb.setStandardButtons(QDialogButtonBox.StandardButton.Ok)
        else:
            self.bb.setStandardButtons(QDialogButtonBox.StandardButton.Yes | QDialogButtonBox.StandardButton.No)
        if det_msg and not show_det:
            self.det_button = QPushButton('Show &details', self)
            self.det_button.setCheckable(True)
            self.det_button.toggled.connect(self.det.setVisible)
            self.bb.addButton(self.det_button, QDialogButtonBox.ButtonRole.ActionRole)
        if html_log:
            self.log_button = QPushButton('View log', self)
            self.log_button.clicked.connect(self.show_log)
            self.bb.addButton(self.log_button, QDialogButtonBox.ButtonRole.ActionRole)
        if action_callback is not None:
            self.action_button = QPushButton(action_label or 'Action', self)
            if action_icon:
                self.action_button.setIcon(action_icon if isinstance(action_icon, QIcon) else QIcon.ic(action_icon))
            self.action_button.clicked.connect(self.do_action)
            self.bb.addButton(self.action_button, QDialogButtonBox.ButtonRole.ActionRole)
        if show_copy_button:
            self.copy_button = QPushButton('&Copy to clipboard', self)
            self.copy_button.clicked.connect(self.copy)
            self.bb.addButton(self.copy_button, QDialogButtonBox.ButtonRole.ActionRole)
        v.addWidget(self.bb)
        self.resize(self.sizeHint().expandedTo(QSize(500, 150)))

    def copy(self):
        from qt.core import QApplication

        QApplication.clipboard().setText(self.msg.text() + '\n\n' + self.det.toPlainText())

    def show_log(self):
        from calibre.gui2.dialogs.message_box import ViewLog

        html = self.html_log
        if self.log_is_file:
            with open(html, 'rb') as f:
                html = f.read().decode('utf-8', 'replace')
        ViewLog(self.log_viewer_title, html, parent=self).exec()

    def do_action(self):
        self.action_callback(self.payload)
        self.accept_without_callback()

    def accept_without_callback(self):
        self._done = True
        QDialog.accept(self)

    def accept(self):
        if getattr(self, '_done', False):
            return QDialog.accept(self)
        self._done = True
        self.payload_checked = self.checkbox.isChecked()
        try:
            if self.checkbox.isVisible():
                try:
                    self.callback(self.payload, self.checkbox.isChecked())
                except TypeError:
                    self.callback(self.payload)
            else:
                self.callback(self.payload)
        finally:
            QDialog.accept(self)

    def reject(self):
        if getattr(self, '_done', False):
            return QDialog.reject(self)
        self._done = True
        try:
            if self.cancel_callback is not None:
                self.cancel_callback(self.payload)
        finally:
            QDialog.reject(self)


def ask(parent, callback, payload, html_log, log_viewer_title, title, msg, **kw):
    """Show the question (modeless: the plugin's work goes on meanwhile,
    as calibre's does)."""
    d = ProceedQuestion(parent, callback, payload, html_log, log_viewer_title, title, msg, **kw)
    d.setAttribute(Qt.WidgetAttribute.WA_DeleteOnClose)
    d.show()
    d.raise_()
    d.activateWindow()
    return d


__all__ = ['ProceedQuestion', 'ask', 'gprefs']
