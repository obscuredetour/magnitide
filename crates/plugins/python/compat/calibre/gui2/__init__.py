"""Mirrors calibre's `calibre.gui2` package root (calibre 9.15): the
`Application`, the GUI settings (`gprefs`, `config`, `dynamic`), the
message dialogs (`error_dialog`, `info_dialog`, `question_dialog`), file
choosers, `Dispatcher`, and the small helpers plugins import from here.

Icons: calibre ships hundreds; Magnitide ships none, so `QIcon.ic(name)`
and `icon_from_name` give a drawn stand-in (a plugin's own images, through
`get_icons`, are its own).
"""

import os
import sys
import weakref
from contextlib import suppress
from functools import lru_cache
from threading import RLock

from calibre import prints
from calibre.constants import DEBUG, config_dir, ismacos, isosx, iswindows  # noqa: F401
from calibre.utils.config import Config, ConfigProxy, JSONConfig, dynamic  # noqa: F401
from calibre.utils.date import UNDEFINED_DATE
from qt.core import (
    QApplication,
    QByteArray,
    QColor,
    QDateTime,
    QFileDialog,
    QFont,
    QFontMetrics,
    QIcon,
    QModelIndex,
    QObject,
    QPainter,
    QPixmap,
    QSettings,
    QSize,
    Qt,
    QTimer,
    QUrl,
    pyqtSignal,
)

NONE = None  # legacy, 'No data' for Qt
UNDEFINED_QDATETIME = QDateTime(UNDEFINED_DATE)
empty_index = QModelIndex()

# --- Preferences ----------------------------------------------------------


def _config():
    c = Config('gui', 'preferences for the calibre GUI')
    c.add_opt('send_to_storage_card_by_default', default=False)
    c.add_opt('confirm_delete', default=False)
    c.add_opt('main_window_geometry', default=None)
    c.add_opt('new_version_notification', default=True)
    c.add_opt('use_roman_numerals_for_series_number', default=True)
    c.add_opt('sort_tags_by', default='name')
    c.add_opt('match_tags_type', default='any')
    c.add_opt('cover_flow_queue_length', default=6)
    c.add_opt('tag_browser_dont_collapse', default=[])
    c.add_opt('tag_browser_collapse_at', default=100)
    c.add_opt('tag_browser_hidden_categories', default=set())
    c.add_opt('tags_browser_partition_method', default='first letter')
    c.add_opt('search_as_you_type', default=False)
    c.add_opt('highlight_search_matches', default=False)
    c.add_opt('show_avg_rating', default=True)
    c.add_opt('disable_animations', default=False)
    c.add_opt('systray_icon', default=False)
    c.add_opt('show_splash_screen', default=True)
    c.add_opt('upload_news_to_device', default=True)
    c.add_opt('delete_news_from_library_on_upload', default=False)
    c.add_opt('separate_cover_flow', default=False)
    c.add_opt('disable_tray_notification', default=False)
    c.add_opt('default_send_to_device_action', default=None)
    c.add_opt('asked_library_thing_password', default=False)
    c.add_opt('autolaunch_server', default=False)
    c.add_opt('oldest_news', default=60)
    c.add_opt('worker_limit', default=6)
    c.add_opt('get_social_metadata', default=True)
    c.add_opt('overwrite_author_title_metadata', default=True)
    c.add_opt('auto_download_cover', default=False)
    c.add_opt('enforce_cpu_limit', default=True)
    c.add_opt('gui_layout', default='wide')
    c.add_opt('show_files_after_save', default=True)
    c.add_opt('auto_add_path', default=None)
    c.add_opt('auto_add_check_for_duplicates', default=False)
    c.add_opt('blocked_auto_formats', default=[])
    c.add_opt('auto_add_auto_convert', default=True)
    c.add_opt('auto_add_everything', default=False)
    c.add_opt('ui_style', default='system')
    c.add_opt('tag_browser_old_look', default=False)
    c.add_opt('book_list_tooltips', default=True)
    c.add_opt('bd_show_cover', default=True)
    c.add_opt('bd_overlay_cover_size', default=False)
    c.add_opt('tags_browser_category_icons', default={})
    c.add_opt('cover_browser_reflections', default=True)
    c.add_opt('book_list_extra_row_spacing', default=0)
    c.add_opt('refresh_book_list_on_bulk_edit', default=True)
    c.add_opt('cover_grid_width', default=0)
    c.add_opt('cover_grid_height', default=0)
    c.add_opt('cover_grid_spacing', default=0)
    c.add_opt('cover_grid_color', default=(80, 80, 80))
    c.add_opt('cover_grid_cache_size_multiple', default=5)
    c.add_opt('cover_grid_disk_cache_size', default=2500)
    c.add_opt('cover_grid_show_title', default=False)
    c.add_opt('cover_grid_texture', default=None)
    c.add_opt('show_vl_tabs', default=False)
    c.add_opt('show_highlight_toggle_button', default=False)
    c.add_opt('add_comments_to_email', default=False)
    c.add_opt('cb_preserve_aspect_ratio', default=False)
    c.add_opt('gpm_template_editor_font_size', default=10)
    c.add_opt('show_emblems', default=False)
    c.add_opt('emblem_size', default=32)
    c.add_opt('emblem_position', default='left')
    c.add_opt('metadata_diff_mark_rejected', default=False)
    c.add_opt('tag_browser_show_counts', default=True)
    c.add_opt('row_numbers_in_book_list', default=True)
    c.add_opt('hidpi', default='auto')
    c.add_opt('tag_browser_item_padding', default=0.5)
    c.add_opt('paste_isbn_prefixes', default=['isbn', 'url', 'amazon', 'google'])
    c.add_opt('preserve_date_on_ctl', default=True)
    c.add_opt('manual_add_auto_convert', default=False)
    c.add_opt('auto_convert_same_fmt', default=False)
    c.add_opt('tag_browser_show_tooltips', default=True)
    c.add_opt('tag_browser_allow_keyboard_focus', default=False)
    c.add_opt('bd_show_cover_at_top', default=True)
    c.add_opt('color_palette', default='system')
    return c


