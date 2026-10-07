"""Loading plugins and telling Magnitide what they are."""

import os
import sys
import threading

from calibre.constants import __version__ as calibre_version
from calibre.customize import (
    CatalogPlugin,
    EditBookToolPlugin,
    FileTypePlugin,
    InterfaceActionBase,
    LibraryClosedPlugin,
    MetadataReaderPlugin,
    MetadataWriterPlugin,
    PreferencesPlugin,
    StoreBase,
    ViewerPlugin,
)
from calibre.customize.conversion import InputFormatPlugin, OutputFormatPlugin
from calibre.customize.zipplugin import loader
from calibre.ebooks.metadata.sources.base import Source
from magnitide_host import COMPAT_VERSION
from magnitide_host.rpc import event, method

# Initialized plugins by zip path, reloaded when the zip changes. One instance
# per plugin, as in calibre: a source caches cover URLs found by identify()
# for download_cover() to use.
_plugins = {}
_plugins_lock = threading.Lock()


def plugin_kind(cls):
    for base, kind in (
        (Source, 'metadataSource'),
        (InterfaceActionBase, 'interfaceAction'),
        (FileTypePlugin, 'fileType'),
        (MetadataReaderPlugin, 'metadataReader'),
        (MetadataWriterPlugin, 'metadataWriter'),
        (InputFormatPlugin, 'conversionInput'),
        (OutputFormatPlugin, 'conversionOutput'),
        (CatalogPlugin, 'catalog'),
        (PreferencesPlugin, 'preferences'),
        (LibraryClosedPlugin, 'libraryClosed'),
        (StoreBase, 'store'),
        (ViewerPlugin, 'viewer'),
        (EditBookToolPlugin, 'editBookTool'),
    ):
        if issubclass(cls, base):
            return kind
    return 'other'


def plugin(path):
    """The initialized plugin from a zip: the registry's instance when the
    zip is installed, else a private one (while inspecting)."""
    from calibre.customize import ui

    path = os.path.abspath(path)
    for p in ui.initialized_plugins():
        if getattr(p, 'plugin_path', None) and os.path.abspath(p.plugin_path) == path:
            return p
    mtime = os.stat(path).st_mtime
    with _plugins_lock:
        cached = _plugins.get(path)
        if cached and cached[0] == mtime:
            return cached[1]
        cls = loader.load(path)
        instance = cls(path)
        instance.initialize()
        _plugins[path] = (mtime, instance)
        return instance


def version(v):
    return '.'.join(str(x) for x in v)


@method('hello')
def m_hello(params):
    return {
        'calibre': calibre_version,
        'compat': COMPAT_VERSION,
        'python': sys.version.split()[0],
        'runtime': 'magnitide',
    }


@method('plugins.set')
def m_plugins_set(params):
    """Magnitide's installed plugins: [{path, name, enabled}]."""
    from calibre.customize import ui

    ui.set_plugins(params.get('plugins') or [])
    return {'errors': ui.load_errors()}


@method('inspect')
def m_inspect(params):
    """What a plugin zip contains, without initializing it."""
    cls = loader.load(params['path'])
    kind = plugin_kind(cls)
    ans = {
        'name': cls.name,
        'version': version(cls.version),
        'author': cls.author,
        'description': cls.description or '',
        'kind': kind,
        'minimumCalibreVersion': version(cls.minimum_calibre_version),
        'capabilities': sorted(getattr(cls, 'capabilities', ()) or ()) if kind == 'metadataSource' else [],
        'fileTypes': sorted(getattr(cls, 'file_types', ()) or ())
        if kind in ('fileType', 'metadataReader', 'metadataWriter')
        else [],
        'priority': int(getattr(cls, 'priority', 1) or 1),
    }
    for flag in (
        'on_import',
        'on_postimport',
        'on_postadd',
        'on_postconvert',
        'on_postdelete',
        'on_preprocess',
        'on_postprocess',
    ):
        ans[_camel(flag)] = bool(getattr(cls, flag, False)) if kind == 'fileType' else False
    try:
        from magnitide_host.coverage import scan_zip

        ans['coverage'] = scan_zip(params['path'])
    except Exception:
        ans['coverage'] = []
    try:
        # A preferences page is nothing but settings.
        instance = cls(params['path'])
        ans['customizable'] = kind == 'preferences' or bool(instance.is_customizable())
    except Exception:
        ans['customizable'] = kind == 'preferences'
    return ans


def _camel(flag):
    parts = flag.split('_')
    return parts[0] + ''.join(p.title() for p in parts[1:])


def _on_registry_change(name, enabled, path=None):
    """A plugin enabled, disabled, added or removed another: tell Magnitide."""
    event('plugins-changed', name=name, enabled=enabled, path=path)


def install_registry_listener():
    from calibre.customize import ui

    ui.set_change_listener(_on_registry_change)
