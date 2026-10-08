"""Mirrors calibre's `calibre.gui2.preferences` (calibre 9.15): the base of
settings pages (`ConfigWidgetBase`, `ConfigWidgetInterface`) and the
`Setting` binding between a widget and a preference."""

from functools import partial

from calibre.gui2 import gprefs, show_restart_warning
from calibre.utils.config import ConfigProxy
from qt.core import (
    QAbstractListModel,
    QCheckBox,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QDoubleSpinBox,
    QFont,
    QLineEdit,
    QPlainTextEdit,
    QSpinBox,
    QTextEdit,
    QVBoxLayout,
    QWidget,
    pyqtSignal,
)


class AbortCommit(Exception):
    pass


class AbortInitialize(Exception):
    pass


class ConfigWidgetInterface:
    """What a settings page implements: `genesis(gui)` to build it,
    `initialize()` to load, `commit()` to save (returns True if a restart
    is needed), `restore_defaults()`; `changed_signal` when edited."""

    changed_signal = None
    supports_restoring_to_defaults = True
    restore_defaults_desc = (
        'Restore settings to default values. You have to click Apply to actually save the default settings.'
    )
    restart_critical = False

    def genesis(self, gui):
        raise NotImplementedError()

    def initialize(self):
        raise NotImplementedError()

    def restore_defaults(self):
        pass

    def commit(self):
        return False

    def refresh_gui(self, gui):
        pass

    def initial_tab_changed(self):
        pass

    def do_on_child_tabs(self, method, *args):
        pass


class Setting:
    """Binds a widget to a config key: `initialize()` loads the value into
    the widget, `commit()` saves it; `changed` fires as it's edited."""

    CHOICES_SEARCH_FLAGS = 0

    def __init__(
        self, name, config_obj, widget, gui_name=None, empty_string_is_None=True, choices=None, restart_required=False
    ):
        self.name, self.gui_name, self.empty_string_is_None, self.choices = (
            name,
            gui_name,
            empty_string_is_None,
            choices,
        )
        self.restart_required = restart_required
        self.widget = widget
        if gui_name is None:
            gui_name = 'opt_' + name
        self.gui_obj = getattr(self.widget, gui_name)
        self.config_obj = config_obj
        if isinstance(self.gui_obj, QCheckBox):
            self.datatype = 'bool'
            self.gui_obj.stateChanged.connect(self.changed)
        elif isinstance(self.gui_obj, (QSpinBox, QDoubleSpinBox)):
            self.datatype = 'number'
            self.gui_obj.valueChanged.connect(self.changed)
        elif isinstance(self.gui_obj, (QLineEdit, QTextEdit, QPlainTextEdit)):
            self.datatype = 'string'
            self.gui_obj.textChanged.connect(self.changed)
            if isinstance(self.gui_obj, (QTextEdit, QPlainTextEdit)):
                self.gui_obj.textChanged.connect(lambda: self.changed(None))
        elif isinstance(self.gui_obj, QComboBox):
            self.datatype = 'choice'
            self.gui_obj.editTextChanged.connect(self.changed)
            self.gui_obj.currentIndexChanged.connect(self.changed)
        else:
            raise ValueError('Unknown data type %s' % self.gui_obj.__class__)

        if isinstance(self.config_obj, ConfigProxy) and not hasattr(self.config_obj, 'help'):
            pass

    def changed(self, *args):
        self.widget.changed_signal.emit()

    def initialize(self):
        self.gui_obj.blockSignals(True)
        if self.datatype == 'choice':
            choices = self.choices or []
            if isinstance(self.config_obj, ConfigProxy):
                choices = self.config_obj.defaults.get(self.name) if False else choices
            self.gui_obj.clear()
            for x in choices:
                if isinstance(x, (tuple, list)):
                    self.gui_obj.addItem(x[0], x[1])
                else:
                    self.gui_obj.addItem(x, x)
        self.set_gui_val(self.get_config_val(default=False))
        self.gui_obj.blockSignals(False)
        self.initial_value = self.get_gui_val()

    def commit(self):
        val = self.get_gui_val()
        oldval = self.get_config_val()
        changed = val != oldval
        if changed:
            self.set_config_val(self.get_gui_val())
        return changed and self.restart_required

    def restore_defaults(self):
        self.set_gui_val(self.get_config_val(default=True))

    def get_config_val(self, default=False):
        if default:
            val = self.config_obj.defaults[self.name]
        else:
            val = self.config_obj[self.name]
        return val

    def set_config_val(self, val):
        self.config_obj[self.name] = val

    def set_gui_val(self, val):
        if self.datatype == 'bool':
            self.gui_obj.setChecked(bool(val))
        elif self.datatype == 'number':
            self.gui_obj.setValue(val)
        elif self.datatype == 'string':
            if isinstance(self.gui_obj, QLineEdit):
                self.gui_obj.setText(val if val else '')
            else:
                self.gui_obj.setPlainText(val if val else '')
        elif self.datatype == 'choice':
            idx = self.gui_obj.findData(val)
            if idx == -1:
                idx = 0
            self.gui_obj.setCurrentIndex(idx)

    def get_gui_val(self):
        if self.datatype == 'bool':
            val = bool(self.gui_obj.isChecked())
        elif self.datatype == 'number':
            val = self.gui_obj.value()
        elif self.datatype == 'string':
            if isinstance(self.gui_obj, QLineEdit):
                val = str(self.gui_obj.text()).strip()
            else:
                val = str(self.gui_obj.toPlainText()).strip()
            if self.empty_string_is_None and not val:
                val = None
        elif self.datatype == 'choice':
            idx = self.gui_obj.currentIndex()
            if idx < 0:
                idx = 0
            val = self.gui_obj.itemData(idx)
        return val


