"""Magnitide's host for calibre plugins.

Runs inside the user's own calibre, as `calibre-debug -e host.py`, so plugins
get calibre's Python, Qt and every calibre module, and load through calibre's
own plugin loader exactly as they do in calibre. Magnitide starts it with
CALIBRE_CONFIG_DIRECTORY pointing at its own folder, so plugin settings never
touch the user's calibre settings.

Protocol: one JSON object per line. Requests arrive on stdin as
{"id": n, "method": "...", "params": {...}}; replies go to the original
stdout as {"id": n, "result": ...} or {"id": n, "error": "..."}, and events
nobody asked for (a GUI plugin's status message, error, or that it's done)
as {"event": "...", ...}. Each request runs on its own thread, so slow
lookups don't hold up others; GUI work runs on the main thread, in Qt. Anything
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


# ---------------------------------------------------------------------------
# GUI plugins (calibre's "interface actions").
#
# They need Qt, which lives on the main thread: it starts the first time a GUI
# plugin is used (metadata sources never need it). Each plugin gets a
# stand-in for calibre's main window (`gui`) with the parts plugins use, on a
# shadow copy of the library that Magnitide writes in calibre's format (see
# crates/plugins/src/shadow.rs). What the plugin changes there, Magnitide
# reads back once the plugin is idle and applies through its own edit path.


def event(name, **fields):
    """Something Magnitide didn't ask about: a status message, an error, idle."""
    send({'event': name, **fields})


_main_calls = Queue()  # run on the main thread before Qt has started
_qt = {}  # 'app' and 'invoker', once started
_qt_ready = Event()
_qt_lock = threading.Lock()


def on_main(fn, wait=True):
    """Run fn on the main thread, where Qt lives; return its result."""
    if threading.current_thread() is threading.main_thread():
        return fn()
    done, box = Event(), {}

    def run():
        try:
            box['value'] = fn()
        except BaseException as e:
            box['error'] = e
        finally:
            done.set()

    invoker = _qt.get('invoker')
    if invoker is not None:
        invoker.call.emit(run)
    else:
        _main_calls.put(run)
    if not wait:
        return None
    done.wait()
    if 'error' in box:
        raise box['error']
    return box.get('value')


def ensure_qt():
    with _qt_lock:
        if not _qt_ready.is_set() and 'starting' not in _qt:
            _qt['starting'] = True
            _main_calls.put(_run_qt)
    _qt_ready.wait()


def _run_qt():
    """On the main thread: start Qt and run its event loop until shutdown."""
    from qt.core import QObject, Qt, pyqtSignal

    from calibre.gui2 import Application

    class Invoker(QObject):
        call = pyqtSignal(object)

        def __init__(self):
            QObject.__init__(self)
            self.call.connect(self.run, type=Qt.ConnectionType.QueuedConnection)

        def run(self, fn):
            fn()

    # On macOS, Qt would make the host a regular app (a Dock icon, and the
    # focus taken from Magnitide as it starts); calibre's Qt leaves it alone
    # with this, as calibre does when it runs headless.
    os.environ['QT_MAC_DISABLE_FOREGROUND_APPLICATION_TRANSFORM'] = '1'
    _macos_accessory()
    app = Application(['magnitide-plugins'])
    app.setQuitOnLastWindowClosed(False)
    # An exception in a plugin's slot must not abort the process (PyQt's
    # default); report it and carry on, as calibre does.
    sys.excepthook = _excepthook
    _raise_plugin_windows(app)
    _qt['app'], _qt['invoker'] = app, Invoker()
    _qt_ready.set()
    while not _main_calls.empty():
        fn = _main_calls.get_nowait()
        if fn is not None:
            _qt['invoker'].call.emit(fn)
    app.exec()


def _objc(receiver, selector, arg=None):
    """macOS: send an Objective-C message (receiver: a class name or an
    object; one integer argument at most) through the runtime."""
    import ctypes
    import ctypes.util

    objc = ctypes.cdll.LoadLibrary(ctypes.util.find_library('objc'))
    objc.objc_getClass.restype = ctypes.c_void_p
    objc.sel_registerName.restype = ctypes.c_void_p
    if isinstance(receiver, bytes):
        receiver = objc.objc_getClass(receiver)
    args = () if arg is None else (arg,)
    # objc_msgSend must be called through a prototype matching the method.
    send = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, *(ctypes.c_long for _ in args))(
        ('objc_msgSend', objc)
    )
    return send(receiver, objc.sel_registerName(selector), *args)


