"""Metadata-source plugins for Find Metadata: identify and covers."""

import builtins
import io
from queue import Empty, Queue
from threading import Event, Thread

from calibre.ebooks.metadata.sources.base import Source, create_log
from magnitide_host.plugins import plugin
from magnitide_host.rpc import method


def run_with_timeout(fn, timeout):
    """Run fn() on a thread; give up (setting the abort flag) after timeout."""
    abort = Event()
    outcome = {}

    def target():
        try:
            outcome['value'] = fn(abort)
        except Exception as e:
            outcome['error'] = e

    t = Thread(target=target, daemon=True)
    t.start()
    t.join(timeout)
    if t.is_alive():
        abort.set()
        raise TimeoutError(f'the plugin took longer than {timeout:g} seconds')
    if 'error' in outcome:
        raise outcome['error']
    return outcome.get('value')


def drain(queue):
    items = []
    while True:
        try:
            items.append(queue.get_nowait())
        except Empty:
            return items


def lookup_args(params):
    return {
        'title': params.get('title') or None,
        'authors': params.get('authors') or None,
        'identifiers': params.get('identifiers') or {},
    }


def languages(values):
    """ISO 639-2 codes, as calibre normalizes them when it merges results
    (sources may give "English" or "en"); ones calibre doesn't know are dropped."""
    from calibre.utils.localization import canonicalize_lang

    codes = []
    for value in values or []:
        code = canonicalize_lang(value)
        if code and code not in codes:
            codes.append(code)
    return codes


def record(source, mi):
    """A calibre Metadata result as Magnitide's SourceRecord fields."""
    from calibre.utils.date import is_date_undefined

    identifiers = {k: v for k, v in (mi.get_identifiers() or {}).items() if k and v}
    published = None
    if mi.pubdate is not None and not is_date_undefined(mi.pubdate):
        published = mi.pubdate.date().isoformat()
    description = mi.comments or None
    if description and not source.has_html_comments:
        from calibre.library.comments import comments_to_html

        description = comments_to_html(description)
    link, key = None, ''
    try:
        found = source.get_book_url(identifiers)
        if found:
            idtype, idval, link = found
            key = f'{idtype}:{idval}'
    except Exception:
        pass
    try:
        has_cover = bool(source.get_cached_cover_url(identifiers))
    except Exception:
        has_cover = False
    # calibre puts a translated "Unknown" where a field is missing.
    unknown = {'Unknown', getattr(builtins, '_', str)('Unknown')}
    return {
        'title': mi.title if mi.title not in unknown else '',
        'authors': [a for a in (mi.authors or []) if a not in unknown],
        'publisher': mi.publisher or None,
        'published': published,
        'languages': languages(mi.languages),
        'identifiers': identifiers,
        'tags': list(mi.tags or []),
        'series': mi.series or None,
        'seriesIndex': float(mi.series_index) if mi.series and mi.series_index is not None else None,
        'descriptionHtml': description,
        'link': link,
        'sourceKey': key,
        'hasCover': has_cover or 'cover' in source.capabilities,
        'relevance': getattr(mi, 'source_relevance', 0),
    }


@method('identify')
def m_identify(params):
    source = plugin(params['path'])
    if not isinstance(source, Source):
        raise ValueError(f'{source.name} is not a metadata source')
    if not source.is_configured():
        return {
            'records': [],
            'error': f'{source.name} needs configuring first (Settings › Plugins › Configure)',
            'unconfigured': True,
            'log': '',
        }
    timeout = float(params.get('timeout', 30))
    buf = io.StringIO()
    log = create_log(buf)
    results = Queue()

    def go(abort):
        return source.identify(log, results, abort, timeout=timeout, **lookup_args(params))

    error = run_with_timeout(go, timeout + 10)
    found = sorted(drain(results), key=lambda mi: getattr(mi, 'source_relevance', 0))
    records = []
    for mi in found:
        try:
            source.clean_downloaded_metadata(mi)
        except Exception:
            log.exception('Cleaning a result failed')
        records.append(record(source, mi))
    return {'records': records, 'error': error or None, 'log': buf.getvalue()[-20000:]}


@method('cover')
def m_cover(params):
    source = plugin(params['path'])
    timeout = float(params.get('timeout', 30))
    buf = io.StringIO()
    log = create_log(buf)
    results = Queue()

    def go(abort):
        return source.download_cover(log, results, abort, timeout=timeout, get_best_cover=True, **lookup_args(params))

    run_with_timeout(go, timeout + 10)
    for item in drain(results):
        data = item[1] if isinstance(item, tuple) else item
        if data:
            with open(params['out'], 'wb') as f:
                f.write(data)
            return {'bytes': len(data), 'log': buf.getvalue()[-20000:]}
    return {'bytes': 0, 'log': buf.getvalue()[-20000:]}
