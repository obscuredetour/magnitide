"""Mirrors calibre's `calibre.utils.wordcount`: word, character and
paragraph counts of a text (the classic "wordcount" heuristics)."""

import re

CJK = re.compile('[一-鿿぀-ヿ가-힯]')


class WordCount:
    def __init__(self, text):
        self.text = text
        self.characters = len(text)
        self.chars_no_spaces = sum(1 for c in text if not c.isspace())
        self.asian_chars = len(CJK.findall(text))
        self.non_asian_words = len([w for w in re.split(r'\s+', CJK.sub(' ', text)) if w])
        self.words = self.non_asian_words + self.asian_chars
        self.paragraphs = len([p for p in re.split(r'\n\s*\n', text) if p.strip()]) or (1 if text.strip() else 0)
        self.lines = text.count('\n') + 1 if text else 0


def get_wordcount_obj(text):
    return WordCount(text)


def get_wordcount(text):
    """(characters, chars_no_spaces, asian_chars, non_asian_words, words, paragraphs)."""
    w = WordCount(text)
    return w.characters, w.chars_no_spaces, w.asian_chars, w.non_asian_words, w.words, w.paragraphs


__all__ = ['get_wordcount_obj', 'get_wordcount', 'WordCount']
