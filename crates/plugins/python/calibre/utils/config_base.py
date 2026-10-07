"""Mirrors calibre's `calibre.utils.config_base` (calibre 9.15): the
configuration machinery (options, `Config`, `StringConfig`, `ConfigProxy`),
calibre's global `prefs`, the `tweaks`, and JSON helpers.

Settings are plain files under `config_dir` (Magnitide's own folder for
plugins, `CALIBRE_CONFIG_DIRECTORY`), with the names calibre uses, so a
plugin's settings are where its author expects them.
"""

import json
import os
import re
import sys
import traceback
from collections import defaultdict
from contextlib import suppress
from functools import partial

from calibre.constants import CONFIG_DIR_MODE, config_dir, preferred_encoding

plugin_dir = os.path.join(config_dir, 'plugins')


def make_config_dir():
    if not os.path.exists(plugin_dir):
        os.makedirs(plugin_dir, mode=CONFIG_DIR_MODE, exist_ok=True)


def to_json(obj):
    """How calibre writes the values json can't: bytes, dates, sets."""
    import datetime

    if isinstance(obj, bytearray):
        from polyglot.binary import as_base64_unicode

        return {'__class__': 'bytearray', '__value__': as_base64_unicode(bytes(obj))}
    if isinstance(obj, bytes):
        from polyglot.binary import as_base64_unicode

        return {'__class__': 'bytearray', '__value__': as_base64_unicode(obj)}
    if isinstance(obj, datetime.datetime):
        from calibre.utils.date import isoformat

        return {'__class__': 'datetime.datetime', '__value__': isoformat(obj, as_utc=True)}
    if isinstance(obj, datetime.date):
        return {'__class__': 'datetime.date', '__value__': obj.isoformat()}
    if isinstance(obj, (set, frozenset)):
        return {'__class__': 'set', '__value__': list(obj)}
    if hasattr(obj, 'toBase64'):  # QByteArray
        return {'__class__': 'bytearray', '__value__': bytes(obj.toBase64()).decode('ascii')}
    raise TypeError(repr(obj) + ' is not JSON serializable')


def from_json(obj):
    custom = obj.get('__class__')
    if custom is not None:
        if custom == 'bytearray':
            from polyglot.binary import from_base64_bytes

            return bytearray(from_base64_bytes(obj['__value__']))
        if custom == 'datetime.datetime':
            from calibre.utils.iso8601 import parse_iso8601

            return parse_iso8601(obj['__value__'], assume_utc=True)
        if custom == 'datetime.date':
            import datetime

            return datetime.date.fromisoformat(obj['__value__'])
        if custom == 'set':
            return set(obj['__value__'])
    return obj


def json_dumps(obj, ignore_unserializable=False):
    ans = json.dumps(
        obj,
        indent=2,
        default=to_json if not ignore_unserializable else lambda o: None,
        sort_keys=True,
        ensure_ascii=False,
    )
    if isinstance(ans, str):
        ans = ans.encode('utf-8')
    return ans


def json_loads(raw):
    if isinstance(raw, bytes):
        raw = raw.decode('utf-8')
    return json.loads(raw, object_hook=from_json)


# --- Options ------------------------------------------------------------


class Option:
    def __init__(
        self,
        name,
        switches=[],
        help='',
        type=None,
        choices=None,
        check=None,
        group=None,
        default=None,
        action=None,
        metavar=None,
    ):
        if choices and not type:
            type = 'choice'
        self.name = name
        self.switches = switches
        self.help = help.replace('%default', repr(default)) if help else None
        self.type = type
        if self.type is None and action is None and choices is None:
            if isinstance(default, float):
                self.type = 'float'
            elif isinstance(default, int) and not isinstance(default, bool):
                self.type = 'int'
        self.choices = choices
        self.check = check
        self.group = group
        self.default = default
        self.action = action
        self.metavar = metavar

    def __eq__(self, other):
        return self.name == getattr(other, 'name', other)

    def __repr__(self):
        return 'Option: ' + self.name

    def __str__(self):
        return repr(self)

    def __hash__(self):
        return hash(self.name)


