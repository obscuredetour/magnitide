"""calibre's own actions that plugins call through `gui.iactions`, as
Magnitide's: real `InterfaceAction`s (so `find_plugin('Edit Metadata')` and
`iactions['Edit Metadata']` work as in calibre) whose work is handed to
Magnitide."""

import os
import traceback

from calibre.gui2.actions import InterfaceAction
from magnitide_host.gui import state
from magnitide_host.gui.session import ask_magnitide
from magnitide_host.rpc import event


class _Mapped(InterfaceAction):
    """An action with no menu of its own; unlisted members are inert."""

    def genesis(self):
        pass

    def __getattr__(self, name):
        if name.startswith('__') or name in ('gui', 'qaction', 'menuless_qaction', 'name'):
            raise AttributeError(name)
        from magnitide_host.gui.window import Inert

        return Inert(f'gui.iactions[{self.name!r}].{name}')


class EditMetadataAction(_Mapped):
    """calibre's Edit Metadata action: its editor is Magnitide's."""

    name = 'Edit Metadata'
    action_spec = ('Edit metadata', 'edit_input.png', 'Change the title/author/cover etc. of books', None)

    def edit_metadata(self, checked=False, bulk=None):
        self.edit_metadata_for(None, self.gui.library_view.get_selected_ids(), bulk=bulk)

    def edit_metadata_for(self, rows, book_ids, bulk=None):
        if book_ids:
            ask_magnitide('edit', book_ids)

    def download_metadata(self, ids=None, ensure_fields=None):
        """calibre's metadata download: Magnitide's Find Metadata."""
        if ids is None:
            ids = self.gui.library_view.get_selected_ids()
        if ids:
            ask_magnitide('download', ids)

    def refresh_gui(self, book_ids, covers_changed=True, tag_browser_changed=True):
        self.gui.changed()

    def apply_metadata_changes(
        self, id_map, title=None, msg='', callback=None, merge_tags=True, merge_comments=False, icon=None
    ):
        """As calibre does it (EditMetadataAction.apply_mi), without its
        progress window: Metadata objects, or (OPF path, cover path) pairs."""
        from calibre.ebooks.metadata.opf2 import OPF
        from calibre.library.comments import merge_comments as merge

        db = self.gui.current_db
        applied = set()
        for book_id, mi in id_map.items():
            if not db.has_id(book_id):
                continue
            try:
                if isinstance(mi, tuple):
                    opf, cover = mi
                    if cover:
                        with open(cover, 'rb') as f:
                            db.set_cover(book_id, f, notify=False, commit=False)
                        applied.add(book_id)
                    if not opf:
                        continue
                    with open(opf, 'rb') as f:
                        mi = OPF(f, basedir=os.path.dirname(opf), populate_spine=False).to_book_metadata()
                set_title, set_authors = not mi.is_null('title'), not mi.is_null('authors')
                idents = db.get_identifiers(book_id, index_is_id=True)
                if mi.identifiers:
                    idents.update(mi.identifiers)
                mi.identifiers = idents
                if mi.is_null('series'):
                    mi.series_index = None
                if merge_tags:
                    old = db.tags(book_id, index_is_id=True)
                    if old:
                        mi.tags = list(set([x.strip() for x in old.split(',')] + (mi.tags or [])))
                if merge_comments:
                    old = db.new_api.field_for('comments', book_id)
                    if old and mi.comments and old != mi.comments:
                        mi.comments = merge(old, mi.comments)
                db.set_metadata(book_id, mi, commit=False, set_title=set_title, set_authors=set_authors, notify=False)
                applied.add(book_id)
            except Exception:
                traceback.print_exc()
        db.commit()
        self.gui.changed()
        if callback is not None:
            callback(list(applied))


class RemoveBooksAction(_Mapped):
    """calibre's Remove Books action: Magnitide asks, and moves them to the Trash."""

    name = 'Remove Books'
    action_spec = ('Remove books', 'trash.png', 'Delete books', None)

    def do_library_delete(self, to_delete_ids):
        if to_delete_ids:
            ask_magnitide('remove', to_delete_ids)

    def delete_books(self, *args):
        self.do_library_delete(self.gui.library_view.get_selected_ids())

    def library_ids_deleted(self, ids_deleted, current_row=None):
        self.gui.changed()

    def library_ids_deleted2(self, ids_deleted, next_id=None):
        self.gui.changed()


