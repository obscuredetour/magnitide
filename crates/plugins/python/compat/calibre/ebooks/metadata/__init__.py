"""Mirrors calibre's `calibre.ebooks.metadata` package root (calibre 9.15):
title and author sort names, author string parsing, ISBN checks and the
`MetaInformation` alias."""

import re
import sys

from calibre import force_unicode, prints, relpath
from calibre.utils.config_base import tweaks

try:
    _author_pat = re.compile(tweaks['authors_split_regex'])
except Exception:
    prints('Author split regexp:', tweaks['authors_split_regex'], 'is invalid, using default')
    _author_pat = re.compile(r'(?i),?\s+(and|with)\s+')


def string_to_authors(raw):
    """'A & B and C' → ['A', 'B', 'C']; '&&' is a literal ampersand."""
    if not raw:
        return []
    raw = raw.replace('&&', '￿')
    raw = _author_pat.sub('&', raw)
    authors = [a.strip().replace('￿', '&') for a in raw.split('&')]
    return [a for a in authors if a]


def authors_to_string(authors):
    if authors is not None:
        return ' & '.join([a.replace('&', '&&') for a in authors if a])
    return ''


def remove_bracketed_text(src, brackets=None):
    from calibre import remove_bracketed_text as rbt

    return rbt(src, brackets)


def author_to_author_sort(author, method=None):
    """The sort name for an author, by the `author_sort_copy_method` tweak:
    'invert' (Last, First), 'copy', 'comma' or 'nocomma'."""
    if not author:
        return ''
    if method is None:
        method = tweaks['author_sort_copy_method']
    sauthor = remove_bracketed_text(author).strip()
    tokens = sauthor.split()
    if len(tokens) < 2:
        return author
    if method == 'copy':
        return author

    ltoks = frozenset(x.lower() for x in tokens)
    copy_words = frozenset(x.lower() for x in tweaks['author_name_copywords'] or ())
    if ltoks.intersection(copy_words):
        return author

    if method == 'comma' and ',' in ''.join(tokens):
        return author

    prefixes = {force_unicode(y).lower() for y in tweaks['author_name_prefixes'] or ()}
    prefixes |= {y + '.' for y in prefixes}
    while True:
        if not tokens:
            return author
        tok = tokens[0].lower()
        if tok in prefixes:
            tokens = tokens[1:]
        else:
            break

    suffixes = {force_unicode(y).lower() for y in tweaks['author_name_suffixes'] or ()}
    suffixes |= {y + '.' for y in suffixes}

    suffix = ''
    while True:
        if not tokens:
            return author
        last = tokens[-1].lower()
        if last in suffixes:
            suffix = tokens[-1] + ' ' + suffix
            tokens = tokens[:-1]
        else:
            break
    suffix = suffix.strip()

    if tweaks['author_use_surname_prefixes']:
        surname_prefixes = {force_unicode(y).lower() for y in tweaks['author_surname_prefixes'] or ()}
        if len(tokens) == 2 and tokens[0].lower() in surname_prefixes:
            return author
        if len(tokens) > 2 and tokens[-2].lower() in surname_prefixes:
            tokens = tokens[:-2] + [tokens[-2] + ' ' + tokens[-1]]

    if method == 'invert' or method == 'comma' or method == 'nocomma':
        if len(tokens) < 2:
            return author
        atokens = tokens[-1:] + tokens[:-1]
        num_toks = len(atokens)
        if suffix:
            atokens.append(suffix)
        if method != 'nocomma' and num_toks > 1:
            atokens[0] += ','
        return ' '.join(atokens)
    return author


def authors_to_sort_string(authors):
    return ' & '.join(map(author_to_author_sort, authors))


_title_pats = {}


def get_title_sort_pat(lang=None):
    ans = _title_pats.get(lang, None)
    if ans is not None:
        return ans
    q = lang
    from calibre.utils.localization import canonicalize_lang, get_lang

    if lang is None:
        q = tweaks['default_language_for_title_sort']
        if q is None:
            q = get_lang()
    q = canonicalize_lang(q) if q else q
    data = tweaks['per_language_title_sort_articles'] or {}
    try:
        ans = data[q]
    except KeyError:
        try:
            ans = data[q.partition('_')[0] if q else 'eng']
        except Exception:
            ans = data.get('eng', (r'A\s+', r'The\s+', r'An\s+'))
    try:
        ans = re.compile(r'^(%s)' % ('|'.join(ans)), re.IGNORECASE)
    except Exception:
        ans = re.compile(r'^(A\s+|The\s+|An\s+)', re.IGNORECASE)
    _title_pats[lang] = ans
    return ans


_ignore_starts = '\'"' + ''.join(chr(x) for x in list(range(0x2018, 0x201E)) + [0x2032, 0x2033])


def title_sort(title, order=None, lang=None):
    """The sort title: leading articles moved to the end ('The Book' →
    'Book, The'), unless the `title_series_sorting` tweak says otherwise."""
    if order is None:
        order = tweaks['title_series_sorting']
    title = title.strip()
    if order == 'strictly_alphabetic':
        return title
    if title and title[0] in _ignore_starts:
        title = title[1:]
    match = get_title_sort_pat(lang).search(title)
    if match:
        try:
            prep = match.group(1)
        except IndexError:
            pass
        else:
            title = title[len(prep) :] + ', ' + prep
            if title[0] in _ignore_starts:
                title = title[1:]
    return title.strip()


