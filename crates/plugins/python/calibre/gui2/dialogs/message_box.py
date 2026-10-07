"""Mirrors calibre's `calibre.gui2.dialogs.message_box` (calibre 9.15):
`MessageBox` (behind `error_dialog`, `info_dialog`, `question_dialog`),
`ViewLog`, `JobError` and `ErrorNotification`."""

import sys
import traceback

from calibre.constants import __version__
from calibre.gui2 import gprefs
from qt.core import (
    QApplication,
    QCheckBox,
    QDialog,
    QDialogButtonBox,
    QHBoxLayout,
    QIcon,
    QLabel,
    QPlainTextEdit,
    QPushButton,
    QSize,
    QSizePolicy,
    Qt,
    QTextBrowser,
    QVBoxLayout,
    pyqtSignal,
)


class Icon(QLabel):
    def __init__(self, parent=None, size=None):
        QLabel.__init__(self, parent)
        self.setSizePolicy(QSizePolicy.Policy.Fixed, QSizePolicy.Policy.Fixed)
        self.icon_size = size or 64

    def set_icon(self, qicon):
        self.setPixmap(qicon.pixmap(self.icon_size, self.icon_size))


class MessageBox(QDialog):
    ERROR = 0
    WARNING = 1
    INFO = 2
    QUESTION = 3

    resize_needed = pyqtSignal()

    def __init__(
        self,
        type_,
        title,
        msg,
        det_msg='',
        q_icon=None,
        show_copy_button=True,
        parent=None,
        default_yes=True,
        yes_text=None,
        no_text=None,
        yes_icon=None,
        no_icon=None,
        add_abort_button=False,
        only_copy_details=False,
    ):
        QDialog.__init__(self, parent)
        self.aborted = False
        if q_icon is None:
            icon = {self.ERROR: 'error', self.WARNING: 'warning', self.INFO: 'information', self.QUESTION: 'question'}[
                type_
            ]
            icon = 'dialog_%s.png' % icon
            self.icon = QIcon.ic(icon)
        elif isinstance(q_icon, QIcon):
            self.icon = q_icon
        else:
            self.icon = QIcon.ic(q_icon)
        self.only_copy_details = only_copy_details
        self.setWindowTitle(title)
        self.setWindowIcon(self.icon)
        l = QVBoxLayout(self)
        h = QHBoxLayout()
        l.addLayout(h)
        self.icon_widget = Icon(self)
        self.icon_widget.set_icon(self.icon)
        h.addWidget(self.icon_widget)
        self.msg = QLabel(msg, self)
        self.msg.setWordWrap(True)
        self.msg.setOpenExternalLinks(True)
        self.msg.setTextInteractionFlags(
            Qt.TextInteractionFlag.TextSelectableByMouse | Qt.TextInteractionFlag.LinksAccessibleByMouse
        )
        h.addWidget(self.msg, 10)
        self.det_msg = QPlainTextEdit(self)
        self.det_msg.setReadOnly(True)
        self.det_msg.setPlainText(det_msg or '')
        self.det_msg.setVisible(False)
        l.addWidget(self.det_msg)
        self.toggle_checkbox = QCheckBox(self)
        self.toggle_checkbox.setVisible(False)
        l.addWidget(self.toggle_checkbox)
        self.bb = QDialogButtonBox(self)
        l.addWidget(self.bb)
        self.bb.accepted.connect(self.accept)
        self.bb.rejected.connect(self.reject)
        self.msg_text = msg
        self.det_msg_text = det_msg or ''
        self.is_question = type_ == self.QUESTION
        if self.is_question:
            self.bb.setStandardButtons(QDialogButtonBox.StandardButton.Yes | QDialogButtonBox.StandardButton.No)
            self.bb.button(
                QDialogButtonBox.StandardButton.Yes if default_yes else QDialogButtonBox.StandardButton.No
            ).setDefault(True)
            self.default_yes = default_yes
            if yes_text is not None:
                self.bb.button(QDialogButtonBox.StandardButton.Yes).setText(yes_text)
            if no_text is not None:
                self.bb.button(QDialogButtonBox.StandardButton.No).setText(no_text)
            if yes_icon is not None:
                self.bb.button(QDialogButtonBox.StandardButton.Yes).setIcon(
                    yes_icon if isinstance(yes_icon, QIcon) else QIcon.ic(yes_icon)
                )
            if no_icon is not None:
                self.bb.button(QDialogButtonBox.StandardButton.No).setIcon(
                    no_icon if isinstance(no_icon, QIcon) else QIcon.ic(no_icon)
                )
        else:
            self.bb.setStandardButtons(QDialogButtonBox.StandardButton.Ok)
            self.bb.button(QDialogButtonBox.StandardButton.Ok).setDefault(True)
        if add_abort_button:
            self.abort_button = self.bb.addButton('Abort', QDialogButtonBox.ButtonRole.ActionRole)
            self.abort_button.clicked.connect(self.on_abort)
        if show_copy_button:
            self.ctc_button = self.bb.addButton('&Copy to clipboard', QDialogButtonBox.ButtonRole.ActionRole)
            self.ctc_button.clicked.connect(self.copy_to_clipboard)
        self.show_det_msg = '&Show details'
        self.hide_det_msg = '&Hide details'
        self.det_button = self.bb.addButton(self.show_det_msg, QDialogButtonBox.ButtonRole.ActionRole)
        self.det_button.setVisible(bool(det_msg))
        self.det_button.clicked.connect(self.toggle_det_msg)
        self.resize_needed.connect(self.do_resize, type=Qt.ConnectionType.QueuedConnection)
        self.do_resize()

    def on_abort(self):
        self.aborted = True
        self.reject()

    def sizeHint(self):
        ans = QDialog.sizeHint(self)
        ans.setWidth(max(min(ans.width(), 700), 450))
        return ans

    def toggle_det_msg(self, *args):
        vis = self.det_msg.isVisible()
        self.det_button.setText(self.show_det_msg if vis else self.hide_det_msg)
        self.det_msg.setVisible(not vis)
        self.do_resize()

    def do_resize(self):
        self.resize(self.sizeHint())

    def copy_to_clipboard(self, *args):
        text = (
            self.det_msg_text
            if self.only_copy_details
            else f'{self.windowTitle()}: {self.msg_text}\n\n{self.det_msg_text}'
        )
        QApplication.clipboard().setText(text)
        if hasattr(self, 'ctc_button'):
            self.ctc_button.setText('Copied')

    def showEvent(self, ev):
        ret = QDialog.showEvent(self, ev)
        if self.is_question:
            try:
                self.bb.button(
                    QDialogButtonBox.StandardButton.Yes if self.default_yes else QDialogButtonBox.StandardButton.No
                ).setFocus(Qt.FocusReason.OtherFocusReason)
            except Exception:
                pass
        else:
            self.bb.button(QDialogButtonBox.StandardButton.Ok).setFocus(Qt.FocusReason.OtherFocusReason)
        return ret

    def set_details(self, msg):
        if not msg:
            msg = ''
        self.det_msg.setPlainText(msg)
        self.det_msg_text = msg
        self.det_button.setVisible(bool(msg))
        self.det_msg.setVisible(False)
        self.det_button.setText(self.show_det_msg)
        self.do_resize()


