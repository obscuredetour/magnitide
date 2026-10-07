"""The other plugin kinds, as Magnitide calls them: file-type plugins when
books are added (and after: on the shadow), metadata readers and writers,
a plugin's own settings dialog, and library-closed plugins."""

import os
import traceback

from magnitide_host.plugins import plugin
from magnitide_host.rpc import method, on_main


def _instances(paths):
    """The initialized plugins for a list of zip paths, in order."""
    out = []
    for p in paths or []:
        try:
            out.append(plugin(p))
        except Exception:
            traceback.print_exc()
    return out


# --- File-type plugins -----------------------------------------------------


@method('filetype.run')
def m_filetype_run(params):
    """`on_import` plugins on a file being added, chained as calibre chains
    them: each gets the previous one's result. Returns the path to add
    (the original when none changed it) and whether it's a temporary file
    Magnitide should delete after storing it."""
    from calibre.ptempfile import base_dir

    path = params['path']
    ext = (params.get('ext') or os.path.splitext(path)[1][1:]).lower()
    nfp = path
    for p in _instances(params.get('plugins')):
        with p:
            try:
                p.original_path_to_file = path
            except Exception:
                pass
            try:
                nfp = p.run(nfp) or nfp
            except Exception:
                print(f'Running file type plugin {p.name} failed with traceback:')
                traceback.print_exc()
    temporary = os.path.abspath(nfp) != os.path.abspath(path) and os.path.abspath(nfp).startswith(
        os.path.abspath(base_dir())
    )
    return {'path': nfp, 'temporary': temporary, 'ext': os.path.splitext(nfp)[1][1:].lower() or ext}


def _db_for(library):
    """The shadow library, through the GUI session when it has it open,
    else a headless database kept for hooks."""
    from magnitide_host.gui import main_window, session

    gui = main_window()
    if (
        gui is not None
        and gui.db is not None
        and not getattr(gui.db, '_magnitide_closed', False)
        and gui.db.library_path == library
    ):
        return gui.db
    return session.headless_db(library)


@method('filetype.post')
def m_filetype_post(params):
    """`postimport`, `postadd`, `postconvert` or `postdelete` on the shadow
    library for one book: {hook, library, bookId, formats: [fmt, …], plugins}."""
    hook = params['hook']
    book_id = int(params['bookId'])
    formats = [f.upper() for f in params.get('formats') or []]
    plugins = _instances(params.get('plugins'))
    ran, errors = [], {}

    def go():
        db = _db_for(params['library'])
        for p in plugins:
            with p:
                try:
                    if hook == 'postadd':
                        fmt_map = {f: db.new_api.format_abspath(book_id, f) for f in formats}
                        p.postadd(book_id, fmt_map, db)
                    else:
                        for f in formats:
                            getattr(p, hook)(book_id, f, db)
                    ran.append(p.name)
                except Exception as e:
                    traceback.print_exc()
                    errors[p.name] = f'{type(e).__name__}: {e}'.strip()
        db.commit()

    from magnitide_host.gui import qt_running

    if qt_running():
        on_main(go)
    else:
        go()
    return {'ran': ran, 'errors': errors}


# --- Metadata readers and writers ------------------------------------------


@method('metadata.read')
def m_metadata_read(params):
    """Read a file's metadata through reader plugins (highest priority
    first); the cover goes to `out`."""

    path, ext, out = params['path'], (params.get('ext') or '').lower(), params.get('out')
    last = None
    for p in _instances(params.get('plugins')):
        with p, open(path, 'rb') as stream:
            try:
                p.quick = False
                mi = p.get_metadata(stream, ext)
            except Exception as e:
                traceback.print_exc()
                last = f'{p.name}: {type(e).__name__}: {e}'
                continue
            if mi is None:
                continue
            ans = book_metadata(mi)
            if out and mi.cover_data and mi.cover_data[1]:
                with open(out, 'wb') as f:
                    f.write(mi.cover_data[1])
                ans['cover'] = out
            return {'metadata': ans, 'plugin': p.name}
    raise ValueError(last or 'no metadata reader answered')


@method('metadata.write')
def m_metadata_write(params):
    """Write metadata into a file through writer plugins (the first that
    handles it)."""
    path, ext = params['path'], (params.get('ext') or '').lower()
    mi = metadata_from_json(params.get('metadata') or {})
    cover = params.get('cover')
    if cover and os.path.exists(cover):
        with open(cover, 'rb') as f:
            data = f.read()
        from calibre.utils.imghdr import what

        mi.cover_data = (what(None, data) or 'jpeg', data)
    last = None
    for p in _instances(params.get('plugins')):
        with p, open(path, 'r+b') as stream:
            try:
                p.apply_null = False
                p.set_metadata(stream, mi, ext)
                return {'plugin': p.name}
            except Exception as e:
                traceback.print_exc()
                last = f'{p.name}: {type(e).__name__}: {e}'
    raise ValueError(last or 'no metadata writer handled the file')


