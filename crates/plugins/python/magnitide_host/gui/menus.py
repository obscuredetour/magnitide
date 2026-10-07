"""Plugins' menus, as calibre would show them on their toolbar buttons."""

import base64
import os
import traceback


def text(action):
    """A menu item's text without its keyboard accelerator ("&Mark" → "Mark")."""
    return action.text().replace('&&', '\0').replace('&', '').replace('\0', '&')


def icon(action):
    from qt.core import QBuffer, QByteArray, QIODevice

    ic = action.icon()
    if ic is None or ic.isNull():
        return None
    data = QByteArray()
    buf = QBuffer(data)
    buf.open(QIODevice.OpenModeFlag.WriteOnly)
    ic.pixmap(64, 64).save(buf, 'PNG')
    return 'data:image/png;base64,' + base64.b64encode(bytes(data)).decode('ascii')


def show(menu):
    """Let the plugin fill its menu, as it would just before calibre shows it."""
    try:
        menu.aboutToShow.emit()
    except Exception:
        traceback.print_exc()


def menu_items(menu, prefix):
    show(menu)
    items = []
    for i, a in enumerate(menu.actions()):
        if not a.isVisible():
            continue
        if a.isSeparator():
            items.append({'separator': True})
            continue
        item = {
            'path': prefix + [i],
            'text': text(a),
            'enabled': a.isEnabled(),
            'checkable': a.isCheckable(),
            'checked': a.isChecked(),
        }
        sub = a.menu()
        if sub is not None:
            item['children'] = menu_items(sub, prefix + [i])
        items.append(item)
    return items


def actions(loaded):
    """The menus of the loaded external plugins (path → InterfaceAction)."""
    out = []
    for path, ac in loaded.items():
        qa = ac.qaction
        menu = qa.menu()
        shortcuts = []
        try:
            for name, (group, default_keys, desc, *_rest) in getattr(ac.gui.keyboard, 'shortcuts', {}).items():
                if ac.name in str(group):
                    shortcuts.append({'name': name, 'group': group, 'defaultKeys': list(default_keys or [])})
        except Exception:
            pass
        out.append(
            {
                'plugin': ac.interface_action_base_plugin.name,
                'file': os.path.basename(path),
                'text': text(qa) or ac.name,
                'icon': icon(qa),
                # Whether the plugin's toolbar button does something itself.
                'main': qa.receivers(qa.triggered) > 0,
                'items': menu_items(menu, []) if menu is not None else [],
                'shortcuts': shortcuts,
            }
        )
    return out


def find(menu, path, texts):
    """The menu item at `path`, checking it still has the text listed (menus
    rebuilt on show can move items); by text if it moved."""
    action = None
    for depth, (index, t) in enumerate(zip(path, texts)):
        if menu is None:
            return None
        show(menu)
        acts = menu.actions()
        action = acts[index] if index < len(acts) and text(acts[index]) == t else None
        if action is None:
            action = next((a for a in acts if text(a) == t), None)
        if action is None:
            return None
        menu = action.menu() if depth < len(path) - 1 else None
    return action