class OptionValues:
    def copy(self):
        import copy

        return copy.deepcopy(self)


class OptionSet:
    OPTION_PATTERN = re.compile(r'(?:^|\n)\s*(\w+)\s*=\s*(.+?)(?=\n\s*\w+\s*=|$)', re.DOTALL)

    def __init__(self, description=''):
        self.description = description
        self.defaults = {}
        self.preferences = []
        self.group_list = []
        self.groups = {}
        self.set_buffer = {}
        self.loads_pat = None

    def has_option(self, name_or_option_object):
        if name_or_option_object in self.preferences:
            return True
        for p in self.preferences:
            if p.name == name_or_option_object:
                return True
        return False

    def get_option(self, name_or_option_object):
        idx = self.preferences.index(name_or_option_object)
        if idx > -1:
            return self.preferences[idx]
        for p in self.preferences:
            if p.name == name_or_option_object:
                return p

    def add_group(self, name, description=''):
        if name in self.group_list:
            raise ValueError('A group by the name %s already exists in this set' % name)
        self.groups[name] = description
        self.group_list.append(name)
        return partial(self.add_opt, group=name)

    def update(self, other):
        for name in other.groups.keys():
            self.groups[name] = other.groups[name]
            if name not in self.group_list:
                self.group_list.append(name)
        for pref in other.preferences:
            if pref in self.preferences:
                self.preferences.remove(pref)
            self.preferences.append(pref)

    def smart_update(self, opts1, opts2):
        """Updates the preference values in opts1 using only the non-default values in opts2."""
        for pref in self.preferences:
            new = getattr(opts2, pref.name, pref.default)
            if new != pref.default:
                setattr(opts1, pref.name, new)

    def remove_opt(self, name):
        if name in self.preferences:
            self.preferences.remove(name)

    def add_opt(
        self, name, switches=[], help=None, type=None, choices=None, group=None, default=None, action=None, metavar=None
    ):
        """Add an option. `switches` is a list of the command-line switches
        for it (unused here), `help` its description, `choices` the allowed
        values, `default` its default."""
        pref = Option(
            name,
            switches=switches,
            help=help,
            type=type,
            choices=choices,
            group=group,
            default=default,
            action=action,
            metavar=None,
        )
        if group is not None and group not in self.groups.keys():
            raise ValueError('Group %s has not been added to this section' % group)
        if pref in self.preferences:
            raise ValueError('An option with the name %s already exists in this set.' % name)
        self.preferences.append(pref)
        self.defaults[name] = default

    def retranslate_help(self):
        pass

    def option_parser(self, user_defaults=None, usage='', gui_mode=False):
        from optparse import OptionGroup, OptionParser

        parser = OptionParser(usage=usage)
        groups = defaultdict(lambda: parser)
        for group, desc in self.groups.items():
            groups[group] = OptionGroup(parser, group, desc)
            parser.add_option_group(groups[group])
        for pref in self.preferences:
            if not pref.switches:
                continue
            g = groups[pref.group]
            action = pref.action
            if action is None:
                action = 'store'
                if pref.default is True or pref.default is False:
                    action = 'store_' + ('false' if pref.default else 'true')
            args = dict(
                dest=pref.name,
                help=pref.help,
                metavar=pref.metavar,
                type=pref.type,
                choices=pref.choices,
                default=getattr(user_defaults, pref.name, pref.default),
                action=action,
            )
            g.add_option(*pref.switches, **args)
        return parser

    def get_override_section(self, src):
        match = self.OVERRIDE_PAT.search(src)
        if match:
            return match.group()
        return ''

    def parse_string(self, src):
        options = {'cPickle': None}
        if src:
            is_old_style = (isinstance(src, bytes) and src.startswith(b'#')) or (
                isinstance(src, str) and src.startswith('#')
            )
            if is_old_style:
                if isinstance(src, bytes):
                    src = src.decode('utf-8')
                options = {'cPickle': None}
                src = src.replace('PyQt%d.QtCore' % 4, 'PyQt5.QtCore')
                src = re.sub(r'cPickle\.loads\(([\'"])', r'cPickle.loads(b\1', src)
                try:
                    exec(src, options)
                except Exception:
                    traceback.print_exc()
            else:
                try:
                    options = json_loads(src)
                    if not isinstance(options, dict):
                        raise Exception('options is not a dictionary')
                except Exception:
                    traceback.print_exc()
        opts = OptionValues()
        for pref in self.preferences:
            val = options.get(pref.name, pref.default)
            formatter = (
                __builtins__.get(pref.type, None)
                if isinstance(__builtins__, dict)
                else getattr(__builtins__, pref.type, None)
                if pref.type
                else None
            )
            if callable(formatter):
                val = formatter(val)
            setattr(opts, pref.name, val)
        return opts

    def serialize(self, opts, ignore_unserializable=False):
        data = {pref.name: getattr(opts, pref.name, pref.default) for pref in self.preferences}
        return json_dumps(data, ignore_unserializable=ignore_unserializable)