def _ns_app():
    return _objc(b'NSApplication', b'sharedApplication')


def _macos_accessory():
    """No Dock icon or menu bar for the host: its windows are plugins'
    dialogs, which belong with Magnitide (NSApplicationActivationPolicyAccessory)."""
    if sys.platform == 'darwin':
        try:
            _objc(_ns_app(), b'setActivationPolicy:', 1)
        except Exception:
            traceback.print_exc()


def _bring_to_front(window):
    """A plugin opened a window: show it in front of Magnitide, whose menu
    item the user just chose."""
    if sys.platform == 'darwin':
        try:
            _objc(_ns_app(), b'activateIgnoringOtherApps:', 1)
        except Exception:
            traceback.print_exc()
    window.raise_()
    window.activateWindow()


def _return_focus():
    """The plugin is done: if its windows had the focus, give it back to
    Magnitide (which started this host and says who it is)."""
    if sys.platform != 'darwin':
        return
    try:
        if not (_objc(_ns_app(), b'isActive') or 0) & 0xFF:
            return
        pid = int(os.environ.get('MAGNITIDE_PID') or os.getppid())
        parent = _objc(b'NSRunningApplication', b'runningApplicationWithProcessIdentifier:', pid)
        if parent:
            _objc(parent, b'activateWithOptions:', 2)  # NSApplicationActivateIgnoringOtherApps
    except Exception:
        traceback.print_exc()


def _raise_plugin_windows(app):
    """Bring each window a plugin opens (its dialogs) to the front."""
    from qt.core import QEvent, QObject

    class Raiser(QObject):
        def eventFilter(self, obj, ev):
            if ev.type() == QEvent.Type.Show and obj.isWidgetType() and obj.isWindow() and obj is not _gui:
                if not obj.inherits('QMenu') and not obj.inherits('QToolTip'):
                    _bring_to_front(obj)
            return False

    _qt['raiser'] = Raiser(app)
    app.installEventFilter(_qt['raiser'])


def _excepthook(kind, value, tb):
    print(''.join(traceback.format_exception(kind, value, tb)), file=sys.stderr, flush=True)
    event('gui-error', plugin=_gui_state.get('plugin'), message=f'{kind.__name__}: {value}'.strip())


# --- Book files: the shadow links to the library's files; a link becomes a
# private copy before calibre hands out its path or writes to it, so a plugin
# can't change the user's files. Only inside shadow libraries.

_shadow_roots = set()


def materialize(path):
    if not path or not os.path.islink(path):
        return
    absolute = os.path.abspath(path)
    if not any(absolute.startswith(root + os.sep) for root in _shadow_roots):
        return
    target = os.path.realpath(path)
    tmp = path + '.magnitide-copy'
    _clone(target, tmp)
    os.replace(tmp, path)


def _clone(src, dst):
    """Copy-on-write where the disk can (APFS, Btrfs, XFS); else a plain copy."""
    import shutil

    if sys.platform == 'darwin':
        import ctypes
        import ctypes.util

        libc = ctypes.CDLL(ctypes.util.find_library('c'), use_errno=True)
        if libc.clonefile(os.fsencode(src), os.fsencode(dst), 0) == 0:
            return
    elif sys.platform.startswith('linux'):
        import fcntl

        try:
            with open(src, 'rb') as s, open(dst, 'wb') as d:
                fcntl.ioctl(d.fileno(), 0x40049409, s.fileno())  # FICLONE
            return
        except OSError:
            pass
    shutil.copy2(src, dst)


