# Derived from calibre's src/calibre/utils/config.py (calibre 9.15).
# Copyright: 2008, Kovid Goyal kovid@kovidgoyal.net. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.utils.config` (calibre 9.15): `JSONConfig`
(what plugins keep their settings in), `DynamicConfig` (`dynamic`), the
`Config`/`StringConfig`/`ConfigProxy` family from `config_base`, `prefs`,
`tweaks`, and `OptionParser`.

Files live under `config_dir`: `JSONConfig('plugins/x')` is
`<config_dir>/plugins/x.json`, as in calibre.
"""

import json
import os
import traceback
from contextlib import suppress

from calibre.constants import config_dir, iswindows
from calibre.utils.config_base import (  # noqa: F401
    Config,
    ConfigInterface,
    ConfigProxy,
    Option,
    OptionSet,
    OptionValues,
    StringConfig,
    Tweak,
    from_json,
    json_dumps,
    json_loads,
    make_config_dir,
    plugin_dir,
    prefs,
    read_custom_tweaks,
    to_json,
    tweaks,
    write_custom_tweaks,
)

__all__ = [
    'Config',
    'ConfigProxy',
    'DynamicConfig',
    'JSONConfig',
    'Option',
    'OptionParser',
    'OptionSet',
    'OptionValues',
    'StringConfig',
    'XMLConfig',
    'config_dir',
    'dynamic',
    'from_json',
    'json_dumps',
    'json_loads',
    'make_config_dir',
    'plugin_dir',
    'prefs',
    'to_json',
    'tweaks',
    'Tweak',
    'write_custom_tweaks',
]


class OptionParser:
    """calibre's OptionParser is optparse's with a nicer formatter; plugins'
    `cli_main` sometimes build one. Plain optparse here."""

    def __new__(
        cls,
        usage='%prog [options] filename',
        version=None,
        epilog=None,
        gui_mode=False,
        conflict_handler='resolve',
        **kwds,
    ):
        import optparse

        return optparse.OptionParser(
            usage=usage, version=version, epilog=epilog, conflict_handler=conflict_handler, **kwds
        )


def commit_to_file(path, data):
    """Write atomically: to a sibling, then over `path`."""
    dpath = os.path.dirname(path)
    if dpath:
        os.makedirs(dpath, exist_ok=True)
    tmp = path + '.tmp'
    with open(tmp, 'wb') as f:
        f.write(data)
    os.replace(tmp, path)


class DynamicConfig(dict):
    """A dict saved as JSON under `config_dir` (`dynamic.json`); every change
    is written at once. `dynamic` is the one calibre keeps GUI state in."""

    def __init__(self, name='dynamic'):
        dict.__init__(self, {})
        self.name = name
        self.defaults = {}
        self.refresh()

    @property
    def file_path(self):
        return os.path.join(config_dir, self.name + '.json')

    def decouple(self, prefix):
        self.name = prefix + self.name
        self.refresh()

    def read_old_serialized_representation(self):
        return {}

    def refresh(self, clear_current=True):
        d = {}
        if os.path.exists(self.file_path):
            with suppress(Exception), open(self.file_path, 'rb') as f:
                raw = f.read()
                try:
                    d = json_loads(raw) if raw.strip() else {}
                except Exception:
                    traceback.print_exc()
                    d = {}
        if clear_current:
            self.clear()
        self.update(d)

    def __getitem__(self, key):
        try:
            return dict.__getitem__(self, key)
        except KeyError:
            return self.defaults.get(key, None)

    def get(self, key, default=None):
        try:
            return dict.__getitem__(self, key)
        except KeyError:
            return self.defaults.get(key, default)

    def __setitem__(self, key, val):
        dict.__setitem__(self, key, val)
        self.commit()

    def set(self, key, val):
        self.__setitem__(key, val)

    def commit(self):
        if not getattr(self, 'name', None):
            return
        if not os.path.exists(config_dir):
            make_config_dir()
        raw = json_dumps(self, ignore_unserializable=True)
        commit_to_file(self.file_path, raw)


dynamic = DynamicConfig()


class XMLConfig(dict):
    """calibre's older plist-based store; kept for plugins that still name it.
    Stored as JSON here, under `<config_dir>/<rel_path_to_xml_file>.json`."""

    EXTENSION = '.plist'

    def __init__(self, rel_path_to_xml_file, base_path=config_dir):
        dict.__init__(self)
        self.no_commit = False
        self.defaults = {}
        self.file_path = os.path.join(base_path, *(rel_path_to_xml_file.split('/')))
        self.file_path = os.path.abspath(self.file_path)
        if not self.file_path.endswith(self.EXTENSION):
            self.file_path += self.EXTENSION
        self.file_path += '.json'
        self.refresh()

    def raw_to_object(self, raw):
        return json_loads(raw)

    def to_raw(self):
        return json_dumps(self, ignore_unserializable=True)

    def refresh(self, clear_current=True):
        d = {}
        if os.path.exists(self.file_path):
            with open(self.file_path, 'rb') as f:
                raw = f.read()
                try:
                    d = self.raw_to_object(raw) if raw.strip() else {}
                except Exception:
                    traceback.print_exc()
                    d = {}
        if clear_current:
            self.clear()
        self.update(d)

    def __getitem__(self, key):
        try:
            return dict.__getitem__(self, key)
        except KeyError:
            return self.defaults.get(key, None)

    def get(self, key, default=None):
        try:
            return dict.__getitem__(self, key)
        except KeyError:
            return self.defaults.get(key, default)

    def __setitem__(self, key, val):
        dict.__setitem__(self, key, val)
        self.commit()

    def set(self, key, val):
        self.__setitem__(key, val)

    def __delitem__(self, key):
        try:
            dict.__delitem__(self, key)
        except KeyError:
            pass  # ignore missing keys
        else:
            self.commit()

    def commit(self):
        if self.no_commit:
            return
        if hasattr(self, 'file_path') and self.file_path:
            dpath = os.path.dirname(self.file_path)
            if not os.path.exists(dpath):
                os.makedirs(dpath, mode=0o700)
            commit_to_file(self.file_path, self.to_raw())

    def __enter__(self):
        self.no_commit = True

    def __exit__(self, *args):
        self.no_commit = False
        self.commit()


class JSONConfig(XMLConfig):
    """A dict saved as JSON at `<config_dir>/<rel_path>.json`, written on
    every change (`__setitem__`), with `defaults` for missing keys. Plugins
    keep their settings in these: `JSONConfig('plugins/my_plugin')`."""

    EXTENSION = '.json'

    def __init__(self, rel_path_to_json_file, base_path=config_dir):
        dict.__init__(self)
        self.no_commit = False
        self.defaults = {}
        self.file_path = os.path.join(base_path, *(rel_path_to_json_file.split('/')))
        self.file_path = os.path.abspath(self.file_path)
        if not self.file_path.endswith(self.EXTENSION):
            self.file_path += self.EXTENSION
        self.refresh()

    def raw_to_object(self, raw):
        return json_loads(raw)

    def to_raw(self):
        return json_dumps(self)

    def __setitem__(self, key, val):
        key = str(key)
        dict.__setitem__(self, key, val)
        self.commit()

    def __getitem__(self, key):
        key = str(key)
        try:
            return dict.__getitem__(self, key)
        except KeyError:
            return self.defaults[key]

    def get(self, key, default=None):
        key = str(key)
        try:
            return dict.__getitem__(self, key)
        except KeyError:
            return self.defaults.get(key, default)

    def __contains__(self, key):
        return dict.__contains__(self, str(key)) or str(key) in self.defaults


def create_global_prefs(conf_obj=None):
    return prefs


def _make_dir_if_missing():
    with suppress(OSError):
        os.makedirs(config_dir, exist_ok=True)


_make_dir_if_missing()


if iswindows:

    def windows_long_path(path):
        return path
else:
    windows_long_path = None

del json
