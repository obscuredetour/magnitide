"""Mirrors the helpers of calibre's `calibre.ebooks.conversion.preprocess`
that plugins call outside a conversion: `smarten_punctuation` (educated
quotes, dashes and ellipses in HTML) and a `HTMLPreProcessor` that offers
it. The rest of calibre's preprocessing is part of its pipeline."""

import re

_TAG = re.compile(r'(<[^>]*>)')
_SKIP = re.compile(r'^<\s*(pre|code|kbd|script|style)\b', re.I)
_SKIP_END = re.compile(r'^<\s*/\s*(pre|code|kbd|script|style)\b', re.I)


def _educate(text):
    # Dashes and ellipses.
    text = text.replace('---', '\u2014').replace('--', '\u2013')
    text = re.sub(r'\.\.\.', '\u2026', text)
    text = re.sub(r'\. \. \.', '\u2026', text)
    # Double quotes: opening after start/space/open bracket, else closing.
    text = re.sub(r'(^|[\s(\[{\u2014\u2013-])"', '\\1\u201c', text)
    text = text.replace('"', '\u201d')
    # Single quotes and apostrophes.
    text = re.sub(r"(^|[\s(\[{\u2014\u2013-])'", '\\1\u2018', text)
    text = text.replace("'", '\u2019')
    return text


def smarten_punctuation(html, log=None):
    """Replace plain quotes, dashes and ellipses with typographic ones,
    leaving tags and the contents of pre/code/script/style alone."""
    out = []
    skipping = 0
    for part in _TAG.split(html):
        if not part:
            continue
        if part.startswith('<'):
            if _SKIP.match(part):
                skipping += 1
            elif _SKIP_END.match(part) and skipping:
                skipping -= 1
            out.append(part)
        elif skipping:
            out.append(part)
        else:
            out.append(_educate(part))
    return ''.join(out)


class HTMLPreProcessor:
    def __init__(self, log=None, extra_opts=None, regex_wizard_callback=None):
        self.log = log
        self.extra_opts = extra_opts
        self.regex_wizard_callback = regex_wizard_callback

    def smarten_punctuation(self, html):
        return smarten_punctuation(html, self.log)

    def __call__(self, html, remove_special_chars=None, get_preprocess_html=False):
        if remove_special_chars is not None:
            html = remove_special_chars.sub('', html)
        html = html.replace('\0', '')
        return html


class Dehyphenator:
    def __init__(self, verbose=0, log=None):
        self.verbose, self.log = verbose, log

    def __call__(self, html, format, length=1):
        return html


__all__ = ['smarten_punctuation', 'HTMLPreProcessor', 'Dehyphenator']