def _install_file_guards():
    if _shadow_roots:
        return  # already installed, with the first library
    from calibre.db.backend import DB
    from calibre.db.cache import Cache

    def wrap(cls, name, before=None, after=None):
        original = getattr(cls, name)

        def wrapper(self, *args, **kwargs):
            if before:
                before(self, *args, **kwargs)
            result = original(self, *args, **kwargs)
            if after:
                after(result)
            return result

        wrapper.__name__ = name
        wrapper.__wrapped__ = original
        setattr(cls, name, wrapper)

    def book_files(backend, path, fmt=None):
        folder = os.path.join(backend.library_path, path)
        try:
            names = os.listdir(folder)
        except OSError:
            return []
        suffix = ('.' + fmt.lower()) if fmt else None
        return [os.path.join(folder, n) for n in names if suffix is None or n.lower().endswith(suffix)]

    # The path calibre hands a plugin, which may write to it.
    wrap(Cache, 'format_abspath', after=materialize)

    # calibre writing into files itself.
    def apply_to_format(self, book_id, path, fname, fmt, *a, **k):
        materialize(os.path.join(self.library_path, path, fname + '.' + fmt.lower()))

    def add_format(self, book_id, fmt, stream, title, author, path, *a, **k):
        for f in book_files(self, path, fmt):
            materialize(f)

    def set_cover(self, book_id, path, *a, **k):
        materialize(os.path.join(self.library_path, path, 'cover.jpg'))

    def copy_format_to(self, book_id, fmt, fname, path, *a, use_hardlink=False, **k):
        if use_hardlink:
            for f in book_files(self, path, fmt):
                materialize(f)

    def copy_cover_to(self, path, *a, use_hardlink=False, **k):
        if use_hardlink:
            materialize(os.path.join(self.library_path, path, 'cover.jpg'))

    wrap(DB, 'apply_to_format', before=apply_to_format)
    wrap(DB, 'add_format', before=add_format)
    wrap(DB, 'set_cover', before=set_cover)
    wrap(DB, 'copy_format_to', before=copy_format_to)
    wrap(DB, 'copy_cover_to', before=copy_cover_to)


# --- The stand-in for calibre's main window.


class Inert:
    """A part of calibre's window that Magnitide doesn't have: any attribute
    or call works and does nothing. The first use of each is logged, to show
    what a plugin is missing."""

    _logged = set()

    def __init__(self, name):
        object.__setattr__(self, '_name', name)

    def __getattr__(self, attr):
        if attr.startswith('__'):
            raise AttributeError(attr)
        return Inert(f'{self._name}.{attr}')

    def __setattr__(self, attr, value):
        pass

    def __call__(self, *args, **kwargs):
        if self._name not in Inert._logged:
            Inert._logged.add(self._name)
            print(f"[magnitide] a plugin used {self._name}(), which Magnitide doesn't have; ignored", file=sys.stderr, flush=True)
        return Inert(self._name + '()')

    def __bool__(self):
        return False

    def __iter__(self):
        return iter(())

    def __len__(self):
        return 0


# --- calibre's own actions that plugins call, mapped to Magnitide's.

_asks = {}  # token -> {'loop': QEventLoop, 'answer': ...}
_ask_seq = [0]


def ask_magnitide(kind, ids):
    """Hand over to Magnitide and wait for it, as calibre's own dialogs make
    a plugin wait: it applies what the plugin changed so far, shows its
    editor (`edit`) or asks before deleting (`remove`), answers through
    `gui_answer` (which changes the shadow to match), notes the shadow as it
    then is, and lets the plugin go on with `gui_resume`. Qt keeps running
    meanwhile (the plugin's windows stay responsive)."""
    from qt.core import QEventLoop

    _ask_seq[0] += 1
    token = _ask_seq[0]
    entry = _asks[token] = {'loop': QEventLoop(), 'answer': None}
    event('gui-ask', plugin=_gui_state.get('plugin'), token=token, kind=kind, ids=list(ids))
    _return_focus()  # Magnitide's editor or question is next
    entry['loop'].exec()
    return _asks.pop(token)['answer'] or {}


