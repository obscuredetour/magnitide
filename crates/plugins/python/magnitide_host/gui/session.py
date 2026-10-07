"""A GUI session: the shadow library open in the stand-in window, the GUI
plugins loaded, their menus, running an item, and the hand-overs to
Magnitide (its editor, its delete) that calibre's own actions turn into."""

import os
import traceback

from magnitide_host.gui import ensure_qt, files, main_window, menus, qt_running, state
from magnitide_host.gui.macos import return_focus
from magnitide_host.rpc import event, method, on_main

_loaded = {}  # zip path -> the plugin's InterfaceAction
_asks = {}  # token -> {'loop': QEventLoop, 'answer': ...}
_ask_seq = [0]


def loaded():
    return _loaded


# --- Hand-overs to Magnitide ---------------------------------------------


def ask_magnitide(kind, ids, **extra):
    """Hand over to Magnitide and wait for it, as calibre's own dialogs make
    a plugin wait: it applies what the plugin changed so far, shows its
    editor (`edit`), asks before deleting (`remove`), finds metadata
    (`download`) or converts (`convert`), answers through `gui_answer`
    (which changes the shadow to match), notes the shadow as it then is,
    and lets the plugin go on with `gui_resume`. Qt keeps running
    meanwhile (the plugin's windows stay responsive)."""
    from qt.core import QEventLoop

    _ask_seq[0] += 1
    token = _ask_seq[0]
    entry = _asks[token] = {'loop': QEventLoop(), 'answer': None}
    event('gui-ask', plugin=state.get('plugin'), token=token, kind=kind, ids=list(ids), **extra)
    return_focus()  # Magnitide's editor or question is next
    entry['loop'].exec()
    return _asks.pop(token)['answer'] or {}


def _apply_edits(books):
    """Magnitide's editor saved these books (shadow id → book, as Magnitide's
    catalog has them): set them in the shadow so the plugin sees them."""
    from calibre.utils.date import UNDEFINED_DATE, parse_date

    api = main_window().db.new_api
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
            link_cover(api, book_id, cover)


def link_cover(api, book_id, cover):
    """The book's cover is Magnitide's file: link it, as the shadow does,
    rather than have calibre re-encode a copy."""
    here = os.path.join(main_window().db.library_path, api.field_for('path', book_id), 'cover.jpg')
    if os.path.islink(here) and os.path.realpath(here) == os.path.realpath(cover):
        return
    with open(cover, 'rb') as f:
        api.set_cover({book_id: f.read()})
    try:
        os.remove(here)
        os.symlink(cover, here)
    except OSError:
        traceback.print_exc()


def _add_formats(formats):
    """Magnitide converted books: `{shadow id: [{format, path}]}`. The
    formats join the shadow as links to Magnitide's files, so the diff sees
    nothing new."""
    api = main_window().db.new_api
    for key, items in (formats or {}).items():
        book_id = int(key)
        if not api.has_id(book_id):
            continue
        for item in items:
            fmt, path = item.get('format', '').upper(), item.get('path')
            if not fmt or not path or not os.path.exists(path):
                continue
            with open(path, 'rb') as f:
                api.add_format(book_id, fmt, f, run_hooks=False)
            here = api.format_abspath(book_id, fmt)
            try:
                if here and os.path.realpath(here) != os.path.realpath(path):
                    os.remove(here)
                    os.symlink(path, here)
            except OSError:
                traceback.print_exc()


# --- The library and the plugins ------------------------------------------


_headless = {}  # library path -> LibraryDatabase, for hooks run without a GUI session


def headless_db(library):
    """The shadow library opened without the window (file-type hooks after
    an add, say). Closed by `gui_close` along with the window's."""
    from calibre.db.legacy import LibraryDatabase

    files.install_file_guards()
    files.shadow_roots.add(os.path.abspath(library))
    db = _headless.get(library)
    if db is None or db.is_closed:
        db = _headless[library] = LibraryDatabase(library)
    return db


def close_headless():
    for db in _headless.values():
        try:
            db.close()
        except Exception:
            pass
    _headless.clear()