config = ConfigProxy(_config())

gprefs = JSONConfig('gui')
defs = gprefs.defaults
defs['auto_convert_same_fmt'] = False
defs['ui_style'] = 'system'
defs['color_palette'] = 'system'
defs['book_display_fields'] = [
    ('title', False),
    ('authors', True),
    ('series', True),
    ('identifiers', True),
    ('tags', True),
    ('formats', True),
    ('path', True),
    ('publisher', False),
    ('rating', False),
    ('languages', False),
    ('timestamp', False),
    ('pubdate', False),
    ('last_modified', False),
    ('size', False),
    ('comments', True),
]
defs['default_author_link'] = 'search-calibre'
defs['tag_browser_dont_collapse'] = []
defs['tag_browser_collapse_at'] = 100
defs['tag_browser_hidden_categories'] = set()
defs['tags_browser_partition_method'] = 'first letter'
defs['show_layout_buttons'] = False
defs['cover_grid_width'] = 0
defs['toolbar_icon_size'] = 'medium'
defs['toolbar_text'] = 'always'
defs['font'] = None
defs['tags_browser_category_icons'] = {}
defs['book_list_extra_row_spacing'] = 0
defs['metadata_single_use_2_cols_for_custom_fields'] = True
defs['edit_metadata_single_layout'] = 'default'
defs['preserve_date_on_ctl'] = True
defs['manual_add_auto_convert'] = False
defs['show_avg_rating'] = True
defs['bd_show_cover'] = True
defs['worker_max_time'] = 0
defs['custom_colors_for_color_dialog'] = []
defs['action-layout-toolbar'] = ()
defs['action-layout-toolbar-device'] = ()
defs['action-layout-context-menu'] = ()
defs['action-layout-context-menu-device'] = ()
defs['action-layout-menubar'] = ()
defs['action-layout-menubar-device'] = ()
defs['action-layout-toolbar-child'] = ()
defs['action-layout-context-menu-split'] = ()
defs['disable_tray_notification'] = False
defs['show_files_after_save'] = True
defs['metadata_edit_custom_column_order'] = []
defs['search_as_you_type'] = False
defs['default_tweak_format'] = None
defs['gui_layout'] = 'wide'
defs['tag_browser_old_look'] = False
defs['virtual_lib_on_startup'] = defs['virt_libs_hidden'] = ()
defs['book_list_tooltips'] = True
defs['bd_overlay_cover_size'] = False
defs['cover_browser_reflections'] = True
defs['sort_tags_by'] = 'name'
defs['match_tags_type'] = 'any'
defs['tag_browser_show_counts'] = True
defs['row_numbers_in_book_list'] = True
defs['hidpi'] = 'auto'
defs['tag_browser_item_padding'] = 0.5
defs['book_details_narrow_comments_layout'] = 'float'
defs['show_emblems'] = False
defs['emblem_size'] = 32
defs['emblem_position'] = 'left'
defs['dnd_merge'] = True
defs['booklist_grid'] = False
defs['book_details_comments_heading_pos'] = 'hide'
defs['cover_corner_radius'] = 0.0
defs['cover_corner_radius_unit'] = 'px'
defs['tag_browser_allow_keyboard_focus'] = False
defs['tag_browser_show_tooltips'] = True
defs['books_autoscroll_time'] = 2.0
defs['dark_palette_name'] = defs['light_palette_name'] = ''
defs['dark_palettes'] = defs['light_palettes'] = {}
defs['auto_add_check_for_duplicates'] = False
del defs


