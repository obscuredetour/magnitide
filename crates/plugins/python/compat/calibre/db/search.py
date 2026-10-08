"""Mirrors calibre's `calibre.db.search` (calibre 9.15): evaluating calibre's
search language against the library.

Supported, as in calibre: plain text (matched in title, authors, tags,
series, publisher, comments, identifiers and custom text columns, or the
columns the `limit_search_columns` tweak names), `field:value` with
`=` (exact), `~` (regular expression) and `^` prefixes, numbers and dates
with `<`, `<=`, `=`, `>`, `>=` (dates also `today`, `yesterday`,
`thismonth`, `Ndaysago`), `true`/`false`, `formats:`, `identifiers:type:val`,
`#custom`, `all`, `and`/`or`/`not` and parentheses.
"""

import re
from datetime import timedelta

from calibre.utils.config_base import prefs
from calibre.utils.date import UNDEFINED_DATE, now, parse_date
from calibre.utils.icu import lower as icu_lower
from calibre.utils.icu import primary_contains, primary_startswith
from calibre.utils.search_query_parser import ParseException, SearchQueryParser

CONTAINS_MATCH = 0
EQUALS_MATCH = 1
REGEXP_MATCH = 2
ACCENT_MATCH = 3

_DATE_WORDS = {'today', 'yesterday', 'thismonth'}


def _matchkind(query, case_sensitive=False):
    matchkind = CONTAINS_MATCH
    if len(query) > 1:
        if query.startswith('\\'):
            query = query[1:]
        elif query.startswith('='):
            matchkind = EQUALS_MATCH
            query = query[1:]
        elif query.startswith('~'):
            matchkind = REGEXP_MATCH
            query = query[1:]
        elif query.startswith('^'):
            matchkind = ACCENT_MATCH
            query = query[1:]
    if not case_sensitive and matchkind != REGEXP_MATCH:
        query = icu_lower(query)
    return matchkind, query


def _match(query, value, matchkind, use_primary_find_in_search=True, case_sensitive=False):
    if query.startswith('..'):
        query = query[1:]
        sq = query[1:]
        internal_match_ok = True
    else:
        internal_match_ok = False
    for t in value:
        try:
            t = icu_lower(t) if not case_sensitive else t
            if matchkind == EQUALS_MATCH:
                if internal_match_ok:
                    if query == t:
                        return True
                    comps = [c.strip() for c in t.split('.') if c.strip()]
                    for comp in comps:
                        if sq == comp:
                            return True
                elif query[0] == '.':
                    if t.startswith(query[1:]):
                        ql = len(query) - 1
                        if (len(t) == ql) or (t[ql : ql + 1] == '.'):
                            return True
                elif query == t:
                    return True
            elif matchkind == REGEXP_MATCH:
                if re.search(query, t, re.I | re.UNICODE if not case_sensitive else re.UNICODE):
                    return True
            elif matchkind == CONTAINS_MATCH:
                if use_primary_find_in_search:
                    if primary_contains(query, t):
                        return True
                elif query in t:
                    return True
            elif matchkind == ACCENT_MATCH:
                if query in t:
                    return True
        except re.error:
            pass
    return False


def _parse_number(raw):
    if not raw:
        return None
    raw = raw.strip()
    mult = 1
    if raw[-1].lower() in 'kmg':
        mult = {'k': 1024, 'm': 1024**2, 'g': 1024**3}[raw[-1].lower()]
        raw = raw[:-1]
    try:
        return float(raw) * mult
    except ValueError:
        return None


class Search:
    """Evaluates searches for a `Cache`."""

    def __init__(self, db, opt_name, all_search_locations=()):
        self.db = db
        self.opt_name = opt_name
        self.all_search_locations = frozenset(all_search_locations)
        self.parser = _Parser(db, self.all_search_locations)

    def change_locations(self, newlocs):
        self.all_search_locations = frozenset(newlocs)
        self.parser.sqp_change_locations(self.all_search_locations)

    def __call__(self, dbcache, query, search_restriction, virtual_fields=None, book_ids=None):
        candidates = set(dbcache.all_book_ids()) if book_ids is None else set(book_ids)
        query = (query or '').strip()
        if search_restriction:
            candidates = self.parser.parse(search_restriction, candidates=candidates)
        if not query:
            return candidates
        return self.parser.parse(query, candidates=candidates)


