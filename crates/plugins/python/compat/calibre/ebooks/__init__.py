"""Mirrors calibre's `calibre.ebooks` package root: the book file types and
a few helpers plugins reach for."""

import os
import re

from calibre.utils.config_base import tweaks  # noqa: F401


class ConversionError(Exception):
    def __init__(self, msg, only_msg=False):
        Exception.__init__(self, msg)
        self.only_msg = only_msg


class UnknownFormatError(Exception):
    pass


class DRMError(ValueError):
    pass


class ParserError(ValueError):
    pass


BOOK_EXTENSIONS = [
    'lrf',
    'rar',
    'zip',
    'rtf',
    'lit',
    'txt',
    'txtz',
    'text',
    'htm',
    'xhtm',
    'html',
    'htmlz',
    'xhtml',
    'pdf',
    'pdb',
    'updb',
    'pdr',
    'prc',
    'mobi',
    'azw',
    'doc',
    'epub',
    'fb2',
    'fbz',
    'djv',
    'djvu',
    'lrx',
    'cbr',
    'cb7',
    'cbz',
    'cbc',
    'oebzip',
    'rb',
    'imp',
    'odt',
    'chm',
    'tpz',
    'azw1',
    'pml',
    'pmlz',
    'mbp',
    'tan',
    'snb',
    'xps',
    'oxps',
    'azw4',
    'book',
    'zbf',
    'pobi',
    'docx',
    'docm',
    'md',
    'textile',
    'markdown',
    'ibook',
    'ibooks',
    'iba',
    'azw3',
    'ps',
    'kepub',
    'kfx',
    'kpf',
]


def return_raster_image(path):
    from calibre.utils.imghdr import what

    if os.access(path, os.R_OK):
        with open(path, 'rb') as f:
            raw = f.read()
        if what(None, raw) not in (None, 'svg'):
            return raw


def extract_cover_from_embedded_svg(html, base, log):
    return None


def extract_calibre_cover(raw, base, log):
    return None


def render_html_svg_workaround(path_to_html, log, width=590, height=750, root=''):
    return None


def check_ebook_format(stream, current_guess):
    ans = current_guess
    if current_guess.lower() in ('prc', 'mobi', 'azw', 'azw1', 'azw3'):
        stream.seek(0)
        if stream.read(3) == b'TPZ':
            ans = 'tpz'
        stream.seek(0)
    return ans


def normalize(x):
    if isinstance(x, str):
        import unicodedata

        x = unicodedata.normalize('NFC', x)
    return x


def calibre_cover(
    title, author_string, series_string=None, output_format='jpg', title_size=46, author_size=36, logo_path=None
):
    from calibre.ebooks.covers import calibre_cover2

    return calibre_cover2(title, author_string, series_string, output_format=output_format, logo_path=logo_path)


UNIT_RE = re.compile(r'^(-*[0-9]*[.]?[0-9]*)\s*(%|em|ex|en|px|mm|cm|in|pt|pc|rem|q)$')


def unit_convert(value, base, font, dpi, body_font_size=12):
    """A CSS length as pixels."""
    if isinstance(value, (int, float)):
        return value
    try:
        return float(value) * 72.0 / dpi
    except Exception:
        pass
    result = value
    m = UNIT_RE.match(value)
    if m is not None and m.group(1):
        value = float(m.group(1))
        unit = m.group(2)
        if unit == '%':
            result = (value / 100.0) * base
        elif unit == 'px':
            result = value * 72.0 / dpi
        elif unit == 'in':
            result = value * 72.0
        elif unit == 'pt':
            result = value
        elif unit == 'em':
            result = value * font
        elif unit in ('ex', 'en'):
            result = value * font * 0.5
        elif unit == 'pc':
            result = value * 12.0
        elif unit == 'mm':
            result = value * 2.8346456693
        elif unit == 'cm':
            result = value * 28.346456693
        elif unit == 'rem':
            result = value * body_font_size
        elif unit == 'q':
            result = value * 0.708661417325
    return result


def generate_masthead(title, output_path=None, width=600, height=60):
    return None


def escape_xpath_attr(value):
    if '"' in value:
        if "'" in value:
            parts = ['"%s"' % x for x in value.split('"')]
            return 'concat(%s)' % ', '.join(parts)
        return "'%s'" % value
    return '"%s"' % value