def _apply_edits(books):
    """Magnitide's editor saved these books (shadow id → book, as Magnitide's
    catalog has them): set them in the shadow so the plugin sees them."""
    from calibre.utils.date import UNDEFINED_DATE, parse_date

    api = _gui.db.new_api
    for key, b in books.items():
        book_id = int(key)
        if not api.has_id(book_id):
            continue
        fields = {
            'title': b.get('title') or _('Unknown'),
            'authors': b.get('authors') or [_('Unknown')],
            'series': b.get('series') or None,
            'tags': b.get('tags') or [],
            'publisher': b.get('publisher') or None,
            'pubdate': parse_date(b['pubdate']) if b.get('pubdate') else UNDEFINED_DATE,
            'comments': b.get('comments') or None,
            'languages': b.get('languages') or [],
            'identifiers': b.get('identifiers') or {},
            'rating': b.get('rating') or 0,
        }
        for field, value in fields.items():
            api.set_field(field, {book_id: value})
        if b.get('series'):
            api.set_field('series_index', {book_id: b.get('series_index') or 1.0})
        if b.get('sort'):
            api.set_field('sort', {book_id: b['sort']})
        cover = b.get('coverPath')
        if cover:
            _link_cover(api, book_id, cover)


def _link_cover(api, book_id, cover):
    """The book's cover is Magnitide's file: link it, as the shadow does,
    rather than have calibre re-encode a copy."""
    here = os.path.join(_gui.db.library_path, api.field_for('path', book_id), 'cover.jpg')
    if os.path.islink(here) and os.path.realpath(here) == os.path.realpath(cover):
        return
    with open(cover, 'rb') as f:
        api.set_cover({book_id: f.read()})
    try:
        os.remove(here)
        os.symlink(cover, here)
    except OSError:
        traceback.print_exc()


class EditMetadata:
    """calibre's Edit Metadata action: its editor is Magnitide's."""

    def __init__(self, gui):
        self.gui = gui
        self.name = 'Edit Metadata'

    def edit_metadata(self, checked=False, bulk=None):
        self.edit_metadata_for(None, self.gui.library_view.get_selected_ids(), bulk=bulk)

    def edit_metadata_for(self, rows, book_ids, bulk=None):
        if book_ids:
            ask_magnitide('edit', book_ids)

    def apply_metadata_changes(
        self, id_map, title=None, msg='', callback=None, merge_tags=True, merge_comments=False, icon=None
    ):
        """As calibre does it (EditMetadataAction.apply_mi), without its
        progress window: Metadata objects, or (OPF path, cover path) pairs."""
        from calibre.ebooks.metadata.opf2 import OPF
        from calibre.library.comments import merge_comments as merge

        db = self.gui.current_db
        applied = set()
        for book_id, mi in id_map.items():
            if not db.has_id(book_id):
                continue
            try:
                if isinstance(mi, tuple):
                    opf, cover = mi
                    if cover:
                        with open(cover, 'rb') as f:
                            db.set_cover(book_id, f, notify=False, commit=False)
                        applied.add(book_id)
                    if not opf:
                        continue
                    with open(opf, 'rb') as f:
                        mi = OPF(f, basedir=os.path.dirname(opf), populate_spine=False).to_book_metadata()
                set_title, set_authors = not mi.is_null('title'), not mi.is_null('authors')
                idents = db.get_identifiers(book_id, index_is_id=True)
                if mi.identifiers:
                    idents.update(mi.identifiers)
                mi.identifiers = idents
                if mi.is_null('series'):
                    mi.series_index = None
                if merge_tags:
                    old = db.tags(book_id, index_is_id=True)
                    if old:
                        mi.tags = list(set([x.strip() for x in old.split(',')] + (mi.tags or [])))
                if merge_comments:
                    old = db.new_api.field_for('comments', book_id)
                    if old and mi.comments and old != mi.comments:
                        mi.comments = merge(old, mi.comments)
                db.set_metadata(book_id, mi, commit=False, set_title=set_title, set_authors=set_authors, notify=False)
                applied.add(book_id)
            except Exception:
                traceback.print_exc()
        db.commit()
        self.gui.changed()
        if callback is not None:
            callback(list(applied))

    def __getattr__(self, name):
        if name.startswith('__'):
            raise AttributeError(name)
        return Inert(f"gui.iactions['Edit Metadata'].{name}")


