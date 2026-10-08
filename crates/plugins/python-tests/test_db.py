"""calibre's database API on a shadow library (the calibre-compat crate's fixture)."""

from calibre.db.legacy import LibraryDatabase
from calibre.ebooks.metadata.book.base import Metadata


def test_reads_and_writes(shadow_library):
    db = LibraryDatabase(shadow_library)
    api = db.new_api
    ids = api.all_book_ids()
    assert ids
    book_id = min(ids)
    title = api.field_for('title', book_id)
    assert isinstance(title, str) and title
    assert db.title(book_id, index_is_id=True) == title
    mi = db.get_metadata(book_id, index_is_id=True)
    assert mi.title == title and mi.authors
    api.set_field('title', {book_id: 'Renamed by a Test'})
    api.set_field('tags', {book_id: ['Tested', 'Twice']})
    assert api.field_for('title', book_id) == 'Renamed by a Test'
    assert set(api.field_for('tags', book_id)) == {'Tested', 'Twice'}
    assert api.search('title:"=Renamed by a Test"') == {book_id}
    assert book_id in api.search('tags:Tested')
    assert api.search('tags:Tested and not title:Renamed') == set()
    new_id = api.create_book_entry(Metadata('Made by a Test', ['Test Author']))
    assert new_id in api.all_book_ids()
    assert api.field_for('authors', new_id) == ('Test Author',)
    api.remove_books([new_id], permanent=True)
    assert new_id not in api.all_book_ids()
    # Legacy API: the same data through the old names.
    assert db.all_ids() and 'Tested' in db.tags(book_id, index_is_id=True)
    db.close()
    again = LibraryDatabase(shadow_library)
    assert again.new_api.field_for('title', book_id) == 'Renamed by a Test', 'written through'
    again.close()


def test_custom_columns(shadow_library):
    db = LibraryDatabase(shadow_library)
    api = db.new_api
    api.create_custom_column('pages', 'Pages', 'int', False)
    db.field_metadata = api.field_metadata
    assert '#pages' in api.field_metadata.custom_field_keys()
    book_id = min(api.all_book_ids())
    api.set_field('#pages', {book_id: 321})
    assert api.field_for('#pages', book_id) == 321
    assert api.search('#pages:>300') == {book_id}
    db.close()
