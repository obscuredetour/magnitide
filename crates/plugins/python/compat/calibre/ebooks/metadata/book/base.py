"""Mirrors calibre's `calibre.ebooks.metadata.book.base` (calibre 9.15): the
`Metadata` object, which carries a book's metadata between calibre's
database, its file readers and writers, and plugins.

Semantics kept from calibre: standard fields are attributes with calibre's
null values (`title` is 'Unknown', `authors` is ['Unknown'], lists are
empty lists); `isbn` is a view on `identifiers`; custom columns (`#name`)
are attributes too once set with `set_user_metadata`; any other attribute
set on it is kept as a plain attribute (plugins hang their own on it);
`smart_update` merges another Metadata the way calibre's does.
"""

import copy
import traceback
from contextlib import suppress

from calibre import prints
from calibre.ebooks.metadata import author_to_author_sort, string_to_authors, title_sort
from calibre.ebooks.metadata.book import (
    SC_COPYABLE_FIELDS,
    SC_FIELDS_COPY_NOT_NULL,
    STANDARD_METADATA_FIELDS,
    TOP_LEVEL_IDENTIFIERS,
)
from calibre.utils.date import UNDEFINED_DATE, isoformat
from calibre.utils.icu import lower as icu_lower
from calibre.utils.icu import sort_key

# Special sets used to optimize the performance of getting and setting
# attributes on Metadata objects
SIMPLE_GET = frozenset(STANDARD_METADATA_FIELDS - TOP_LEVEL_IDENTIFIERS)
SIMPLE_SET = frozenset(SIMPLE_GET - {'identifiers'})


def human_readable(size, precision=2):
    """A size in bytes as a short string."""
    if size is None:
        return ''
    from calibre import human_readable as hr

    return hr(size)


NULL_VALUES = {
    'user_metadata': {},
    'cover_data': (None, None),
    'tags': [],
    'identifiers': {},
    'languages': [],
    'device_collections': [],
    'author_sort_map': {},
    'authors': ['Unknown'],
    'author_sort': 'Unknown',
    'title': 'Unknown',
    'user_categories': {},
    'author_link_map': {},
    'link_maps': {},
    'language': 'und',
}


def _unknown():
    return _('Unknown')


def reset_field_metadata():
    global field_metadata
    from calibre.library.field_metadata import FieldMetadata

    field_metadata = FieldMetadata()


field_metadata = None
reset_field_metadata()

ck = lambda typ: icu_lower(typ).strip().replace(':', '').replace(',', '')  # noqa: E731
cv = lambda val: val.strip().replace(',', '|')  # noqa: E731


