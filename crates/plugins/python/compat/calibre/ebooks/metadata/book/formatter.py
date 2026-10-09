# Derived from calibre's src/calibre/ebooks/metadata/book/formatter.py (calibre 9.15).
# Copyright: 2013, Kovid Goyal <kovid at kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.ebooks.metadata.book.formatter`: the
template formatter `Metadata` uses for composite columns."""

from calibre.utils.formatter import TemplateFormatter


class SafeFormat(TemplateFormatter):
    """Formats templates against a Metadata, never raising: errors come back
    as the error string given."""

    def __init__(self):
        TemplateFormatter.__init__(self)
        from calibre.ebooks.metadata.book.base import field_metadata

        self.field_metadata = field_metadata

    def get_value(self, orig_key, args, kwargs):
        if not orig_key:
            return ''
        key = orig_key = orig_key.lower()
        if key != 'title_sort' and key not in self.field_metadata.custom_field_keys(include_composites=True):
            key = self.field_metadata.search_term_to_field_key(key)
            if key is None or key not in self.book.all_field_keys():
                if hasattr(self.book, orig_key):
                    key = orig_key
                else:
                    raise ValueError('Value: unknown field ' + orig_key)
        try:
            b = self.book.get_user_metadata(key, False)
        except Exception:
            b = None
        if b is not None and b['datatype'] == 'composite':
            if key in self.composite_values:
                return self.composite_values[key]
            self.composite_values[key] = 'RECURSIVE_COMPOSITE FIELD (S2D) ' + key
            self.composite_values[key] = self.evaluate(b['display']['composite_template'], [], kwargs, self.global_vars)
            return self.composite_values[key]
        if isinstance(kwargs, dict) and key in kwargs:
            ans = kwargs[key]
            if isinstance(ans, list):
                ans = ', '.join(ans)
            return ans
        if key in self.book.all_field_keys():
            ans = self.book.get(key, None)
            if ans is None:
                return ''
            if isinstance(ans, (list, tuple)):
                return ', '.join(str(x) for x in ans)
            return str(ans)
        raise ValueError('Value: unknown field ' + key)
