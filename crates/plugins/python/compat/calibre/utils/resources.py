# Derived from calibre's src/calibre/utils/resources.py (calibre 9.15).
# Copyright: 2009, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.utils.resources` (calibre 9.15): `P()` and
`I()`, which calibre installs as built-ins for finding resource files.

Magnitide ships few of calibre's resources: the tweaks file, mime types
and language data. calibre's icon names are drawn with Magnitide's icon
set (`calibre.utils.lucide_icons`): `I('config.png')` is a rendered PNG in
the cache folder, and `QIcon.ic()` draws unmapped names as a stand-in (see
`calibre.gui2`). A `resources/` folder under the user's config folder
overrides, as in calibre.
"""

import builtins
import os
import sys

from calibre.constants import config_dir, resources_location

user_dir = os.path.join(config_dir, 'resources')


class PathResolver:
    def __init__(self):
        self.locations = [resources_location()]
        self.cache = {}

        def suitable(path):
            try:
                return os.path.exists(path) and os.path.isdir(path) and os.listdir(path)
            except OSError:
                pass
            return False

        self.default_path = resources_location()
        self.using_develop_from = False
        self.user_path = None
        if suitable(user_dir):
            self.locations.insert(0, user_dir)
            self.user_path = user_dir

    def __call__(self, path, allow_user_override=True):
        path = path.replace(os.sep, '/')
        key = (path, allow_user_override)
        ans = self.cache.get(key, None)
        if ans is None:
            for base in self.locations:
                if not allow_user_override and base == self.user_path:
                    continue
                fpath = os.path.join(base, *path.split('/'))
                if os.path.exists(fpath):
                    ans = fpath
                    break
            if ans is None:
                ans = os.path.join(self.default_path, *path.split('/'))
            self.cache[key] = ans
        return ans


_resolver = PathResolver()


def get_path(path, data=False, allow_user_override=True):
    fpath = _resolver(path, allow_user_override=allow_user_override)
    if data:
        with open(fpath, 'rb') as f:
            return f.read()
    return fpath


def get_image_path(path, data=False, allow_user_override=True):
    if not path:
        return get_path('images', allow_user_override=allow_user_override)
    fpath = _resolver('images/' + path, allow_user_override=allow_user_override)
    if not os.path.exists(fpath):
        # calibre's own icons, drawn with Magnitide's icon set: a PNG that
        # exists, so QIcon(I('config.png')) and open(I(...)) work.
        from calibre.utils.lucide_icons import png_path

        rendered = png_path(path)
        if rendered is not None:
            fpath = rendered
    if data:
        with open(fpath, 'rb') as f:
            return f.read()
    return fpath


def get_user_path(path):
    return os.path.join(user_dir, path)


def set_resources_path(path):
    _resolver.default_path = path
    _resolver.locations = [path] + [x for x in _resolver.locations if x != path]
    _resolver.cache.clear()


builtins.__dict__['P'] = get_path
builtins.__dict__['I'] = get_image_path

del sys