class ViewLog(QDialog):
    def __init__(self, title, html, parent=None, unique_name=None):
        QDialog.__init__(self, parent)
        self.l = l = QVBoxLayout(self)
        self.tb = QTextBrowser(self)
        self.tb.setHtml('<pre style="font-family: monospace">%s</pre>' % html if '<' not in html[:100] else html)
        l.addWidget(self.tb)
        self.bb = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok)
        self.bb.accepted.connect(self.accept)
        self.bb.rejected.connect(self.reject)
        self.copy_button = self.bb.addButton('Copy to clipboard', QDialogButtonBox.ButtonRole.ActionRole)
        self.copy_button.setIcon(QIcon.ic('edit-copy.png'))
        self.copy_button.clicked.connect(self.copy_to_clipboard)
        l.addWidget(self.bb)
        self.unique_name = unique_name or 'view-log-dialog'
        self.finished.connect(self.dialog_closing)
        self.resize(QSize(800, 600))
        geom = gprefs.get(self.unique_name, None)
        if geom:
            try:
                self.restoreGeometry(bytes(geom))
            except Exception:
                pass
        self.setModal(False)
        self.setWindowTitle(title)
        self.setWindowIcon(QIcon.ic('debug.png'))
        self.show()

    def copy_to_clipboard(self):
        txt = self.tb.toPlainText()
        QApplication.clipboard().setText(txt)

    def dialog_closing(self, result):
        gprefs[self.unique_name] = bytearray(self.saveGeometry())


