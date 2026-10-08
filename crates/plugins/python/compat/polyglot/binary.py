"""Mirrors calibre's `polyglot.binary`: base64 and hex helpers that take and
return the natural types."""

from base64 import b64decode, b64encode, standard_b64decode, standard_b64encode
from binascii import hexlify, unhexlify


def as_base64_bytes(x, enc='utf-8'):
    if isinstance(x, str):
        x = x.encode(enc)
    return standard_b64encode(x)


def as_base64_unicode(x, enc='utf-8'):
    return as_base64_bytes(x, enc).decode('ascii')


def from_base64_bytes(x):
    if isinstance(x, str):
        x = x.encode('ascii')
    return standard_b64decode(x)


def from_base64_unicode(x, enc='utf-8'):
    return from_base64_bytes(x).decode(enc)


def as_hex_bytes(x, enc='utf-8'):
    if isinstance(x, str):
        x = x.encode(enc)
    return hexlify(x)


def as_hex_unicode(x, enc='utf-8'):
    return as_hex_bytes(x, enc).decode('ascii')


def from_hex_bytes(x):
    if isinstance(x, str):
        x = x.encode('ascii')
    return unhexlify(x)


def from_hex_unicode(x, enc='utf-8'):
    return from_hex_bytes(x).decode(enc)


__all__ = [
    'as_base64_bytes',
    'as_base64_unicode',
    'from_base64_bytes',
    'from_base64_unicode',
    'as_hex_bytes',
    'as_hex_unicode',
    'from_hex_bytes',
    'from_hex_unicode',
    'b64decode',
    'b64encode',
]