class Metadata:
    """A book's metadata. Standard fields are attributes; custom ones are
    `#name` attributes after `set_user_metadata`. `is_null(field)` tells a
    null value from a set one."""

    __calibre_serializable__ = True

    def __init__(self, title, authors=('Unknown',), other=None, template_cache=None, formatter=None):
        """`title` and `authors` may be None (the null values are used);
        `other` is a Metadata (or an object with the same attributes) to
        copy from."""
        _data = copy.deepcopy(NULL_VALUES)
        _data['title'] = _data['authors'][0] = _data['author_sort'] = _unknown()
        _data.pop('language')
        object.__setattr__(self, '_data', _data)
        if other is not None:
            self.smart_update(other)
        else:
            if title:
                self.title = title
            if authors:
                # List of strings or []
                self.author = list(authors) if authors else []  # Needed for backward compatibility
                self.authors = list(authors) if authors else []
        from calibre.ebooks.metadata.book.formatter import SafeFormat

        self.formatter = SafeFormat() if formatter is None else formatter
        self.template_cache = template_cache

    def is_null(self, field):
        """True if `field` is null or the null value: None, empty, Unknown,
        an undefined date, or 0 for a rating."""
        null_val = NULL_VALUES.get(field, None)
        val = getattr(self, field, None)
        if val is None:
            return True
        if isinstance(val, str) and not val:
            return True
        if field in ('title', 'authors', 'author_sort') and (
            val == _unknown() or val == ['Unknown'] or val == 'Unknown'
        ):
            return True
        if field == 'authors' and val == [_unknown()]:
            return True
        if field == 'rating':
            return not val
        if field in ('pubdate', 'timestamp', 'last_modified'):
            return val == UNDEFINED_DATE or (hasattr(val, 'year') and val.year <= UNDEFINED_DATE.year)
        if field == 'series_index':
            return False
        if field == 'cover_data':
            return not (val and val[1])
        return not val if null_val is not None and val == null_val else val == null_val

    def set_null(self, field):
        null_val = copy.copy(NULL_VALUES.get(field))
        setattr(self, field, null_val)

    def __getattribute__(self, field):
        _data = object.__getattribute__(self, '_data')
        if field in SIMPLE_GET:
            return _data.get(field, None)
        if field in TOP_LEVEL_IDENTIFIERS:
            return _data.get('identifiers').get(field, None)
        if field == 'language':
            try:
                return _data.get('languages', [])[0]
            except Exception:
                return NULL_VALUES['language']
        try:
            return object.__getattribute__(self, field)
        except AttributeError:
            pass
        if field in _data['user_metadata']:
            d = _data['user_metadata'][field]
            val = d['#value#']
            if d['datatype'] != 'composite':
                return val
            if val is None:
                d['#value#'] = 'RECURSIVE_COMPOSITE FIELD (Metadata) ' + field
                val = d['#value#'] = self.formatter.safe_format(
                    d['display']['composite_template'],
                    self,
                    'TEMPLATE ERROR',
                    self,
                    column_name=field,
                    template_cache=self.template_cache,
                ).strip()
            return val
        if field.startswith('#') and field.endswith('_index'):
            try:
                return self.get_extra(field[:-6])
            except Exception:
                pass
        raise AttributeError(f'Metadata object has no attribute named: {field!r}')

    def __setattr__(self, field, val, extra=None):
        _data = object.__getattribute__(self, '_data')
        if field in TOP_LEVEL_IDENTIFIERS:
            field, val = self._clean_identifier(field, val)
            identifiers = _data['identifiers']
            identifiers.pop(field, None)
            if val:
                identifiers[field] = val
        elif field == 'identifiers':
            if not val:
                val = copy.copy(NULL_VALUES.get('identifiers', None))
            self.set_identifiers(val)
        elif field == 'language':
            langs = []
            if val and val.lower() != 'und':
                langs = [val]
            _data['languages'] = langs
        elif field in SIMPLE_SET:
            if val is None:
                val = copy.copy(NULL_VALUES.get(field, None))
                if field in ('title', 'author_sort'):
                    val = _unknown()
                elif field == 'authors':
                    val = [_unknown()]
            _data[field] = val
        elif field in _data['user_metadata']:
            _data['user_metadata'][field]['#value#'] = val
            _data['user_metadata'][field]['#extra#'] = extra
        else:
            # You are allowed to stick arbitrary attributes onto this object as
            # long as they don't conflict with global or user metadata names
            # Don't abuse this privilege
            self.__dict__[field] = val

    def __iter__(self):
        return iter(object.__getattribute__(self, '_data'))

    def has_key(self, key):
        return key in object.__getattribute__(self, '_data')

    def __contains__(self, key):
        return self.has_key(key)

    def deepcopy(self, class_generator=lambda: Metadata(None)):
        """A copy; `class_generator` gives the object to copy into."""
        m = class_generator()
        if not isinstance(m, Metadata):
            return None
        object.__setattr__(m, '__dict__', copy.deepcopy(self.__dict__))
        return m

    def deepcopy_metadata(self):
        m = Metadata(None)
        object.__setattr__(m, '_data', copy.deepcopy(object.__getattribute__(self, '_data')))
        return m

    def get(self, field, default=None):
        try:
            return self.__getattribute__(field)
        except AttributeError:
            return default

    def get_extra(self, field, default=None):
        _data = object.__getattribute__(self, '_data')
        if field in _data['user_metadata']:
            try:
                return _data['user_metadata'][field]['#extra#']
            except Exception:
                return default
        raise AttributeError(f'Metadata object has no attribute named: {field!r}')

    def set(self, field, val, extra=None):
        self.__setattr__(field, val, extra)

    def get_identifiers(self):
        """A copy of the identifiers dict."""
        ans = object.__getattribute__(self, '_data')['identifiers']
        if not ans:
            ans = {}
        return copy.deepcopy(ans)

    def _clean_identifier(self, typ, val):
        if typ:
            typ = ck(typ)
        if val:
            val = cv(val)
        return typ, val

    def set_identifiers(self, identifiers):
        """Replace all identifiers."""
        cleaned = {ck(k): cv(v) for k, v in identifiers.items() if k and v}
        object.__getattribute__(self, '_data')['identifiers'] = cleaned

    def set_identifier(self, typ, val):
        """Set one identifier; an empty `val` removes it."""
        typ, val = self._clean_identifier(typ, val)
        if not typ:
            return
        identifiers = object.__getattribute__(self, '_data')['identifiers']
        if not val and typ in identifiers:
            identifiers.pop(typ)
        if val:
            identifiers[typ] = val

    def has_identifier(self, typ):
        identifiers = object.__getattribute__(self, '_data')['identifiers']
        return typ in identifiers

    # field-oriented interface. Intended to be the same as in LibraryDatabase

    def standard_field_keys(self):
        """The standard fields that have been set."""
        return STANDARD_METADATA_FIELDS

    def custom_field_keys(self):
        """The custom fields set on this object."""
        _data = object.__getattribute__(self, '_data')
        return iter(_data['user_metadata'])

    def all_field_keys(self):
        """Every field key: standard and custom."""
        _data = object.__getattribute__(self, '_data')
        return frozenset(ALL_METADATA_FIELDS.union(frozenset(_data['user_metadata'])))

    def metadata_for_field(self, key):
        """The field description (as in field_metadata) for `key`."""
        if key not in self.custom_field_keys():
            return self.get_standard_metadata(key, make_copy=False)
        return self.get_user_metadata(key, make_copy=False)

    def all_non_none_fields(self):
        """A dict of every field that isn't None."""
        result = {}
        _data = object.__getattribute__(self, '_data')
        for attr in STANDARD_METADATA_FIELDS:
            v = _data.get(attr, None)
            if v is not None:
                result[attr] = v
        # separate these because it uses the self.get(), not _data.get()
        for attr in TOP_LEVEL_IDENTIFIERS:
            v = self.get(attr, None)
            if v is not None:
                result[attr] = v
        for attr in _data['user_metadata']:
            v = self.get(attr, None)
            if v is not None:
                result[attr] = v
                if _data['user_metadata'][attr]['datatype'] == 'series':
                    result[attr + '_index'] = _data['user_metadata'][attr]['#extra#']
        return result

    # End of field-oriented interface

    # Extended interfaces. These permit one to get copies of metadata dictionaries, and to
    # get and set custom field metadata

    def get_standard_metadata(self, field, make_copy):
        """The description of a standard field (as in field_metadata)."""
        if field in field_metadata and field_metadata[field]['kind'] == 'field':
            if make_copy:
                return copy.deepcopy(field_metadata[field])
            return field_metadata[field]
        return None

    def get_all_user_metadata(self, make_copy):
        """The custom fields' descriptions (with values), by key."""
        _data = object.__getattribute__(self, '_data')
        user_metadata = _data['user_metadata']
        if not make_copy:
            return user_metadata
        res = {}
        for k in user_metadata:
            res[k] = copy.deepcopy(user_metadata[k])
        return res

    def get_user_metadata(self, field, make_copy):
        """One custom field's description (with its value), or None."""
        _data = object.__getattribute__(self, '_data')
        _data = _data['user_metadata']
        if field in _data:
            if make_copy:
                return copy.deepcopy(_data[field])
            return _data[field]
        return None

    def set_all_user_metadata(self, metadata):
        """Replace the custom fields with `metadata` (key → description)."""
        if metadata is None:
            traceback.print_stack()
            return

        um = {}
        for key, meta in metadata.items():
            m = meta.copy()
            if '#value#' not in m:
                if m['datatype'] == 'composite':
                    m['#value#'] = 'RECURSIVE_COMPOSITE FIELD (Metadata) ' + key
                else:
                    m['#value#'] = None
            um[key] = m
        _data = object.__getattribute__(self, '_data')
        _data['user_metadata'] = um

    def set_user_metadata(self, field, metadata):
        """Add or replace one custom field's description (with `#value#`
        and `#extra#`)."""
        if field is not None:
            if not field.startswith('#'):
                raise AttributeError("Custom field name %s must begin with '#'" % repr(field))
            if metadata is None:
                traceback.print_stack()
                return
            m = dict(metadata)
            # Copying the elements should not be necessary. The objects referenced
            # in the dict should not change. Of course, they can be replaced.
            # for k,v in metadata.iteritems():
            #     m[k] = copy.copy(v)
            if '#value#' not in m:
                if m['datatype'] == 'composite':
                    m['#value#'] = 'RECURSIVE_COMPOSITE FIELD (Metadata) ' + field
                else:
                    m['#value#'] = None
            if '#extra#' not in m:
                m['#extra#'] = None
            _data = object.__getattribute__(self, '_data')
            _data['user_metadata'][field] = m

    def remove_stale_user_metadata(self, other_mi):
        """Remove custom fields that `other_mi` doesn't have."""
        _data = object.__getattribute__(self, '_data')
        um = _data['user_metadata']
        other_keys = set(other_mi.custom_field_keys())
        new = {}
        for k, v in um.items():
            if k in other_keys:
                new[k] = v
        _data['user_metadata'] = new

    def template_to_attribute(self, other, ops):
        """Copy fields from `other` by (source, dest) pairs."""
        if not ops:
            return
        from calibre.ebooks.metadata.book.formatter import SafeFormat

        formatter = SafeFormat()
        for op in ops:
            try:
                src = op[0]
                dest = op[1]
                val = formatter.safe_format(src, other, 'PLUGBOARD TEMPLATE ERROR', other)
                if dest == 'tags':
                    self.set(dest, [f.strip() for f in val.split(',') if f.strip()])
                elif dest == 'authors':
                    self.set(dest, [f.strip() for f in val.split('&') if f.strip()])
                else:
                    self.set(dest, val)
            except Exception:
                if getattr(self, 'formatter', None) is not None:
                    traceback.print_exc()

    # Old Metadata API {{{
    def print_all_attributes(self):
        for x in STANDARD_METADATA_FIELDS:
            prints('%s:' % x, getattr(self, x, 'None'))
        for x in self.custom_field_keys():
            meta = self.get_user_metadata(x, make_copy=False)
            if meta is not None:
                prints(x, meta)
        prints('--------------')

    def smart_update(self, other, replace_metadata=False):
        """Merge `other`'s metadata into this: non-null values of `other`
        win (title and authors only when not Unknown), tags are the union,
        identifiers merge; `replace_metadata` replaces fields outright."""

        def copy_not_none(dest, src, attr):
            v = getattr(src, attr, None)
            if v not in (None, NULL_VALUES.get(attr, None)):
                setattr(dest, attr, copy.deepcopy(v))

        unknown = _unknown()
        if other.title and other.title != unknown:
            self.title = other.title
            if hasattr(other, 'title_sort'):
                self.title_sort = other.title_sort

        if other.authors and (other.authors[0] != unknown or replace_metadata):
            self.authors = list(other.authors)
            if hasattr(other, 'author_sort_map'):
                self.author_sort_map = dict(other.author_sort_map)
            if hasattr(other, 'author_sort'):
                self.author_sort = other.author_sort

        if replace_metadata:
            # SPECIAL_FIELDS = frozenset(['lpath', 'size', 'comments', 'thumbnail'])
            for attr in SC_COPYABLE_FIELDS:
                setattr(self, attr, getattr(other, attr, 1.0 if attr == 'series_index' else None))
            self.tags = other.tags
            self.cover_data = getattr(other, 'cover_data', NULL_VALUES['cover_data'])
            self.set_all_user_metadata(other.get_all_user_metadata(make_copy=True))
            for x in SC_FIELDS_COPY_NOT_NULL:
                copy_not_none(self, other, x)
            if callable(getattr(other, 'get_identifiers', None)):
                self.set_identifiers(other.get_identifiers())
            # language is handled below
        else:
            for attr in SC_COPYABLE_FIELDS:
                copy_not_none(self, other, attr)
            for x in SC_FIELDS_COPY_NOT_NULL:
                copy_not_none(self, other, x)

            if other.tags:
                # Case-insensitive union, keeping this object's order.
                self.tags = list(self.tags or [])
                seen = {t.lower() for t in self.tags}
                for t in other.tags:
                    if t.lower() not in seen:
                        self.tags.append(t)
                        seen.add(t.lower())

            if getattr(other, 'cover_data', False):
                other_cover = other.cover_data[-1]
                self_cover = self.cover_data[-1] if self.cover_data else b''
                if not self_cover:
                    self_cover = b''
                if not other_cover:
                    other_cover = b''
                if len(other_cover) > len(self_cover):
                    self.cover_data = other.cover_data

            if callable(getattr(other, 'custom_field_keys', None)):
                for x in other.custom_field_keys():
                    meta = other.get_user_metadata(x, make_copy=True)
                    if meta is not None:
                        self_tags = self.get(x, [])
                        if isinstance(self_tags, str):
                            self_tags = [self_tags]
                        self.set_user_metadata(x, meta)  # get... did the deepcopy
                        other_tags = other.get(x, [])
                        if meta['datatype'] == 'text' and meta['is_multiple']:
                            if isinstance(other_tags, str):
                                other_tags = [other_tags]
                            if other_tags:
                                self.set(x, list(set(self_tags or []).union(other_tags)))

            if callable(getattr(other, 'get_identifiers', None)):
                identifiers = self.get_identifiers()
                identifiers.update(other.get_identifiers())
                self.set_identifiers(identifiers)

        # language is handled specially: 'und' means unknown
        if other.languages:
            langs = [x for x in other.languages if x and x != 'und']
            if langs or replace_metadata:
                self.languages = langs

        for attr in ('comments', 'publisher'):
            v = getattr(other, attr, None)
            if v and not self.get(attr, None):
                setattr(self, attr, v)

        for x in ('author_sort_map', 'user_categories', 'link_maps', 'author_link_map'):
            v = getattr(other, x, None)
            if v:
                setattr(self, x, dict(v))

    def format_series_index(self, val=None):
        from calibre.ebooks.metadata import fmt_sidx

        v = self.series_index if val is None else val
        try:
            x = float(v)
        except Exception:
            x = 1
        return fmt_sidx(x)

    def authors_from_string(self, raw):
        self.authors = string_to_authors(raw)

    def format_authors(self):
        from calibre.ebooks.metadata import authors_to_string

        return authors_to_string(self.authors)

    def format_tags(self):
        return ', '.join([str(t) for t in sorted(self.tags, key=sort_key)])

    def format_rating(self, v=None, divide_by=1.0):
        if v is None:
            if self.rating is not None:
                return str(self.rating / divide_by)
            return 'None'
        return str(v / divide_by)

    def format_field(self, key, series_with_index=True):
        """(name, value as a string) for any field, custom ones included."""
        name, val, ign, ign = self.format_field_extended(key, series_with_index)
        return name, val

    def format_field_extended(self, key, series_with_index=True):
        """(name, value as a string, the raw value, the field's description)."""
        from calibre.ebooks.metadata import authors_to_string
        from calibre.utils.date import format_date

        # Handle custom series index
        if key.startswith('#') and key.endswith('_index'):
            tkey = key[:-6]  # strip the _index
            cmeta = self.get_user_metadata(tkey, make_copy=False)
            if cmeta and cmeta['datatype'] == 'series':
                if self.get(tkey):
                    res = self.get_extra(tkey)
                    return str(cmeta['name'] + '_index'), self.format_series_index(res), res, cmeta
                return str(cmeta['name'] + '_index'), '', '', cmeta

        if key in self.custom_field_keys():
            res = self.get(key, None)
            cmeta = self.get_user_metadata(key, make_copy=False)
            name = str(cmeta['name'])
            if res is None or res == '':  # can't test for False because of numeric fields
                return name, res, None, None
            orig_res = res
            datatype = cmeta['datatype']
            if datatype == 'text' and cmeta['is_multiple']:
                res = cmeta['is_multiple']['list_to_ui'].join(res)
            elif datatype == 'series' and series_with_index:
                if self.get_extra(key) is not None:
                    res = res + ' [%s]' % self.format_series_index(val=self.get_extra(key))
            elif datatype == 'datetime':
                res = format_date(res, cmeta['display'].get('date_format', 'dd MMM yyyy'))
            elif datatype == 'bool':
                res = _('Yes') if res else _('No')
            elif datatype == 'rating':
                res = '%.2g' % (res / 2)
            elif datatype in ['int', 'float']:
                try:
                    fmt = cmeta['display'].get('number_format', None)
                    res = fmt.format(res)
                except Exception:
                    pass
            return name, str(res), orig_res, cmeta

        # convert top-level ids to their format
        if key in TOP_LEVEL_IDENTIFIERS:
            fmeta = field_metadata['identifiers']
            res = self.get(key, None)
            return key, res, res, fmeta

        if key in field_metadata and field_metadata[key]['kind'] == 'field':
            res = self.get(key, None)
            fmeta = field_metadata[key]
            name = str(fmeta['name'])
            if res is None or res == '':
                return name, res, None, None
            orig_res = res
            name = str(fmeta['name'])
            datatype = fmeta['datatype']
            if key == 'authors':
                res = authors_to_string(res)
            elif key == 'series_index':
                res = self.format_series_index(res)
            elif datatype == 'text' and fmeta['is_multiple']:
                if isinstance(res, dict):
                    res = [k + ':' + v for k, v in res.items()]
                res = fmeta['is_multiple']['list_to_ui'].join(sorted(filter(None, res), key=sort_key))
            elif datatype == 'series' and series_with_index:
                res = res + ' [%s]' % self.format_series_index()
            elif datatype == 'datetime':
                res = format_date(res, fmeta['display'].get('date_format', 'dd MMM yyyy'))
            elif datatype == 'rating':
                res = '%.2g' % (res / 2)
            elif key == 'size':
                res = human_readable(res)
            return name, str(res), orig_res, fmeta

        return None, None, None, None

    def __str__(self):
        """A readable listing of the set fields."""
        from calibre.ebooks.metadata import authors_to_string

        ans = []

        def fmt(x, y):
            ans.append('%-20s: %s' % (str(x), str(y)))

        fmt('Title', self.title)
        if self.title_sort:
            fmt('Title sort', self.title_sort)
        if self.authors:
            fmt(
                'Author(s)',
                authors_to_string(self.authors)
                + ((' [' + self.author_sort + ']') if self.author_sort and self.author_sort != _unknown() else ''),
            )
        if self.publisher:
            fmt('Publisher', self.publisher)
        if getattr(self, 'book_producer', False):
            fmt('Book Producer', self.book_producer)
        if self.tags:
            fmt('Tags', ', '.join([str(t) for t in self.tags]))
        if self.series:
            fmt('Series', self.series + ' #%s' % self.format_series_index())
        if not self.is_null('languages'):
            fmt('Languages', ', '.join(self.languages))
        if self.rating is not None:
            fmt('Rating', ('%.2g' % (float(self.rating) / 2)) if self.rating else '')
        if self.timestamp is not None:
            fmt('Timestamp', isoformat(self.timestamp))
        if self.pubdate is not None:
            fmt('Published', isoformat(self.pubdate))
        if self.rights is not None:
            fmt('Rights', str(self.rights))
        if self.identifiers:
            fmt('Identifiers', ', '.join(['%s:%s' % (k, v) for k, v in self.identifiers.items()]))
        if self.comments:
            fmt('Comments', self.comments)

        for key in self.custom_field_keys():
            val = self.get(key, None)
            if val:
                (name, val) = self.format_field(key)
                fmt(name, str(val))
        return '\n'.join(ans)

    def to_html(self):
        """The set fields as an HTML table."""
        from calibre.ebooks.metadata import authors_to_string

        ans = [(_('Title'), str(self.title))]
        ans += [(_('Author(s)'), (authors_to_string(self.authors) if self.authors else _('Unknown')))]
        ans += [(_('Publisher'), str(self.publisher))]
        ans += [(_('Producer'), str(self.book_producer))]
        ans += [(_('Comments'), str(self.comments))]
        ans += [('ISBN', str(self.isbn))]
        ans += [(_('Tags'), ', '.join([str(t) for t in self.tags]))]
        if self.series:
            ans += [(ngettext('Series', 'Series', 1), str(self.series) + ' #%s' % self.format_series_index())]
        ans += [(_('Languages'), ', '.join(self.languages))]
        if self.timestamp is not None:
            ans += [(_('Timestamp'), str(isoformat(self.timestamp, as_utc=False, sep=' ')))]
        if self.pubdate is not None:
            ans += [(_('Published'), str(isoformat(self.pubdate, as_utc=False, sep=' ')))]
        if self.rights is not None:
            ans += [(_('Rights'), str(self.rights))]
        for key in self.custom_field_keys():
            val = self.get(key, None)
            if val:
                (name, val) = self.format_field(key)
                ans += [(name, val)]
        for i, x in enumerate(ans):
            ans[i] = '<tr><td><b>%s</b></td><td>%s</td></tr>' % x
        return '<table>%s</table>' % '\n'.join(ans)

    def __bool__(self):
        return not (self.title == _unknown() and self.authors == [_unknown()] and self.publisher is None)

    def __repr__(self):
        return f'<Metadata {self.title!r} by {self.authors!r}>'

    # }}}