def _create_cache_dir():
    from calibre.constants import cache_dir

    return cache_dir()


# --- Icons ----------------------------------------------------------------

_icon_cache = {}
_icon_logged = set()


def _placeholder_pixmap(name, size=48):
    """A drawn stand-in for a calibre icon: a rounded tile with the name's
    initial, in a colour from its name."""
    import hashlib

    pm = QPixmap(size, size)
    pm.fill(Qt.GlobalColor.transparent)
    hue = int(hashlib.md5(name.encode('utf-8')).hexdigest()[:4], 16) % 360
    color = QColor.fromHsv(hue, 110, 200)
    p = QPainter(pm)
    p.setRenderHint(QPainter.RenderHint.Antialiasing)
    p.setPen(Qt.PenStyle.NoPen)
    p.setBrush(color)
    p.drawRoundedRect(2, 2, size - 4, size - 4, size / 5, size / 5)
    initial = name.rpartition('/')[-1].partition('.')[0][:1].upper() or '?'
    f = QFont()
    f.setPixelSize(int(size * 0.55))
    f.setBold(True)
    p.setFont(f)
    p.setPen(QColor(255, 255, 255))
    p.drawText(pm.rect(), Qt.AlignmentFlag.AlignCenter, initial)
    p.end()
    return pm


def icon_from_name(name, fallback_data=b''):
    """A QIcon for a calibre icon name ('plus.png'): a user-provided image
    in the config folder's resources/images, else a drawn stand-in."""
    if isinstance(name, QIcon):
        return name
    if not name:
        return QIcon()
    name = name.replace('__', '/')
    ans = _icon_cache.get(name)
    if ans is not None:
        return ans
    path = I(name)  # noqa: F821 (built-in from calibre.utils.resources)
    lucide = _lucide_icon(name)
    if os.path.exists(path):
        ans = QIcon(path)
    elif lucide is not None:
        ans = lucide
    elif fallback_data:
        pm = QPixmap()
        pm.loadFromData(fallback_data)
        ans = QIcon(pm)
    else:
        if name not in _icon_logged and DEBUG:
            _icon_logged.add(name)
            prints(f'[magnitide] no icon named {name!r}; using a stand-in', file=sys.stderr)
        ans = QIcon(_placeholder_pixmap(name))
    _icon_cache[name] = ans
    return ans


def _lucide_icon(name):
    """calibre's built-in icon names drawn with Magnitide's icon set (see
    `calibre.utils.lucide_icons`), at the sizes menus and buttons use."""
    from calibre.utils.lucide_icons import render, svg_path

    if svg_path(name) is None:
        return None
    ans = QIcon()
    for size in (16, 24, 32, 48, 64):
        img = render(name, size)
        if img is None:
            return None
        ans.addPixmap(QPixmap.fromImage(img))
    return ans


class IconResourceManager:
    def __call__(self, name):
        return icon_from_name(name)

    def set_theme(self, *a, **k):
        pass

    def initialize(self, *a, **k):
        pass

    def override_icon(self, *a, **k):
        pass


icon_resource_manager = IconResourceManager()

if not hasattr(QIcon, 'ic'):
    QIcon.ic = staticmethod(icon_from_name)


