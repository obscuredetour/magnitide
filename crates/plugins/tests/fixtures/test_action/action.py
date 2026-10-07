import io

from qt.core import QBuffer, QByteArray, QColor, QDialog, QImage, QIODevice, QLabel, QMenu, QTimer, QVBoxLayout

from calibre.ebooks.metadata.book.base import Metadata
from calibre.gui2.actions import InterfaceAction


def cover_png():
    """A small solid cover, as PNG (calibre stores it as JPEG)."""
    image = QImage(60, 90, QImage.Format.Format_RGB32)
    image.fill(QColor('#3366aa'))
    data = QByteArray()
    buf = QBuffer(data)
    buf.open(QIODevice.OpenModeFlag.WriteOnly)
    image.save(buf, 'PNG')
    return bytes(data)


class TestAction(InterfaceAction):
    name = 'Magnitide Test Action'
    action_spec = ('Test Action', None, 'Change the selected books', None)
    action_type = 'current'

    def genesis(self):
        self.menu_actions = []
        self.menu = QMenu(self.gui)
        self.qaction.setMenu(self.menu)
        self.qaction.triggered.connect(self.mark)
        # Rebuilt each time it's shown, as many plugins do.
        self.menu.aboutToShow.connect(self.rebuild_menu)
        self.rebuild_menu()

    def rebuild_menu(self):
        # As kiwidude's plugins do: unregister the old items' shortcuts first.
        for ac in self.menu_actions:
            self.gui.keyboard.unregister_shortcut(ac.calibre_shortcut_unique_name)
        self.menu_actions = []
        m = self.menu
        m.clear()

        def add(name, text, triggered):
            self.menu_actions.append(self.create_menu_action(m, name, text, triggered=triggered))

        add('mark', '&Mark selected', self.mark)
        add('pages', 'Count pages', self.count_pages)
        add('rewrite', 'Rewrite EPUB', self.rewrite_epub)
        add('cover', 'Set cover', self.set_cover)
        m.addSeparator()
        add('add', 'Add a book', self.add_book)
        add('status', 'Say hello', self.say_hello)
        add('dialog', 'Open dialog…', self.open_dialog)
        m.addSeparator()
        add('new_edit', 'New book, then edit it', self.new_then_edit)
        add('remove', 'Remove selected', self.remove_selected)
        add('apply', 'Apply metadata changes', self.apply_changes)
        sub = m.addMenu('More')
        off = sub.addAction('Unavailable')
        off.setEnabled(False)
        on = sub.addAction('A setting')
        on.setCheckable(True)
        on.setChecked(True)
        self.gui.keyboard.finalize()

    def selected(self):
        return self.gui.library_view.get_selected_ids()

    def mark(self):
        db = self.gui.current_db.new_api
        ids = self.selected()
        db.set_field('title', {i: db.field_for('title', i) + ' (marked)' for i in ids})
        # The legacy API too, as most plugins use it.
        legacy = self.gui.current_db
        for i in ids:
            mi = legacy.get_metadata(i, index_is_id=True)
            mi.tags = list(mi.tags) + ['Marked']
            legacy.set_metadata(i, mi, commit=True)
        self.gui.library_view.model().refresh_ids(ids)

    def count_pages(self):
        db = self.gui.current_db.new_api
        if '#pages' not in db.field_metadata.custom_field_keys():
            self.gui.status_bar.show_message('Create a #pages column first', 3000)
            return
        db.set_field('#pages', {i: 100 + i for i in self.selected()})
        self.gui.library_view.model().refresh_ids(self.selected())

    def rewrite_epub(self):
        db = self.gui.current_db.new_api
        for i in self.selected():
            path = db.format_abspath(i, 'EPUB')
            if path:
                with open(path, 'ab') as f:
                    f.write(b' + metadata')

    def set_cover(self):
        db = self.gui.current_db.new_api
        png = cover_png()
        db.set_cover({i: png for i in self.selected()})

    def add_book(self):
        db = self.gui.current_db.new_api
        new_id = db.create_book_entry(Metadata('Added by a Plugin', ['Plugin Author']))
        db.add_format(new_id, 'TXT', io.BytesIO(b'hello from a plugin'), run_hooks=False)
        self.gui.library_view.model().books_added(1)

    def say_hello(self):
        self.gui.status_bar.show_message(f'Hello from the test plugin ({len(self.selected())} selected)', 3000)

    def open_dialog(self):
        d = QDialog(self.gui)
        d.setWindowTitle('Test plugin dialog')
        QVBoxLayout(d).addWidget(QLabel('Closes by itself.'))
        # Closes itself, so the host has to wait for it before reporting idle.
        QTimer.singleShot(1500, d.accept)
        if d.exec() == QDialog.DialogCode.Accepted:
            self.mark()

    def new_then_edit(self):
        """As EpubMerge does: make the book, select it, have it edited (the
        plugin waits), then build its file from what was saved."""
        db = self.gui.current_db.new_api
        new_id = db.create_book_entry(Metadata('Untitled Merge', ['Plugin Author']))
        self.gui.library_view.model().books_added(1)
        self.gui.library_view.select_rows([new_id])
        self.gui.iactions['Edit Metadata'].edit_metadata(False)
        title = db.field_for('title', new_id)
        db.add_format(new_id, 'TXT', io.BytesIO(f'Merged as {title}'.encode()), run_hooks=False)
        self.gui.status_bar.show_message(f'Merged as “{title}”', 3000)

    def remove_selected(self):
        """As EpubMerge and FanFicFare do: hand books to Remove Books."""
        ids = self.selected()
        self.gui.iactions['Remove Books'].do_library_delete(ids)
        db = self.gui.current_db.new_api
        gone = [i for i in ids if not db.has_id(i)]
        self.gui.status_bar.show_message(f'Removed {len(gone)} of {len(ids)}', 3000)

    def apply_changes(self):
        """As kiwidude's plugins do: a batch of metadata, merged into what's there."""
        id_map = {}
        for i in self.selected():
            mi = Metadata(_('Unknown'))
            mi.tags = ['Applied']
            mi.publisher = 'Plugin Press'
            id_map[i] = mi

        def done(applied):
            self.gui.status_bar.show_message(f'Applied to {len(applied)}', 3000)

        self.gui.iactions['Edit Metadata'].apply_metadata_changes(id_map, callback=done)
