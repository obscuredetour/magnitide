"""Mirrors calibre's `calibre.utils.icu` (calibre 9.15) without ICU: the
collation, case and search functions calibre and plugins use for sorting
and matching text.

calibre's module binds ICU. This one uses Unicode normalization: a sort key
has a primary level (letters with accents and case removed), a secondary
one (the accents) and a tertiary one (case), and a collator's `strength`
says how many levels count. Digit runs sort by value when `numeric`;
`ignore_alternate_chars` (ICU's "shifted") ignores spaces and punctuation.

Known differences from ICU: no language-specific tailorings (German
phonebook order, say), no contractions (`œ` sorts as `o` then `e`), and the
exact ordering of symbols and punctuation. PyICU is used when it's
importable.
"""

import re
import unicodedata
from collections import OrderedDict
from functools import lru_cache

try:  # pragma: no cover
    import icu as _pyicu  # PyICU
except Exception:
    _pyicu = None

icu_unicode_version = unicodedata.unidata_version

UCOL_PRIMARY, UCOL_SECONDARY, UCOL_TERTIARY, UCOL_QUATERNARY, UCOL_IDENTICAL = 0, 1, 2, 3, 15
UCOL_DEFAULT = -1
UCOL_ALTERNATE_HANDLING = 1
UCOL_NON_IGNORABLE, UCOL_SHIFTED = 21, 20
UPPER_CASE, LOWER_CASE, TITLE_CASE = 0, 1, 2
UBRK_CHARACTER, UBRK_WORD, UBRK_LINE, UBRK_SENTENCE = 0, 1, 2, 3

_locale = None
_none = ''


def _is_mark(ch):
    return unicodedata.category(ch).startswith('M')


_VARIABLE = frozenset('PZS')  # punctuation, separators, symbols: what "shifted" ignores
_DIGITS = re.compile(r'\d+')


@lru_cache(4096)
def _decompose(ch):
    """A character's base letters (lower case) and its combining marks."""
    nfkd = unicodedata.normalize('NFKD', ch)
    base = ''.join(c for c in nfkd if not _is_mark(c))
    marks = ''.join(c for c in nfkd if _is_mark(c))
    return base, marks


class Collator:
    """A pure-Python stand-in for ICU's collator."""

    def __init__(self, locale=None, strength=UCOL_TERTIARY, numeric=False, upper_first=None, shifted=False):
        self.locale = locale
        self.strength = strength
        self.numeric = numeric
        self.upper_first = upper_first
        self.shifted = shifted

    def clone(self):
        return Collator(self.locale, self.strength, self.numeric, self.upper_first, self.shifted)

    def set_attribute(self, attr, val):
        if attr == UCOL_ALTERNATE_HANDLING:
            self.shifted = val == UCOL_SHIFTED

    def get_attribute(self, attr):
        if attr == UCOL_ALTERNATE_HANDLING:
            return UCOL_SHIFTED if self.shifted else UCOL_NON_IGNORABLE
        return 0

    def contractions(self):
        return ()

    # -- keys --

    def _fold(self, s):
        """(primary string, marks string, case bits, index map). The index
        map gives, for each character of the primary string, the index of
        the original character it came from."""
        primary, marks, case, origin = [], [], [], []
        for i, ch in enumerate(s):
            base, m = _decompose(ch)
            if m and not base:
                marks.append(m)
                continue
            if not base:
                continue
            if self.shifted and unicodedata.category(base[0])[0] in _VARIABLE:
                continue
            marks.append(m)
            low = base.casefold()
            upper = base != low and base == base.upper()
            for c in low:
                primary.append(c)
                origin.append(i)
                case.append('1' if upper else '0')
        return ''.join(primary), ''.join(marks), ''.join(case), origin

    def _primary_bytes(self, primary):
        if not self.numeric:
            return primary.encode('utf-8', 'surrogatepass')
        out = []
        pos = 0
        for m in _DIGITS.finditer(primary):
            out.append(primary[pos : m.start()].encode('utf-8', 'surrogatepass'))
            digits = m.group().lstrip('0') or '0'
            # \x01 sorts before every printable character, as digits do in ICU.
            out.append(b'\x01' + len(digits).to_bytes(4, 'big') + digits.encode('ascii'))
            pos = m.end()
        out.append(primary[pos:].encode('utf-8', 'surrogatepass'))
        return b''.join(out)

    def sort_key(self, s):
        if s is None:
            s = ''
        elif isinstance(s, bytes):
            s = s.decode('utf-8', 'replace')
        elif not isinstance(s, str):
            s = str(s)
        primary, marks, case, _ = self._fold(s)
        key = self._primary_bytes(primary)
        if self.strength >= UCOL_SECONDARY:
            key += b'\x00' + marks.encode('utf-8', 'surrogatepass')
        if self.strength >= UCOL_TERTIARY:
            if self.upper_first:
                case = case.translate({48: 49, 49: 48})
            key += b'\x00' + case.encode('ascii')
        if self.strength >= UCOL_IDENTICAL:
            key += b'\x00' + s.encode('utf-8', 'surrogatepass')
        return key

    def strcmp(self, a, b):
        ka, kb = self.sort_key(a), self.sort_key(b)
        return (ka > kb) - (ka < kb)

    def _searchable(self, s):
        """The string folded to this collator's strength, with the index map."""
        primary, marks, case, origin = self._fold(s)
        if self.strength <= UCOL_PRIMARY:
            return primary, origin
        # Keep accents (and case, at tertiary) by folding each character to
        # its decomposition without case folding beyond what the strength says.
        out, omap = [], []
        for i, ch in enumerate(s):
            base, m = _decompose(ch)
            if not base:
                if m:
                    out.append(m)
                    omap.extend([i] * len(m))
                continue
            if self.shifted and unicodedata.category(base[0])[0] in _VARIABLE:
                continue
            text = base if self.strength >= UCOL_TERTIARY else base.casefold()
            text += m
            out.append(text)
            omap.extend([i] * len(text))
        return ''.join(out), omap

    def find(self, needle, haystack):
        if not needle or not haystack:
            return -1, 0
        n, _ = self._searchable(needle)
        h, omap = self._searchable(haystack)
        if not n:
            return -1, 0
        pos = h.find(n)
        if pos < 0:
            return -1, 0
        start = omap[pos]
        end = omap[pos + len(n) - 1] + 1
        return start, end - start

    def contains(self, needle, haystack):
        if not needle:
            return True
        return self.find(needle, haystack)[0] > -1

    def startswith(self, haystack, prefix):
        if not prefix:
            return True
        h, _ = self._searchable(haystack)
        p, _ = self._searchable(prefix)
        return h.startswith(p)

    def collation_order(self, s):
        primary, _, _, _ = self._fold(s or '')
        if not primary:
            return (0, 0)
        return (ord(primary[0]), 1)

    def collation_order_for_partitioning(self, s):
        return self.collation_order(s)

    def upper(self, s):
        return s.upper()

    def lower(self, s):
        return s.lower()

    def swap_case(self, s):
        return s.swapcase()

    def title_case(self, s):
        return title_case(s)