def open_library(library):
    """Open (or reopen, after Magnitide rebuilt it) the shadow library."""
    from calibre.db.legacy import LibraryDatabase
    from magnitide_host.gui.window import build_gui

    gui = main_window() or build_gui()
    files.install_file_guards()
    files.shadow_roots.add(os.path.abspath(library))
    old = gui.db
    if old is not None and not getattr(old, '_magnitide_closed', False) and old.library_path == library:
        return
    close_headless()
    db = LibraryDatabase(library)
    gui.set_library(db)
    _watch_jobs(gui)
    for ac in _loaded.values():
        try:
            ac.library_changed(db)
        except Exception:
            traceback.print_exc()


_jobs_watched = [False]


def _watch_jobs(gui):
    """Report the job manager's jobs to Magnitide as they start, progress
    and finish (its job bar shows them)."""
    if _jobs_watched[0]:
        return
    _jobs_watched[0] = True
    jm = gui.job_manager
    seen = {}

    def describe(job):
        return {'job': id(job), 'plugin': state.get('plugin'), 'description': getattr(job, 'description', '')}

    def added(row):
        try:
            job = jm.jobs[row]
        except Exception:
            return
        seen[id(job)] = (0, '')
        event('job-start', **describe(job))

    def done(row):
        try:
            job = jm.jobs[row]
        except Exception:
            return
        details = ''
        try:
            details = (job.details or '')[-4000:]
        except Exception:
            pass
        event('job-done', failed=bool(getattr(job, 'failed', False)), details=details, **describe(job))
        seen.pop(id(job), None)

    def progress():
        for job in list(jm.jobs):
            if id(job) not in seen or job.is_finished:
                continue
            try:
                job.update()
                cur = (int(job.percent), job.status_text)
            except Exception:
                continue
            if cur != seen[id(job)]:
                seen[id(job)] = cur
                event('job-progress', percent=cur[0], message=cur[1], **describe(job))

    jm.job_added.connect(added)
    jm.job_done.connect(done)
    from qt.core import QTimer

    timer = QTimer(gui)
    timer.setInterval(500)
    timer.timeout.connect(progress)
    timer.start()


def load_plugins():
    """Load the GUI plugins that are on (calibre's own actions first), as
    calibre does at start-up."""
    from calibre.customize import InterfaceActionBase
    from calibre.customize.ui import interface_actions

    gui = main_window()
    errors, new = {}, []
    for base in interface_actions():
        path = base.plugin_path or f'builtin:{base.name}'
        if path in _loaded:
            continue
        try:
            if not isinstance(base, InterfaceActionBase):
                raise ValueError(f'{base.name} is not a GUI plugin')
            new.append(load_plugin(base, gui))
        except Exception as e:
            traceback.print_exc()
            errors[base.plugin_path or base.name] = f'{type(e).__name__}: {e}'.strip()
    try:
        gui.keyboard.finalize()
    except Exception:
        traceback.print_exc()
    for ac in new:
        for step in ('gui_layout_complete', 'initialization_complete'):
            try:
                getattr(ac, step)()
            except Exception:
                traceback.print_exc()
    return errors


def load_plugin(base, gui):
    """Load one GUI plugin's action into the window (genesis included) and
    remember it; the base's `actual_plugin_` is set."""
    from calibre.customize import PluginInstallationType

    ac = base.load_actual_plugin(gui)
    ac.plugin_path = base.plugin_path
    ac.interface_action_base_plugin = base
    ac.installation_type = base.installation_type or PluginInstallationType.EXTERNAL
    ac.do_genesis()
    gui.iactions[ac.name] = ac
    _loaded[base.plugin_path or f'builtin:{base.name}'] = ac
    return ac


def external_loaded():
    return {p: ac for p, ac in _loaded.items() if not p.startswith('builtin:')}


def busy():
    """A plugin is still working: in an action, a window of its open, or a job."""
    from qt.core import QApplication

    gui = main_window()
    if state['running']:
        return True
    for w in QApplication.topLevelWidgets():
        if w is not gui and w.isVisible() and not w.inherits('QMenu'):
            return True
    try:
        # Until the job manager has reported a job done (its callback has
        # run by then), not merely until its thread finished.
        return any(not getattr(j, '_reported', False) for j in gui.job_manager.jobs)
    except Exception:
        return False


