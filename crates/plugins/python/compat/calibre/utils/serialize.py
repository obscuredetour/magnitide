# Derived from calibre's src/calibre/utils/serialize.py (calibre 9.15).
# Copyright: 2017, Kovid Goyal <kovid at kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.utils.serialize`: JSON and msgpack with
calibre's encoders for dates, bytes and sets."""

import json

MSGPACK_MIME = 'application/x-msgpack'
CANARY = 'jPoAv3zOyHvQ5JFNYg4hJ9kJ7hPvh9k'


def encoded(typ, data, for_json):
    if for_json:
        return {CANARY: typ, 'v': data}
    return (typ, data)


def encoder(obj, for_json=False):
    import datetime

    if isinstance(obj, datetime.datetime):
        from calibre.utils.date import isoformat

        return encoded(0, isoformat(obj, as_utc=True), for_json)
    if isinstance(obj, (set, frozenset)):
        return encoded(1, tuple(obj), for_json)
    if hasattr(obj, 'toBase64'):  # QByteArray
        return encoded(2, bytes(obj.toBase64()).decode('ascii'), for_json)
    if getattr(obj, '__calibre_serializable__', False):
        from calibre.ebooks.metadata.book.base import Metadata
        from calibre.library.field_metadata import FieldMetadata, fm_as_dict

        if isinstance(obj, Metadata):
            from calibre.ebooks.metadata.book.serialize import metadata_as_dict

            return encoded(3, metadata_as_dict(obj), for_json)
        if isinstance(obj, FieldMetadata):
            return encoded(4, fm_as_dict(obj), for_json)
    if isinstance(obj, bytes):
        from polyglot.binary import as_base64_unicode

        return encoded(5, as_base64_unicode(obj), for_json)
    raise TypeError(f'Cannot serialize objects of type {type(obj)}')


def msgpack_dumps(obj):
    import msgpack

    return msgpack.packb(obj, default=encoder, use_bin_type=True)


def json_dumps(data, **kw):
    kw['default'] = lambda obj: encoder(obj, for_json=True)
    kw['ensure_ascii'] = False
    ans = json.dumps(data, **kw)
    if not isinstance(ans, bytes):
        ans = ans.encode('utf-8')
    return ans


def decode_metadata(x, for_json):
    from calibre.ebooks.metadata.book.serialize import metadata_from_dict

    obj = metadata_from_dict(x)
    if for_json and obj.cover_data and obj.cover_data[1]:
        from polyglot.binary import from_base64_bytes

        obj.cover_data = obj.cover_data[0], from_base64_bytes(obj.cover_data[1])
    return obj


def decode_field_metadata(x, for_json):
    from calibre.library.field_metadata import fm_from_dict

    return fm_from_dict(x)


def decode_datetime(x, fj):
    from calibre.utils.iso8601 import parse_iso8601

    return parse_iso8601(x, assume_utc=True)


def decode_bytes(x, fj):
    from polyglot.binary import from_base64_bytes

    return from_base64_bytes(x)


decoders = (
    decode_datetime,
    lambda x, fj: set(x),
    lambda x, fj: x,
    decode_metadata,
    decode_field_metadata,
    decode_bytes,
)


def json_decoder(obj):
    typ = obj.get(CANARY)
    if typ is None:
        return obj
    return decoders[typ](obj['v'], True)


def msgpack_decoder(code, data):
    return decoders[code](msgpack_loads(data, use_list=True), False)


def msgpack_loads(data, use_list=True):
    import msgpack

    return msgpack.unpackb(data, ext_hook=msgpack_decoder, raw=False, use_list=use_list, strict_map_key=False)


def json_loads(data):
    return json.loads(data, object_hook=json_decoder)


def pickle_dumps(data):
    import pickle

    return pickle.dumps(data, -1)


def pickle_loads(dump):
    import pickle

    return pickle.loads(dump, encoding='utf-8')