# --- Module-level API, as calibre's --------------------------------------


def change_locale(locale=None):
    global _locale
    _locale = locale


def _tweak(name, default):
    try:
        from calibre.utils.config_base import tweaks

        v = tweaks[name]
        return default if v is None else v
    except Exception:
        return default


def collator(strength=None, numeric=None, ignore_alternate_chars=None, upper_first=None):
    """A collator with the given settings (defaults: tertiary strength, not
    numeric, punctuation significant, lower case first)."""
    c = Collator(_locale)
    if strength is not None:
        c.strength = strength
    if numeric is not None:
        c.numeric = numeric
    if ignore_alternate_chars is not None:
        c.shifted = bool(ignore_alternate_chars)
    if upper_first is not None:
        c.upper_first = upper_first
    return c


def primary_collator():
    return collator(strength=UCOL_PRIMARY)


def primary_collator_without_punctuation():
    return collator(strength=UCOL_PRIMARY, ignore_alternate_chars=True)


def sort_collator():
    return collator(strength=UCOL_SECONDARY, numeric=_tweak('numeric_collation', False))


def non_numeric_sort_collator():
    return collator(strength=UCOL_SECONDARY, numeric=False)


def numeric_collator():
    return collator(strength=UCOL_SECONDARY, numeric=True)


def case_sensitive_collator():
    return collator(strength=UCOL_TERTIARY, numeric=_tweak('numeric_collation', False), upper_first=False)


def make_sort_key_func(collator_function, func_name='sort_key'):
    def sort_key(a):
        return getattr(collator_function(), func_name)(a)

    return sort_key


def make_two_arg_func(collator_function, func_name='strcmp'):
    def two_args(a, b):
        return getattr(collator_function(), func_name)(a, b)

    return two_args


def make_change_case_func(which, name):
    def change_case(x):
        if which == UPPER_CASE:
            return x.upper()
        if which == LOWER_CASE:
            return x.lower()
        return title_case(x)

    change_case.__name__ = name
    return change_case


sort_key = make_sort_key_func(sort_collator)
numeric_sort_key = make_sort_key_func(numeric_collator)
primary_sort_key = make_sort_key_func(primary_collator)
case_sensitive_sort_key = make_sort_key_func(case_sensitive_collator)
collation_order = make_sort_key_func(sort_collator, 'collation_order')
collation_order_for_partitioning = make_sort_key_func(non_numeric_sort_collator, 'collation_order')