def is_dark_theme():
    app = QApplication.instance()
    if app is None:
        return False
    pal = app.palette()
    return pal.color(pal.ColorRole.Window).lightness() < 128


dark_link_color = QColor(0x6C, 0xB4, 0xEE)


# --- Dialogs --------------------------------------------------------------


def _dialog(kind, parent, title, msg, det_msg='', show=False, show_copy_button=True, **kw):
    from calibre.gui2.dialogs.message_box import MessageBox

    d = MessageBox(kind, title, msg, det_msg, parent=parent, show_copy_button=show_copy_button, **kw)
    if show:
        return d.exec()
    return d


def error_dialog(parent, title, msg, det_msg='', show=False, show_copy_button=True):
    from calibre.gui2.dialogs.message_box import MessageBox

    return _dialog(MessageBox.ERROR, parent, title, msg, det_msg, show, show_copy_button)


def warning_dialog(parent, title, msg, det_msg='', show=False, show_copy_button=True):
    from calibre.gui2.dialogs.message_box import MessageBox

    return _dialog(MessageBox.WARNING, parent, title, msg, det_msg, show, show_copy_button)


def info_dialog(parent, title, msg, det_msg='', show=False, show_copy_button=True, only_copy_details=False):
    from calibre.gui2.dialogs.message_box import MessageBox

    return _dialog(
        MessageBox.INFO, parent, title, msg, det_msg, show, show_copy_button, only_copy_details=only_copy_details
    )


def question_dialog(
    parent,
    title,
    msg,
    det_msg='',
    show_copy_button=False,
    default_yes=True,
    skip_dialog_name=None,
    skip_dialog_msg='Show this confirmation again',
    skip_dialog_skipped_value=True,
    skip_dialog_skip_precheck=True,
    yes_text=None,
    no_text=None,
    yes_icon=None,
    no_icon=None,
    add_abort_button=False,
    override_icon=None,
):
    from calibre.gui2.dialogs.message_box import MessageBox

    if not isinstance(skip_dialog_name, str):
        skip_dialog_name = None
    try:
        auto_skip = set(gprefs.get('questions_to_auto_skip', ()))
    except Exception:
        auto_skip = set()
    if skip_dialog_name is not None and skip_dialog_name in auto_skip:
        return bool(skip_dialog_skipped_value)

    d = MessageBox(
        MessageBox.QUESTION,
        title,
        msg,
        det_msg,
        parent=parent,
        show_copy_button=show_copy_button,
        default_yes=default_yes,
        q_icon=override_icon,
        yes_text=yes_text,
        no_text=no_text,
        yes_icon=yes_icon,
        no_icon=no_icon,
        add_abort_button=add_abort_button,
    )

    if skip_dialog_name is not None and skip_dialog_msg:
        tc = d.toggle_checkbox
        tc.setVisible(True)
        tc.setText(skip_dialog_msg)
        tc.setChecked(bool(skip_dialog_skip_precheck))
        d.resize_needed.emit()

    ret = d.exec() == d.DialogCode.Accepted
    if add_abort_button and d.aborted:
        raise Aborted()

    if skip_dialog_name is not None and not d.toggle_checkbox.isChecked():
        auto_skip.add(skip_dialog_name)
        gprefs.set('questions_to_auto_skip', list(auto_skip))

    return ret


class Aborted(Exception):
    pass


def show_restart_warning(msg, parent=None):
    d = warning_dialog(parent, 'Restart needed', msg, show_copy_button=False)
    b = d.bb.addButton('Restart calibre now', d.bb.ButtonRole.AcceptRole)
    b.setIcon(QIcon.ic('lt.png'))
    d.do_restart = False

    def rf():
        d.do_restart = True

    b.clicked.connect(rf)
    d.set_details('')
    d.exec()
    b.clicked.disconnect()
    return d.do_restart


# --- File choosers --------------------------------------------------------


def _dir_key(name):
    return name.replace(' ', '_') + '_dir'


def select_initial_dir(q):
    while q:
        c = os.path.dirname(q)
        if c == q:
            break
        if os.path.exists(c):
            return c
        q = c
    return os.path.expanduser('~')


def _filters(filters, all_files):
    out = []
    for name, exts in filters:
        exts = ['*.' + e.lower() for e in exts]
        out.append(f'{name} ({" ".join(exts)})')
    if all_files:
        out.append('All files (*)')
    return ';;'.join(out)