class RemoveBooks:
    """calibre's Remove Books action: Magnitide asks, and moves them to the Trash."""

    def __init__(self, gui):
        self.gui = gui
        self.name = 'Remove Books'

    def do_library_delete(self, to_delete_ids):
        if to_delete_ids:
            ask_magnitide('remove', to_delete_ids)

    def delete_books(self, *args):
        self.do_library_delete(self.gui.library_view.get_selected_ids())

    def __getattr__(self, name):
        if name.startswith('__'):
            raise AttributeError(name)
        return Inert(f"gui.iactions['Remove Books'].{name}")


class ConvertBooks:
    """calibre's Convert Books action: conversions a plugin asks for are
    Magnitide's, run when the plugin is done (after the books it added are
    in the library). Like calibre's, these calls return at once."""

    def __init__(self, gui):
        self.gui = gui
        self.name = 'Convert Books'

    def _queue(self, book_ids, fmt, skip_if_present=True):
        db = self.gui.current_db.new_api
        fmt = fmt.upper()
        ids = [i for i in book_ids if db.has_id(i) and not (skip_if_present and fmt in db.formats(i))]
        if ids:
            event('gui-convert', plugin=_gui_state.get('plugin'), ids=ids, format=fmt)

    def auto_convert_auto_add(self, book_ids):
        """To the output format, for books that don't have it (FanFicFare)."""
        from calibre.gui2 import gprefs
        from calibre.utils.config import prefs

        self._queue(book_ids, prefs['output_format'], skip_if_present=not gprefs['auto_convert_same_fmt'])

    def auto_convert(self, book_ids, on_card, format):
        self._queue(book_ids, format)

    def convert_ebooks_to_format(self, book_ids, to_fmt):
        self._queue(book_ids, to_fmt)

    def __getattr__(self, name):
        if name.startswith('__'):
            raise AttributeError(name)
        return Inert(f"gui.iactions['Convert Books'].{name}")


_gui = None
_gui_state = {'running': 0, 'plugin': None, 'watching': False, 'changed': False}
_loaded = {}  # zip path -> the plugin's InterfaceAction


