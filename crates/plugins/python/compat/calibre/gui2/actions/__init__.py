# Derived from calibre's src/calibre/gui2/actions/__init__.py (calibre 9.15).
# Copyright: 2010, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.gui2.actions` (calibre 9.15): `InterfaceAction`,
the base of GUI plugins, with its action/menu helpers and lifecycle hooks."""

from functools import partial

from calibre.gui2 import Dispatcher
from qt.core import QAction, QIcon, QKeySequence, QMenu, QObject, QToolButton


def menu_action_unique_name(plugin, unique_name):
    return 'Interface Action: %s (%s) : menu action : %s' % (plugin.unique_name, plugin.name, unique_name)


class InterfaceActionWithLibraryDrop:
    """Mixin for actions that accept books dropped on them (drops aren't
    emulated in Magnitide)."""

    accepts_drops = True

    def accept_enter_event(self, event, mime_data):
        return mime_data.hasFormat('application/calibre+from_library')

    def accept_drag_move_event(self, event, mime_data):
        return self.accept_enter_event(event, mime_data)

    def drop_event(self, event, mime_data):
        return False

    def do_drop(self):
        pass


class InterfaceAction(QObject):
    """A plugin's action in calibre's window: `qaction` (its toolbar button
    and menu), `create_menu_action` for its menu items, and the hooks calibre
    calls (`genesis`, `initialization_complete`, `library_changed`, …).
    `self.gui` is the window."""

    #: The plugin name. If two plugins with the same name are present, the one
    #: with higher priority wins.
    name = 'Implement me'

    #: The plugin priority. Higher priority plugins take precedence.
    priority = 1

    #: The menu popup type for when this plugin is added to a toolbar
    popup_type = QToolButton.ToolButtonPopupMode.MenuButtonPopup

    #: Whether this action should be auto repeated when its shortcut
    #: key is held down.
    auto_repeat = False

    #: Of the form: (text, icon_path, tooltip, keyboard shortcut)
    #: icon, tooltip and keyboard shortcut can be None
    #: shortcut must be a string, None or tuple of shortcuts.
    #: If None, a keyboard shortcut corresponding to the action is not
    #: registered. If you pass an empty tuple, then the shortcut is registered
    #: with no default key binding.
    action_spec = ('text', 'icon', None, None)

    #: If True, a menu is automatically created and added to self.qaction
    action_add_menu = False

    #: If True, a clone of self.qaction is added to the menu of self.qaction
    #: If you want the text of this action to be different from that of
    #: self.qaction, set this variable to the new text
    action_menu_clone_qaction = False

    #: Set of locations to which this action must not be added.
    #: See :attr:`all_locations` for a list of possible locations
    dont_add_to = frozenset()

    #: Set of locations from which this action must not be removed.
    #: See :attr:`all_locations` for a list of possible locations
    dont_remove_from = frozenset()

    all_locations = frozenset(
        [
            'toolbar',
            'toolbar-device',
            'context-menu',
            'context-menu-device',
            'toolbar-child',
            'menubar',
            'menubar-device',
            'context-menu-cover-browser',
            'context-menu-split',
        ]
    )

    #: Type of action
    #: 'current' means acts on the current view
    #: 'global' means an action that does not act on the current view, but rather
    #: on calibre as a whole
    action_type = 'global'

    #: If True, then this InterfaceAction will have the opportunity to interact
    #: with drag and drop events. See the methods, :meth:`accept_enter_event`,
    #: :meth`:accept_drag_move_event`, :meth:`drop_event` for details.
    accepts_drops = False

    def __init__(self, parent, site_customization):
        QObject.__init__(self, parent)
        self.setObjectName(self.name)
        self.gui = parent
        self.site_customization = site_customization
        self.interface_action_base_plugin = None

    def accept_enter_event(self, event, mime_data):
        return False

    def accept_drag_move_event(self, event, mime_data):
        return False

    def drop_event(self, event, mime_data):
        return False

    def do_drop(self):
        pass

    @property
    def unique_name(self):
        bn = self.__class__.__name__
        if self.interface_action_base_plugin.name:
            bn = self.interface_action_base_plugin.name
        return 'Interface Action: %s (%s)' % (bn, self.name)

    def create_action(self, spec=None, attr='qaction', shortcut_name=None, persist_shortcut=False):
        if spec is None:
            spec = self.action_spec
        text, icon, tooltip, shortcut = spec
        if icon is not None:
            action = QAction(QIcon.ic(icon) if isinstance(icon, str) else icon, text, self.gui)
        else:
            action = QAction(text, self.gui)
        if attr == 'qaction':
            if hasattr(self.action_menu_clone_qaction, 'rstrip'):
                mt = str(self.action_menu_clone_qaction)
            else:
                mt = action.text()
            self.menuless_qaction = ma = QAction(action.icon(), mt, self.gui)
            ma.triggered.connect(action.trigger)
        for a in (action, ma) if attr == 'qaction' else (action,):
            a.setAutoRepeat(self.auto_repeat)
            text = tooltip if tooltip else text
            a.setToolTip(text)
            a.setStatusTip(text)
            a.setWhatsThis(text)
        shortcut_action = action
        desc = tooltip if tooltip else None
        if attr == 'qaction':
            shortcut_action = ma
        if shortcut is not None:
            keys = (shortcut,) if isinstance(shortcut, str) else tuple(shortcut)
            if shortcut_name is None and spec[0]:
                shortcut_name = str(spec[0])

            if (
                shortcut_name
                and self.action_spec[0]
                and not (attr == 'qaction' and self.popup_type == QToolButton.ToolButtonPopupMode.InstantPopup)
            ):
                try:
                    self.gui.keyboard.register_shortcut(
                        self.unique_name + ' - ' + attr,
                        shortcut_name,
                        default_keys=keys,
                        action=shortcut_action,
                        description=desc,
                        group=self.action_spec[0],
                        persist_shortcut=persist_shortcut,
                    )
                except Exception:
                    import traceback

                    traceback.print_exc()
                shortcut_action.setShortcuts(
                    [QKeySequence(key, QKeySequence.SequenceFormat.PortableText) for key in keys]
                )
            else:
                shortcut_action.setShortcuts(
                    [QKeySequence(key, QKeySequence.SequenceFormat.PortableText) for key in keys]
                )
        if attr is not None:
            setattr(self, attr, action)
        if attr == 'qaction' and self.action_add_menu:
            menu = QMenu()
            action.setMenu(menu)
            if self.action_menu_clone_qaction:
                menu.addAction(self.menuless_qaction)
        return action

    def create_menu_action(
        self,
        menu,
        unique_name,
        text,
        icon=None,
        shortcut=None,
        description=None,
        triggered=None,
        shortcut_name=None,
        persist_shortcut=False,
    ):
        """Add an action to `menu` with a registered (customizable) shortcut."""
        if shortcut_name is None:
            shortcut_name = str(text)
        ac = menu.addAction(text)
        if icon is not None:
            if not isinstance(icon, QIcon):
                icon = QIcon.ic(icon)
            ac.setIcon(icon)
        keys = ()
        if shortcut is not None and shortcut is not False:
            keys = (shortcut,) if isinstance(shortcut, str) else tuple(shortcut)
        unique_name = menu_action_unique_name(self, unique_name)
        if description is not None:
            ac.setToolTip(description)
            ac.setStatusTip(description)
            ac.setWhatsThis(description)

        ac.calibre_shortcut_unique_name = unique_name
        if shortcut is not False:
            self.gui.keyboard.register_shortcut(
                unique_name,
                shortcut_name,
                default_keys=keys,
                action=ac,
                description=description,
                group=self.action_spec[0],
                persist_shortcut=persist_shortcut,
            )
        if triggered is not None:
            ac.triggered.connect(triggered)
        return ac

    def load_resources(self, names):
        """Bytes of files in the plugin zip, by name."""
        if self.plugin_path is None:
            raise ValueError('This plugin was not loaded from a ZIP file')
        ans = {}
        import zipfile

        with zipfile.ZipFile(self.plugin_path, 'r') as zf:
            for candidate in zf.namelist():
                if candidate in names:
                    ans[candidate] = zf.read(candidate)
        return ans

    def genesis(self):
        """Setup this plugin. Only called once during initialization. self.gui is
        available. The action specified by :attr:`action_spec` is available as
        ``self.qaction``."""
        pass

    def location_selected(self, loc):
        """Called whenever the book list being edited in calibre changes.
        Currently values for loc are: ``library, main, card and cardb``."""
        pass

    def library_about_to_change(self, olddb, db):
        pass

    def library_changed(self, db):
        """Called whenever the current library is changed."""
        pass

    def gui_layout_complete(self):
        """Called once per action when the layout of the main GUI is
        completed. If your action needs to make changes to the layout, they
        should be done here."""
        pass

    def initialization_complete(self):
        """Called once per action when the initialization of the main GUI is
        completed."""
        pass

    def tag_browser_context_action(self, index):
        """Called when displaying the context menu in the Tag browser."""
        pass

    def shutting_down(self):
        """Called once per plugin when the main GUI is in the process of shutting
        down. Release any used resources, but try not to block the shutdown for
        long periods of time."""
        pass

    def do_genesis(self):
        self.Dispatcher = partial(Dispatcher, parent=self)
        self.create_action()
        self.genesis()

    #: Whether the library (not a device) is the current view; set by `location_selected`.
    is_library_selected = True

    @property
    def plugin_path(self):
        return (
            getattr(self.interface_action_base_plugin, 'plugin_path', None)
            if getattr(self, '_plugin_path', None) is None
            else self._plugin_path
        )

    @plugin_path.setter
    def plugin_path(self, val):
        self._plugin_path = val


__all__ = ['InterfaceAction', 'InterfaceActionWithLibraryDrop', 'menu_action_unique_name']