def choose_dir(window, name, title, default_dir='~', no_save_dir=False):
    initial = (
        dynamic.get(_dir_key(name), os.path.expanduser(default_dir))
        if not no_save_dir
        else os.path.expanduser(default_dir)
    )
    if not initial or not os.path.isdir(initial):
        initial = os.path.expanduser('~')
    d = QFileDialog.getExistingDirectory(window, title, initial)
    if d and not no_save_dir:
        dynamic[_dir_key(name)] = d
    return d or None


def choose_files(window, name, title, filters=[], all_files=True, select_only_single_file=False, default_dir='~'):
    initial = dynamic.get(_dir_key(name), os.path.expanduser(default_dir))
    if not initial or not os.path.isdir(initial):
        initial = os.path.expanduser('~')
    f = _filters(filters, all_files)
    if select_only_single_file:
        ans, _ = QFileDialog.getOpenFileName(window, title, initial, f)
        ans = [ans] if ans else []
    else:
        ans, _ = QFileDialog.getOpenFileNames(window, title, initial, f)
    if ans:
        dynamic[_dir_key(name)] = os.path.dirname(ans[0])
        return list(ans)
    return None


def choose_save_file(window, name, title, filters=[], all_files=True, initial_path=None, initial_filename=None):
    if initial_path is not None:
        initial = initial_path
    else:
        initial = dynamic.get(_dir_key(name), os.path.expanduser('~'))
        if initial_filename:
            initial = os.path.join(initial, initial_filename)
    ans, _ = QFileDialog.getSaveFileName(window, title, initial, _filters(filters, all_files))
    if ans:
        dynamic[_dir_key(name)] = os.path.dirname(ans)
    return ans or None


def choose_images(
    window, name, title, select_only_single_file=True, formats=('png', 'gif', 'jpg', 'jpeg', 'svg', 'webp')
):
    return choose_files(
        window,
        name,
        title,
        filters=[('Images', list(formats))],
        all_files=False,
        select_only_single_file=select_only_single_file,
    )


def choose_osx_app(window, name, title, default_dir='/Applications'):
    return choose_files(window, name, title, default_dir=default_dir, select_only_single_file=True)


class FileDialog:
    """calibre's file dialog wrapper: built, then `get_files()` (the paths
    chosen, or an empty list). `mode` is a QFileDialog.FileMode."""

    def __init__(
        self,
        title=_('Choose files'),
        filters=[],
        add_all_files_filter=True,
        parent=None,
        modal=True,
        name='',
        mode=None,
        default_dir='~',
        no_save_dir=False,
        select_dir=False,
    ):
        from qt.core import QFileDialog

        self.selected_files = []
        self.fd = None
        if mode is None:
            mode = QFileDialog.FileMode.ExistingFiles
        initial = os.path.expanduser(default_dir)
        if not no_save_dir and name:
            initial = dynamic.get(_dir_key(name), initial)
        if not initial or not os.path.isdir(initial):
            initial = os.path.expanduser('~')
        f = _filters(filters, add_all_files_filter)
        if select_dir or mode == QFileDialog.FileMode.Directory:
            d = QFileDialog.getExistingDirectory(parent, title, initial)
            self.selected_files = [d] if d else []
        elif mode == QFileDialog.FileMode.AnyFile:
            ans, _ = QFileDialog.getSaveFileName(parent, title, initial, f)
            self.selected_files = [ans] if ans else []
        elif mode == QFileDialog.FileMode.ExistingFile:
            ans, _ = QFileDialog.getOpenFileName(parent, title, initial, f)
            self.selected_files = [ans] if ans else []
        else:
            ans, _ = QFileDialog.getOpenFileNames(parent, title, initial, f)
            self.selected_files = list(ans or [])
        if self.selected_files and name and not no_save_dir:
            chosen = self.selected_files[0]
            dynamic[_dir_key(name)] = chosen if os.path.isdir(chosen) else os.path.dirname(chosen)

    def get_files(self):
        return list(self.selected_files)


# --- Opening things -------------------------------------------------------


def open_url(qurl):
    from qt.core import QDesktopServices

    if isinstance(qurl, str):
        qurl = QUrl(qurl)
    QDesktopServices.openUrl(qurl)