class JobError(QDialog):
    """calibre's "job failed" dialog: one message at a time, queued."""

    WIDTH = 600
    do_pop = pyqtSignal()

    def __init__(self, parent):
        QDialog.__init__(self, parent)
        self.queue = []
        self.do_pop.connect(self.pop, type=Qt.ConnectionType.QueuedConnection)
        self._layout = l = QVBoxLayout(self)
        h = QHBoxLayout()
        l.addLayout(h)
        self.icon = QIcon.ic('dialog_error.png')
        self.setWindowIcon(self.icon)
        self.icon_label = Icon(self)
        self.icon_label.set_icon(self.icon)
        h.addWidget(self.icon_label)
        self.msg_label = QLabel('<p>&nbsp;')
        self.msg_label.setWordWrap(True)
        self.msg_label.setTextFormat(Qt.TextFormat.RichText)
        h.addWidget(self.msg_label, 10)
        self.det_msg = QPlainTextEdit(self)
        self.det_msg.setVisible(False)
        l.addWidget(self.det_msg)
        self.bb = QDialogButtonBox(QDialogButtonBox.StandardButton.Close, parent=self)
        self.bb.accepted.connect(self.accept)
        self.bb.rejected.connect(self.reject)
        self.ctc_button = self.bb.addButton('&Copy to clipboard', QDialogButtonBox.ButtonRole.ActionRole)
        self.ctc_button.clicked.connect(self.copy_to_clipboard)
        self.show_det_msg = '&Show details'
        self.hide_det_msg = '&Hide details'
        self.det_button = self.bb.addButton(self.show_det_msg, QDialogButtonBox.ButtonRole.ActionRole)
        self.det_button.clicked.connect(self.toggle_det_msg)
        l.addWidget(self.bb)
        self.setModal(False)
        self.base_height = max(200, self.sizeHint().height() + 20)
        self.do_resize()

    def copy_to_clipboard(self, *args):
        d = QApplication.clipboard()
        d.setText(
            'calibre, version %s\n%s: %s\n\n%s'
            % (__version__, str(self.windowTitle()), str(self.msg_label.text()), str(self.det_msg.toPlainText()))
        )
        if hasattr(self, 'ctc_button'):
            self.ctc_button.setText('Copied')

    def toggle_det_msg(self, *args):
        vis = self.det_msg.isVisible()
        self.det_button.setText(self.show_det_msg if vis else self.hide_det_msg)
        self.det_msg.setVisible(not vis)
        self.do_resize()

    def do_resize(self):
        h = self.base_height
        if self.det_msg.isVisible():
            h += 250
        self.resize(QSize(self.WIDTH, h))

    def showEvent(self, ev):
        ret = QDialog.showEvent(self, ev)
        self.bb.button(QDialogButtonBox.StandardButton.Close).setFocus(Qt.FocusReason.OtherFocusReason)
        return ret

    def show_error(self, title, msg, det_msg='', retry_func=None):
        self.queue.append((title, msg, det_msg, retry_func))
        self.update_suppress_state()
        self.pop()

    def update_suppress_state(self):
        pass

    def pop(self):
        if not self.queue or self.isVisible():
            return
        title, msg, det_msg, retry_func = self.queue.pop(0)
        self.setWindowTitle(title)
        self.msg_label.setText(msg)
        self.det_msg.setPlainText(det_msg)
        self.det_msg.setVisible(False)
        self.det_button.setText(self.show_det_msg)
        self.det_button.setVisible(bool(det_msg))
        self.do_resize()
        self.show()

    def done(self, r):
        QDialog.done(self, r)
        self.do_pop.emit()


class ErrorNotification(MessageBox):
    def __init__(self, html_log, log_viewer_title, title, msg, det_msg='', show_copy_button=False, parent=None):
        MessageBox.__init__(
            self, MessageBox.ERROR, title, msg, det_msg=det_msg, show_copy_button=show_copy_button, parent=parent
        )
        self.html_log = html_log
        self.log_viewer_title = log_viewer_title
        self.finished.connect(self.show_log if False else lambda *a: None)
        self.vlb = self.bb.addButton('View log', QDialogButtonBox.ButtonRole.ActionRole)
        self.vlb.setIcon(QIcon.ic('debug.png'))
        self.vlb.clicked.connect(self.show_log)
        self.det_msg.setVisible(False)
        self.setModal(False)

    def show_log(self):
        self.log_viewer = ViewLog(self.log_viewer_title, self.html_log, parent=self)


def show_error_notification(*a, **k):
    pass


__all__ = [
    'MessageBox',
    'ViewLog',
    'JobError',
    'ErrorNotification',
    'Icon',
    'show_error_notification',
    'sys',
    'traceback',
    'QPushButton',
]
