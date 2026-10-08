"""Mirrors calibre's `TemplateDialog` as plugins use it: a dialog to edit a
template, with `rule` (its result) and the text box. calibre's dialog has a
function reference and a live preview; here the template is checked
against the book given, if any."""

from qt.core import QDialog, QDialogButtonBox, QLabel, QPlainTextEdit, QVBoxLayout


class TemplateDialog(QDialog):
    def __init__(
        self,
        parent,
        text,
        mi=None,
        fm=None,
        color_field=None,
        icon_field_key=None,
        icon_rule_kind=None,
        doing_emblem=False,
        text_is_placeholder=False,
        dialog_is_st_editor=False,
        global_vars=None,
        all_functions=None,
        builtin_functions=None,
        python_context_object=None,
        dialog_number=None,
        formatter=None,
        icon_dir='cc_icons',
    ):
        QDialog.__init__(self, parent)
        self.setWindowTitle(_('Edit template'))
        self.mi = mi
        self.fm = fm
        self.rule = (None, '')
        layout = QVBoxLayout(self)
        layout.addWidget(QLabel(_('Template:'), self))
        self.textbox = QPlainTextEdit(self)
        if text_is_placeholder:
            self.textbox.setPlaceholderText(text or '')
        else:
            self.textbox.setPlainText(text or '')
        layout.addWidget(self.textbox)
        self.template_value = QLabel('', self)
        self.template_value.setWordWrap(True)
        layout.addWidget(self.template_value)
        self.textbox.textChanged.connect(self.text_changed)
        bb = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel, self)
        bb.accepted.connect(self.accept)
        bb.rejected.connect(self.reject)
        layout.addWidget(bb)
        self.resize(600, 400)
        self.text_changed()

    def text_changed(self):
        if not self.mi:
            return
        from calibre.utils.formatter import validation_formatter

        try:
            mi = self.mi[0] if isinstance(self.mi, (list, tuple)) else self.mi
            self.template_value.setText(
                validation_formatter.safe_format(self.textbox.toPlainText(), mi, _('EXCEPTION:'), mi)
            )
        except Exception as e:
            self.template_value.setText(_('Error: %s') % e)

    def accept(self):
        self.rule = (None, self.textbox.toPlainText())
        QDialog.accept(self)

    def exec_(self):
        return self.exec()


class EmbeddedTemplateDialog(TemplateDialog):
    pass


__all__ = ['TemplateDialog', 'EmbeddedTemplateDialog']
