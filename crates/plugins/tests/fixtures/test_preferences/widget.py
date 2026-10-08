from calibre.gui2.preferences import ConfigWidgetBase
from calibre.utils.config import JSONConfig
from qt.core import QLabel, QLineEdit, QVBoxLayout

prefs = JSONConfig('plugins/magnitide_test_preferences')
prefs.defaults['greeting'] = 'hello'


class ConfigWidget(ConfigWidgetBase):
    def genesis(self, gui):
        self.gui = gui
        layout = QVBoxLayout(self)
        layout.addWidget(QLabel('Greeting'))
        # calibre finds the widget for a setting by name: opt_<setting>.
        self.opt_greeting = QLineEdit(self)
        layout.addWidget(self.opt_greeting)
        self.register('greeting', prefs)

    def commit(self):
        ConfigWidgetBase.commit(self)
        return True  # restart required