def book_metadata(mi):
    """A calibre Metadata as Magnitide's book metadata JSON."""
    from calibre.utils.date import is_date_undefined

    unknown = {'Unknown', _('Unknown')}
    authors = []
    sort_map = getattr(mi, 'author_sort_map', None) or {}
    for a in mi.authors or []:
        if a in unknown:
            continue
        authors.append({'name': a, 'sort': sort_map.get(a) or None})
    published = None
    if mi.pubdate is not None and not is_date_undefined(mi.pubdate):
        published = mi.pubdate.date().isoformat()
    description = mi.comments or None
    if description and '<' not in description:
        from calibre.library.comments import comments_to_html

        description = comments_to_html(description)
    from calibre.utils.localization import canonicalize_lang

    languages = []
    for l in mi.languages or []:
        code = canonicalize_lang(l)
        if code and code != 'und' and code not in languages:
            languages.append(code)
    return {
        'title': mi.title if mi.title not in unknown else None,
        'titleSort': getattr(mi, 'title_sort', None) or None,
        'authors': authors,
        'series': mi.series or None,
        'seriesIndex': float(mi.series_index) if mi.series and mi.series_index is not None else None,
        'publisher': mi.publisher or None,
        'published': published,
        'languages': languages,
        'identifiers': {k: v for k, v in (mi.get_identifiers() or {}).items() if k and v},
        'description': description,
        'tags': list(mi.tags or []),
        'rating': int(mi.rating) if mi.rating else None,
        'uuid': getattr(mi, 'uuid', None) or None,
    }


def metadata_from_json(d):
    """The inverse: Magnitide's book metadata JSON as a calibre Metadata."""
    from calibre.ebooks.metadata.book.base import Metadata
    from calibre.utils.date import parse_date

    authors = [a['name'] if isinstance(a, dict) else a for a in d.get('authors') or []]
    mi = Metadata(d.get('title') or _('Unknown'), authors or [_('Unknown')])
    sort_map = {a['name']: a.get('sort') for a in d.get('authors') or [] if isinstance(a, dict) and a.get('sort')}
    if sort_map:
        mi.author_sort_map = sort_map
        mi.author_sort = ' & '.join(sort_map.get(a) or a for a in authors)
    if d.get('titleSort'):
        mi.title_sort = d['titleSort']
    if d.get('series'):
        mi.series = d['series']
        mi.series_index = d.get('seriesIndex') or 1.0
    for key in ('publisher', 'description', 'uuid'):
        if d.get(key):
            setattr(mi, 'comments' if key == 'description' else key, d[key])
    if d.get('published'):
        try:
            mi.pubdate = parse_date(d['published'], assume_utc=True)
        except Exception:
            pass
    if d.get('languages'):
        mi.languages = list(d['languages'])
    if d.get('identifiers'):
        mi.set_identifiers(dict(d['identifiers']))
    if d.get('tags'):
        mi.tags = list(d['tags'])
    if d.get('rating') is not None:
        mi.rating = int(d['rating'])
    return mi


# --- A plugin's own settings ------------------------------------------------


@method('plugin.configure')
def m_plugin_configure(params):
    """Open a plugin's settings (its `config_widget`, or a PreferencesPlugin's
    page) in a dialog and wait for it. Returns whether it saved, and
    whether it asked for a restart."""
    from magnitide_host.gui import ensure_qt, main_window

    ensure_qt()
    p = plugin(params['path'])

    def go():
        from calibre.customize import InterfaceActionBase, PreferencesPlugin
        from magnitide_host.gui.macos import return_focus
        from qt.core import QDialog, QDialogButtonBox, QScrollArea, QVBoxLayout

        parent = main_window()
        changed = restart = False
        if isinstance(p, PreferencesPlugin):
            d = QDialog(parent)
            d.setWindowTitle(p.gui_name or p.name)
            l = QVBoxLayout(d)
            w = p.create_widget(d)
            sa = QScrollArea(d)
            sa.setWidget(w)
            sa.setWidgetResizable(True)
            l.addWidget(sa)
            bb = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel, d)
            bb.accepted.connect(d.accept)
            bb.rejected.connect(d.reject)
            l.addWidget(bb)
            try:
                w.initialize()
            except Exception:
                traceback.print_exc()
            d.resize(d.sizeHint().expandedTo(__import__('qt.core', fromlist=['QSize']).QSize(600, 400)))
            if d.exec() == QDialog.DialogCode.Accepted:
                changed = True
                try:
                    restart = bool(w.commit())
                except Exception:
                    traceback.print_exc()
        else:
            changed = bool(p.do_user_config(parent))
            if changed and isinstance(p, InterfaceActionBase):
                ac = p.actual_plugin_
                if ac is not None and hasattr(ac, 'apply_settings'):
                    try:
                        ac.apply_settings()
                    except Exception:
                        traceback.print_exc()
        return_focus()
        return {'changed': changed, 'restart': restart}

    return on_main(go)


# --- Library closed -----------------------------------------------------------


@method('library.closed')
def m_library_closed(params):
    """Library-closed plugins get the library (the shadow, as calibre's
    `LibraryDatabase`) as calibre gives them its db on closing."""
    plugins = _instances(params.get('plugins'))
    ran, errors = [], {}

    def go():
        db = _db_for(params['library'])
        for p in plugins:
            with p:
                try:
                    p.run(db)
                    ran.append(p.name)
                except Exception as e:
                    traceback.print_exc()
                    errors[p.name] = f'{type(e).__name__}: {e}'.strip()
        db.commit()

    from magnitide_host.gui import qt_running

    if qt_running():
        on_main(go)
    else:
        go()
    return {'ran': ran, 'errors': errors}