coding = list(
    zip(
        [1000, 900, 500, 400, 100, 90, 50, 40, 10, 9, 5, 4, 1],
        ['m', 'cm', 'd', 'cd', 'c', 'xc', 'l', 'xl', 'x', 'ix', 'v', 'iv', 'i'],
    )
)


def roman(num):
    if num <= 0 or num >= 4000 or int(num) != num:
        return str(num)
    result = []
    for d, r in coding:
        while num >= d:
            result.append(r)
            num -= d
    return ''.join(result)


def fmt_sidx(i, fmt='%.2f', use_roman=False):
    """A series index for display: 1.0 → '1', 1.5 → '1.50'."""
    if i is None or i == '':
        i = 1
    try:
        i = float(i)
    except Exception:
        return str(i)
    if int(i) == float(i):
        return roman(int(i)) if use_roman else '%d' % int(i)
    return fmt % i


def check_isbn10(isbn):
    try:
        digits = tuple(map(int, isbn[:9]))
        products = [(i + 1) * digits[i] for i in range(9)]
        check = sum(products) % 11
        if (check == 10 and isbn[9] == 'X') or check == int(isbn[9]):
            return isbn
    except Exception:
        pass
    return None


def check_isbn13(isbn):
    try:
        digits = tuple(map(int, isbn[:12]))
        products = [(1 if i % 2 == 0 else 3) * digits[i] for i in range(12)]
        check = 10 - (sum(products) % 10)
        if check == 10:
            check = 0
        if str(check) == isbn[12]:
            return isbn
    except Exception:
        pass
    return None


def check_isbn(isbn, simple_sanitize=False):
    """The ISBN with separators removed if it checks out, else None."""
    if not isbn:
        return None
    if simple_sanitize:
        isbn = isbn.upper().replace('-', '').strip().replace(' ', '')
    else:
        isbn = re.sub(r'[^0-9X]', '', isbn.upper())
    all_same = re.match(r'(\d)\1{9,12}$', isbn)
    if all_same is not None:
        return None
    if len(isbn) == 10:
        return check_isbn10(isbn)
    if len(isbn) == 13:
        return check_isbn13(isbn)
    return None


def normalize_isbn(isbn):
    if not isbn:
        return isbn
    ans = check_isbn(isbn)
    if ans is None:
        return isbn
    if len(ans) == 10:
        from itertools import cycle

        ans = '978' + ans[:-1]
        digits = tuple(map(int, ans))
        check = 10 - (sum(d * w for d, w in zip(digits, cycle((1, 3)))) % 10)
        if check == 10:
            check = 0
        ans += str(check)
    return ans


def check_issn(issn):
    if not issn:
        return None
    issn = re.sub(r'[^0-9X]', '', issn.upper())
    try:
        digits = tuple(map(int, issn[:7]))
        products = [(8 - i) * d for i, d in enumerate(digits)]
        check = 11 - sum(products) % 11
        if (check == 10 and issn[7] == 'X') or check == int(issn[7]):
            return issn
    except Exception:
        pass
    return None


def check_doi(doi):
    """A DOI, cleaned of a URL prefix, if it looks like one."""
    if not doi:
        return None
    doi_check = re.search(r'10\.\d{4,}/\S+', doi)
    if doi_check is not None:
        return doi_check.group()
    return None


def format_isbn(isbn):
    cisbn = check_isbn(isbn)
    if not cisbn:
        return isbn
    if len(cisbn) == 13:
        return f'{cisbn[:3]}-{cisbn[3]}-{cisbn[4:9]}-{cisbn[9:12]}-{cisbn[12]}'
    return f'{cisbn[0]}-{cisbn[1:6]}-{cisbn[6:9]}-{cisbn[9]}'


def rating_to_stars(value, allow_half_stars=False, star='★', half='⯨'):
    r = max(0, min(int(value or 0), 10))
    ans = star * (r // 2)
    if allow_half_stars and r % 2:
        ans += half
    return ans


def resolve_default_cover_path(title, author):
    return None


def MetaInformation(title, authors=('Unknown',)):
    """calibre's old name for `Metadata`; `title` may be a Metadata to copy."""
    from calibre.ebooks.metadata.book.base import Metadata

    mi = None
    if hasattr(title, 'title') and hasattr(title, 'authors'):
        mi = title
        title = mi.title
        authors = mi.authors
    return Metadata(title, authors, other=mi)


def parse_opf_version(raw):
    parts = (raw or '').split('.')
    try:
        major = int(parts[0])
    except Exception:
        return 2, 0
    try:
        v = (major, int(parts[1] if len(parts) > 1 else 0))
    except Exception:
        v = (major, 0)
    return v


def get_default_author_sort():
    return tweaks['author_sort_copy_method']


__all__ = [
    'string_to_authors',
    'authors_to_string',
    'author_to_author_sort',
    'authors_to_sort_string',
    'title_sort',
    'get_title_sort_pat',
    'fmt_sidx',
    'roman',
    'check_isbn',
    'check_isbn10',
    'check_isbn13',
    'normalize_isbn',
    'check_issn',
    'check_doi',
    'format_isbn',
    'rating_to_stars',
    'MetaInformation',
    'remove_bracketed_text',
    'parse_opf_version',
    'resolve_default_cover_path',
]
del relpath, sys
