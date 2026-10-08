"""Mirrors calibre's `calibre.customize` (calibre 9.15): the plugin base
classes every calibre plugin subclasses, with the attributes and methods
calibre's plugin API documents (https://manual.calibre-ebook.com/plugins.html).
"""

import os
import sys
import zipfile
from contextlib import suppress
from enum import IntEnum

from calibre.constants import ismacos, iswindows, numeric_version

if iswindows:
    platform = 'windows'
elif ismacos:
    platform = 'osx'
else:
    platform = 'linux'


class PluginNotFound(ValueError):
    pass


class InvalidPlugin(ValueError):
    pass


class PluginInstallationType(IntEnum):
    EXTERNAL = 1
    SYSTEM = 2
    BUILTIN = 3


class Plugin:
    """A calibre plugin. Subclass it (or one of the kinds below) and set the
    attributes; calibre's API reference documents each."""

    #: List of platforms this plugin works on. For example: ``['windows', 'osx', 'linux']``
    supported_platforms = []

    #: The name of this plugin. You must set it something other than Trivial Plugin for it to work.
    name = 'Trivial Plugin'

    #: The version of this plugin as a 3-tuple (major, minor, revision)
    version = (1, 0, 0)

    #: A short string describing what this plugin does
    description = 'Does absolutely nothing'

    #: The author of this plugin
    author = 'Unknown'

    #: When more than one plugin exists for a filetype, the plugins are run in order of decreasing priority.
    priority = 1

    #: The earliest version of calibre this plugin requires
    minimum_calibre_version = (0, 4, 118)

    #: The way this plugin is installed
    installation_type = None

    #: If False, the user will not be able to disable this plugin.
    can_be_disabled = True

    #: The type of this plugin. Used for categorizing plugins in the GUI
    type = 'Base'

    def __init__(self, plugin_path):
        self.plugin_path = plugin_path
        self.site_customization = None

    def initialize(self):
        """Called once when calibre plugins are initialized. Plugins are
        re-initialized every time a new plugin is added."""
        pass

    def config_widget(self):
        """A QWidget for configuring the plugin, or None."""
        raise NotImplementedError()

    def save_settings(self, config_widget):
        """Save the settings specified by the user with config_widget."""
        raise NotImplementedError()

    def do_user_config(self, parent=None):
        """Show a dialog to configure the plugin: its `config_widget()`, or
        a text box for the site customization string when it has none.
        Returns True if the user saved."""
        from qt.core import QDialog, QDialogButtonBox, QLabel, QLineEdit, QScrollArea, QVBoxLayout

        prefname = 'plugin config dialog:' + self.type + ':' + self.name
        geom = None
        with suppress(Exception):
            from calibre.gui2 import gprefs

            geom = gprefs.get(prefname, None)

        config_dialog = QDialog(parent)
        button_box = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel)
        v = QVBoxLayout(config_dialog)

        def size_dialog():
            if geom is None:
                config_dialog.resize(config_dialog.sizeHint())
            else:
                from qt.core import QApplication

                with suppress(Exception):
                    config_dialog.restoreGeometry(bytes(geom))
                    QApplication.instance()

        button_box.accepted.connect(config_dialog.accept)
        button_box.rejected.connect(config_dialog.reject)
        config_dialog.setWindowTitle('Customize ' + self.name)
        try:
            config_widget = self.config_widget()
        except NotImplementedError:
            config_widget = None

        if isinstance(config_widget, tuple):
            from calibre.gui2 import warning_dialog

            warning_dialog(parent, 'Cannot configure', config_widget[0], det_msg=config_widget[1], show=True)
            return False

        if config_widget is not None:
            sw = QScrollArea(config_dialog)
            sw.setWidget(config_widget)
            sw.setWidgetResizable(True)
            v.addWidget(sw)
            v.addWidget(button_box)
            size_dialog()
            config_dialog.exec()

            if config_dialog.result() == QDialog.DialogCode.Accepted:
                if hasattr(config_widget, 'validate'):
                    if config_widget.validate():
                        self.save_settings(config_widget)
                else:
                    self.save_settings(config_widget)
        else:
            from calibre.customize.ui import customize_plugin, plugin_customization

            help_text = self.customization_help(gui=True)
            help_text = QLabel(help_text, config_dialog)
            help_text.setWordWrap(True)
            help_text.setOpenExternalLinks(True)
            v.addWidget(help_text)
            sc = plugin_customization(self)
            if not sc:
                sc = ''
            sc = sc.strip()
            sc = QLineEdit(sc, config_dialog)
            v.addWidget(sc)
            v.addWidget(button_box)
            size_dialog()
            config_dialog.exec()

            if config_dialog.result() == QDialog.DialogCode.Accepted:
                sc = str(sc.text()).strip()
                customize_plugin(self, sc)

        with suppress(Exception):
            from calibre.gui2 import gprefs

            gprefs[prefname] = bytearray(config_dialog.saveGeometry())
        return config_dialog.result()

    def load_resources(self, names):
        """Return a dict of name → bytes for the files in the plugin zip."""
        if self.plugin_path is None:
            raise ValueError('This plugin was not loaded from a ZIP file')
        ans = {}
        with zipfile.ZipFile(self.plugin_path, 'r') as zf:
            for candidate in zf.namelist():
                if candidate in names:
                    ans[candidate] = zf.read(candidate)
        return ans

    def customization_help(self, gui=False):
        """A string describing how to customize this plugin, or raise
        NotImplementedError when there is nothing to customize."""
        raise NotImplementedError()

    def temporary_file(self, suffix):
        """A temporary file whose path stays valid after it's closed; the
        file object has a `name` attribute."""
        from calibre.ptempfile import PersistentTemporaryFile

        return PersistentTemporaryFile(suffix)

    def is_customizable(self):
        try:
            self.customization_help()
            return True
        except NotImplementedError:
            return False

    def __enter__(self, *args):
        """Puts the plugin's zip on sys.path, for code that imports from it directly."""
        if self.plugin_path is not None:
            from calibre.utils.zipfile import ZipFile

            zf = ZipFile(self.plugin_path)
            extensions = {x.rpartition('.')[-1].lower() for x in zf.namelist()}
            zip_safe = True
            for ext in ('pyd', 'so', 'dll', 'dylib'):
                if ext in extensions:
                    zip_safe = False
                    break
            if zip_safe:
                sys.path.insert(0, self.plugin_path)
                self.sys_insertion_path = self.plugin_path
            else:
                from calibre.ptempfile import TemporaryDirectory

                self._sys_insertion_tdir = TemporaryDirectory('plugin_unzip')
                self.sys_insertion_path = self._sys_insertion_tdir.__enter__(*args)
                zf.extractall(self.sys_insertion_path)
                sys.path.insert(0, self.sys_insertion_path)
            zf.close()

    def __exit__(self, *args):
        ip, it = getattr(self, 'sys_insertion_path', None), getattr(self, '_sys_insertion_tdir', None)
        if ip in sys.path:
            sys.path.remove(ip)
        if hasattr(it, '__exit__'):
            it.__exit__(*args)

    def cli_main(self, args):
        """Run as `calibre-debug -r "Plugin Name" -- args`."""
        raise NotImplementedError('The %s plugin has no command line interface' % self.name)

    def __repr__(self):
        return f'<{self.__class__.__name__} {self.name!r} v{".".join(map(str, self.version))}>'