def safe_open_url(qurl):
    if isinstance(qurl, str):
        qurl = QUrl(qurl)
    if qurl.scheme() in ('http', 'https', 'mailto', 'calibre'):
        open_url(qurl)


def open_local_file(path):
    open_url(QUrl.fromLocalFile(path))


def open_local_file_with(path, app):
    open_local_file(path)


def get_current_db():
    from calibre.gui2.ui import get_gui

    gui = get_gui()
    if gui is not None and gui.current_db is not None:
        return gui.current_db


def pixmap_to_data(pixmap, format='JPEG', quality=None):
    from qt.core import QBuffer, QIODevice

    if quality is None:
        quality = 90
    ba = QByteArray()
    buf = QBuffer(ba)
    buf.open(QIODevice.OpenModeFlag.WriteOnly)
    pixmap.save(buf, format, quality=quality)
    return bytes(ba)


def elided_text(text, font=None, width=300, pos='middle'):
    from qt.core import QFontMetrics as FM

    fm = FM(font) if font is not None else QApplication.fontMetrics() if QApplication.instance() else None
    if fm is None:
        return text
    mode = {
        'middle': Qt.TextElideMode.ElideMiddle,
        'left': Qt.TextElideMode.ElideLeft,
        'right': Qt.TextElideMode.ElideRight,
    }[pos]
    return fm.elidedText(text, mode, width)


def rating_font():
    return 'calibre Rating'


def file_icon_provider():
    from qt.core import QFileIconProvider

    return QFileIconProvider()


def max_available_height():
    return available_height()


def available_height():
    screen = QApplication.primaryScreen() if QApplication.instance() else None
    return screen.availableGeometry().height() if screen else 800


def available_width():
    screen = QApplication.primaryScreen() if QApplication.instance() else None
    return screen.availableGeometry().width() if screen else 1200


def timed_print(*a, **kw):
    prints(*a, **kw)


class sanitize_env_vars:
    def __enter__(self):
        pass

    def __exit__(self, *args):
        pass


def set_app_uid(*a, **k):
    pass


# --- Dispatchers ----------------------------------------------------------


class Dispatcher(QObject):
    """Call `func` on the GUI thread from any thread: `d(*args)` queues the
    call (or runs it directly when already on the GUI thread, if not
    `queued`)."""

    dispatch_signal = pyqtSignal(object, object)

    def __init__(self, func, queued=True, parent=None):
        QObject.__init__(self, parent)
        self.func = func
        typ = Qt.ConnectionType.QueuedConnection
        if not queued:
            typ = Qt.ConnectionType.AutoConnection if queued is None else Qt.ConnectionType.DirectConnection
        self.dispatch_signal.connect(self.dispatch, type=typ)

    def __call__(self, *args, **kwargs):
        self.dispatch_signal.emit(args, kwargs)

    def dispatch(self, args, kwargs):
        self.func(*args, **kwargs)


class FunctionDispatcher(QObject):
    """Like `Dispatcher`, but waits for and returns the result."""

    dispatch_signal = pyqtSignal(object, object, object)

    def __init__(self, func, queued=True, parent=None):
        global gui_thread
        if gui_thread is None:
            gui_thread = QApplication.instance().thread() if QApplication.instance() else None
        QObject.__init__(self, parent)
        self.func = func
        typ = Qt.ConnectionType.QueuedConnection
        if not queued:
            typ = Qt.ConnectionType.AutoConnection if queued is None else Qt.ConnectionType.DirectConnection
        self.dispatch_signal.connect(self.dispatch, type=typ)
        self.q = __import__('queue').Queue()
        self.lock = RLock()

    def __call__(self, *args, **kwargs):

        if is_gui_thread():
            return self.func(*args, **kwargs)
        with self.lock:
            self.dispatch_signal.emit(self.q, args, kwargs)
            res = self.q.get()
        return res

    def dispatch(self, q, args, kwargs):
        try:
            res = self.func(*args, **kwargs)
        except Exception:
            res = None
            import traceback

            traceback.print_exc()
        q.put(res)


gui_thread = None


def is_gui_thread():
    from qt.core import QThread

    app = QApplication.instance()
    return app is not None and QThread.currentThread() is app.thread()


# --- The application ------------------------------------------------------


