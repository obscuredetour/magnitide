# Derived from calibre's src/calibre/gui2/comments_editor.py (calibre 9.15).
# Copyright: 2010, Kovid Goyal <kovid at kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.gui2.comments_editor`: the rich-text editor
for a book's comments. `EditorWidget` is a QTextEdit with calibre's `html`
property and the `plugin_actions` hook (Comments Cleaner adds a toolbar
button through it); `Editor` wraps it with a small toolbar. calibre's
editor has more formatting tools; the HTML it produces is Qt's."""

from qt.core import QAction, QKeySequence, QTextEdit, QToolBar, QVBoxLayout, QWidget, pyqtSignal


class EditorWidget(QTextEdit):
    data_changed = pyqtSignal()
    #: Callables taking the widget; each gets a toolbar action (plugins add their own).
    plugin_actions = set()

    def __init__(self, parent=None):
        QTextEdit.__init__(self, parent)
        self.setAcceptRichText(True)
        self.base_url = None
        self.textChanged.connect(self.data_changed)

    @property
    def html(self):
        from calibre.library.comments import sanitize_comments_html

        raw = self.toHtml()
        body = raw
        start, end = raw.find('<body'), raw.rfind('</body>')
        if start != -1 and end != -1:
            body = raw[raw.find('>', start) + 1 : end]
        if not self.toPlainText().strip():
            return ''
        return sanitize_comments_html(body.strip())

    @html.setter
    def html(self, val):
        self.setHtml(val or '')

    def set_html(self, val, allow_undo=True):
        self.html = val

    def set_base_url(self, qurl):
        self.base_url = qurl

    def smarten_punctuation(self):
        from calibre.ebooks.conversion.preprocess import smarten_punctuation

        self.html = smarten_punctuation(self.html)

    def remove_format(self):
        self.setPlainText(self.toPlainText())

    def action_for(self, name):
        return getattr(self, 'action_' + name, None)


class Editor(QWidget):
    def __init__(self, parent=None, one_line_toolbar=False, toolbar_prefs_name=None):
        QWidget.__init__(self, parent)
        self.toolbar_prefs_name = toolbar_prefs_name
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        self.toolbar1 = QToolBar(self)
        self.toolbar2 = QToolBar(self)
        self.toolbar3 = QToolBar(self)
        self.editor = EditorWidget(self)
        layout.addWidget(self.toolbar1)
        layout.addWidget(self.editor)
        self.tabs = None
        for name, text, fn in (
            ('bold', _('Bold'), 'bold'),
            ('italic', _('Italic'), 'italic'),
            ('underline', _('Underline'), 'underline'),
        ):
            ac = QAction(text, self)
            ac.setCheckable(True)
            ac.triggered.connect(getattr(self, '_' + fn))
            setattr(self.editor, 'action_' + name, ac)
            self.toolbar1.addAction(ac)
        for action in list(EditorWidget.plugin_actions):
            try:
                action(self.editor)
            except Exception:
                import traceback

                traceback.print_exc()
        if one_line_toolbar:
            self.toolbar2.hide()
            self.toolbar3.hide()

    def _bold(self, checked):
        from qt.core import QFont

        self.editor.setFontWeight(QFont.Weight.Bold if checked else QFont.Weight.Normal)

    def _italic(self, checked):
        self.editor.setFontItalic(checked)

    def _underline(self, checked):
        self.editor.setFontUnderline(checked)

    @property
    def html(self):
        return self.editor.html

    @html.setter
    def html(self, v):
        self.editor.html = v

    def set_html(self, v, allow_undo=True):
        self.editor.set_html(v, allow_undo=allow_undo)

    def change_tab(self, index):
        pass

    def set_base_url(self, qurl):
        self.editor.set_base_url(qurl)

    def set_minimum_height_for_editor(self, val):
        self.editor.setMinimumHeight(val)

    def hide_toolbars(self):
        self.toolbar1.hide()
        self.toolbar2.hide()
        self.toolbar3.hide()

    def show_toolbars(self):
        self.toolbar1.show()

    def toggle_toolbars(self):
        if self.toolbar1.isVisible():
            self.hide_toolbars()
        else:
            self.show_toolbars()

    def hide_tabs(self):
        pass

    def smarten_punctuation(self):
        self.editor.smarten_punctuation()

    @property
    def toolbars_visible(self):
        return self.toolbar1.isVisible()

    def sizeHint(self):
        from qt.core import QSize

        return QSize(600, 300)


__all__ = ['Editor', 'EditorWidget', 'QKeySequence']
