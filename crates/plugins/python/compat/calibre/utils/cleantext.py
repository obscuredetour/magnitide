"""Mirrors calibre's `calibre.utils.cleantext`: removing characters XML
can't hold, and unescaping entities."""

import re
from html.entities import name2codepoint

_ascii_pat = None
_illegal_xml = None


def clean_ascii_chars(txt, charlist=None):
    """Remove ASCII control characters (except tab, newline and return) from
    `txt`; or just those in `charlist`."""
    global _ascii_pat
    if not txt:
        return ''
    if charlist is None:
        if _ascii_pat is None:
            chars = set(range(8)) | set(range(0xB, 0xE)) | set(range(0xE, 0x1F)) | {0x7F}
            _ascii_pat = re.compile('|'.join(map(chr, chars)))
        pat = _ascii_pat
    else:
        pat = re.compile('|'.join(map(chr, charlist)))
    return pat.sub('', txt)


def clean_xml_chars(unicode_text):
    """Remove characters XML 1.0 doesn't allow."""
    global _illegal_xml
    if _illegal_xml is None:
        _illegal_xml = re.compile('[\x00-\x08\x0b\x0c\x0e-\x1f￾￿\ud800-\udfff]')
    return _illegal_xml.sub('', unicode_text)


def unescape(text, rm=False, rchar=''):
    """Unescape HTML entities in `text`; unknown ones are removed if `rm`."""

    def fixup(m, rm=rm, rchar=rchar):
        text = m.group(0)
        if text[:2] == '&#':
            try:
                if text[:3] == '&#x':
                    return chr(int(text[3:-1], 16))
                return chr(int(text[2:-1]))
            except ValueError:
                pass
        else:
            try:
                text = chr(name2codepoint[text[1:-1]])
            except KeyError:
                pass
        if rm:
            return rchar
        return text

    return re.sub(r'&#?\w+;', fixup, text)


def anglicize(text):
    from calibre.utils.localization import get_udc

    return get_udc().decode(text)
