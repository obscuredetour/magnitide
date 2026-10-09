# Derived from calibre's src/calibre/ebooks/metadata/book/json_codec.py (calibre 9.15).
# Copyright: 2010, Charles Haley. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.ebooks.metadata.book.json_codec`: Metadata
objects to and from JSON (used for device metadata caches)."""

import json
import traceback

from calibre.constants import filesystem_encoding, preferred_encoding
from calibre.ebooks.metadata.book import SERIALIZABLE_FIELDS
from calibre.utils.date import datetime_to_string, string_to_datetime  # noqa: F401
from polyglot.binary import as_base64_bytes, from_base64_bytes


def string_to_authors(string):
    from calibre.ebooks.metadata import string_to_authors as s2a

    return s2a(string)


def encode_thumbnail(thumbnail):
    """thumbnail → (width, height, base64 bytes)"""
    if thumbnail is None:
        return None
    if not isinstance(thumbnail, (tuple, list)):
        try:
            from calibre.utils.imghdr import identify

            fmt, width, height = identify(thumbnail)
            if width < 0 or height < 0:
                return None
            thumbnail = (width, height, thumbnail)
        except Exception:
            return None
    return (thumbnail[0], thumbnail[1], as_base64_bytes(thumbnail[2]))


def decode_thumbnail(tup):
    if tup is None:
        return None
    return (tup[0], tup[1], from_base64_bytes(tup[2]))


def object_to_unicode(obj, enc=preferred_encoding):

    def dec(x):
        return x.decode(enc, 'replace')

    if isinstance(obj, bytes):
        return dec(obj)
    if isinstance(obj, (list, tuple)):
        return [dec(x) if isinstance(x, bytes) else object_to_unicode(x) for x in obj]
    if isinstance(obj, dict):
        ans = {}
        for k, v in obj.items():
            k = object_to_unicode(k)
            v = object_to_unicode(v)
            ans[k] = v
        return ans
    return obj


def encode_is_multiple(fm):
    if fm.get('is_multiple', None):
        # migrate is_multiple back to a character
        fm['is_multiple2'] = fm.get('is_multiple', {})
        dt = fm.get('datatype', None)
        if dt == 'composite':
            fm['is_multiple'] = ','
        else:
            fm['is_multiple'] = '|'
    else:
        fm['is_multiple'] = None
        fm['is_multiple2'] = {}


def decode_is_multiple(fm):
    im = fm.get('is_multiple2', None)
    if im:
        fm['is_multiple'] = im
        del fm['is_multiple2']
    else:
        # Must migrate the is_multiple from char to dict
        im = fm.get('is_multiple', {})
        if im:
            dt = fm.get('datatype', None)
            if dt == 'composite':
                im = {'cache_to_list': ',', 'ui_to_list': ',', 'list_to_ui': ', '}
            else:
                im = {'cache_to_list': '|', 'ui_to_list': ',', 'list_to_ui': ', '}
        elif im is None:
            im = {}
        fm['is_multiple'] = im


class JsonCodec:
    def __init__(self, field_metadata=None):
        from calibre.ebooks.metadata.book.base import field_metadata as fm

        self.field_metadata = field_metadata or fm

    def encode_to_file(self, file_, booklist):
        data = json.dumps(self.encode_booklist_metadata(booklist), indent=2)
        if not isinstance(data, bytes):
            data = data.encode('utf-8')
        file_.write(data)

    def encode_booklist_metadata(self, booklist):
        result = []
        for book in booklist:
            result.append(self.encode_book_metadata(book))
        return result

    def encode_book_metadata(self, book):
        result = {}
        for key in SERIALIZABLE_FIELDS:
            result[key] = self.encode_metadata_attr(book, key)
        return result

    def encode_metadata_attr(self, book, key):
        if key == 'user_metadata':
            meta = book.get_all_user_metadata(make_copy=True)
            for fm in meta.values():
                if fm['datatype'] == 'datetime':
                    fm['#value#'] = datetime_to_string(fm['#value#'])
                encode_is_multiple(fm)
            return meta
        if key in self.field_metadata:
            datatype = self.field_metadata[key]['datatype']
        else:
            datatype = None
        value = book.get(key)
        if key == 'thumbnail':
            return encode_thumbnail(value)
        if isinstance(value, bytes):  # str includes bytes
            enc = filesystem_encoding if key == 'lpath' else preferred_encoding
            return object_to_unicode(value, enc=enc)
        if datatype == 'datetime':
            return datetime_to_string(value)
        return object_to_unicode(value)

    def decode_from_file(self, file_, booklist, book_class, prefix):
        js = []
        try:
            js = json.load(file_)
            for item in js:
                entry = self.raw_to_book(item, book_class, prefix)
                if entry is not None:
                    booklist.append(entry)
        except Exception:
            print('exception during JSON decode_from_file')
            traceback.print_exc()

    def raw_to_book(self, json_book, book_class, prefix):
        try:
            book = book_class(prefix, json_book.get('lpath', None))
            for key, val in json_book.items():
                meta = self.decode_metadata(key, val)
                if key == 'user_metadata':
                    book.set_all_user_metadata(meta)
                else:
                    if key == 'classifiers':
                        key = 'identifiers'
                    setattr(book, key, meta)
            return book
        except Exception:
            print('exception during JSON decoding')
            traceback.print_exc()

    def decode_metadata(self, key, value):
        if key == 'classifiers':
            key = 'identifiers'
        if key == 'user_metadata':
            for fm in value.values():
                if fm['datatype'] == 'datetime':
                    fm['#value#'] = string_to_datetime(fm['#value#'])
                decode_is_multiple(fm)
            return value
        if key in self.field_metadata:
            datatype = self.field_metadata[key]['datatype']
        else:
            datatype = None
        if key == 'thumbnail':
            return decode_thumbnail(value)
        if datatype == 'datetime':
            return string_to_datetime(value)
        return value
