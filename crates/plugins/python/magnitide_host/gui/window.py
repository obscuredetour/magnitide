"""The stand-in for calibre's main window: the parts plugins use, backed by
Magnitide, and inert stand-ins (logged on first use) for the rest."""

import sys

from magnitide_host.gui import state
from magnitide_host.rpc import event


class Inert:
    """A part of calibre's window that Magnitide doesn't have: any attribute
    or call works and does nothing. The first use of each is logged, to show
    what a plugin is missing."""

    _logged = set()

    def __init__(self, name):
        object.__setattr__(self, '_name', name)

    def __getattr__(self, attr):
        if attr.startswith('__'):
            raise AttributeError(attr)
        return Inert(f'{self._name}.{attr}')

    def __setattr__(self, attr, value):
        pass

    def __call__(self, *args, **kwargs):
        if self._name not in Inert._logged:
            Inert._logged.add(self._name)
            print(
                f"[magnitide] a plugin used {self._name}(), which Magnitide doesn't have; ignored",
                file=sys.stderr,
                flush=True,
            )
        return Inert(self._name + '()')

    def __bool__(self):
        return False

    def __iter__(self):
        return iter(())

    def __len__(self):
        return 0

    def __eq__(self, other):
        return other is None or isinstance(other, Inert)

    def __hash__(self):
        return hash(self._name)