class ConfigInterface:
    def __init__(self, description):
        self.option_set = OptionSet(description=description)
        self.add_opt = self.option_set.add_opt
        self.add_group = self.option_set.add_group
        self.remove_opt = self.remove = self.option_set.remove_opt
        self.parse_string = self.option_set.parse_string
        self.get_option = self.option_set.get_option
        self.preferences = self.option_set.preferences

    def update(self, other):
        self.option_set.update(other.option_set)

    def option_parser(self, usage='', gui_mode=False):
        return self.option_set.option_parser(user_defaults=self.parse(), usage=usage, gui_mode=gui_mode)

    def smart_update(self, opts1, opts2):
        self.option_set.smart_update(opts1, opts2)


class Config(ConfigInterface):
    """A `Config` lives in a file under `config_dir` named by `basename`
    (`.py` historically; JSON now, as calibre's), read on `parse()`, written
    by `set()`."""

    def __init__(self, basename, description=''):
        ConfigInterface.__init__(self, description)
        self.filename_base = basename

    @property
    def config_file_path(self):
        return os.path.join(config_dir, self.filename_base + '.py.json')

    def parse(self):
        src = ''
        migrate = False
        path = self.config_file_path
        if os.path.exists(path):
            with open(path, 'rb') as f:
                src = f.read().decode('utf-8')
        else:
            path = path.rpartition('.')[0]
            if os.path.exists(path):
                with open(path, 'rb') as f:
                    src = f.read().decode('utf-8', 'replace')
                migrate = bool(src)
        ans = self.option_set.parse_string(src)
        if migrate:
            new_src = self.option_set.serialize(ans, ignore_unserializable=True)
            with suppress(Exception):
                with open(self.config_file_path, 'wb') as f:
                    f.write(new_src)
        return ans

    def set(self, name, val):
        if not self.option_set.has_option(name):
            raise ValueError('The option %s is not defined.' % name)
        if not os.path.exists(config_dir):
            make_config_dir()
        opts = self.parse()
        setattr(opts, name, val)
        src = self.option_set.serialize(opts)
        path = self.config_file_path
        tmp = path + '.tmp'
        with open(tmp, 'wb') as f:
            f.write(src)
        os.replace(tmp, path)


class StringConfig(ConfigInterface):
    """A `Config` kept in a string instead of a file."""

    def __init__(self, src, description=''):
        ConfigInterface.__init__(self, description)
        self.set_src(src)

    def set_src(self, src):
        self.src = src
        if isinstance(self.src, bytes):
            self.src = self.src.decode('utf-8')

    def parse(self):
        return self.option_set.parse_string(self.src)

    def set(self, name, val):
        if not self.option_set.has_option(name):
            raise ValueError('The option %s is not defined.' % name)
        opts = self.parse()
        setattr(opts, name, val)
        self.set_src(self.option_set.serialize(opts))


