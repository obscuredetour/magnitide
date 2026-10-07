"""A file-type plugin for Magnitide's plugin host tests: rewrites TXT files
as they are added (through a temporary file, as rewriting plugins do),
and marks books after they're added, converted or lose a format. It has
settings of its own (the suffix it writes)."""

import os

from calibre.customize import FileTypePlugin
from calibre.utils.config import JSONConfig

prefs = JSONConfig('plugins/magnitide_test_filetype')
prefs.defaults['suffix'] = 'hooked'


class TestFileType(FileTypePlugin):
    name = 'Magnitide Test File Type'
    description = 'Rewrites TXT files on add and tags them afterwards'
    author = 'Magnitide'
    version = (1, 0, 0)
    minimum_calibre_version = (5, 0, 0)
    supported_platforms = ['windows', 'osx', 'linux']
    file_types = {'txt'}
    on_import = True
    on_postimport = True
    on_postadd = True
    on_postconvert = True
    on_postdelete = True
    priority = 2

    def run(self, path_to_ebook):
        if 'crash' in os.path.basename(path_to_ebook).lower():
            raise ValueError('crashed on purpose')
        with open(path_to_ebook, 'rb') as f:
            data = f.read()
        out = self.temporary_file('.txt')
        out.write(data + f'\n[{prefs["suffix"]}]'.encode())
        out.close()
        return out.name

    def _tag(self, db, book_id, tag):
        tags = sorted(set(db.new_api.field_for('tags', book_id)) | {tag})
        db.new_api.set_field('tags', {book_id: tags})

    def postimport(self, book_id, book_format, db):
        self._tag(db, book_id, 'Hooked')

    def postadd(self, book_id, fmt_map, db):
        db.new_api.set_field('publisher', {book_id: 'Postadd ' + ','.join(sorted(fmt_map))})

    def postconvert(self, book_id, book_format, db):
        self._tag(db, book_id, f'Converted:{book_format}')

    def postdelete(self, book_id, book_format, db):
        self._tag(db, book_id, f'Deleted:{book_format}')

    def is_customizable(self):
        return True

    def config_widget(self):
        from qt.core import QLabel, QLineEdit, QVBoxLayout, QWidget

        w = QWidget()
        layout = QVBoxLayout(w)
        layout.addWidget(QLabel('Suffix written into added TXT files'))
        w.suffix = QLineEdit(prefs['suffix'], w)
        layout.addWidget(w.suffix)
        return w

    def save_settings(self, config_widget):
        prefs['suffix'] = config_widget.suffix.text()
