"""calibre's metadata helpers and the Metadata object."""

from calibre.ebooks.metadata import author_to_author_sort, authors_to_string, check_isbn, string_to_authors, title_sort
from calibre.ebooks.metadata.book.base import Metadata


def test_title_sort():
    assert title_sort('The Left Hand of Darkness') == 'Left Hand of Darkness, The'
    assert title_sort('A Memory Called Empire') == 'Memory Called Empire, A'
    assert title_sort('Dune') == 'Dune'


def test_author_sort():
    from calibre.utils.config import tweaks

    # calibre's defaults: the last word is the surname, suffixes stay at the end.
    assert author_to_author_sort('Ursula K. Le Guin') == 'Guin, Ursula K. Le'
    assert author_to_author_sort('Frank Herbert') == 'Herbert, Frank'
    assert author_to_author_sort('Martin Luther King Jr.') == 'King, Martin Luther Jr.'
    tweaks['author_use_surname_prefixes'] = True
    try:
        assert author_to_author_sort('Ursula K. Le Guin') == 'Le Guin, Ursula K.'
    finally:
        tweaks['author_use_surname_prefixes'] = False


def test_authors_strings():
    assert string_to_authors('A & B') == ['A', 'B']
    assert authors_to_string(['A', 'B']) == 'A & B'


def test_isbn():
    assert check_isbn('978-0-593-13520-4') == '9780593135204'
    assert check_isbn('0-306-40615-2') == '0306406152'
    assert check_isbn('not an isbn') is None


def test_metadata_object():
    mi = Metadata('Dune', ['Frank Herbert'])
    assert mi.title == 'Dune' and mi.authors == ['Frank Herbert']
    assert mi.is_null('series') and mi.is_null('tags')
    mi.set_identifier('isbn', '9780441013593')
    assert mi.isbn == '9780441013593'
    assert mi.get_identifiers() == {'isbn': '9780441013593'}
    mi.tags = ['SF']
    other = Metadata('Dune Messiah', ['Frank Herbert'])
    other.publisher = 'Chilton'
    mi.smart_update(other)
    assert mi.publisher == 'Chilton' and mi.tags == ['SF']
    assert 'Dune' in str(mi)
    copy = mi.deepcopy()
    copy.tags.append('Classic')
    assert mi.tags == ['SF']
