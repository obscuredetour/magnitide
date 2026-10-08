"""Mirrors calibre's `calibre.utils.titlecase`: title case with the small
words left lower, the way editors do it."""

import re

from calibre.utils.icu import capitalize
from calibre.utils.icu import lower as icu_lower
from calibre.utils.icu import upper as icu_upper

__all__ = ['titlecase']

SMALL = r'a|an|and|as|at|but|by|en|for|if|in|of|on|or|the|to|v\.?|via|vs\.?'
PUNCT = r"""!"#$%&'‘()*+,\-‒–—―./:;?@[\\\]_`{|}~"""

SMALL_WORDS = re.compile(r'^(%s)$' % SMALL, re.I)
INLINE_PERIOD = re.compile(r'[a-z][.][a-z]', re.I)
UC_ELSEWHERE = re.compile(r'[%s]*?[a-zA-Z]+[A-Z]+?' % PUNCT)
CAPFIRST = re.compile(r'^[%s]*?([\w])' % PUNCT)
SMALL_FIRST = re.compile(r'^([%s]*)(%s)\b' % (PUNCT, SMALL), re.I)
SMALL_LAST = re.compile(r'\b(%s)[%s]?$' % (SMALL, PUNCT), re.I)
SMALL_AFTER_NUM = re.compile(r'(\d+\s+)(a|an|the)\b', re.I | re.U)
SUBPHRASE = re.compile(r'([:.;?!][ ])(%s)' % SMALL)
APOS_SECOND = re.compile(r"^[dol]{1}['‘]{1}[a-z]+$", re.I)
UC_INITIALS = re.compile(r'^(?:[A-Z]{1}\.{1}|[A-Z]{1}\.{1}[A-Z]{1})+$')
MAC_MC = re.compile(r'^([Mm]a?c)(\w+.*)')

_lower = lambda x: icu_lower(x)  # noqa: E731
_upper = lambda x: icu_upper(x)  # noqa: E731


def titlecase(text):
    """
    Titlecases input text

    This filter changes all words to Title Caps, and attempts to be clever
    about *un*capitalizing SMALL words like a/an/the in the input.

    The list of "SMALL words" which are not capped comes from
    the New York Times Manual of Style, plus 'vs' and 'v'.

    """
    all_caps = _upper(text) == text

    pat = re.compile(r'(\s+)')
    line = []
    for word in pat.split(text):
        if not word:
            continue
        if pat.match(word):
            line.append(word)
            continue
        if all_caps:
            if UC_INITIALS.match(word):
                line.append(word)
                continue
            else:
                word = _lower(word)

        if APOS_SECOND.match(word):
            word = word.replace(word[0], _upper(word[0]), 1)
            word = word[:2] + _upper(word[2]) + word[3:]
            line.append(word)
            continue
        if INLINE_PERIOD.search(word) or UC_ELSEWHERE.match(word):
            line.append(word)
            continue
        if SMALL_WORDS.match(word):
            line.append(_lower(word))
            continue

        match = MAC_MC.match(word)
        if match and (match.group(2)[:3] != 'hin'):
            line.append('%s%s' % (capitalize(match.group(1)), capitalize(match.group(2))))
            continue

        hyphenated = []
        for item in word.split('-'):
            hyphenated.append(CAPFIRST.sub(lambda m: _upper(m.group(0)), item))
        line.append('-'.join(hyphenated))

    result = ''.join(line)

    result = SMALL_FIRST.sub(lambda m: '%s%s' % (m.group(1), capitalize(m.group(2))), result)

    result = SMALL_AFTER_NUM.sub(lambda m: '%s%s' % (m.group(1), capitalize(m.group(2))), result)

    result = SMALL_LAST.sub(lambda m: capitalize(m.group(0)), result)

    result = SUBPHRASE.sub(lambda m: '%s%s' % (m.group(1), capitalize(m.group(2))), result)

    return result
