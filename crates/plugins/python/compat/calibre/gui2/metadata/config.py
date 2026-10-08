"""Mirrors calibre's `calibre.gui2.metadata.config`: the settings widget a
metadata source gets for free, built from its `options` and
`touched_fields` (fields to ignore)."""

from calibre.ebooks.metadata.book.base import field_metadata
from qt.core import (
    QCheckBox,
    QComboBox,
    QDoubleSpinBox,
    QGridLayout,
    QGroupBox,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QScrollArea,
    QSpinBox,
    Qt,
    QVBoxLayout,
    QWidget,
)


class FieldsList(QListWidget):
    """The fields the source may download, each with a tick; unticked ones
    are ignored (`prefs['ignore_fields']`)."""

    def __init__(self, plugin, parent=None):
        QListWidget.__init__(self, parent)
        self.plugin = plugin
        self.setSelectionMode(QListWidget.SelectionMode.NoSelection)
        self.initialize()

    def initialize(self):
        self.clear()
        ignored = set(self.plugin.prefs['ignore_fields'])
        for field in sorted(self.plugin.touched_fields, key=self._name):
            item = QListWidgetItem(self._name(field), self)
            item.setData(Qt.ItemDataRole.UserRole, field)
            item.setFlags(item.flags() | Qt.ItemFlag.ItemIsUserCheckable)
            item.setCheckState(Qt.CheckState.Unchecked if field in ignored else Qt.CheckState.Checked)

    def _name(self, field):
        if field.startswith('identifier:'):
            return 'Identifier: ' + field.partition(':')[-1]
        fm = field_metadata.get(field)
        return fm['name'] if fm and fm.get('name') else field

    def ignored(self):
        ans = []
        for i in range(self.count()):
            item = self.item(i)
            if item.checkState() != Qt.CheckState.Checked:
                ans.append(item.data(Qt.ItemDataRole.UserRole))
        return ans

    def commit(self):
        self.plugin.prefs['ignore_fields'] = self.ignored()

    def restore_defaults(self):
        for i in range(self.count()):
            self.item(i).setCheckState(Qt.CheckState.Checked)


class ConfigWidget(QWidget):
    def __init__(self, plugin):
        QWidget.__init__(self)
        self.plugin = plugin
        self.l = l = QVBoxLayout(self)
        if plugin.config_help_message:
            self.help = QLabel(plugin.config_help_message, self)
            self.help.setWordWrap(True)
            self.help.setOpenExternalLinks(True)
            l.addWidget(self.help)
        self.fields_box = QGroupBox('Metadata fields to download', self)
        fl = QVBoxLayout(self.fields_box)
        self.fields_view = FieldsList(plugin, self)
        fl.addWidget(QLabel('If you uncheck any fields, metadata for those fields will not be downloaded', self))
        fl.addWidget(self.fields_view)
        l.addWidget(self.fields_box)
        self.fields_box.setVisible(bool(plugin.touched_fields))
        self.overrides = {}
        if plugin.options:
            self.opts_box = QGroupBox('Plugin specific options', self)
            sa = QScrollArea(self)
            sa.setWidgetResizable(True)
            inner = QWidget()
            self.grid = QGridLayout(inner)
            sa.setWidget(inner)
            ob = QVBoxLayout(self.opts_box)
            ob.addWidget(sa)
            l.addWidget(self.opts_box)
            for row, opt in enumerate(plugin.options):
                self.add_option(opt, row)

    def add_option(self, opt, row):
        val = self.plugin.prefs[opt.name]
        label = QLabel(opt.label, self)
        label.setToolTip(opt.desc or '')
        if opt.type == 'bool':
            w = QCheckBox(opt.label, self)
            w.setChecked(bool(val))
            label.setVisible(False)
        elif opt.type == 'number':
            w = QDoubleSpinBox(self) if isinstance(opt.default, float) else QSpinBox(self)
            w.setRange(-1_000_000, 1_000_000)
            w.setValue(val)
        elif opt.type == 'string':
            w = QLineEdit(self)
            w.setText(val or '')
        elif opt.type == 'choices':
            w = QComboBox(self)
            choices = opt.choices or {}
            items = list(choices.items()) if isinstance(choices, dict) else [(c, c) for c in choices]
            for k, v in items:
                w.addItem(v, k)
            idx = w.findData(val)
            if idx > -1:
                w.setCurrentIndex(idx)
        else:
            return
        w.setToolTip(opt.desc or '')
        self.overrides[opt.name] = (opt, w)
        self.grid.addWidget(label, row, 0)
        self.grid.addWidget(w, row, 1)

    def value(self, name):
        opt, w = self.overrides[name]
        if opt.type == 'bool':
            return w.isChecked()
        if opt.type == 'number':
            return w.value()
        if opt.type == 'string':
            return w.text().strip()
        if opt.type == 'choices':
            return w.currentData()

    def commit(self):
        self.fields_view.commit()
        for name in self.overrides:
            self.plugin.prefs[name] = self.value(name)

    def restore_defaults(self):
        self.fields_view.restore_defaults()
        for name, (opt, w) in self.overrides.items():
            if opt.type == 'bool':
                w.setChecked(bool(opt.default))
            elif opt.type == 'number':
                w.setValue(opt.default)
            elif opt.type == 'string':
                w.setText(opt.default or '')
            elif opt.type == 'choices':
                idx = w.findData(opt.default)
                if idx > -1:
                    w.setCurrentIndex(idx)


__all__ = ['ConfigWidget', 'FieldsList']
