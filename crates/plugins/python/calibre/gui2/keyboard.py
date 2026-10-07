"""Mirrors calibre's `calibre.gui2.keyboard`: the keyboard-shortcut
`Manager`. Shortcuts plugins register are kept (and any the user set in
calibre's `shortcuts/main.json` honoured) and applied to the actions on
`finalize()`; Magnitide shows no shortcut editor yet."""

from collections import OrderedDict

from calibre.utils.config import JSONConfig
from qt.core import QKeySequence, QObject, QWidget


class NameConflict(ValueError):
    pass


def finalize(shortcuts, custom_keys_map={}):
    """Apply the shortcuts to their actions: the user's keys when set, the
    defaults otherwise. Returns a map of key → unique name."""
    keys_map = {}
    for unique_name, shortcut in shortcuts.items():
        custom_keys = custom_keys_map.get(unique_name, None)
        if custom_keys is None:
            candidates = shortcut['default_keys']
        else:
            candidates = custom_keys
        keys = []
        for x in candidates:
            ks = QKeySequence(x, QKeySequence.SequenceFormat.PortableText)
            x = str(ks.toString(QKeySequence.SequenceFormat.PortableText))
            if x in keys_map:
                continue
            keys.append(ks)
            keys_map[x] = unique_name
            ac = shortcut['action']
            if ac is None or getattr(ac, 'is_dead', False) if hasattr(ac, 'is_dead') else ac is None:
                continue
        try:
            ac = shortcut['action']
            if ac is not None:
                ac.setShortcuts(list(keys))
        except Exception:
            pass
        shortcut['keys'] = tuple(keys)
    return keys_map


class Manager(QObject):
    def __init__(self, parent=None, config_name='shortcuts/main'):
        QObject.__init__(self, parent)
        self.config = JSONConfig(config_name)
        self.shortcuts = OrderedDict()
        self.keys_map = {}
        self.groups = {}

    def register_shortcut(
        self, unique_name, name, default_keys=(), description=None, action=None, group=None, persist_shortcut=False
    ):
        """Register a shortcut with calibre. `unique_name` must be unique
        among every shortcut; `name` is what the user sees; `default_keys`
        the keys as strings ('Ctrl+A'); `action` the QAction to apply it to."""
        if unique_name in self.shortcuts:
            name = self.shortcuts[unique_name]['name']
            raise NameConflict('Shortcut for %r already registered by %s' % (unique_name, name))
        shortcut = {
            'name': name,
            'desc': description,
            'action': action,
            'default_keys': tuple(default_keys),
            'persist_shortcut': persist_shortcut,
        }
        self.shortcuts[unique_name] = shortcut
        group = group if group else 'Miscellaneous'
        self.groups[group] = self.groups.get(group, []) + [unique_name]

    def unregister_shortcut(self, unique_name):
        """Remove a shortcut (for menus rebuilt on show)."""
        self.shortcuts.pop(unique_name, None)
        for group in self.groups.values():
            if unique_name in group:
                group.remove(unique_name)

    def replace_action(self, unique_name, new_action):
        """Replace the action associated with a shortcut, keeping its keys."""
        sc = self.shortcuts[unique_name]
        sc['action'] = new_action
        new_action.setShortcuts(list(sc.get('keys', ())))

    def finalize(self):
        custom_keys_map = {un: tuple(keys) for un, keys in self.config.get('map', {}).items()}
        self.keys_map = finalize(self.shortcuts, custom_keys_map=custom_keys_map)

    def shortcut_for(self, unique_name):
        """The key sequences of a registered shortcut, as text."""
        sc = self.shortcuts.get(unique_name)
        if sc is None:
            return ()
        return tuple(k.toString(QKeySequence.SequenceFormat.NativeText) for k in sc.get('keys', ()))

    def dispatch_key_event(self, *args, **kwargs):
        pass

    def shortcut_for_action(self, action):
        for un, sc in self.shortcuts.items():
            if sc['action'] is action:
                return un


__all__ = ['Manager', 'NameConflict', 'finalize']


class ShortcutConfig(QWidget):
    """Mirrors calibre's shortcut editor as far as plugins drive it: a list
    of the registered shortcuts and their keys. Editing keys isn't offered
    in Magnitide yet; `commit()` keeps what's there."""

    def __init__(self, parent=None):
        from qt.core import QLabel, QTreeWidget, QVBoxLayout

        QWidget.__init__(self, parent)
        self.manager = None
        self.group_name = None
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(QLabel(_('Keyboard shortcuts (set in Magnitide; shown here as calibre would)'), self))
        self.view = QTreeWidget(self)
        self.view.setHeaderLabels([_('Action'), _('Shortcut')])
        self.view.setRootIsDecorated(True)
        layout.addWidget(self.view)

    def initialize(self, keyboard):
        from qt.core import QTreeWidgetItem

        self.manager = keyboard
        self.view.clear()
        for group, names in sorted(keyboard.groups.items()):
            top = QTreeWidgetItem(self.view, [group, ''])
            for unique_name in names:
                sc = keyboard.shortcuts.get(unique_name)
                if sc is None:
                    continue
                keys = ', '.join(k.toString(QKeySequence.SequenceFormat.NativeText) for k in sc.get('keys', ()))
                if not keys:
                    keys = ', '.join(sc.get('default_keys', ()))
                QTreeWidgetItem(top, [sc['name'], keys])
            top.setExpanded(False)

    def highlight_group(self, group_name):
        self.group_name = group_name
        for i in range(self.view.topLevelItemCount()):
            item = self.view.topLevelItem(i)
            item.setExpanded(item.text(0) == group_name)

    def commit(self):
        pass

    def is_customizable(self):
        return False