class Application(QApplication):
    """calibre's QApplication, as much as plugins see of it: the name, the
    system style, `is_dark_theme`, and a few hooks."""

    shutdown_signal = pyqtSignal()
    palette_changed = pyqtSignal()

    def __init__(
        self,
        args,
        force_calibre_style=False,
        override_program_name=None,
        headless=False,
        color_prefs=gprefs,
        windows_app_uid=None,
    ):
        self.file_event_hook = None
        if headless:
            os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
        QApplication.setApplicationName('calibre')
        QApplication.setOrganizationName('calibre-ebook.com')
        QApplication.setOrganizationDomain(QApplication.organizationName())
        QApplication.setAttribute(Qt.ApplicationAttribute.AA_DontShowIconsInMenus, False)
        QApplication.__init__(self, args)
        self.headless = headless
        self._file_open_paths = []
        self._file_open_lock = RLock()
        self.palette_manager = _PaletteManager(self)
        self.pi = None
        self.color_prefs = color_prefs
        with suppress(Exception):
            self.setApplicationDisplayName('Magnitide plugins')

    @property
    def is_dark_theme(self):
        return is_dark_theme()

    @property
    def using_calibre_style(self):
        return False

    def load_builtin_fonts(self, scan_for_fonts=False):
        pass

    def setup_styles(self, force_calibre_style=False):
        pass

    def set_dark_mode_palette(self):
        pass

    def exec(self):
        try:
            return QApplication.exec()
        finally:
            self.shutdown_signal.emit()

    exec_ = exec

    def safe_restore_geometry(self, widget, geom):
        if geom:
            with suppress(Exception):
                widget.restoreGeometry(bytes(geom))
                return True
        return False

    def ensure_window_on_screen(self, widget):
        pass

    def set_geometry(self, widget, geom):
        return self.safe_restore_geometry(widget, geom)

    def get_file_open_paths(self):
        with self._file_open_lock:
            ans, self._file_open_paths = self._file_open_paths, []
        return ans


class _PaletteManager:
    def __init__(self, app):
        self.app = app
        self.using_calibre_style = False
        self.original_palette = app.palette()

    @property
    def is_dark_theme(self):
        return is_dark_theme()

    def set_palette(self, *a, **k):
        pass

    def setup_styles(self, *a, **k):
        pass

    def on_palette_change(self):
        pass

    def tree_view_hover_style(self):
        return ''


def must_use_qt(headless=True):
    """Ensure a QApplication exists (headless by default when one must be created)."""
    global gui_thread
    app = QApplication.instance()
    if app is None:
        app = Application([], headless=headless)
    if gui_thread is None:
        gui_thread = app.thread()


ensure_app = must_use_qt


def app_is_headless():
    app = QApplication.instance()
    return getattr(app, 'headless', False)


def is_ok_to_use_qt():
    try:
        must_use_qt()
    except Exception:
        return False
    return True


def destroy_app():
    app = QApplication.instance()
    if app is not None:
        app.quit()


def setup_unix_signals(self):
    pass


def setup_gui_option_parser(parser):
    pass


def set_sidebar_width(*a, **k):
    pass


def gui_prefs():
    return gprefs


@lru_cache(2)
def qt_app():
    return QApplication.instance()


__all__ = [
    'Application',
    'Dispatcher',
    'FunctionDispatcher',
    'gprefs',
    'config',
    'dynamic',
    'error_dialog',
    'warning_dialog',
    'info_dialog',
    'question_dialog',
    'choose_dir',
    'choose_files',
    'choose_save_file',
    'choose_images',
    'open_url',
    'open_local_file',
    'safe_open_url',
    'must_use_qt',
    'ensure_app',
    'is_ok_to_use_qt',
    'is_gui_thread',
    'elided_text',
    'pixmap_to_data',
    'rating_font',
    'UNDEFINED_QDATETIME',
    'NONE',
    'empty_index',
    'icon_from_name',
    'icon_resource_manager',
    'is_dark_theme',
    'dark_link_color',
    'show_restart_warning',
    'available_height',
    'available_width',
    'max_available_height',
    'get_current_db',
    'file_icon_provider',
    'Aborted',
    'sanitize_env_vars',
    'app_is_headless',
    'destroy_app',
    'select_initial_dir',
    'QSettings',
    'QSize',
    'QTimer',
    'QFontMetrics',
    'weakref',
    'config_dir',
]
