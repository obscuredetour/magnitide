# Derived from calibre's src/calibre/customize/ui.py (calibre 9.15).
# Copyright: 2008, Kovid Goyal <kovid at kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.customize.ui` (calibre 9.15): the registry of
plugins and the functions calibre's code (and plugins) use to find and run
them.

In Magnitide the installed plugins are the app's list (Settings › Plugins);
the host passes it in with `set_plugins()`. Built-in plugins are the
metadata readers and writers in `calibre.customize.builtins`, plus the
stand-ins for calibre's own actions. Enabling or disabling from a plugin
is noted here and reported to Magnitide through an event.
"""

import os
import sys
import traceback
from contextlib import suppress
from functools import partial

from calibre.customize import (
    CatalogPlugin,
    EditBookToolPlugin,
    FileTypePlugin,
    InterfaceActionBase,
    InvalidPlugin,
    LibraryClosedPlugin,
    MetadataReaderPlugin,
    MetadataWriterPlugin,
    PluginInstallationType,
    PreferencesPlugin,
    StoreBase,
)
from calibre.customize.conversion import InputFormatPlugin, OutputFormatPlugin
from calibre.customize.profiles import InputProfile, OutputProfile
from calibre.customize.zipplugin import loader
from calibre.ebooks.metadata.sources.base import Source
from calibre.utils.config import JSONConfig, make_config_dir

builtin_names = frozenset(p.name for p in __import__('calibre.customize.builtins', fromlist=['plugins']).plugins)
BLACKLISTED_PLUGINS = frozenset()


class NameConflict(ValueError):
    pass


# --- The registry ---------------------------------------------------------

_initialized_plugins = []
_external = {}  # name -> {'path', 'enabled', 'kind', 'instance'}
_on_change = None  # callback(name, enabled) → Magnitide

_config = None


def config():
    """calibre keeps `customize.py.json` with the disabled set and the
    customization strings; so do we, for plugins that read it."""
    global _config
    if _config is None:
        c = JSONConfig('customize')
        c.defaults['plugins'] = {}
        c.defaults['filetype_mapping'] = {}
        c.defaults['plugin_customization'] = {}
        c.defaults['disabled_plugins'] = set()
        c.defaults['enabled_plugins'] = set()
        _config = c
    return _config


def set_change_listener(func):
    global _on_change
    _on_change = func


def set_plugins(entries):
    """Magnitide's installed plugins: `[{'path', 'name', 'enabled'}]`.
    Loads any not loaded yet, drops ones gone, and syncs enabled states."""
    seen = set()
    for e in entries:
        name = e.get('name') or ''
        path = os.path.abspath(e['path'])
        seen.add(name)
        rec = _external.get(name)
        if rec is None or rec['path'] != path or rec.get('mtime') != _mtime(path):
            try:
                cls = loader.load(path)
                instance = cls(path)
                instance.installation_type = PluginInstallationType.EXTERNAL
                instance.initialize()
                rec = {
                    'path': path,
                    'enabled': bool(e.get('enabled', True)),
                    'instance': instance,
                    'mtime': _mtime(path),
                    'error': None,
                }
                if instance.name != name:
                    name = instance.name
                    seen.add(name)
            except Exception as err:
                traceback.print_exc()
                rec = {
                    'path': path,
                    'enabled': False,
                    'instance': None,
                    'mtime': _mtime(path),
                    'error': f'{type(err).__name__}: {err}',
                }
            _external[name] = rec
        rec['enabled'] = bool(e.get('enabled', True))
    for name in list(_external):
        if name not in seen:
            del _external[name]
    _rebuild()


def _mtime(path):
    try:
        return os.stat(path).st_mtime
    except OSError:
        return None


def _rebuild():
    global _initialized_plugins
    plugins = []
    for cls in builtin_plugins():
        try:
            p = cls(None)
            p.installation_type = PluginInstallationType.BUILTIN
            p.initialize()
            plugins.append(p)
        except Exception:
            traceback.print_exc()
    for name, rec in _external.items():
        if rec['instance'] is not None:
            plugins.append(rec['instance'])
    _initialized_plugins = plugins
    with suppress(Exception):
        config()['plugins'] = {
            p.name: rec['path'] for p, rec in ((r['instance'], r) for r in _external.values()) if p is not None
        }


def builtin_plugins():
    from calibre.customize import builtins

    return builtins.plugins


def initialize_plugins(perf=False):
    if not _initialized_plugins:
        _rebuild()


def initialized_plugins():
    initialize_plugins()
    yield from _initialized_plugins


def load_errors():
    return {name: rec['error'] for name, rec in _external.items() if rec.get('error')}


def plugin_for_name(name):
    for p in initialized_plugins():
        if p.name == name:
            return p


def find_plugin(name):
    for plugin in initialized_plugins():
        if plugin.name == name:
            return plugin


def has_external_plugins():
    return bool(_external)


# --- Enabling and disabling -----------------------------------------------


def _name(plugin_or_name):
    return plugin_or_name if isinstance(plugin_or_name, str) else plugin_or_name.name


def is_disabled(plugin):
    name = _name(plugin)
    rec = _external.get(name)
    if rec is not None:
        return not rec['enabled']
    return name in config()['disabled_plugins']


def disabled_plugins():
    return [p for p in initialized_plugins() if is_disabled(p)]


def enabled_plugins():
    return [p for p in initialized_plugins() if not is_disabled(p)]


def enable_plugin(plugin_or_name):
    name = _name(plugin_or_name)
    rec = _external.get(name)
    if rec is not None:
        rec['enabled'] = True
    else:
        dp = config()['disabled_plugins']
        dp.discard(name)
        config()['disabled_plugins'] = dp
    if _on_change is not None:
        _on_change(name, True)


def disable_plugin(plugin_or_name):
    name = _name(plugin_or_name)
    p = find_plugin(name)
    if p is not None and not p.can_be_disabled:
        raise ValueError(f'Plugin {name} cannot be disabled')
    rec = _external.get(name)
    if rec is not None:
        rec['enabled'] = False
    else:
        dp = config()['disabled_plugins']
        dp.add(name)
        config()['disabled_plugins'] = dp
    if _on_change is not None:
        _on_change(name, False)


def default_disabled_plugins():
    return frozenset()


# --- Customization strings ------------------------------------------------


def plugin_customization(plugin):
    return config()['plugin_customization'].get(_name(plugin), '')


def customize_plugin(plugin, custom):
    d = config()['plugin_customization']
    d[_name(plugin)] = custom.strip()
    config()['plugin_customization'] = d
    initialize_plugin(plugin, plugin.plugin_path, plugin.installation_type)


def initialize_plugin(plugin, path_to_zip_file, installation_type):
    try:
        p = plugin(path_to_zip_file) if isinstance(plugin, type) else plugin
        p.installation_type = installation_type
        p.site_customization = plugin_customization(p) or None
        p.initialize()
        return p
    except Exception:
        tb = traceback.format_exc()
        raise InvalidPlugin(f'Plugin {getattr(plugin, "name", plugin)!r} failed to initialize:\n{tb}')


# --- Installing from a plugin (PluginUpdater and friends) -----------------


def add_plugin(path_to_zip_file):
    """Load a plugin from a zip into this session's registry. Magnitide is
    told through an event; its own list is what persists."""
    make_config_dir()
    cls = loader.load(path_to_zip_file)
    if cls.name in builtin_names:
        raise NameConflict(f'A builtin plugin with the name {cls.name!r} already exists')
    instance = cls(path_to_zip_file)
    instance.installation_type = PluginInstallationType.EXTERNAL
    instance.initialize()
    _external[cls.name] = {
        'path': os.path.abspath(path_to_zip_file),
        'enabled': True,
        'instance': instance,
        'mtime': _mtime(path_to_zip_file),
        'error': None,
    }
    _rebuild()
    if _on_change is not None:
        _on_change(cls.name, True, path_to_zip_file)
    return instance


def remove_plugin(plugin_or_name):
    name = _name(plugin_or_name)
    rec = _external.pop(name, None)
    _rebuild()
    if _on_change is not None:
        _on_change(name, None)
    return rec is not None


# --- Kinds ----------------------------------------------------------------


def _kind(cls, enabled_only=True):
    for p in initialized_plugins():
        if isinstance(p, cls) and (not enabled_only or not is_disabled(p)):
            yield p


def interface_actions():
    return list(_kind(InterfaceActionBase))


def preferences_plugins():
    return list(_kind(PreferencesPlugin))


def available_library_closed_plugins():
    return list(_kind(LibraryClosedPlugin))


def library_closed_plugins():
    return available_library_closed_plugins()


def all_edit_book_tool_plugins():
    return list(_kind(EditBookToolPlugin))


def store_plugins():
    return list(_kind(StoreBase))


def available_store_plugins():
    return store_plugins()


def stores():
    return {p.name: p for p in store_plugins()}


def available_stores():
    return stores()


def device_plugins(include_disabled=False):
    return []


def disabled_device_plugins():
    return []


def usbms_plugins(include_disabled=False):
    return []


def catalog_plugins():
    return list(_kind(CatalogPlugin))


def available_catalog_formats():
    formats = set()
    for plugin in catalog_plugins():
        formats |= set(plugin.file_types)
    return formats


def plugin_for_catalog_format(fmt):
    for plugin in catalog_plugins():
        if fmt.lower() in plugin.file_types:
            return plugin


def input_format_plugins():
    return list(_kind(InputFormatPlugin))


def output_format_plugins():
    return list(_kind(OutputFormatPlugin))


def plugin_for_input_format(fmt):
    return None


def plugin_for_output_format(fmt):
    return None


def available_input_formats():
    from calibre.ebooks import BOOK_EXTENSIONS

    return set(BOOK_EXTENSIONS)


def all_input_formats():
    return available_input_formats()


def available_output_formats():
    return {'epub', 'azw3', 'mobi', 'kepub', 'pdf', 'docx', 'txt'}


def input_profiles():
    from calibre.customize.profiles import input_profiles as ip

    return list(ip)


def output_profiles():
    from calibre.customize.profiles import output_profiles as op

    return list(op)


def plugin_for_input_profile(short_name):
    for p in input_profiles():
        if p.short_name == short_name:
            return p
    return InputProfile


def plugin_for_output_profile(short_name):
    for p in output_profiles():
        if p.short_name == short_name:
            return p
    return OutputProfile


# --- Metadata sources -----------------------------------------------------


def all_metadata_plugins():
    return list(_kind(Source, enabled_only=False))


def metadata_plugins(capabilities=frozenset()):
    capabilities = frozenset(capabilities)
    for plugin in _kind(Source):
        if capabilities.issubset(plugin.capabilities):
            yield plugin


def patch_metadata_plugins(possibly_updated_plugins):
    patches = {}
    for i, plugin in enumerate(_initialized_plugins):
        if isinstance(plugin, Source) and plugin.name in builtin_names:
            pup = possibly_updated_plugins.get(plugin.name)
            if pup is not None and pup.version > plugin.version:
                patches[i] = pup(None)
                patches[i].initialize()
    for i, pup in patches.items():
        _initialized_plugins[i] = pup


# --- Metadata readers and writers -----------------------------------------


def metadata_readers():
    return list(_kind(MetadataReaderPlugin))


def metadata_writers():
    return list(_kind(MetadataWriterPlugin))


def _readers_for(ftype):
    ftype = ftype.lower().strip()
    ans = [p for p in metadata_readers() if ftype in p.file_types or '*' in p.file_types]
    ans.sort(key=lambda p: p.priority, reverse=True)
    return ans


def _writers_for(ftype):
    ftype = ftype.lower().strip()
    ans = [p for p in metadata_writers() if ftype in p.file_types or '*' in p.file_types]
    ans.sort(key=lambda p: p.priority, reverse=True)
    return ans


class QuickMetadata:
    def __init__(self):
        self.quick = False

    def __enter__(self):
        self.quick = True

    def __exit__(self, *args):
        self.quick = False


quick_metadata = QuickMetadata()


class ApplyNullMetadata:
    def __init__(self):
        self.apply_null = False

    def __enter__(self):
        self.apply_null = True

    def __exit__(self, *args):
        self.apply_null = False


apply_null_metadata = ApplyNullMetadata()


class ForceIdentifiers:
    def __init__(self):
        self.force_identifiers = False

    def __enter__(self):
        self.force_identifiers = True

    def __exit__(self, *args):
        self.force_identifiers = False


force_identifiers = ForceIdentifiers()


def get_file_type_metadata(stream, ftype):
    """Metadata from a file, through the readers for its type (highest
    priority first); the first that answers wins."""
    from calibre.ebooks.metadata.meta import get_metadata_from_stream_plain

    mi = None
    ftype = ftype.lower().strip()
    for plugin in _readers_for(ftype):
        with plugin:
            try:
                plugin.quick = quick_metadata.quick
                mi = plugin.get_metadata(stream, ftype)
                break
            except Exception:
                traceback.print_exc()
                with suppress(Exception):
                    stream.seek(0)
                continue
    if mi is None:
        return get_metadata_from_stream_plain(stream, ftype)
    return mi


def set_file_type_metadata(stream, mi, ftype, report_error=None):
    ftype = ftype.lower().strip()
    for plugin in _writers_for(ftype):
        with plugin:
            try:
                plugin.apply_null = apply_null_metadata.apply_null
                plugin.set_metadata(stream, mi, ftype)
                break
            except Exception:
                if report_error is None:
                    from calibre import prints

                    prints(
                        'Failed to set metadata for the',
                        ftype.upper(),
                        'format of:',
                        getattr(mi, 'title', ''),
                        file=sys.stderr,
                    )
                    traceback.print_exc()
                else:
                    report_error(mi, ftype, traceback.format_exc())


def can_set_metadata(ftype):
    return bool(_writers_for(ftype))


# --- File-type hooks -------------------------------------------------------


def _file_type_plugins(ftype, flag):
    ftype = ftype.lower().strip()
    ans = [
        p for p in _kind(FileTypePlugin) if getattr(p, flag, False) and (ftype in p.file_types or '*' in p.file_types)
    ]
    ans.sort(key=lambda p: p.priority, reverse=True)
    return ans


def _run_filetype_plugins(path_to_file, ft=None, occasion='preprocess'):
    occasion_flag = 'on_' + occasion
    customization = config()['plugin_customization']
    if ft is None:
        ft = os.path.splitext(path_to_file)[-1].lower().replace('.', '')
    nfp = path_to_file
    for plugin in _file_type_plugins(ft, occasion_flag):
        plugin.site_customization = customization.get(plugin.name, '')
        oo, oe = sys.stdout, sys.stderr  # plugins may print, which must not corrupt the protocol
        with plugin:
            try:
                plugin.original_path_to_file = path_to_file
            except Exception:
                pass
            try:
                nfp = plugin.run(nfp) or nfp
            except Exception:
                print(f'Running file type plugin {plugin.name} failed with traceback:', file=oe)
                traceback.print_exc(file=oe)
            finally:
                sys.stdout, sys.stderr = oo, oe

    def x(j):
        return os.path.normpath(os.path.normcase(j))

    if occasion == 'postprocess' and x(nfp) != x(path_to_file):
        import shutil

        shutil.copyfile(nfp, path_to_file)
        nfp = path_to_file
    return nfp


run_plugins_on_import = partial(_run_filetype_plugins, occasion='import')
run_plugins_on_preprocess = partial(_run_filetype_plugins, occasion='preprocess')
run_plugins_on_postprocess = partial(_run_filetype_plugins, occasion='postprocess')


def run_plugins_on_postimport(db, book_id, fmt):
    for plugin in _file_type_plugins(fmt, 'on_postimport'):
        with plugin:
            try:
                plugin.postimport(book_id, fmt, db)
            except Exception:
                print(f'Running file type plugin {plugin.name} failed with traceback:', file=sys.stderr)
                traceback.print_exc()


def run_plugins_on_postconvert(db, book_id, fmt):
    for plugin in _file_type_plugins(fmt, 'on_postconvert'):
        with plugin:
            try:
                plugin.postconvert(book_id, fmt, db)
            except Exception:
                print(f'Running file type plugin {plugin.name} failed with traceback:', file=sys.stderr)
                traceback.print_exc()


def run_plugins_on_postdelete(db, book_id, fmt):
    for plugin in _file_type_plugins(fmt, 'on_postdelete'):
        with plugin:
            try:
                plugin.postdelete(book_id, fmt, db)
            except Exception:
                print(f'Running file type plugin {plugin.name} failed with traceback:', file=sys.stderr)
                traceback.print_exc()


def run_plugins_on_postadd(db, book_id, fmt_map):
    plugins = set()
    for fmt in fmt_map:
        for plugin in _file_type_plugins(fmt, 'on_postadd'):
            plugins.add(plugin)
    for plugin in plugins:
        with plugin:
            try:
                plugin.postadd(book_id, fmt_map, db)
            except Exception:
                print(f'Running file type plugin {plugin.name} failed with traceback:', file=sys.stderr)
                traceback.print_exc()


def epub_fixers():
    return []


def reread_filetype_plugins():
    pass


def reread_metadata_plugins():
    pass


def plugin_for_input_profile_name(name):
    return plugin_for_input_profile(name)


def load_plugin(path_to_zip_file):
    return loader.load(path_to_zip_file)


__all__ = [
    n for n in dir() if not n.startswith('_') and n not in ('os', 'sys', 'traceback', 'suppress', 'partial', 'DEBUG')
]