class FileTypePlugin(Plugin):
    """A plugin that runs on files of certain types (when they're added,
    before or after conversion, …)."""

    #: Set of file types for which this plugin should be run. Use '*' for all.
    file_types = set()

    #: If True, this plugin is run when books are added to the database
    on_import = False

    #: If True, this plugin is run after books are added to the database.
    on_postimport = False

    #: If True, this plugin is run after a book is converted
    on_postconvert = False

    #: If True, this plugin is run after a book file is deleted from the database
    on_postdelete = False

    #: If True, this plugin is run just before a conversion
    on_preprocess = False

    #: If True, this plugin is run after conversion, on the final file
    on_postprocess = False

    type = 'File type'

    def run(self, path_to_ebook):
        """Run the plugin on `path_to_ebook`; return the path of the file to
        use instead (may be the same)."""
        return path_to_ebook

    def postimport(self, book_id, book_format, db):
        pass

    def postconvert(self, book_id, book_format, db):
        pass

    def postdelete(self, book_id, book_format, db):
        pass

    def postadd(self, book_id, fmt_map, db):
        pass


class MetadataReaderPlugin(Plugin):
    """A plugin that reads metadata from files of certain types."""

    file_types = set()
    supported_platforms = ['windows', 'osx', 'linux']
    version = numeric_version
    author = 'Kovid Goyal'
    type = 'Metadata reader'

    def __init__(self, *args, **kwargs):
        Plugin.__init__(self, *args, **kwargs)
        self.quick = False

    def get_metadata(self, stream, type):
        """Return a `Metadata` for the file in `stream` (of `type`), or raise."""
        return None