class ConvertBooksAction(_Mapped):
    """calibre's Convert Books action: conversions a plugin asks for are
    Magnitide's, run when the plugin is done (after the books it added are
    in the library). Like calibre's, these calls return at once; the
    conversion dialog (`convert_ebook`) waits for Magnitide instead."""

    name = 'Convert Books'
    action_spec = ('Convert books', 'convert.png', 'Convert books between different e-book formats', None)

    def _queue(self, book_ids, fmt, skip_if_present=True):
        db = self.gui.current_db.new_api
        fmt = fmt.upper()
        ids = [i for i in book_ids if db.has_id(i) and not (skip_if_present and fmt in db.formats(i))]
        if ids:
            event('gui-convert', plugin=state.get('plugin'), ids=ids, format=fmt)

    def auto_convert_auto_add(self, book_ids):
        """To the output format, for books that don't have it (FanFicFare)."""
        from calibre.gui2 import gprefs
        from calibre.utils.config import prefs

        self._queue(book_ids, prefs['output_format'], skip_if_present=not gprefs['auto_convert_same_fmt'])

    def auto_convert(self, book_ids, on_card, format):
        self._queue(book_ids, format)

    def convert_ebooks_to_format(self, book_ids, to_fmt):
        self._queue(book_ids, to_fmt)

    def convert_ebook(self, checked=False, bulk=None, book_ids=None):
        """The conversion dialog: Magnitide's picker, then its conversion."""
        ids = book_ids or self.gui.library_view.get_selected_ids()
        if ids:
            ask_magnitide('convert', ids)

    def do_convert(self, book_ids, bulk=None, auto_conversion=False, output_fmt=None):
        if output_fmt:
            self._queue(book_ids, output_fmt, skip_if_present=False)
        else:
            self.convert_ebook(book_ids=book_ids)


class AddBooksAction(_Mapped):
    """calibre's Add Books: books added to the shadow join the library when
    the plugin is done."""

    name = 'Add Books'
    action_spec = ('Add books', 'add_book.png', 'Add books to the calibre library/device', None)

    def _add_books(self, paths, to_device, on_card=None):
        from calibre.customize.ui import run_plugins_on_import
        from calibre.ebooks.metadata.meta import get_metadata

        db = self.gui.current_db.new_api
        added = []
        for path in paths:
            try:
                ext = os.path.splitext(path)[1][1:].lower()
                path = run_plugins_on_import(path, ext)
                with open(path, 'rb') as f:
                    mi = get_metadata(f, ext, force_read_metadata=True)
                ids, duplicates = db.add_books([(mi, {ext.upper(): path})], run_hooks=True)
                added.extend(ids)
            except Exception:
                traceback.print_exc()
        if added:
            self.gui.library_view.model().books_added(len(added))
        return added

    def add_filesystem_book(self, paths, allow_device=True, delete_after_add=()):
        if isinstance(paths, str):
            paths = [paths]
        return self._add_books(paths, False)

    def files_dropped(self, paths):
        return self._add_books(paths, False)

    def add_books(self, *args):
        from calibre.gui2 import choose_files

        paths = choose_files(self.gui, 'add books dialog', 'Select books')
        if paths:
            self._add_books(paths, False)


class ChooseLibraryAction(_Mapped):
    """calibre's Choose Library: Magnitide's open library stays open."""

    name = 'Choose Library'
    action_spec = ('Choose library', 'lt.png', 'Choose calibre library to work with', None)

    def switch_library(self, *args, **kwargs):
        self.gui.status_bar.show_message("Magnitide doesn't switch libraries for plugins", 3000)

    def stats_pretty(self, *args):
        return []


class ViewAction(_Mapped):
    """calibre's View: open the book in the default app (Magnitide opens it)."""

    name = 'View'
    action_spec = ('View', 'view.png', 'Read books', None)

    def _open(self, book_id, fmt=None):
        event('gui-open-book', plugin=state.get('plugin'), id=book_id, format=fmt)

    def view_book(self, *args):
        for i in self.gui.library_view.get_selected_ids():
            self._open(i)

    def view_format(self, row, fmt):
        self._open(self.gui.library_view.model().id(row), fmt)

    def view_format_by_id(self, book_id, fmt, open_at=None):
        self._open(book_id, fmt)

    def view_specific_format(self, *args):
        self.view_book()

    def _view_books(self, ids):
        for i in ids:
            self._open(i)


_BUILTIN = (
    ('Edit Metadata', 'EditMetadataAction'),
    ('Remove Books', 'RemoveBooksAction'),
    ('Convert Books', 'ConvertBooksAction'),
    ('Add Books', 'AddBooksAction'),
    ('Choose Library', 'ChooseLibraryAction'),
    ('View', 'ViewAction'),
)
_builtin_bases = None


def builtin_bases():
    """The stand-ins for calibre's own actions as `InterfaceActionBase`
    plugins, for loading into the window under a real calibre (whose own
    builtins drive calibre's GUI, not ours)."""
    global _builtin_bases
    if _builtin_bases is None:
        from calibre.customize import InterfaceActionBase

        _builtin_bases = []
        for name, cls_name in _BUILTIN:
            base = type(
                'Magnitide' + cls_name,
                (InterfaceActionBase,),
                {
                    'name': name,
                    'author': 'Magnitide',
                    'description': f"calibre's {name}, as Magnitide's",
                    'actual_plugin': f'magnitide_host.gui.actions:{cls_name}',
                    'can_be_disabled': False,
                },
            )
            _builtin_bases.append(base(None))
    return _builtin_bases
