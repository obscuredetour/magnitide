"""Collation without ICU, and the template language subset."""

from calibre.ebooks.metadata.book.base import Metadata
from calibre.ebooks.metadata.book.formatter import SafeFormat
from calibre.utils.icu import lower, numeric_sort_key, primary_sort_key, sort_key, strcmp, upper


def test_sort_keys():
    assert sorted(['banana', 'Apple', 'cherry'], key=sort_key) == ['Apple', 'banana', 'cherry']
    assert sorted(['a10', 'a2', 'a1'], key=numeric_sort_key) == ['a1', 'a2', 'a10']
    assert primary_sort_key('Éclair') == primary_sort_key('eclair')
    assert strcmp('a', 'B') < 0 and strcmp('b', 'B') == 0
    assert upper('straße') == 'STRASSE' and lower('ÀB') == 'àb'


def test_templates():
    mi = Metadata('Dune', ['Frank Herbert'])
    mi.series, mi.series_index = 'Dune', 1
    mi.tags = ['SF', 'Classic']
    f = SafeFormat()
    assert f.safe_format('{title} by {authors}', mi, 'ERR', mi) == 'Dune by Frank Herbert'
    assert f.safe_format('{title}{series:| (|)}{series_index}', mi, 'ERR', mi) == 'Dune (Dune)1'
    assert f.safe_format('{authors:re(Frank,F.)}', mi, 'ERR', mi) == 'F. Herbert'
    assert f.safe_format('{title:uppercase()}', mi, 'ERR', mi) == 'DUNE'
    assert f.safe_format('{publisher:ifempty(none)}', mi, 'ERR', mi) == 'none'
    assert 'ERR' in f.safe_format('{title:bogus()}', mi, 'ERR', mi)