class ConfigProxy:
    """A dict-like front to a `Config`: `prefs['key']`, `prefs['key'] = value`."""

    def __init__(self, config):
        self.__config = config
        self.__opts = None

    @property
    def defaults(self):
        return self.__config.option_set.defaults

    def refresh(self):
        self.__opts = self.__config.parse()

    def retranslate_help(self):
        self.__config.option_set.retranslate_help()

    def __getitem__(self, key):
        return self.get(key)

    def __setitem__(self, key, val):
        return self.set(key, val)

    def get(self, key):
        if self.__opts is None:
            self.refresh()
        return getattr(self.__opts, key)

    def set(self, key, val):
        if self.__opts is None:
            self.refresh()
        setattr(self.__opts, key, val)
        return self.__config.set(key, val)

    def help(self, key):
        return self.__config.get_option(key).help


def _prefs():
    c = Config('global', 'calibre wide preferences')
    c.add_opt(
        'database_path',
        default=os.path.expanduser('~/library1.db'),
        help='Path to the database in which books are stored',
    )
    c.add_opt(
        'filename_pattern', default='(?P<title>.+) - (?P<author>[^_]+)', help='Pattern to guess metadata from filenames'
    )
    c.add_opt('isbndb_com_key', default='', help='Access key for isbndb.com')
    c.add_opt('network_timeout', default=5, help='Default timeout for network operations (seconds)')
    c.add_opt('library_path', default=None, help='Path to folder in which your library of books is stored')
    c.add_opt('language', default=None, help='The language in which to display the user interface')
    c.add_opt('output_format', default='EPUB', help='The default output format for e-book conversions.')
    c.add_opt(
        'input_format_order',
        default=[
            'EPUB',
            'AZW3',
            'MOBI',
            'LIT',
            'PRC',
            'FB2',
            'HTML',
            'HTM',
            'XHTM',
            'SHTML',
            'XHTML',
            'ZIP',
            'DOCX',
            'ODT',
            'RTF',
            'PDF',
            'TXT',
        ],
        help='Ordered list of formats to prefer for input.',
    )
    c.add_opt('read_file_metadata', default=True, help='Read metadata from files')
    c.add_opt('worker_process_priority', default='normal', help='The priority of worker processes.')
    c.add_opt('swap_author_names', default=False, help='Swap author first and last names when reading metadata')
    c.add_opt('add_formats_to_existing', default=False, help='Add new formats to existing book records')
    c.add_opt('check_for_dupes_on_ctl', default=False, help='Check for duplicates when copying to another library')
    c.add_opt('installation_uuid', default=None, help='Installation UUID')
    c.add_opt('new_book_tags', default=[], help='Tags to apply to books added to the library')
    c.add_opt('mark_new_books', default=False, help='Mark newly added books')
    c.add_opt('saved_searches', default={}, help='List of named saved searches')
    c.add_opt('user_categories', default={}, help='User-created Tag browser categories')
    c.add_opt('manage_device_metadata', default='manual', help='How metadata on a device is managed')
    c.add_opt(
        'limit_search_columns',
        default=False,
        help='When searching for text without using lookup prefixes, as for example, Red instead of title:Red, limit the columns searched to those named below.',
    )
    c.add_opt(
        'limit_search_columns_to',
        default=['title', 'authors', 'tags', 'series', 'publisher'],
        help='Choose columns to be searched when not using prefixes',
    )
    c.add_opt(
        'use_primary_find_in_search',
        default=True,
        help='Characters typed in the search box will match their accented versions',
    )
    c.add_opt('case_sensitive', default=False, help='Make searches case-sensitive')
    c.add_opt('migrated', default=False, help="For Internal use. Don't modify.")
    return c


