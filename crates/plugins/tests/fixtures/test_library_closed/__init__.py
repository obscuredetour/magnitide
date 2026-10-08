"""A library-closed plugin for Magnitide's plugin host tests: leaves a note
in the library when it closes."""

import os

from calibre.customize import LibraryClosedPlugin


class TestLibraryClosed(LibraryClosedPlugin):
    name = 'Magnitide Test Library Closed'
    description = 'Writes a note when the library closes'
    author = 'Magnitide'
    version = (1, 0, 0)
    minimum_calibre_version = (5, 0, 0)
    supported_platforms = ['windows', 'osx', 'linux']

    def run(self, db):
        count = len(db.new_api.all_book_ids())
        with open(os.path.join(db.library_path, 'closed-by-plugin.txt'), 'w') as f:
            f.write(f'{count} books')
