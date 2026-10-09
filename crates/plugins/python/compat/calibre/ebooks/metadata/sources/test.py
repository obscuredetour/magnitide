# Derived from calibre's src/calibre/ebooks/metadata/sources/test.py (calibre 9.15).
# Copyright: 2011, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.ebooks.metadata.sources.test`: the harness
metadata-source authors run their plugin with (`test_identify_plugin`)."""

import os
import tempfile
import time

from calibre.customize.ui import metadata_plugins
from calibre.ebooks.metadata.sources.base import create_log
from calibre.utils.date import as_utc


def isbn_test(isbn):
    from calibre.ebooks.metadata import check_isbn

    isbn_ = check_isbn(isbn)

    def test(mi):
        misbn = check_isbn(mi.isbn)
        if misbn and misbn == isbn_:
            return True
        print("ISBN test failed. Expected: '%s' found '%s'" % (isbn_, misbn))
        return False

    return test


def title_test(title, exact=False):
    title = title.lower()

    def test(mi):
        mt = mi.title.lower()
        if (exact and mt == title) or (not exact and title in mt):
            return True
        print("Title test failed. Expected: '%s' found '%s'" % (title, mt))
        return False

    return test


def authors_test(authors):
    authors = {x.lower() for x in authors}

    def test(mi):
        au = {x.lower() for x in mi.authors}
        if au == authors:
            return True
        print("Author test failed. Expected: '%s' found '%s'" % (authors, au))
        return False

    return test


def tags_test(tags):
    tags = {x.lower() for x in tags}

    def test(mi):
        t = {x.lower() for x in mi.tags}
        if t == tags:
            return True
        print("Tags test failed. Expected: '%s' found '%s'" % (tags, t))
        return False

    return test


def series_test(series, series_index):
    series = series.lower()

    def test(mi):
        ms = (mi.series or '').lower()
        if ms == series and series_index == mi.series_index:
            return True
        if mi.series:
            print(
                "Series test failed. Expected: '%s [%d]' found '%s[%d]'" % (series, series_index, ms, mi.series_index)
            )
        else:
            print("Series test failed. Expected: '%s [%d]' found no series" % (series, series_index))
        return False

    return test


def comments_test(sentinel):

    def test(mi):
        comm = mi.comments.lower() if mi.comments else ''
        if sentinel.lower() in comm:
            return True
        print('comments test failed. %s not in comments' % sentinel)
        return False

    return test


def pubdate_test(year, month, day):

    def test(mi):
        p = mi.pubdate
        if p is not None and p.year == year and p.month == month and p.day == day:
            return True
        print("pubdate test failed. Expected: '%s-%s-%s' found '%s'" % (year, month, day, p))
        return False

    return test


def init_test(tdir_name):
    tdir = tempfile.gettempdir()
    lf = os.path.join(tdir, tdir_name.replace(' ', '') + '_identify_test.txt')
    log = create_log(open(lf, 'wb'))
    return tdir, lf, log


def test_identify_plugin(name, tests, modify_plugin=lambda plugin: None, fail_missing_meta=True):
    """Run `tests`, a list of (query, [test functions]) through the plugin `name`,
    printing what each found and whether the tests passed."""
    plugin = None
    for x in metadata_plugins(['identify']):
        if x.name == name:
            plugin = x
            break
    if plugin is None:
        raise SystemExit(f'No metadata source named {name!r} is installed')
    modify_plugin(plugin)
    _tdir, lf, log = init_test(plugin.name)
    from queue import Queue

    from calibre.utils.logging import default_log

    try:
        prints = log
    except Exception:
        prints = default_log
    results = []
    for kwargs, test_funcs in tests:
        prints('Running test with:', kwargs)
        rq = Queue()
        args = (log, rq, kwargs.get('abort', __import__('threading').Event()))
        start_time = time.time()
        err = plugin.identify(
            *args,
            title=kwargs.get('title'),
            authors=kwargs.get('authors'),
            identifiers=kwargs.get('identifiers', {}),
            timeout=kwargs.get('timeout', 30),
        )
        total_time = time.time() - start_time
        results_ = []
        while True:
            try:
                results_.append(rq.get_nowait())
            except Exception:
                break
        prints('Found', len(results_), 'results in', '%.2f' % total_time, 'seconds', '(error: %s)' % err if err else '')
        match_found = None
        for mi in results_:
            if all(func(mi) for func in test_funcs):
                match_found = mi
                break
        if match_found is None:
            prints('No matching results found for:', kwargs)
            results.append((kwargs, False))
        else:
            prints('Matched:', match_found.title, 'by', ' & '.join(match_found.authors))
            if mi_date := getattr(match_found, 'pubdate', None):
                as_utc(mi_date)
            results.append((kwargs, True))
    failed = [k for k, ok in results if not ok]
    if failed:
        prints('Failed tests:', failed)
    print('Log at', lf)
    return not failed


__all__ = [
    'test_identify_plugin',
    'isbn_test',
    'title_test',
    'authors_test',
    'tags_test',
    'series_test',
    'comments_test',
    'pubdate_test',
]