def watch_until_idle():
    """Tell Magnitide when the plugin is done, so it can read back its changes."""
    from qt.core import QTimer

    if state['watching']:
        return
    state['watching'] = True
    timer = QTimer(main_window())
    timer.setInterval(400)

    def check():
        if busy():
            return
        timer.stop()
        timer.deleteLater()
        state['watching'] = False
        return_focus()
        event('gui-idle', plugin=state.get('plugin'), changed=state['changed'])
        state['changed'] = False

    timer.timeout.connect(check)
    timer.start()


def trigger(name, path, texts, selection, search=''):
    from qt.core import QTimer

    gui = main_window()
    ac = next((a for a in _loaded.values() if a.interface_action_base_plugin.name == name), None)
    if ac is None:
        raise ValueError(f'{name} is not loaded')
    target = ac.qaction if not path else menus.find(ac.qaction.menu(), path, texts)
    if target is None:
        raise ValueError(f'{name} no longer has “{" › ".join(texts)}”')
    if not target.isEnabled():
        raise ValueError(f'“{texts[-1] if texts else name}” is not available right now')
    gui.library_view.set_selection(selection)
    gui.search.text = search or ''
    state['plugin'] = name

    def run():
        state['running'] += 1
        try:
            target.trigger()
        finally:
            state['running'] -= 1

    # Later, so this request answers even if the action opens a modal dialog.
    QTimer.singleShot(0, run)
    watch_until_idle()
    return {'started': True}


# --- Methods --------------------------------------------------------------


@method('gui_open')
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
        open_library(params['library'])
        errors = load_plugins()
        return {'actions': menus.actions(external_loaded()), 'errors': errors}

    return on_main(go)


@method('gui_close')
def m_gui_close(params):
    """Close the shadow library so Magnitide can rebuild it. Refused while a
    plugin is working."""
    if not qt_running():
        close_headless()
        return {'closed': True}

    def go():
        close_headless()
        gui = main_window()
        if gui is None or gui.db is None:
            return {'closed': True}
        if busy():
            return {'closed': False}
        gui.db.close()
        gui.db._magnitide_closed = True
        return {'closed': True}

    return on_main(go)


@method('gui_actions')
def m_gui_actions(params):
    if not qt_running():
        return {'actions': []}
    return on_main(lambda: {'actions': menus.actions(external_loaded()) if main_window() is not None else []})


@method('gui_trigger')
def m_gui_trigger(params):
    ensure_qt()
    return on_main(
        lambda: trigger(
            params['plugin'],
            params.get('path') or [],
            params.get('texts') or [],
            params.get('selection') or [],
            params.get('search') or '',
        )
    )


@method('gui_answer')
def m_gui_answer(params):
    """Magnitide's answer to `ask_magnitide`: the books as the user saved
    them (`books`, by shadow id), the ones they deleted (`removed`), or the
    formats it converted (`formats`). The shadow is changed to match before
    this replies; the plugin waits on until `gui_resume`, so nothing it does
    next is mistaken for this."""

    def go():
        entry = _asks.get(params.get('token'))
        if entry is None:
            return {'answered': False}
        entry['answer'] = params
        gui = main_window()
        _apply_edits(params.get('books') or {})
        removed = [i for i in params.get('removed') or [] if gui.db.new_api.has_id(i)]
        if removed:
            gui.db.new_api.remove_books(removed, permanent=True)
        _add_formats(params.get('formats') or {})
        gui.library_view.model().reload()
        return {'answered': True}

    return on_main(go)


@method('gui_resume')
def m_gui_resume(params):
    """Let a plugin waiting in `ask_magnitide` go on."""

    def go():
        entry = _asks.get(params.get('token'))
        if entry is not None:
            entry['loop'].quit()
        return {'resumed': entry is not None}

    return on_main(go)


@method('gui_busy')
def m_gui_busy(params):
    if not qt_running():
        return {'busy': False}
    return on_main(lambda: {'busy': main_window() is not None and busy()})
