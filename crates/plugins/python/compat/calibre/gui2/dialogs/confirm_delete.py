"""Mirrors calibre's `calibre.gui2.dialogs.confirm_delete`: a question with
a "Show this confirmation again" box, remembered in `dynamic`."""

from calibre import confirm_config_name
from calibre.gui2 import dynamic
from qt.core import QCheckBox, QDialog, QDialogButtonBox, QHBoxLayout, QIcon, QLabel, QSize, Qt, QVBoxLayout


class Dialog(QDialog):
    def __init__(
        self,
        msg,
        name,
        parent,
        config_set=dynamic,
        icon='dialog_warning.png',
        title=None,
        confirm_msg=None,
        show_cancel_button=True,
        extra_button=None,
        extra_button_choice=None,
    ):
        QDialog.__init__(self, parent)
        self.setWindowTitle(title or 'Are you sure?')
        self.setWindowIcon(QIcon.ic(icon))
        self.extra_button_choice = extra_button_choice
        self.choice = None
        l = QVBoxLayout(self)
        h = QHBoxLayout()
        l.addLayout(h)
        il = QLabel(self)
        il.setPixmap(QIcon.ic(icon).pixmap(64, 64))
        h.addWidget(il)
        self.msg = QLabel(msg, self)
        self.msg.setWordWrap(True)
        self.msg.setOpenExternalLinks(True)
        h.addWidget(self.msg, 10)
        self.again = QCheckBox(confirm_msg or 'Show this confirmation again', self)
        self.again.setChecked(True)
        self.again.stateChanged.connect(self.toggle)
        l.addWidget(self.again)
        self.bb = QDialogButtonBox(self)
        self.bb.setStandardButtons(
            QDialogButtonBox.StandardButton.Ok
            | (
                QDialogButtonBox.StandardButton.Cancel
                if show_cancel_button
                else QDialogButtonBox.StandardButton.NoButton
            )
        )
        self.bb.accepted.connect(self.accept)
        self.bb.rejected.connect(self.reject)
        if extra_button:
            b = self.bb.addButton(extra_button, QDialogButtonBox.ButtonRole.AcceptRole)
            b.clicked.connect(self.extra_clicked)
        l.addWidget(self.bb)
        self.name = name
        self.config_set = config_set
        self.resize(self.sizeHint().expandedTo(QSize(450, 100)))

    def extra_clicked(self):
        self.choice = self.extra_button_choice

    def toggle(self, *args):
        self.config_set[confirm_config_name(self.name)] = self.again.isChecked()


def confirm(
    msg,
    name,
    parent=None,
    pixmap='dialog_warning.png',
    title=None,
    show_cancel_button=True,
    confirm_msg=None,
    config_set=None,
    extra_button=None,
    extra_button_choice=None,
):
    """Ask, unless the user turned the question off before. Returns True
    for OK (or the `extra_button_choice` when that button was pressed)."""
    config_set = config_set or dynamic
    if not config_set.get(confirm_config_name(name), True):
        return True
    d = Dialog(
        msg,
        name,
        parent,
        config_set=config_set,
        icon=pixmap,
        title=title,
        confirm_msg=confirm_msg,
        show_cancel_button=show_cancel_button,
        extra_button=extra_button,
        extra_button_choice=extra_button_choice,
    )
    ok = d.exec() == QDialog.DialogCode.Accepted
    if ok and d.choice is not None:
        return d.choice
    return ok


__all__ = ['confirm', 'Dialog', 'Qt']