class _Parser(SearchQueryParser):
    def __init__(self, db, locations):
        self.db = db
        SearchQueryParser.__init__(
            self, locations, optimize=True, lookup_saved_search=self._lookup_saved_search, parse_cache={}
        )

    def _lookup_saved_search(self, name):
        ss = self.db.backend.prefs.get('saved_searches', {}) or {}
        q = ss.get(name)
        if q is None:
            q = (prefs['saved_searches'] or {}).get(name)
        return q

    def universal_set(self):
        return set(self.db.all_book_ids())

    def _values(self, field, book_id):
        """A book's value for a field as a list of strings (lists expanded)."""
        try:
            v = self.db.field_for(field, book_id)
        except KeyError:
            return []
        if v is None:
            return []
        if isinstance(v, dict):
            return [f'{k}:{x}' for k, x in v.items()]
        if isinstance(v, (list, tuple)):
            return [str(x) for x in v]
        if isinstance(v, bool):
            return ['yes' if v else 'no']
        return [str(v)]

    def get_matches(self, location, query, candidates=None):
        if candidates is None:
            candidates = self.universal_set()
        location = location.lower().strip()
        if not query:
            return set()
        if location == 'all' and query.lower() == 'all':
            return set(candidates) if query == 'all' else set()
        fm = self.db.field_metadata
        if location == 'search':
            return SearchQueryParser.evaluate_token(self, ['search', query], candidates)
        if location == 'vl':
            return set(candidates) & set(self.db.books_in_virtual_library(query))
        if location == 'template':
            return set()
        location = fm.search_term_to_field_key(location) if location != 'all' else 'all'
        if location == 'all':
            return self._matches_all(query, candidates)
        if location == 'formats' or location == 'format':
            return self._formats(query, candidates)
        if location == 'identifiers':
            return self._identifiers(query, candidates)
        if location not in fm:
            raise ParseException('No column named %s' % location)
        meta = fm[location]
        dt = meta['datatype']
        if dt == 'datetime':
            return self._dates(location, query, candidates)
        if dt in ('int', 'float', 'rating'):
            return self._numbers(location, query, candidates, dt)
        if dt == 'bool':
            return self._bools(location, query, candidates)
        if location == 'cover':
            want = query.lower() in ('true', 'yes')
            return {b for b in candidates if bool(self.db.field_for('cover', b)) == want}
        if location == 'series_index':
            return self._numbers('series_index', query, candidates, 'float')
        return self._text(location, query, candidates)

    def _text(self, location, query, candidates):
        if query.lower() == 'true':
            return {b for b in candidates if self._values(location, b)}
        if query.lower() == 'false':
            return {b for b in candidates if not self._values(location, b)}
        matchkind, q = _matchkind(query, case_sensitive=prefs['case_sensitive'])
        if location == 'authors' and matchkind == CONTAINS_MATCH:
            pass
        ans = set()
        for b in candidates:
            vals = self._values(location, b)
            if location == 'authors':
                vals = vals + [v.replace(',', ' ') for v in vals]
            if _match(
                q,
                vals,
                matchkind,
                use_primary_find_in_search=prefs['use_primary_find_in_search'],
                case_sensitive=prefs['case_sensitive'],
            ):
                ans.add(b)
        return ans

    def _matches_all(self, query, candidates):
        fields = ['title', 'authors', 'tags', 'series', 'publisher', 'comments', 'identifiers']
        if prefs['limit_search_columns']:
            fields = [f for f in (prefs['limit_search_columns_to'] or fields)]
        for key in self.db.custom_field_keys(include_composites=False):
            if self.db.field_metadata[key]['datatype'] in ('text', 'comments', 'series', 'enumeration'):
                fields.append(key)
        ans = set()
        for f in fields:
            try:
                ans |= self._text(f, query, candidates - ans)
            except Exception:
                continue
        return ans

    def _formats(self, query, candidates):
        q = query.lower()
        if q == 'true':
            return {b for b in candidates if self.db.field_for('formats', b)}
        if q == 'false':
            return {b for b in candidates if not self.db.field_for('formats', b)}
        matchkind, q = _matchkind(query)
        return {
            b for b in candidates if _match(q, [f.lower() for f in (self.db.field_for('formats', b) or ())], matchkind)
        }

    def _identifiers(self, query, candidates):
        q = query.lower()
        if q in ('true', 'false'):
            want = q == 'true'
            return {b for b in candidates if bool(self.db.field_for('identifiers', b)) == want}
        typ, sep, val = query.partition(':')
        ans = set()
        for b in candidates:
            ids = self.db.field_for('identifiers', b) or {}
            if not sep:
                matchkind, qq = _matchkind(query)
                if _match(qq, [f'{k}:{v}' for k, v in ids.items()] + list(ids), matchkind):
                    ans.add(b)
                continue
            mk, qq = _matchkind(val) if val else (CONTAINS_MATCH, '')
            for k, v in ids.items():
                if k.lower() == typ.lower().lstrip('=~^') and (not val or _match(qq, [v], mk)):
                    ans.add(b)
                    break
        return ans

    def _bools(self, location, query, candidates):
        q = query.lower()
        ans = set()
        for b in candidates:
            v = self.db.field_for(location, b)
            if q in ('true', 'yes', 'checked', '_yes_') and v is True:
                ans.add(b)
            elif q in ('false', 'no', 'unchecked', '_no_') and v is False:
                ans.add(b)
            elif q in ('empty', 'blank', '_empty_') and v is None:
                ans.add(b)
        return ans

    def _numbers(self, location, query, candidates, dt):
        q = query.strip()
        if q.lower() == 'false':
            return {b for b in candidates if self.db.field_for(location, b) in (None, 0)}
        if q.lower() == 'true':
            return {b for b in candidates if self.db.field_for(location, b) not in (None, 0)}
        op = '='
        for candidate_op in ('>=', '<=', '=', '>', '<', '!='):
            if q.startswith(candidate_op):
                op = candidate_op
                q = q[len(candidate_op) :]
                break
        val = _parse_number(q)
        if val is None:
            raise ParseException('Not a number: %s' % query)
        if dt == 'rating':
            val = val * 2
        ans = set()
        for b in candidates:
            v = self.db.field_for(location, b)
            if v is None:
                continue
            try:
                v = float(v)
            except Exception:
                continue
            if {'=': v == val, '!=': v != val, '>': v > val, '<': v < val, '>=': v >= val, '<=': v <= val}[op]:
                ans.add(b)
        return ans

    def _dates(self, location, query, candidates):
        q = query.strip()
        if q.lower() == 'false':
            return {
                b
                for b in candidates
                if self.db.field_for(location, b) in (None, UNDEFINED_DATE)
                or self.db.field_for(location, b).year <= UNDEFINED_DATE.year
            }
        if q.lower() == 'true':
            return {
                b
                for b in candidates
                if self.db.field_for(location, b) not in (None, UNDEFINED_DATE)
                and self.db.field_for(location, b).year > UNDEFINED_DATE.year
            }
        op = '='
        for candidate_op in ('>=', '<=', '=', '>', '<', '!='):
            if q.startswith(candidate_op):
                op = candidate_op
                q = q[len(candidate_op) :].strip()
                break
        today = now().replace(hour=0, minute=0, second=0, microsecond=0)
        ql = q.lower()
        precision = 'day'
        if ql == 'today':
            target = today
        elif ql == 'yesterday':
            target = today - timedelta(days=1)
        elif ql == 'thismonth':
            target = today.replace(day=1)
            precision = 'month'
        elif ql.endswith('daysago'):
            target = today - timedelta(days=int(ql[:-7] or 0))
        else:
            try:
                target = parse_date(q, as_utc=False)
            except Exception:
                raise ParseException('Not a date: %s' % query)
            parts = len(re.split(r'[-/.]', q.strip()))
            precision = {1: 'year', 2: 'month'}.get(parts, 'day')
        ans = set()
        for b in candidates:
            v = self.db.field_for(location, b)
            if v is None or v == UNDEFINED_DATE or v.year <= UNDEFINED_DATE.year:
                continue
            v = v.astimezone(today.tzinfo)
            if precision == 'year':
                a, c = v.year, target.year
            elif precision == 'month':
                a, c = (v.year, v.month), (target.year, target.month)
            else:
                a, c = v.date(), target.date()
            if {'=': a == c, '!=': a != c, '>': a > c, '<': a < c, '>=': a >= c, '<=': a <= c}[op]:
                ans.add(b)
        return ans


__all__ = [
    'Search',
    'ParseException',
    'CONTAINS_MATCH',
    'EQUALS_MATCH',
    'REGEXP_MATCH',
    'ACCENT_MATCH',
    'primary_startswith',
]