strcmp = make_two_arg_func(sort_collator)
case_sensitive_strcmp = make_two_arg_func(case_sensitive_collator)
primary_strcmp = make_two_arg_func(primary_collator)
upper = make_change_case_func(UPPER_CASE, 'upper')
lower = make_change_case_func(LOWER_CASE, 'lower')

_word_start = re.compile(r"(?<![\w'’])(\w)", re.UNICODE)


def title_case(x):
    """Capitalize the first letter of every word, leaving the rest as is."""
    return _word_start.sub(lambda m: m.group(1).upper(), x)


def capitalize(x):
    if not x:
        return x
    return x[0].upper() + x[1:].lower()


def swapcase(x):
    return x.swapcase()


swap_case = swapcase

find = make_two_arg_func(collator, 'find')
primary_find = make_two_arg_func(primary_collator, 'find')
primary_no_punc_find = make_two_arg_func(primary_collator_without_punctuation, 'find')
contains = make_two_arg_func(collator, 'contains')
primary_contains = make_two_arg_func(primary_collator, 'contains')
primary_no_punc_contains = make_two_arg_func(primary_collator_without_punctuation, 'contains')
startswith = make_two_arg_func(collator, 'startswith')
primary_startswith = make_two_arg_func(primary_collator, 'startswith')


def safe_chr(n):
    try:
        return chr(n)
    except ValueError, OverflowError:
        return '�'


def ord_string(s):
    return tuple(ord(c) for c in s)


def character_name(string):
    try:
        return unicodedata.name(string[0])
    except Exception:
        return None


def character_name_from_code(code):
    try:
        return unicodedata.name(chr(code))
    except Exception:
        return None


def normalize(text, mode='NFC'):
    return unicodedata.normalize(mode, text)


def contractions(col=None):
    return frozenset()


def partition_by_first_letter(items, reverse=False, key=lambda x: x):
    """Group `items` by the first letter of `key(item)` (accents and case
    ignored), as an OrderedDict in sorted order."""
    items = sorted(items, key=lambda x: sort_key(key(x)), reverse=reverse)
    ans = OrderedDict()
    last_c, last_ordnum = ' ', 0
    for item in items:
        c = key(item)
        ordnum, ordlen = collation_order(c)
        if last_ordnum != ordnum:
            last_c = c[:ordlen] if c else ''
            last_ordnum = ordnum
        try:
            ans[last_c].append(item)
        except KeyError:
            ans[last_c] = [item]
    return ans


string_length = len


def utf16_length(string):
    return len(string.encode('utf-16-le')) // 2


def remove_accents(txt):
    return ''.join(c for c in unicodedata.normalize('NFD', txt) if not _is_mark(c))


remove_accents_regex = remove_accents_icu = remove_accents


def word_prefix_find(collator, words, text, prefix):
    """The index of the first of `words` (an iterable of (pos, len) in
    `text`) that starts with `prefix` under `collator`, else -1."""
    for i, (pos, length) in enumerate(words):
        if collator.startswith(text[pos : pos + length], prefix):
            return i
    return -1


class _Icu:
    """The names calibre's `_icu` C module has, for code that pokes at it."""

    UCOL_PRIMARY, UCOL_SECONDARY, UCOL_TERTIARY = UCOL_PRIMARY, UCOL_SECONDARY, UCOL_TERTIARY
    UCOL_ALTERNATE_HANDLING, UCOL_SHIFTED, UCOL_NON_IGNORABLE = (
        UCOL_ALTERNATE_HANDLING,
        UCOL_SHIFTED,
        UCOL_NON_IGNORABLE,
    )
    UPPER_CASE, LOWER_CASE, TITLE_CASE = UPPER_CASE, LOWER_CASE, TITLE_CASE
    UBRK_CHARACTER, UBRK_WORD, UBRK_LINE, UBRK_SENTENCE = UBRK_CHARACTER, UBRK_WORD, UBRK_LINE, UBRK_SENTENCE
    NFC, NFD, NFKC, NFKD = 'NFC', 'NFD', 'NFKC', 'NFKD'
    unicode_version = icu_unicode_version
    Collator = Collator
    chr = staticmethod(safe_chr)
    ord_string = staticmethod(ord_string)
    swap_case = staticmethod(swapcase)
    normalize = staticmethod(lambda mode, text: unicodedata.normalize(mode, text))

    @staticmethod
    def set_default_encoding(enc):
        pass

    @staticmethod
    def set_filesystem_encoding(enc):
        pass

    @staticmethod
    def character_name(s):
        return character_name(s)

    @staticmethod
    def character_name_from_code(c):
        return character_name_from_code(c)


_icu = _Icu()