class MetadataWriterPlugin(Plugin):
    """A plugin that writes metadata into files of certain types."""

    file_types = set()
    supported_platforms = ['windows', 'osx', 'linux']
    version = numeric_version
    author = 'Kovid Goyal'
    type = 'Metadata writer'

    def __init__(self, *args, **kwargs):
        Plugin.__init__(self, *args, **kwargs)
        self.apply_null = False

    def set_metadata(self, stream, mi, type):
        pass


class CatalogPlugin(Plugin):
    """A plugin that generates a catalog of the library (not run by Magnitide yet)."""

    resources_path = None
    file_types = set()
    type = 'Catalog generator'
    cli_options = []

    def _field_sorter(self, key):

        if key.startswith('#'):
            return '~%s' % key[1:]
        return key

    def search_sort_db(self, db, opts):
        db.search(opts.search_text)
        if opts.sort_by:
            db.sort(opts.sort_by, True)
        return db.get_data_as_dict(ids=opts.ids)

    def get_output_fields(self, db, opts):
        all_std_fields = {
            'author_sort',
            'authors',
            'comments',
            'cover',
            'formats',
            'id',
            'isbn',
            'library_name',
            'ondevice',
            'pubdate',
            'publisher',
            'rating',
            'series_index',
            'series',
            'size',
            'tags',
            'timestamp',
            'title_sort',
            'title',
            'uuid',
            'languages',
            'identifiers',
        }
        all_custom_fields = set(db.custom_field_keys())
        for field in list(all_custom_fields):
            fm = db.field_metadata[field]
            if fm['datatype'] == 'series':
                all_custom_fields.add(field + '_index')
        all_fields = all_std_fields.union(all_custom_fields)

        if opts.fields != 'all':
            fields = {x.strip() for x in opts.fields.split(',')}
            fields = fields.intersection(all_fields)
        else:
            fields = all_fields

        if not opts.connected_device['is_device_connected'] and 'ondevice' in fields:
            fields.remove('ondevice')
        return sorted(fields, key=self._field_sorter)

    def initialize(self):
        """Copy the plugin's resources (the zip's files) to a folder, for
        catalogs that build from templates."""
        from calibre.ptempfile import PersistentTemporaryDirectory

        if self.plugin_path is not None:
            self.resources_path = PersistentTemporaryDirectory('_plugin_resources')
            with zipfile.ZipFile(self.plugin_path, 'r') as zf:
                for f in zf.namelist():
                    if f.endswith('/'):
                        continue
                    target = os.path.join(self.resources_path, *f.split('/'))
                    os.makedirs(os.path.dirname(target), exist_ok=True)
                    with open(target, 'wb') as out:
                        out.write(zf.read(f))

    def run(self, path_to_output, opts, db, ids, notification=None):
        raise NotImplementedError('CatalogPlugin.generate_catalog() must be overridden')


