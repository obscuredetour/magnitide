"""Mirrors the names plugins import from calibre's `calibre.gui2.library.models`:
`BooksModel` (the book list's model; the host's own model is an instance of
a compatible class) and `DeviceBooksModel`."""

from qt.core import QAbstractTableModel, QModelIndex, Qt, pyqtSignal


class BooksModel(QAbstractTableModel):
    """The base of the book list model. `gui.library_view.model()` in
    Magnitide is the host's model, which offers `db`, `id(row)`,
    `row(book_id)`, `refresh_ids`, `books_added`, `refresh`, `count`."""

    about_to_be_sorted = pyqtSignal(object, name='aboutToBeSorted')
    sorting_done = pyqtSignal(object, name='sortingDone')
    database_changed = pyqtSignal(object, name='databaseChanged')
    new_bookdisplay_data = pyqtSignal(object)
    count_changed_signal = pyqtSignal(int)
    searched = pyqtSignal(object)
    search_done = pyqtSignal()

    orig_headers = {
        'title': 'Title',
        'ondevice': 'On Device',
        'authors': 'Author(s)',
        'size': 'Size (MB)',
        'timestamp': 'Date',
        'pubdate': 'Published',
        'rating': 'Rating',
        'publisher': 'Publisher',
        'tags': 'Tags',
        'series': 'Series',
        'last_modified': 'Modified',
        'languages': 'Languages',
        'formats': 'Formats',
        'id': 'Id',
        'path': 'Path',
    }

    def __init__(self, parent=None, buffer=40):
        QAbstractTableModel.__init__(self, parent)
        self.db = None
        self.column_map = []
        self.headers = {}
        self.ids = []

    def set_database(self, db):
        self.db = db

    def rowCount(self, parent=QModelIndex()):
        return 0 if parent.isValid() else len(self.ids)

    def columnCount(self, parent=QModelIndex()):
        return len(self.column_map) or 1

    def data(self, index, role=Qt.ItemDataRole.DisplayRole):
        return None

    def id(self, row):
        return self.ids[row.row() if hasattr(row, 'row') else row]

    def row(self, book_id):
        return self.ids.index(book_id)

    def count(self):
        return len(self.ids)

    def refresh_ids(self, ids, current_row=-1):
        pass

    def refresh(self, reset=True):
        pass

    def books_added(self, num):
        pass

    def books_deleted(self):
        pass

    def set_book_on_device_func(self, func):
        pass


class DeviceBooksModel(BooksModel):
    pass


__all__ = ['BooksModel', 'DeviceBooksModel']
