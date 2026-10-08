"""Mirrors calibre's `calibre.ebooks.covers`: a generated cover (title,
authors, series on a coloured background) when a book has none. calibre
draws styled covers with its own font bundle; this draws a plain one with
Qt's fonts, honouring the size, colours and fonts in `prefs`."""

import random

from calibre.utils.config import JSONConfig
from qt.core import QColor, QFont, QImage, QPainter, QRect, QRectF, Qt

cprefs = JSONConfig('cover_generation')
cprefs.defaults['title_font_size'] = 60
cprefs.defaults['subtitle_font_size'] = 40
cprefs.defaults['footer_font_size'] = 40
cprefs.defaults['cover_width'] = 1200
cprefs.defaults['cover_height'] = 1600
cprefs.defaults['title_font_family'] = None
cprefs.defaults['subtitle_font_family'] = None
cprefs.defaults['footer_font_family'] = None
cprefs.defaults['color_themes'] = {}
cprefs.defaults['disabled_color_themes'] = []
cprefs.defaults['disabled_styles'] = []
cprefs.defaults['title_template'] = '<b>{title}'
cprefs.defaults['subtitle_template'] = (
    """{series:'test($, strcat("<i>", $, "</i> - ", raw_field("formatted_series_index")), "")'}"""
)
cprefs.defaults['footer_template'] = r"""program:
# Show at most two authors, on separate lines.
authors = field('authors');
authors = sublist(authors, 0, 2, ' & ');
authors = list_re(authors, ' & ', '(.*)', '<b>\1');
authors = re(authors, ' & ', '<br>');
re(authors, '&&', '&')
"""

DEFAULT_THEMES = {
    'Mocha': ('e8d9ac', 'c7b07b', '564628', '382d1a'),
    'Blue': ('e5e9ec', 'c8d4dc', '3c5566', '2a3a44'),
    'Grey': ('f0f0f0', 'd0d0d0', '404040', '202020'),
    'Green': ('e4efe1', 'bfd6b8', '3d5a35', '28391f'),
    'Rose': ('f5e4e6', 'e3bfc5', '6b3744', '45232c'),
}


def default_color_themes():
    return dict(DEFAULT_THEMES)


def color_themes():
    ans = default_color_themes()
    ans.update(cprefs.get('color_themes') or {})
    for name in cprefs.get('disabled_color_themes') or ():
        ans.pop(name, None)
    return ans or default_color_themes()


def override_prefs(base_prefs, **overrides):
    ans = dict(base_prefs)
    ans.update(overrides)
    return ans


def _text(mi, template, default=''):
    from calibre.utils.formatter import SafeFormat

    try:
        return SafeFormat().safe_format(template, mi, 'EXCEPTION', mi).strip() or default
    except Exception:
        return default


def _strip_html(text):
    import re

    text = re.sub(r'<br\s*/?>', '\n', text, flags=re.I)
    return re.sub(r'<[^>]+>', '', text).replace('&amp;', '&').strip()


def _draw_text(painter, rect, text, font_family, size, color, align=Qt.AlignmentFlag.AlignHCenter):
    if not text:
        return 0
    font = QFont(font_family) if font_family else QFont()
    font.setPointSizeF(max(8, size))
    painter.setFont(font)
    painter.setPen(QColor(color))
    flags = int(align | Qt.AlignmentFlag.AlignTop | Qt.TextFlag.TextWordWrap)
    bounding = painter.boundingRect(rect, flags, text)
    painter.drawText(rect, flags, text)
    return bounding.height()


def generate_cover(mi, prefs=None, as_qimage=False):
    """A cover for `mi` (a Metadata): PNG bytes, or a QImage when `as_qimage`."""
    prefs = prefs or cprefs
    width, height = int(prefs.get('cover_width') or 1200), int(prefs.get('cover_height') or 1600)
    themes = color_themes()
    theme = random.Random(mi.title or 'x').choice(sorted(themes))
    bg, fg2, fg, accent = ('#' + c for c in themes[theme])
    img = QImage(width, height, QImage.Format.Format_RGB32)
    img.fill(QColor(bg))
    p = QPainter(img)
    p.setRenderHint(QPainter.RenderHint.Antialiasing)
    p.setRenderHint(QPainter.RenderHint.TextAntialiasing)
    margin = width // 12
    try:
        p.fillRect(QRectF(0, height * 0.72, width, height * 0.02), QColor(accent))
        p.fillRect(QRectF(0, 0, width * 0.06, height), QColor(fg2))
        title = _strip_html(_text(mi, prefs.get('title_template') or '{title}', mi.title or ''))
        subtitle = _strip_html(_text(mi, prefs.get('subtitle_template') or '', ''))
        footer = _strip_html(_text(mi, prefs.get('footer_template') or '{authors}', ' & '.join(mi.authors or [])))
        y = margin
        rect = QRect(margin, y, width - 2 * margin, int(height * 0.55))
        y += _draw_text(p, rect, title, prefs.get('title_font_family'), float(prefs.get('title_font_size') or 60), fg)
        y += margin // 2
        rect = QRect(margin, y, width - 2 * margin, int(height * 0.2))
        _draw_text(
            p, rect, subtitle, prefs.get('subtitle_font_family'), float(prefs.get('subtitle_font_size') or 40), fg
        )
        rect = QRect(margin, int(height * 0.76), width - 2 * margin, int(height * 0.2))
        _draw_text(p, rect, footer, prefs.get('footer_font_family'), float(prefs.get('footer_font_size') or 40), fg)
    finally:
        p.end()
    if as_qimage:
        return img
    from calibre.utils.img import image_to_data

    return image_to_data(img, fmt='PNG')


def create_cover(title, authors, series=None, series_index=1, prefs=None, as_qimage=False):
    from calibre.ebooks.metadata.book.base import Metadata

    mi = Metadata(title, authors)
    if series:
        mi.series, mi.series_index = series, series_index
    return generate_cover(mi, prefs=prefs, as_qimage=as_qimage)


def message_image(text, width=500, height=400, font_size=20):
    img = QImage(width, height, QImage.Format.Format_RGB32)
    img.fill(Qt.GlobalColor.white)
    p = QPainter(img)
    f = QFont()
    f.setPointSize(font_size)
    p.setFont(f)
    p.drawText(
        QRect(10, 10, width - 20, height - 20), int(Qt.AlignmentFlag.AlignCenter | Qt.TextFlag.TextWordWrap), text
    )
    p.end()
    return img


__all__ = [
    'generate_cover',
    'create_cover',
    'cprefs',
    'override_prefs',
    'color_themes',
    'default_color_themes',
    'message_image',
]