def _build_gui():
    from qt.core import QAbstractTableModel, QItemSelection, QItemSelectionModel, QMainWindow, QModelIndex, Qt, QTableView

    from calibre.gui2.jobs import JobManager
    from calibre.gui2.keyboard import Manager
    from calibre.gui2.ui import get_gui

    class BooksModel(QAbstractTableModel):
        """calibre's book list model: every book in the library, one row each."""

        def __init__(self, gui):
            QAbstractTableModel.__init__(self, gui)
            self.gui = gui
            self.ids = []

        @property
        def db(self):
            return self.gui.current_db

        def reload(self):
            self.beginResetModel()
            self.ids = sorted(self.db.new_api.all_book_ids()) if self.db is not None else []
            self.endResetModel()

        def rowCount(self, parent=QModelIndex()):
            return 0 if parent.isValid() else len(self.ids)

        def columnCount(self, parent=QModelIndex()):
            return 1

        def data(self, index, role=Qt.ItemDataRole.DisplayRole):
            if role == Qt.ItemDataRole.DisplayRole and index.isValid():
                return self.db.new_api.field_for('title', self.ids[index.row()])

        def id(self, row):
            return self.ids[row.row() if hasattr(row, 'row') else row]

        def row(self, book_id):
            return self.ids.index(book_id)

        def count(self):
            return len(self.ids)

        # Plugins tell the book list what changed; Magnitide reads it back.
        def refresh_ids(self, ids, current_row=-1):
            self.gui.changed()

        def refresh_rows(self, rows, current_row=-1):
            self.gui.changed()

        def refresh(self, reset=True):
            self.reload()
            self.gui.changed()

        def books_added(self, num):
            self.reload()
            self.gui.changed()

        def books_deleted(self):
            self.reload()
            self.gui.changed()

        def __getattr__(self, name):
            if name.startswith('__'):
                raise AttributeError(name)
            return Inert(f'gui.library_view.model().{name}')

    class LibraryView(QTableView):
        """calibre's book list; its selection is Magnitide's."""

        def __init__(self, gui):
            QTableView.__init__(self, gui)
            self.setModel(BooksModel(gui))

        def get_selected_ids(self, as_set=False):
            m = self.model()
            ids = [m.id(i) for i in self.selectionModel().selectedRows()]
            return set(ids) if as_set else ids

        def set_selection(self, ids):
            m, sel = self.model(), QItemSelection()
            rows = [m.row(i) for i in ids if i in m.ids]
            for r in rows:
                sel.select(m.index(r, 0), m.index(r, 0))
            flags = QItemSelectionModel.SelectionFlag
            self.selectionModel().select(sel, flags.ClearAndSelect | flags.Rows)
            if rows:
                self.selectionModel().setCurrentIndex(m.index(rows[0], 0), flags.NoUpdate)

        def select_rows(self, identifiers, using_ids=True, change_current=True, scroll=True):
            m = self.model()
            ids = [i for i in identifiers] if using_ids else [m.id(r) for r in identifiers]
            self.set_selection(ids)
            event('gui-select', ids=ids)

        @property
        def current_id(self):
            index = self.currentIndex()
            return self.model().id(index) if index.isValid() else None

        def __getattr__(self, name):
            if name.startswith('__'):
                raise AttributeError(name)
            return Inert(f'gui.library_view.{name}')

    class StatusBar:
        def show_message(self, msg, timeout=0, show_notification=True):
            event('gui-status', plugin=_gui_state.get('plugin'), message=str(msg), timeout=timeout or 0)

        def clear_message(self):
            pass

        def __getattr__(self, name):
            if name.startswith('__'):
                raise AttributeError(name)
            return Inert(f'gui.status_bar.{name}')

    class Actions(dict):
        """`gui.iactions`: the hosted plugins, and calibre's own actions that
        Magnitide has (Edit Metadata, Remove Books), by name. Others are
        inert stand-ins, logged when used."""

        def __missing__(self, name):
            return Inert(f'gui.iactions[{name!r}]')

    class MagnitideGui(QMainWindow):
        """Stands in for calibre's main window (never shown)."""

        def __init__(self):
            QMainWindow.__init__(self)
            self.setWindowTitle('Magnitide')
            self.db = None
            self.library_path = None
            self.keyboard = Manager(self)
            self.iactions = Actions(
                {'Edit Metadata': EditMetadata(self), 'Remove Books': RemoveBooks(self), 'Convert Books': ConvertBooks(self)}
            )
            self.status_bar = StatusBar()
            self.job_manager = JobManager()
            self.library_view = LibraryView(self)
            setattr(get_gui, 'ans', self)

        @property
        def current_db(self):
            return self.db

        def set_library(self, db):
            self.db = db
            self.library_path = db.library_path
            self.library_view.model().reload()

        def changed(self):
            _gui_state['changed'] = True

        def __getattr__(self, name):
            if name.startswith('__'):
                raise AttributeError(name)
            return Inert(f'gui.{name}')

    return MagnitideGui()


def _open_library(library):
    """Open (or reopen, after Magnitide rebuilt it) the shadow library."""
    from calibre.db.legacy import LibraryDatabase

    global _gui
    if _gui is None:
        _gui = _build_gui()
    _install_file_guards()
    _shadow_roots.add(os.path.abspath(library))
    old = _gui.db
    if old is not None and not getattr(old, '_magnitide_closed', False) and old.library_path == library:
        return
    db = LibraryDatabase(library)
    _gui.set_library(db)
    for ac in _loaded.values():
        try:
            ac.library_changed(db)
        except Exception:
            traceback.print_exc()


def _load_plugins(paths):
    """Load GUI plugins not loaded yet, as calibre does at start-up."""
    from calibre.customize import InterfaceActionBase, PluginInstallationType

    errors, new = {}, []
    for path in paths:
        if path in _loaded:
            continue
        try:
            base = plugin(path)
            if not isinstance(base, InterfaceActionBase):
                raise ValueError(f'{base.name} is not a GUI plugin')
            ac = base.load_actual_plugin(_gui)
            ac.plugin_path = path
            ac.interface_action_base_plugin = base
            ac.installation_type = PluginInstallationType.EXTERNAL
            ac.do_genesis()
            _gui.iactions[ac.name] = ac
            _loaded[path] = ac
            new.append(ac)
        except Exception as e:
            traceback.print_exc()
            errors[path] = f'{type(e).__name__}: {e}'.strip()
    try:
        _gui.keyboard.finalize()
    except Exception:
        traceback.print_exc()
    for ac in new:
        for step in ('gui_layout_complete', 'initialization_complete'):
            try:
                getattr(ac, step)()
            except Exception:
                traceback.print_exc()
    return errors


