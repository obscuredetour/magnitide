"""Mirrors calibre's `calibre.spell.break_iterator` without ICU: words are
split on Unicode word boundaries as `regex` sees them."""

import regex

WORD = regex.compile(r'\w[\w\'’\-]*', regex.UNICODE | regex.VERSION1)


def split_into_words(text, lang='en'):
    return [m.group() for m in WORD.finditer(text)]


def split_into_words_and_positions(text, lang='en'):
    return [(m.start(), m.end() - m.start()) for m in WORD.finditer(text)]


def count_words(text, lang='en'):
    return sum(1 for _ in WORD.finditer(text))


def index_of(needle, haystack, lang='en', start=0):
    nl = needle.lower()
    for m in WORD.finditer(haystack, start):
        if m.group().lower() == nl:
            return m.start()
    return -1


def split_into_sentences(text, lang='en'):
    return [s for s in regex.split(r'(?<=[.!?])\s+', text) if s]


def sentence_positions(text, lang='en'):
    pos = 0
    ans = []
    for s in split_into_sentences(text, lang):
        start = text.find(s, pos)
        ans.append((start, len(s)))
        pos = start + len(s)
    return ans


__all__ = [
    'split_into_words',
    'split_into_words_and_positions',
    'count_words',
    'index_of',
    'split_into_sentences',
    'sentence_positions',
]