class InterfaceActionBase(Plugin):
    """The installable half of a GUI plugin: names the `InterfaceAction`
    class (`actual_plugin`, as 'calibre_plugins.name.module:Class') that
    calibre loads once its window exists."""

    supported_platforms = ['windows', 'osx', 'linux']
    author = 'Kovid Goyal'
    type = 'User interface action'
    can_be_disabled = False

    actual_plugin = None

    def __init__(self, *args, **kwargs):
        Plugin.__init__(self, *args, **kwargs)
        self.actual_plugin_ = None

    def load_actual_plugin(self, gui):
        """Import and instantiate the `InterfaceAction` named by `actual_plugin`."""
        ac = self.actual_plugin_
        if ac is None:
            mod, cls = self.actual_plugin.split(':')
            import importlib

            ac = getattr(importlib.import_module(mod), cls)(gui, self.site_customization)
            self.actual_plugin_ = ac
        return ac

    def actual_plugin_object(self):
        return self.actual_plugin_


class PreferencesPlugin(Plugin):
    """A plugin that adds a page to calibre's Preferences. In Magnitide the
    page is shown from Settings › Plugins › Configure."""

    supported_platforms = ['windows', 'osx', 'linux']
    author = 'Kovid Goyal'
    type = 'Preferences'
    can_be_disabled = False

    #: Import path to module that contains a class named ConfigWidget which
    #: implements the ConfigWidgetInterface.
    config_widget = None

    category_order = 100
    name_order = 100
    category = None
    gui_category = None
    gui_name = None
    icon = None
    description = None

    def create_widget(self, parent=None):
        """The settings widget (a `ConfigWidgetInterface`)."""
        base, _, wc = self.config_widget.partition(':')
        if not wc:
            wc = 'ConfigWidget'
        import importlib

        base = importlib.import_module(base)
        widget = getattr(base, wc)(parent)
        return widget  # calibre's Preferences dialog calls genesis(gui) and initialize() itself


class StoreBase(Plugin):
    """The base of calibre's Get Books store plugins (not run by Magnitide)."""

    supported_platforms = ['windows', 'osx', 'linux']
    author = 'John Schember'
    type = 'Store'
    minimum_calibre_version = (0, 8, 0)
    version = (1, 0, 1)

    actual_plugin = None
    headquarters = ''
    formats = []
    drm_free_only = False
    affiliate = False

    def load_actual_plugin(self, gui):
        mod, cls = self.actual_plugin.split(':')
        import importlib

        self.actual_plugin_object = getattr(importlib.import_module(mod), cls)(gui, self.name)
        return self.actual_plugin_object

    def customization_help(self, gui=False):
        if getattr(self, 'actual_plugin_object', None) is not None:
            return self.actual_plugin_object.customization_help(gui)
        raise NotImplementedError()

    def config_widget(self):
        if getattr(self, 'actual_plugin_object', None) is not None:
            return self.actual_plugin_object.config_widget()
        raise NotImplementedError()

    def save_settings(self, config_widget):
        if getattr(self, 'actual_plugin_object', None) is not None:
            return self.actual_plugin_object.save_settings(config_widget)
        raise NotImplementedError()


class ViewerPlugin(Plugin):
    """calibre's E-book viewer plugins (Magnitide has no viewer; not run)."""

    type = 'Viewer'

    def load_javascript(self, evaljs):
        pass

    def load_fonts(self):
        pass

    def run_javascript(self, evaljs):
        pass

    def customize_ui(self, ui):
        pass

    def customize_context_menu(self, menu, event, hit_test_result):
        pass


class EditBookToolPlugin(Plugin):
    """calibre's book editor tools (Magnitide has no editor; not run)."""

    type = 'Edit book tool'
    minimum_calibre_version = (1, 46, 0)


class LibraryClosedPlugin(Plugin):
    """A plugin run when a library is closed."""

    type = 'Library closed'
    minimum_calibre_version = (2, 70, 0)

    def run(self, db):
        raise NotImplementedError('LibraryClosedPlugin.run() must be overridden')


# A few plugins poke at this.
ui_plugins = {}


__all__ = [
    'Plugin',
    'FileTypePlugin',
    'MetadataReaderPlugin',
    'MetadataWriterPlugin',
    'CatalogPlugin',
    'InterfaceActionBase',
    'PreferencesPlugin',
    'StoreBase',
    'ViewerPlugin',
    'EditBookToolPlugin',
    'LibraryClosedPlugin',
    'PluginInstallationType',
    'PluginNotFound',
    'InvalidPlugin',
    'platform',
    'numeric_version',
]