def field_from_string(field, raw, field_metadata):
    """Parse a string into the value for `field`, by its datatype."""
    dt = field_metadata['datatype']
    val = object
    if dt in {'int', 'float'}:
        val = int(raw) if dt == 'int' else float(raw)
    elif dt == 'rating':
        val = float(raw) * 2
    elif dt == 'datetime':
        from calibre.utils.iso8601 import parse_iso8601

        try:
            val = parse_iso8601(raw, require_aware=True)
        except Exception:
            from calibre.utils.date import parse_only_date

            val = parse_only_date(raw)
    elif dt == 'bool':
        if raw.lower() in {'true', 'yes', 'y'}:
            val = True
        elif raw.lower() in {'false', 'no', 'n'}:
            val = False
        else:
            raise ValueError('Unknown value for %s: %s' % (field, raw))
    elif dt == 'text':
        ism = field_metadata['is_multiple']
        if ism:
            val = [x.strip() for x in raw.split(ism['ui_to_list'])]
            if field == 'identifiers':
                val = {x.partition(':')[0]: x.partition(':')[-1] for x in val}
            elif field == 'languages':
                from calibre.utils.localization import canonicalize_lang

                val = [canonicalize_lang(x) for x in val]
                val = [x for x in val if x]
    if val is object:
        val = raw
    return val


from calibre.ebooks.metadata.book import ALL_METADATA_FIELDS  # noqa: E402

__all__ = [
    'Metadata',
    'NULL_VALUES',
    'SIMPLE_GET',
    'SIMPLE_SET',
    'field_metadata',
    'field_from_string',
    'reset_field_metadata',
    'human_readable',
    'author_to_author_sort',
    'title_sort',
    'suppress',
]
