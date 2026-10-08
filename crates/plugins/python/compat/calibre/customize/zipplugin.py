"""Mirrors calibre's `calibre.customize.zipplugin` (calibre 9.15): loading a
plugin from its zip file.

A plugin zip holds `__init__.py` (defining the plugin class) and, for
multi-file plugins, an empty `plugin-import-name-<name>.txt` that makes its
modules importable as `calibre_plugins.<name>.<module>`. The loader is an
import hook: plugin modules are read straight from the zip. Each module
gets `get_resources`, `get_icons` and `load_translations` as globals, as
calibre provides them.
"""

import importlib
import importlib.abc
import importlib.machinery
import importlib.util
import os
import posixpath
import re
import sys
import threading
import zipfile
from collections import OrderedDict
from functools import partial

from calibre import as_unicode
from calibre.customize import InvalidPlugin, Plugin, PluginNotFound, numeric_version, platform


def get_resources(zfp, name_or_list_of_names, print_tracebacks_for_missing_resources=True):
    """Bytes of a file in the zip (None if missing), or for a list of names
    a dict of the ones found."""
    names = name_or_list_of_names
    if isinstance(names, str):
        names = [names]
    ans = {}
    with zipfile.ZipFile(zfp) as zf:
        for name in names:
            try:
                ans[name] = zf.read(name)
            except Exception:
                if print_tracebacks_for_missing_resources:
                    import traceback

                    traceback.print_exc()
    if len(names) == 1:
        ans = ans.pop(names[0], None)
    return ans


def get_icons(zfp, name_or_list_of_names, plugin_name='', print_tracebacks_for_missing_resources=True):
    """QIcons for images in the zip, by name or list of names. A missing
    image gives calibre's icon of that name (a stand-in, in Magnitide)."""
    from qt.core import QIcon, QPixmap

    names = name_or_list_of_names
    ans = get_resources(zfp, names, print_tracebacks_for_missing_resources=print_tracebacks_for_missing_resources)
    if isinstance(names, str):
        names = [names]
        ans = {names[0]: ans}
    ians = {}
    for name in names:
        data = ans.get(name)
        if data:
            pixmap = QPixmap()
            pixmap.loadFromData(data)
            ians[name] = QIcon(pixmap)
        else:
            ians[name] = QIcon.ic(name.rpartition('/')[-1])
    if len(names) == 1:
        return ians[names[0]]
    return ians


_translations_cache = {}


def load_translations(namespace, zfp):
    """Install the plugin's translations (`translations/<lang>.mo` in the
    zip) as `_` and `ngettext` in `namespace` (the module's globals)."""
    null = object()
    trans = _translations_cache.get(zfp, null)
    if trans is None:
        return
    if trans is null:
        from calibre.utils.localization import get_lang

        lang = get_lang()
        if not lang or lang == 'en' or lang.startswith('en_'):
            _translations_cache[zfp] = None
            return
        with zipfile.ZipFile(zfp) as zf:
            mo_path = None
            candidates = [lang, lang.replace('_', '-')]
            if '_' in lang:
                candidates.append(lang.partition('_')[0])
            names = set(zf.namelist())
            for c in candidates:
                p = f'translations/{c}.mo'
                if p in names:
                    mo_path = p
                    break
            if mo_path is None:
                _translations_cache[zfp] = None
                return
            data = zf.read(mo_path)
        import gettext
        from io import BytesIO

        trans = _translations_cache[zfp] = gettext.GNUTranslations(BytesIO(data))
    namespace['_'] = trans.gettext
    namespace['ngettext'] = trans.ngettext