def _text(action):
    """A menu item's text without its keyboard accelerator ("&Mark" → "Mark")."""
    return action.text().replace('&&', '\0').replace('&', '').replace('\0', '&')


def _icon(action):
    from qt.core import QBuffer, QByteArray, QIODevice

    icon = action.icon()
    if icon is None or icon.isNull():
        return None
    data = QByteArray()
    buf = QBuffer(data)
    buf.open(QIODevice.OpenModeFlag.WriteOnly)
    icon.pixmap(64, 64).save(buf, 'PNG')
    import base64

    return 'data:image/png;base64,' + base64.b64encode(bytes(data)).decode('ascii')


def _show(menu):
    """Let the plugin fill its menu, as it would just before calibre shows it."""
    try:
        menu.aboutToShow.emit()
    except Exception:
        traceback.print_exc()


def _menu_items(menu, prefix):
    _show(menu)
    items = []
    for i, a in enumerate(menu.actions()):
        if not a.isVisible():
            continue
        if a.isSeparator():
            items.append({'separator': True})
            continue
        item = {
            'path': prefix + [i],
            'text': _text(a),
            'enabled': a.isEnabled(),
            'checkable': a.isCheckable(),
            'checked': a.isChecked(),
        }
        sub = a.menu()
        if sub is not None:
            item['children'] = _menu_items(sub, prefix + [i])
        items.append(item)
    return items


def _actions():
    out = []
    for path, ac in _loaded.items():
        qa = ac.qaction
        menu = qa.menu()
        out.append({
            'plugin': ac.interface_action_base_plugin.name,
            'file': os.path.basename(path),
            'text': _text(qa) or ac.name,
            'icon': _icon(qa),
            # Whether the plugin's toolbar button does something itself.
            'main': qa.receivers(qa.triggered) > 0,
            'items': _menu_items(menu, []) if menu is not None else [],
        })
    return out


def _find(menu, path, texts):
    """The menu item at `path`, checking it still has the text listed (menus
    rebuilt on show can move items); by text if it moved."""
    action = None
    for depth, (index, text) in enumerate(zip(path, texts)):
        if menu is None:
            return None
        _show(menu)
        actions = menu.actions()
        action = actions[index] if index < len(actions) and _text(actions[index]) == text else None
        if action is None:
            action = next((a for a in actions if _text(a) == text), None)
        if action is None:
            return None
        menu = action.menu() if depth < len(path) - 1 else None
    return action


def _busy():
    """A plugin is still working: in an action, a window of its open, or a job."""
    from qt.core import QApplication

    if _gui_state['running']:
        return True
    for w in QApplication.topLevelWidgets():
        if w is not _gui and w.isVisible() and not w.inherits('QMenu'):
            return True
    try:
        return bool(_gui.job_manager.unfinished_jobs())
    except Exception:
        return False


def _watch_until_idle():
    """Tell Magnitide when the plugin is done, so it can read back its changes."""
    from qt.core import QTimer

    if _gui_state['watching']:
        return
    _gui_state['watching'] = True
    timer = QTimer(_gui)
    timer.setInterval(400)

    def check():
        if _busy():
            return
        timer.stop()
        timer.deleteLater()
        _gui_state['watching'] = False
        _return_focus()
        event('gui-idle', plugin=_gui_state.get('plugin'), changed=_gui_state['changed'])
        _gui_state['changed'] = False

    timer.timeout.connect(check)
    timer.start()


