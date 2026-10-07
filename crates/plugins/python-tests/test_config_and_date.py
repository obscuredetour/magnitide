"""JSONConfig, tweaks and dates."""

import datetime

from calibre.utils.config import JSONConfig, prefs, tweaks
from calibre.utils.date import (
    UNDEFINED_DATE,
    as_utc,
    format_date,
    is_date_undefined,
    isoformat,
    parse_date,
    parse_only_date,
)


def test_json_config_round_trips():
    c = JSONConfig('plugins/magnitide_compat_test')
    c.defaults['n'] = 1
    assert c['n'] == 1
    c['n'] = 5
    c['name'] = 'x'
    again = JSONConfig('plugins/magnitide_compat_test')
    assert again['n'] == 5 and again['name'] == 'x'
    assert again.get('missing', 'd') == 'd'


def test_prefs_and_tweaks_have_calibres_defaults():
    assert prefs['output_format'].upper() in ('EPUB', 'MOBI', 'AZW3')
    assert tweaks['author_sort_copy_method'] in ('invert', 'copy', 'comma', 'nocomma')


def test_dates():
    d = parse_date('2021-05-04', assume_utc=True)
    assert (d.year, d.month, d.day) == (2021, 5, 4)
    assert isoformat(d).startswith('2021-05-04')
    assert parse_only_date('2021-05-04').day == 4
    assert is_date_undefined(UNDEFINED_DATE)
    assert format_date(d, 'dd MMM yyyy', as_utc=True) == '04 May 2021'
    assert as_utc(datetime.datetime(2021, 5, 4, tzinfo=datetime.UTC)).tzinfo is not None