prefs = ConfigProxy(_prefs())

# --- Tweaks ---------------------------------------------------------------

_tweaks = None


def default_tweaks_raw():
    """The default tweaks file (ours documents the keys that matter here)."""
    from calibre.utils.resources import get_path

    path = get_path('default_tweaks.py', allow_user_override=False)
    try:
        with open(path, 'rb') as f:
            return f.read()
    except OSError:
        return b''


def exec_tweaks(path):
    if isinstance(path, bytes):
        raw = path
        fname = '<string>'
    else:
        with open(path, 'rb') as f:
            raw = f.read()
            fname = f.name
    code = compile(raw, fname, 'exec')
    l = {}
    g = {'__file__': fname}
    exec(code, g, l)
    return l


def read_custom_tweaks():
    make_config_dir()
    tf = os.path.join(config_dir, 'tweaks.json')
    if os.path.exists(tf):
        with open(tf, 'rb') as f:
            raw = f.read()
        raw = raw.strip()
        if not raw:
            return {}
        try:
            return json_loads(raw)
        except Exception:
            traceback.print_exc()
            return {}
    tf = os.path.join(config_dir, 'tweaks.py')
    if os.path.exists(tf):
        with suppress(Exception):
            return exec_tweaks(tf)
    return {}


def default_tweaks():
    return exec_tweaks(default_tweaks_raw())


def read_tweaks():
    default_tweaks = exec_tweaks(default_tweaks_raw())
    try:
        custom_tweaks = read_custom_tweaks()
    except Exception:
        custom_tweaks = {}
    default_tweaks.update(custom_tweaks)
    return default_tweaks


def write_custom_tweaks(tweaks_dict):
    make_config_dir()
    tf = os.path.join(config_dir, 'tweaks.json')
    raw = json_dumps(tweaks_dict)
    with open(tf, 'wb') as f:
        f.write(raw)


class Tweak:
    """`with Tweak('name', value):` sets a tweak temporarily."""

    def __init__(self, name, value):
        self.name, self.value = name, value

    def __enter__(self):
        self.origval = tweaks[self.name]
        tweaks[self.name] = self.value

    def __exit__(self, *args):
        tweaks[self.name] = self.origval


class _Tweaks(dict):
    def __missing__(self, key):
        # calibre raises; plugins sometimes ask for a tweak that doesn't
        # exist in this version, so None is kinder.
        return None


def _load_tweaks():
    global _tweaks
    if _tweaks is None:
        _tweaks = _Tweaks(read_tweaks())
    return _tweaks


class _TweaksProxy:
    """The `tweaks` mapping, read on first use."""

    def __getattr__(self, name):
        return getattr(_load_tweaks(), name)

    def __getitem__(self, key):
        return _load_tweaks()[key]

    def __setitem__(self, key, value):
        _load_tweaks()[key] = value

    def __contains__(self, key):
        return key in _load_tweaks()

    def __iter__(self):
        return iter(_load_tweaks())

    def __len__(self):
        return len(_load_tweaks())

    def __repr__(self):
        return repr(_load_tweaks())


tweaks = _TweaksProxy()


def reset_tweaks_to_default():
    global _tweaks
    _tweaks = None
    _load_tweaks()


__all__ = [
    'Option',
    'OptionValues',
    'OptionSet',
    'ConfigInterface',
    'Config',
    'StringConfig',
    'ConfigProxy',
    'prefs',
    'tweaks',
    'Tweak',
    'to_json',
    'from_json',
    'json_dumps',
    'json_loads',
    'config_dir',
    'plugin_dir',
    'make_config_dir',
    'read_tweaks',
    'read_custom_tweaks',
    'write_custom_tweaks',
    'default_tweaks',
    'default_tweaks_raw',
    'exec_tweaks',
    'preferred_encoding',
    'reset_tweaks_to_default',
]

if 'sys' in globals():
    del sys
