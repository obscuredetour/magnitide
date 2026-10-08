"""A GUI (interface action) plugin for Magnitide's plugin host tests. It does
what real plugins do: edits metadata through both of calibre's database APIs,
writes into a book file in place, sets a cover, adds a book, reports through
the status bar and opens a dialog."""

from calibre.customize import InterfaceActionBase


class TestActionBase(InterfaceActionBase):
    name = 'Magnitide Test Action'
    description = 'Changes the selected books in ways real plugins do'
    author = 'Magnitide'
    version = (2, 0, 0)
    minimum_calibre_version = (5, 0, 0)
    supported_platforms = ['windows', 'osx', 'linux']
    actual_plugin = 'calibre_plugins.magnitide_test_action.action:TestAction'

    def is_customizable(self):
        return True

    def config_widget(self):
        from qt.core import QLabel, QLineEdit, QVBoxLayout, QWidget

        # As real plugins do: the settings widget reaches the main window
        # through the loaded action, so the action must be loaded by now.
        gui = self.actual_plugin_.gui
        shortcuts = len(gui.keyboard.shortcuts)
        w = QWidget()
        layout = QVBoxLayout(w)
        layout.addWidget(QLabel(f'{shortcuts} shortcuts registered', w))
        w.greeting = QLineEdit(w)
        layout.addWidget(w.greeting)
        return w

    def save_settings(self, config_widget):
        from calibre.utils.config import JSONConfig

        prefs = JSONConfig('plugins/magnitide_test_action')
        prefs['greeting'] = config_widget.greeting.text()
        if self.actual_plugin_ is not None:
            self.actual_plugin_.apply_settings()
