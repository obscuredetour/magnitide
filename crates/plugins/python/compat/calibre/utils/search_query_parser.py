# Derived from calibre's src/calibre/utils/search_query_parser.py (calibre 9.15).
# Copyright: 2008, Kovid Goyal kovid@kovidgoyal.net. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.utils.search_query_parser` (calibre 9.15): the
grammar of calibre's search language, split from what each `field:value`
term matches (`get_matches`, which the database's search implements).

Grammar: terms are `value` or `location:value` (quoted when they hold
spaces or operators), joined by `and`, `or`, `not` (`and` is implied
between adjacent terms), grouped with parentheses.
"""

import re

from calibre.constants import preferred_encoding
from calibre.utils.icu import lower as icu_lower


class ParseException(Exception):
    @property
    def msg(self):
        if len(self.args) > 0:
            return self.args[0]
        return ''


REPLACEMENTS = tuple(('\\' + x, chr(i + 1)) for i, x in enumerate('\\"()'))


class Parser:
    def __init__(self):
        self.current_token = 0
        self.tokens = None

    OPCODE = 1
    WORD = 2
    QUOTED_WORD = 3
    EOF = 4
    REPLACEMENTS = REPLACEMENTS

    # Had to use string.lowercase for python 2.x compatibility
    lex_scanner = re.Scanner(
        [
            (r'[()]', lambda x, t: (Parser.OPCODE, t)),
            (r'@.+?:[^")\s]+', lambda x, t: (Parser.WORD, str(t))),
            (r'[^"()\s]+', lambda x, t: (Parser.WORD, str(t))),
            (r'".*?((?<!\\)")', lambda x, t: (Parser.QUOTED_WORD, t[1:-1])),
            (r'\s+', None),
        ],
        flags=re.DOTALL,
    )

    def token(self, advance=False):
        if self.is_eof():
            return None
        res = self.tokens[self.current_token][1]
        if advance:
            self.current_token += 1
        return res

    def lcase_token(self, advance=False):
        if self.is_eof():
            return None
        res = self.tokens[self.current_token][1]
        if advance:
            self.current_token += 1
        return icu_lower(res)

    def token_type(self):
        if self.is_eof():
            return self.EOF
        return self.tokens[self.current_token][0]

    def is_eof(self):
        return self.current_token >= len(self.tokens)

    def advance(self):
        self.current_token += 1

    def tokenize(self, expr):
        # Strip out escaped backslashes, quotes and parens so that the
        # lex scanner doesn't get confused. We put them back later.
        for k, v in self.REPLACEMENTS:
            expr = expr.replace(k, v)
        tokens = self.lex_scanner.scan(expr)[0]

        def unescape(x):
            for k, v in self.REPLACEMENTS:
                x = x.replace(v, k[1:])
            return x

        return [(tt, unescape(tv) if tt in (self.WORD, self.QUOTED_WORD) else tv) for tt, tv in tokens]

    def parse(self, expr, locations):
        self.locations = locations
        self.tokens = self.tokenize(expr)
        self.current_token = 0
        prog = self.or_expression()
        if not self.is_eof():
            raise ParseException('Extra characters at end of search')
        return prog

    def or_expression(self):
        lhs = self.and_expression()
        if self.lcase_token() == 'or':
            self.advance()
            return ['or', lhs, self.or_expression()]
        return lhs

    def and_expression(self):
        lhs = self.not_expression()
        if self.lcase_token() == 'and':
            self.advance()
            return ['and', lhs, self.and_expression()]

        # Account for the optional 'and'
        if (self.token_type() in [self.WORD, self.QUOTED_WORD] or self.token() == '(') and self.lcase_token() != 'or':
            return ['and', lhs, self.and_expression()]
        return lhs

    def not_expression(self):
        if self.lcase_token() == 'not':
            self.advance()
            return ['not', self.not_expression()]
        return self.location_expression()

    def location_expression(self):
        if self.token_type() == self.OPCODE and self.token() == '(':
            self.advance()
            res = self.or_expression()
            if self.token_type() != self.OPCODE or self.token(advance=True) != ')':
                raise ParseException('missing )')
            return res
        if self.token_type() not in (self.WORD, self.QUOTED_WORD):
            raise ParseException('Invalid syntax. Expected a lookup name or a word')

        return self.base_token()

    def base_token(self):
        if self.token_type() == self.QUOTED_WORD:
            return ['token', 'all', self.token(advance=True)]

        words = self.token(advance=True).split(':')

        # The complexity here comes from having colon-separated search
        # values. That forces us to check that the first "word" in a colon-
        # separated group is a valid location. If not, then the token must
        # be reconstructed. We also have the problem that locations can be
        # followed by quoted strings that appear as the next token. and that
        # tokens can be a sequence of colons.

        # We have a location if there is more than one word and the first
        # word is in locations. This check could produce a "wrong" answer if
        # the search string is something like 'author: "foo"' because it
        # will be interpreted as the search 'author:foo'.
        if len(words) > 1 and words[0].lower() in self.locations:
            loc = words[0].lower()
            words = words[1:]
            if len(words) == 1 and self.token_type() == self.QUOTED_WORD:
                return ['token', loc, self.token(advance=True)]
            return ['token', loc, ':'.join(words)]

        return ['token', 'all', ':'.join(words)]


class SearchQueryParser:
    """Parses a search and evaluates it: subclasses implement
    `universal_set()` (every id) and `get_matches(location, query,
    candidates=None)` (the ids matching one term)."""

    SORT_FIELDS = []

    def __init__(self, locations, test=False, optimize=False, lookup_saved_search=None, parse_cache=None):
        self.sqp_initialize(locations, test=test, optimize=optimize)
        self.parser = Parser()
        self.lookup_saved_search = lookup_saved_search
        self.sqp_parse_cache = parse_cache

    def sqp_change_locations(self, locations):
        self.sqp_initialize(locations, optimize=self.optimize)
        if self.sqp_parse_cache is not None:
            self.sqp_parse_cache.clear()

    def sqp_initialize(self, locations, test=False, optimize=False):
        self.locations = locations
        self._tests_failed = False
        self.optimize = optimize

    def get_queried_fields(self, query):
        tree = self._get_tree(query)
        yield from self._walk_expr(tree)

    def _walk_expr(self, tree):
        if tree[0] in ('or', 'and'):
            yield from self._walk_expr(tree[1])
            yield from self._walk_expr(tree[2])
        elif tree[0] == 'not':
            yield from self._walk_expr(tree[1])
        else:
            if tree[1] == 'search':
                yield from self._walk_expr(self._get_tree(self._get_saved_search_text(tree[2])))
            else:
                yield tree[1], tree[2]

    def _get_tree(self, query):
        try:
            res = self.sqp_parse_cache.get(query, None)
        except AttributeError:
            res = None
        if res is not None:
            return res
        try:
            res = self.parser.parse(query, self.locations)
        except RuntimeError:
            import repr as reprlib

            raise ParseException('Failed to parse query, recursion limit reached: %s' % reprlib.repr(query))
        if self.sqp_parse_cache is not None:
            self.sqp_parse_cache[query] = res
        return res

    def parse(self, query, candidates=None):
        """Evaluate `query`; `candidates` limits the ids searched."""
        self.recurse_level = 0
        tree = self._get_tree(query)
        if candidates is None:
            candidates = self.universal_set()
        t = self.evaluate(tree, candidates)
        return t

    def method(self, group_name):
        return getattr(self, 'evaluate_' + group_name)

    def evaluate(self, parse_result, candidates):
        return self.method(parse_result[0])(parse_result[1:], candidates)

    def evaluate_and(self, argument, candidates):
        # RHS checks only those items that pass LHS check.
        l = self.evaluate(argument[0], candidates)
        return l.intersection(self.evaluate(argument[1], l))

    def evaluate_or(self, argument, candidates):
        # RHS checks only those elements not matched by LHS
        l = self.evaluate(argument[0], candidates)
        return l.union(self.evaluate(argument[1], candidates.difference(l)))

    def evaluate_not(self, argument, candidates):
        # unary op checks only candidates. Result: list of items matching
        # the test.
        l = self.evaluate(argument[0], candidates)
        return candidates.difference(l)

    def _get_saved_search_text(self, query):
        if query.startswith('='):
            query = query[1:]
        try:
            if self.lookup_saved_search is None:
                raise ParseException('No saved searches available')
            ss = self.lookup_saved_search(query)
            if ss is None:
                raise ParseException(f'Unknown saved search: {query}')
            return ss
        except ParseException as e:
            raise e
        except Exception:
            raise ParseException('Error: ' + 'Unknown error during evaluation of saved search')

    def evaluate_token(self, argument, candidates):
        location = argument[0]
        query = argument[1]
        if location.lower() == 'search':
            if self.recurse_level > 5:
                self.searches_seen.add(query)
                raise ParseException(f'Recursive saved search: {query}')
            if self.recurse_level == 0:
                self.searches_seen = set()
            self.recurse_level += 1
            if query in self.searches_seen:
                raise ParseException(f'Recursive saved search: {query}')
            self.searches_seen.add(query)
            try:
                res = self.parse(self._get_saved_search_text(query), candidates)
            finally:
                self.recurse_level -= 1
            return res
        return self._get_matches(location, query, candidates)

    def _get_matches(self, location, query, candidates):
        if self.optimize:
            return self.get_matches(location, query, candidates=candidates)
        return self.get_matches(location, query)

    def get_matches(self, location, query, candidates=None):
        """The ids matching one term. `location` is the field ('title',
        '#pages', 'all'); `query` the text. Subclasses implement it."""
        return set()

    def universal_set(self):
        """Every id."""
        return set()


def saved_searches():
    from calibre.utils.config import prefs

    class _SS:
        def lookup(self, name):
            return prefs['saved_searches'].get(name)

        def names(self):
            return sorted(prefs['saved_searches'])

        def add(self, name, val):
            d = prefs['saved_searches']
            d[name] = val
            prefs['saved_searches'] = d

        def delete(self, name):
            d = prefs['saved_searches']
            d.pop(name, None)
            prefs['saved_searches'] = d

    return _SS()


__all__ = ['SearchQueryParser', 'ParseException', 'Parser', 'saved_searches', 'preferred_encoding']