class PluginLoader(importlib.abc.MetaPathFinder, importlib.abc.Loader):
    """The import hook for `calibre_plugins.*`."""

    def __init__(self):
        self.loaded_plugins = {}  # plugin name -> (zip path, set of names in the zip)
        self._lock = threading.RLock()
        self._identifier_pat = re.compile(r'[a-zA-Z][_0-9a-zA-Z]*')

    def _get_actual_fullname(self, fullname):
        parts = fullname.split('.')
        if parts[0] == 'calibre_plugins':
            if len(parts) == 1:
                return parts, None
            plugin_name = parts[1]
            with self._lock:
                names = self.loaded_plugins.get(plugin_name, None)
                if names is None:
                    return None, None
                names = names[1]
            fullname = '.'.join(parts[2:])
            if not fullname:
                fullname = '__init__'
            if fullname in names:
                return fullname, plugin_name
            if fullname + '/__init__' in names:
                return fullname + '/__init__', plugin_name
            if fullname.endswith('.__init__') and fullname.replace('.', '/') in names:
                return fullname.replace('.', '/'), plugin_name
            if fullname.replace('.', '/') in names:
                return fullname.replace('.', '/'), plugin_name
            if fullname.replace('.', '/') + '/__init__' in names:
                return fullname.replace('.', '/') + '/__init__', plugin_name
        return None, None

    def find_spec(self, fullname, path=None, target=None):
        if fullname == 'calibre_plugins':
            return importlib.machinery.ModuleSpec(fullname, self, is_package=True)
        if not fullname.startswith('calibre_plugins.'):
            return None
        fname, plugin_name = self._get_actual_fullname(fullname)
        if fname is None:
            return None
        is_package = fname.endswith('__init__')
        spec = importlib.machinery.ModuleSpec(fullname, self, is_package=is_package, origin=fullname)
        spec.magnitide_file = fname
        spec.magnitide_plugin = plugin_name
        return spec

    def create_module(self, spec):
        return None

    def exec_module(self, module):
        spec = module.__spec__
        fullname = spec.name
        if fullname == 'calibre_plugins':
            module.__path__ = []
            return
        fname = getattr(spec, 'magnitide_file', None)
        plugin_name = getattr(spec, 'magnitide_plugin', None)
        if fname is None:
            fname, plugin_name = self._get_actual_fullname(fullname)
        if fname is None:
            raise ImportError(f'No module named {fullname!r}')
        with self._lock:
            zfp = self.loaded_plugins[plugin_name][0]
        with zipfile.ZipFile(zfp) as zf:
            try:
                code = zf.read(fname + '.py')
            except KeyError:
                raise ImportError(f'No module named {fullname!r} in {zfp}')
        code = code.replace(b'\r\n', b'\n')
        module.__file__ = f'{zfp}/{fname}.py'
        if spec.submodule_search_locations is not None or fname.endswith('__init__'):
            module.__path__ = [f'{zfp}/{posixpath.dirname(fname + ".py")}'.rstrip('/')]
        module.__dict__['get_resources'] = partial(get_resources, zfp)
        module.__dict__['get_icons'] = partial(get_icons, zfp, plugin_name=plugin_name)
        module.__dict__['load_translations'] = partial(load_translations, module.__dict__, zfp)
        compiled = compile(code, module.__file__, 'exec', dont_inherit=True)
        exec(compiled, module.__dict__)

    def get_source(self, fullname):
        fname, plugin_name = self._get_actual_fullname(fullname)
        if fname is None:
            return None
        with self._lock:
            zfp = self.loaded_plugins[plugin_name][0]
        with zipfile.ZipFile(zfp) as zf:
            return zf.read(fname + '.py').decode('utf-8', 'replace')

    def is_package(self, fullname):
        fname, plugin_name = self._get_actual_fullname(fullname)
        return bool(fname and fname.endswith('__init__'))

    # -- loading a plugin --

    def _plugin_name_from_zip(self, zf, path_to_zip_file):
        names = frozenset(zf.namelist())
        plugin_name = None
        for name in names:
            name, ext = posixpath.splitext(name)
            if name.startswith('plugin-import-name-') and ext == '.txt':
                plugin_name = name.rpartition('-')[-1]
        if plugin_name is None:
            # A single-file plugin, named after its zip.
            plugin_name = os.path.splitext(os.path.basename(path_to_zip_file))[0]
            plugin_name = re.sub(r'[^0-9a-zA-Z_]', '_', plugin_name)
            if not plugin_name or plugin_name[0].isdigit():
                plugin_name = 'p' + plugin_name
        if self._identifier_pat.match(plugin_name) is None:
            raise InvalidPlugin(f'The plugin at {path_to_zip_file!r} has an invalid import name: {plugin_name!r}')
        pynames = {n[:-3] for n in names if n.endswith('.py')}
        return plugin_name, pynames

    def load(self, path_to_zip_file):
        """Load the plugin class from a zip; the first `Plugin` subclass its
        `__init__.py` defines."""
        if not os.access(path_to_zip_file, os.R_OK):
            raise PluginNotFound(f'Cannot access {path_to_zip_file!r}')
        with zipfile.ZipFile(path_to_zip_file) as zf:
            plugin_name, names = self._plugin_name_from_zip(zf, path_to_zip_file)
        with self._lock:
            path_to_zip_file = os.path.abspath(path_to_zip_file)
            existing = self.loaded_plugins.get(plugin_name)
            self.loaded_plugins[plugin_name] = (path_to_zip_file, names)
            # A reloaded plugin (updated zip) must not use stale modules.
            if existing is not None and existing[0] != path_to_zip_file or existing is not None:
                prefix = f'calibre_plugins.{plugin_name}'
                for mod in list(sys.modules):
                    if mod == prefix or mod.startswith(prefix + '.'):
                        del sys.modules[mod]
        try:
            m = importlib.import_module(f'calibre_plugins.{plugin_name}')
        except ImportError:
            raise
        except Exception as e:
            raise InvalidPlugin(f'The plugin at {path_to_zip_file!r} could not be loaded: {as_unicode(e)}') from e
        plugin_classes = []
        for x in m.__dict__.values():
            if isinstance(x, type) and issubclass(x, Plugin) and x.name != 'Trivial Plugin':
                plugin_classes.append(x)
        if not plugin_classes:
            raise InvalidPlugin(f'No plugin class found in {path_to_zip_file!r}:{plugin_name}')
        # The one defined in the plugin itself, rather than imported into it.
        own = [x for x in plugin_classes if x.__module__ == m.__name__]
        ans = (own or plugin_classes)[0]
        if ans.minimum_calibre_version > numeric_version:
            raise InvalidPlugin(
                f'The plugin at {path_to_zip_file!r} needs a version of calibre >= '
                f'{".".join(map(str, ans.minimum_calibre_version))}'
            )
        if platform not in ans.supported_platforms:
            raise InvalidPlugin(f'The plugin at {path_to_zip_file!r} cannot be used on {platform}')
        ans.zip_path = path_to_zip_file
        ans.plugin_import_name = plugin_name
        return ans

    def unload(self, plugin_name):
        with self._lock:
            self.loaded_plugins.pop(plugin_name, None)
            prefix = f'calibre_plugins.{plugin_name}'
            for mod in list(sys.modules):
                if mod == prefix or mod.startswith(prefix + '.'):
                    del sys.modules[mod]


loader = PluginLoader()
if not any(isinstance(f, PluginLoader) for f in sys.meta_path):
    sys.meta_path.insert(0, loader)


def get_plugin_info(raw, check_for_qt5=False):
    """Read a plugin's name, version and so on from its zip's bytes without
    importing it (a best-effort parse of `__init__.py`)."""
    import ast
    from io import BytesIO

    with zipfile.ZipFile(BytesIO(raw)) as zf:
        src = zf.read('__init__.py').decode('utf-8', 'replace')
    tree = ast.parse(src)
    info = OrderedDict()
    for node in ast.walk(tree):
        if isinstance(node, ast.ClassDef):
            for stmt in node.body:
                if isinstance(stmt, ast.Assign) and len(stmt.targets) == 1 and isinstance(stmt.targets[0], ast.Name):
                    try:
                        info[stmt.targets[0].id] = ast.literal_eval(stmt.value)
                    except Exception:
                        pass
            if 'name' in info:
                break
    return info