def build_gui():
    from calibre.gui2.jobs import JobManager
    from calibre.gui2.keyboard import Manager
    from calibre.gui2.ui import set_gui
    from qt.core import (
        QAbstractTableModel,
        QItemSelection,
        QItemSelectionModel,
        QMainWindow,
        QModelIndex,
        Qt,
        QTableView,
    )

    class BooksModel(QAbstractTableModel):
        """calibre's book list model: every book in the library, one row each."""

        def __init__(self, gui):
            QAbstractTableModel.__init__(self, gui)
            self.gui = gui
            self.ids = []

        @property
        def db(self):
            return self.gui.current_db

        def reload(self):
            self.beginResetModel()
            self.ids = sorted(self.db.new_api.all_book_ids()) if self.db is not None else []
            self.endResetModel()

        def rowCount(self, parent=QModelIndex()):
            return 0 if parent.isValid() else len(self.ids)

        def columnCount(self, parent=QModelIndex()):
            return 1

        def data(self, index, role=Qt.ItemDataRole.DisplayRole):
            if role == Qt.ItemDataRole.DisplayRole and index.isValid():
                return self.db.new_api.field_for('title', self.ids[index.row()])

        def id(self, row):
            return self.ids[row.row() if hasattr(row, 'row') else row]

        def row(self, book_id):
            return self.ids.index(book_id)

        def count(self):
            return len(self.ids)

        @property
        def custom_columns(self):
            return self.db.field_metadata.custom_field_metadata() if self.db is not None else {}

        @property
        def column_map(self):
            return [
                'title',
                'ondevice',
                'authors',
                'size',
                'timestamp',
                'rating',
                'publisher',
                'tags',
                'series',
                'pubdate',
            ]

        def book_on_device(self, book_id):
            return None

        # Plugins tell the book list what changed; Magnitide reads it back.
        def refresh_ids(self, ids, current_row=-1):
            self.gui.changed()

        def refresh_rows(self, rows, current_row=-1):
            self.gui.changed()

        def refresh(self, reset=True):
            self.reload()
            self.gui.changed()

        def resort(self, reset=True):
            pass

        def research(self, reset=True):
            self.gui.changed()

        def books_added(self, num):
            self.reload()
            self.gui.changed()

        def books_deleted(self):
            self.reload()
            self.gui.changed()

        def current_changed(self, *args):
            pass

        def __getattr__(self, name):
            if name.startswith('__'):
                raise AttributeError(name)
            return Inert(f'gui.library_view.model().{name}')

    class LibraryView(QTableView):
        """calibre's book list; its selection is Magnitide's."""

        def __init__(self, gui):
            QTableView.__init__(self, gui)
            self.setModel(BooksModel(gui))

        def get_selected_ids(self, as_set=False):
            m = self.model()
            ids = [m.id(i) for i in self.selectionModel().selectedRows()]
            return set(ids) if as_set else ids

        def set_selection(self, ids):
            m, sel = self.model(), QItemSelection()
            rows = [m.row(i) for i in ids if i in m.ids]
            for r in rows:
                sel.select(m.index(r, 0), m.index(r, 0))
            flags = QItemSelectionModel.SelectionFlag
            self.selectionModel().select(sel, flags.ClearAndSelect | flags.Rows)
            if rows:
                self.selectionModel().setCurrentIndex(m.index(rows[0], 0), flags.NoUpdate)

        def select_rows(self, identifiers, using_ids=True, change_current=True, scroll=True):
            m = self.model()
            ids = [i for i in identifiers] if using_ids else [m.id(r) for r in identifiers]
            self.set_selection(ids)
            event('gui-select', ids=ids)

        def set_current_row(self, row=0, select=True, for_sync=False):
            m = self.model()
            if 0 <= row < len(m.ids):
                self.set_selection([m.ids[row]])

        def row_count(self):
            return self.model().count()

        def current_changed(self, *args):
            pass

        @property
        def current_id(self):
            index = self.currentIndex()
            return self.model().id(index) if index.isValid() else None

        def __getattr__(self, name):
            if name.startswith('__'):
                raise AttributeError(name)
            return Inert(f'gui.library_view.{name}')

    class StatusBar:
        def show_message(self, msg, timeout=0, show_notification=True):
            event('gui-status', plugin=state.get('plugin'), message=str(msg), timeout=timeout or 0)

        def clear_message(self):
            pass

        def __getattr__(self, name):
            if name.startswith('__'):
                raise AttributeError(name)
            return Inert(f'gui.status_bar.{name}')

    class Search:
        """calibre's search box: what Magnitide's search says, and changes a
        plugin asks for go to Magnitide."""

        def __init__(self):
            self.text = ''

        @property
        def current_text(self):
            return self.text

        def set_search_string(self, text, store_in_history=False, emit_changed=True):
            self.text = text or ''
            event('gui-search', text=self.text)

        def clear(self, emit_search=True):
            self.set_search_string('')

        def do_search(self, *args):
            event('gui-search', text=self.text)

        def __getattr__(self, name):
            if name.startswith('__'):
                raise AttributeError(name)
            return Inert(f'gui.search.{name}')

    class Actions(dict):
        """`gui.iactions`: the hosted plugins, and calibre's own actions that
        Magnitide has, by name. Others are inert stand-ins, logged when used."""

        def __missing__(self, name):
            return Inert(f'gui.iactions[{name!r}]')

    class MagnitideGui(QMainWindow):
        """Stands in for calibre's main window (never shown)."""

        def __init__(self):
            QMainWindow.__init__(self)
            self.setWindowTitle('Magnitide')
            self.db = None
            self.library_path = None
            self.keyboard = Manager(self)
            self.iactions = Actions()
            self.status_bar = StatusBar()
            self.job_manager = JobManager()
            self.library_view = LibraryView(self)
            self.search = Search()
            self.tags_view = Inert('gui.tags_view')
            self.device_manager = Inert('gui.device_manager')
            self.book_details = Inert('gui.book_details')
            self.cover_flow = Inert('gui.cover_flow')
            self.stack = Inert('gui.stack')
            self.library_broker = Inert('gui.library_broker')
            set_gui(self)

        @property
        def current_db(self):
            return self.db

        def current_view(self):
            return self.library_view

        def set_library(self, db):
            self.db = db
            self.library_path = db.library_path
            self.library_view.model().reload()

        def changed(self):
            state['changed'] = True

        def proceed_question(
            self,
            callback,
            payload,
            html_log,
            log_viewer_title,
            title,
            msg,
            det_msg='',
            show_copy_button=False,
            cancel_callback=None,
            log_is_file=False,
            checkbox_msg=None,
            checkbox_checked=False,
            action_callback=None,
            action_label=None,
            action_icon=None,
            focus_action=False,
            show_det=False,
            show_ok=False,
            icon=None,
            log_viewer_unique_name=None,
            **kw,
        ):
            """calibre's question-with-a-log: a standalone dialog here."""
            from calibre.gui2.proceed import ask

            ask(
                self,
                callback,
                payload,
                html_log,
                log_viewer_title,
                title,
                msg,
                det_msg=det_msg,
                show_copy_button=show_copy_button,
                cancel_callback=cancel_callback,
                log_is_file=log_is_file,
                checkbox_msg=checkbox_msg,
                checkbox_checked=checkbox_checked,
                action_callback=action_callback,
                action_label=action_label,
                action_icon=action_icon,
                show_det=show_det,
                show_ok=show_ok,
                icon=icon,
            )

        def __getattr__(self, name):
            if name.startswith('__'):
                raise AttributeError(name)
            return Inert(f'gui.{name}')

    gui = MagnitideGui()
    state['window'] = gui
    return gui
