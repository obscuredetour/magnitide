"""Magnitide's host for calibre plugins.

Runs inside the user's own calibre, as `calibre-debug -e host.py`, so plugins
get calibre's Python, Qt and every calibre module, and load through calibre's
own plugin loader exactly as they do in calibre. Magnitide starts it with
CALIBRE_CONFIG_DIRECTORY pointing at its own folder, so plugin settings never
touch the user's calibre settings.

Protocol: one JSON object per line. Requests arrive on stdin as
{"id": n, "method": "...", "params": {...}}; replies go to the original
stdout as {"id": n, "result": ...} or {"id": n, "error": "..."}. Each request
runs on its own thread, so slow lookups don't hold up others. Anything
calibre or a plugin prints goes to stderr, which Magnitide keeps as the
host's log, so it can't corrupt the replies.
"""

import io
import json
import os
import sys
import threading
import traceback
from queue import Empty, Queue
from threading import Event

# Replies get a private copy of stdout; everything else printed goes to stderr.
_proto = os.fdopen(os.dup(1), 'w', encoding='utf-8', buffering=1)
os.dup2(2, 1)
sys.stdout = sys.stderr
_write_lock = threading.Lock()


def send(message):
    line = json.dumps(message, ensure_ascii=False, default=str)
    with _write_lock:
        _proto.write(line + '\n')
        _proto.flush()


from calibre.constants import __version__ as calibre_version  # noqa: E402
from calibre.customize import (  # noqa: E402
    CatalogPlugin,
    FileTypePlugin,
    InterfaceActionBase,
    MetadataReaderPlugin,
    MetadataWriterPlugin,
)
from calibre.customize.conversion import InputFormatPlugin, OutputFormatPlugin  # noqa: E402
from calibre.customize.zipplugin import loader  # noqa: E402
from calibre.ebooks.metadata.sources.base import Source, create_log  # noqa: E402

# Initialized plugins by zip path, reloaded when the zip changes. One instance
# per plugin, as in calibre: a source caches cover URLs found by identify()
# for download_cover() to use.
_plugins = {}
_plugins_lock = threading.Lock()


def plugin_kind(cls):
    for base, kind in (
        (Source, 'metadataSource'),
        (InterfaceActionBase, 'interfaceAction'),
        (FileTypePlugin, 'fileType'),
        (MetadataReaderPlugin, 'metadataReader'),
        (MetadataWriterPlugin, 'metadataWriter'),
        (InputFormatPlugin, 'conversionInput'),
        (OutputFormatPlugin, 'conversionOutput'),
        (CatalogPlugin, 'catalog'),
    ):
        if issubclass(cls, base):
            return kind
    return 'other'


def plugin(path):
    mtime = os.stat(path).st_mtime
    with _plugins_lock:
        cached = _plugins.get(path)
        if cached and cached[0] == mtime:
            return cached[1]
        cls = loader.load(path)
        instance = cls(path)
        instance.initialize()
        _plugins[path] = (mtime, instance)
        return instance


def version(v):
    return '.'.join(str(x) for x in v)


def m_hello(params):
    return {'calibre': calibre_version, 'python': sys.version.split()[0]}


def m_inspect(params):
    """What a plugin zip contains, without initializing it."""
    cls = loader.load(params['path'])
    kind = plugin_kind(cls)
    return {
        'name': cls.name,
        'version': version(cls.version),
        'author': cls.author,
        'description': cls.description or '',
        'kind': kind,
        'minimumCalibreVersion': version(cls.minimum_calibre_version),
        'capabilities': sorted(getattr(cls, 'capabilities', ()) or ()) if kind == 'metadataSource' else [],
    }


def run_with_timeout(fn, timeout):
    """Run fn() on a thread; give up (setting the abort flag) after timeout."""
    abort = Event()
    outcome = {}

    def target():
        try:
            outcome['value'] = fn(abort)
        except Exception as e:
            outcome['error'] = e

    t = threading.Thread(target=target, daemon=True)
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
    import builtins

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


def m_identify(params):
    source = plugin(params['path'])
    if not isinstance(source, Source):
        raise ValueError(f'{source.name} is not a metadata source')
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


def m_cover(params):
    source = plugin(params['path'])
    timeout = float(params.get('timeout', 30))
    buf = io.StringIO()
    log = create_log(buf)
    results = Queue()

    def go(abort):
        return source.download_cover(
            log, results, abort, timeout=timeout, get_best_cover=True, **lookup_args(params)
        )

    run_with_timeout(go, timeout + 10)
    for item in drain(results):
        data = item[1] if isinstance(item, tuple) else item
        if data:
            with open(params['out'], 'wb') as f:
                f.write(data)
            return {'bytes': len(data), 'log': buf.getvalue()[-20000:]}
    return {'bytes': 0, 'log': buf.getvalue()[-20000:]}


METHODS = {'hello': m_hello, 'inspect': m_inspect, 'identify': m_identify, 'cover': m_cover}


def handle(request):
    rid = request.get('id')
    method = METHODS.get(request.get('method'))
    if method is None:
        send({'id': rid, 'error': f"unknown method {request.get('method')!r}"})
        return
    try:
        send({'id': rid, 'result': method(request.get('params') or {})})
    except Exception as e:
        print(traceback.format_exc(), file=sys.stderr, flush=True)
        send({'id': rid, 'error': f'{type(e).__name__}: {e}'.strip()})


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            request = json.loads(line)
        except ValueError:
            continue
        if request.get('method') == 'shutdown':
            send({'id': request.get('id'), 'result': None})
            break
        threading.Thread(target=handle, args=(request,), daemon=True).start()


main()