def _trigger(name, path, texts, selection):
    from qt.core import QTimer

    ac = next((a for a in _loaded.values() if a.interface_action_base_plugin.name == name), None)
    if ac is None:
        raise ValueError(f'{name} is not loaded')
    target = ac.qaction if not path else _find(ac.qaction.menu(), path, texts)
    if target is None:
        raise ValueError(f'{name} no longer has “{" › ".join(texts)}”')
    if not target.isEnabled():
        raise ValueError(f'“{texts[-1] if texts else name}” is not available right now')
    _gui.library_view.set_selection(selection)
    _gui_state['plugin'] = name

    def run():
        _gui_state['running'] += 1
        try:
            target.trigger()
        finally:
            _gui_state['running'] -= 1

    # Later, so this request answers even if the action opens a modal dialog.
    QTimer.singleShot(0, run)
    _watch_until_idle()
    return {'started': True}


def m_gui_open(params):
    """Open the shadow library (reopening it if Magnitide rebuilt it) and load
    the GUI plugins; returns their menus."""
    ensure_qt()

    def go():
        if params.get('outputFormat'):
            from calibre.utils.config import prefs

            # Magnitide's setting, where plugins (and Convert Books) look for it.
            if prefs['output_format'] != params['outputFormat'].lower():
                prefs['output_format'] = params['outputFormat'].lower()
        _open_library(params['library'])
        errors = _load_plugins(params.get('plugins') or [])
        return {'actions': _actions(), 'errors': errors}

    return on_main(go)


def m_gui_close(params):
    """Close the shadow library so Magnitide can rebuild it. Refused while a
    plugin is working."""
    if 'invoker' not in _qt:
        return {'closed': True}

    def go():
        if _gui is None or _gui.db is None:
            return {'closed': True}
        if _busy():
            return {'closed': False}
        _gui.db.close()
        _gui.db._magnitide_closed = True
        return {'closed': True}

    return on_main(go)


def m_gui_actions(params):
    if 'invoker' not in _qt:
        return {'actions': []}
    return on_main(lambda: {'actions': _actions() if _gui is not None else []})


def m_gui_trigger(params):
    ensure_qt()
    return on_main(
        lambda: _trigger(params['plugin'], params.get('path') or [], params.get('texts') or [], params.get('selection') or [])
    )


def m_gui_answer(params):
    """Magnitide's answer to `ask_magnitide`: the books as the user saved
    them (`books`, by shadow id) or the ones they deleted (`removed`). The
    shadow is changed to match before this replies; the plugin waits on
    until `gui_resume`, so nothing it does next is mistaken for this."""

    def go():
        entry = _asks.get(params.get('token'))
        if entry is None:
            return {'answered': False}
        entry['answer'] = params
        _apply_edits(params.get('books') or {})
        removed = [i for i in params.get('removed') or [] if _gui.db.new_api.has_id(i)]
        if removed:
            _gui.db.new_api.remove_books(removed, permanent=True)
        _gui.library_view.model().reload()
        return {'answered': True}

    return on_main(go)


def m_gui_resume(params):
    """Let a plugin waiting in `ask_magnitide` go on."""

    def go():
        entry = _asks.get(params.get('token'))
        if entry is not None:
            entry['loop'].quit()
        return {'resumed': entry is not None}

    return on_main(go)


def m_gui_busy(params):
    if 'invoker' not in _qt:
        return {'busy': False}
    return on_main(lambda: {'busy': _gui is not None and _busy()})


METHODS = {
    'hello': m_hello,
    'inspect': m_inspect,
    'identify': m_identify,
    'cover': m_cover,
    'gui_open': m_gui_open,
    'gui_close': m_gui_close,
    'gui_actions': m_gui_actions,
    'gui_trigger': m_gui_trigger,
    'gui_busy': m_gui_busy,
    'gui_answer': m_gui_answer,
    'gui_resume': m_gui_resume,
}


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


def read_requests():
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
    # stdin closed or shutdown: stop Qt, if it's running, and the main loop.
    app = _qt.get('app')
    if app is not None:
        _qt['invoker'].call.emit(app.quit)
    _main_calls.put(None)


def main():
    # Requests are read on a thread; the main thread is kept for Qt, which
    # starts the first time a GUI plugin needs it.
    threading.Thread(target=read_requests, daemon=True).start()
    while True:
        fn = _main_calls.get()
        if fn is None:
            break
        fn()


main()