class CommaSeparatedList(Setting):
    def set_gui_val(self, val):
        x = ''
        if val:
            x = ', '.join(val)
        self.gui_obj.setText(x)

    def get_gui_val(self):
        val = str(self.gui_obj.text()).strip()
        ans = []
        if val:
            ans = [x.strip() for x in val.split(',')]
            ans = [x for x in ans if x]
        return ans


class ConfigWidgetBase(QWidget, ConfigWidgetInterface):
    """A settings page: `register(name, config_obj)` binds `self.opt_<name>`
    widgets to settings; `initialize`/`commit`/`restore_defaults` then work."""

    changed_signal = pyqtSignal()
    supports_restoring_to_defaults = True
    restart_critical = False

    def __init__(self, parent=None):
        QWidget.__init__(self, parent)
        self.settings = {}

    def register(
        self,
        name,
        config_obj,
        gui_name=None,
        choices=None,
        restart_required=False,
        empty_string_is_None=True,
        setting=Setting,
    ):
        setting = setting(
            name,
            config_obj,
            self,
            gui_name=gui_name,
            choices=choices,
            restart_required=restart_required,
            empty_string_is_None=empty_string_is_None,
        )
        return self.register_setting(setting)

    def register_setting(self, setting):
        self.settings[setting.name] = setting
        return setting

    def initialize(self):
        for setting in self.settings.values():
            setting.initialize()

    def commit(self, *args):
        restart_required = False
        for setting in self.settings.values():
            rr = setting.commit()
            if rr:
                restart_required = True
        return restart_required

    def restore_defaults(self, *args):
        for setting in self.settings.values():
            setting.restore_defaults()


class LazyConfigWidgetBase(ConfigWidgetBase):
    def __init__(self, parent=None):
        ConfigWidgetBase.__init__(self, parent)
        self.lazy_initialized = False

    def initialize(self):
        if not self.lazy_initialized:
            self.lazy_initialized = True
            ConfigWidgetBase.initialize(self)

    def commit(self, *args):
        if self.lazy_initialized:
            return ConfigWidgetBase.commit(self, *args)
        return False


def get_plugin(category, name):
    from calibre.customize.ui import preferences_plugins

    for plugin in preferences_plugins():
        if plugin.category == category and plugin.name == name:
            return plugin
    raise ValueError('No Preferences Plugin with category: %s and name: %s found' % (category, name))


class ConfigDialog(QDialog):
    def set_widget(self, w):
        self.w = w

    def accept(self):
        try:
            self.restart_required = self.w.commit()
        except AbortCommit:
            return
        QDialog.accept(self)

    def reject(self):
        try:
            self.w.refresh_gui(getattr(self.w, 'gui', None))
        except Exception:
            pass
        QDialog.reject(self)


def init_gui():
    from calibre.gui2.ui import get_gui

    return get_gui()


def show_config_widget(category, name, gui=None, show_restart_msg=False, parent=None, never_shutdown=False):
    """Show the settings page of a PreferencesPlugin in a dialog; returns
    whether a restart is needed."""
    pl = get_plugin(category, name)
    d = ConfigDialog(parent)
    d.resize(750, 550)
    conf_name = 'config_widget_dialog_geometry_%s_%s' % (category, name)
    geom = gprefs.get(conf_name, None)
    d.setWindowTitle(pl.gui_name or name)
    d.setWindowIcon(QFont and None or None) if False else None
    bb = QDialogButtonBox(d)
    bb.setStandardButtons(
        QDialogButtonBox.StandardButton.Apply
        | QDialogButtonBox.StandardButton.Cancel
        | QDialogButtonBox.StandardButton.RestoreDefaults
    )
    bb.accepted.connect(d.accept)
    bb.rejected.connect(d.reject)
    w = pl.create_widget(d)
    d.set_widget(w)
    bb.button(QDialogButtonBox.StandardButton.RestoreDefaults).clicked.connect(w.restore_defaults)
    bb.button(QDialogButtonBox.StandardButton.RestoreDefaults).setEnabled(w.supports_restoring_to_defaults)
    bb.button(QDialogButtonBox.StandardButton.Apply).setEnabled(False)
    bb.button(QDialogButtonBox.StandardButton.Apply).clicked.connect(d.accept)

    def onchange():
        b = bb.button(QDialogButtonBox.StandardButton.Apply)
        b.setEnabled(True)
        b.setDefault(True)
        b.setAutoDefault(True)

    w.changed_signal.connect(onchange)
    bb.button(QDialogButtonBox.StandardButton.Cancel).setFocus(True)
    l = QVBoxLayout()
    d.setLayout(l)
    l.addWidget(w)
    l.addWidget(bb)
    if gui is None:
        gui = init_gui()
    w.genesis(gui)
    w.initialize()
    if geom is not None:
        d.restoreGeometry(bytes(geom))
    d.exec()
    geom = bytearray(d.saveGeometry())
    gprefs[conf_name] = geom
    rr = getattr(d, 'restart_required', False)
    if show_restart_msg and rr:
        show_restart_warning('Restart needed', parent=parent)
    return rr


def test_widget(category, name, gui=None):
    show_config_widget(category, name, gui=gui, show_restart_msg=True)


__all__ = [
    'ConfigWidgetInterface',
    'ConfigWidgetBase',
    'LazyConfigWidgetBase',
    'Setting',
    'CommaSeparatedList',
    'AbortCommit',
    'AbortInitialize',
    'show_config_widget',
    'get_plugin',
    'test_widget',
    'QAbstractListModel',
    'partial',
]
