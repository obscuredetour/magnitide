"""calibre_extensions.speedup: small helpers calibre implements in C."""

import re
from datetime import UTC, datetime, timedelta, timezone

_ISO = re.compile(
    r'^(?P<year>\d{4})-?(?P<month>\d{2})?-?(?P<day>\d{2})?'
    r'(?:[T ](?P<hour>\d{2}):?(?P<minute>\d{2})?:?(?P<second>\d{2})?(?:[.,](?P<fraction>\d+))?'
    r'(?P<tz>Z|[+-]\d{2}:?\d{2})?)?$'
)


def parse_iso8601(raw, assume_utc=False, as_utc=True, require_aware=False):
    """(datetime, aware: bool, tzseconds) as the extension returns."""
    m = _ISO.match(raw.strip())
    if m is None:
        raise ValueError(f'Not an ISO 8601 date: {raw!r}')
    g = m.groupdict()
    tz = g.get('tz')
    offset = 0
    aware = tz is not None
    if tz and tz != 'Z':
        sign = 1 if tz[0] == '+' else -1
        hh, mm = tz[1:3], tz[-2:]
        offset = sign * (int(hh) * 3600 + int(mm) * 60)
    micro = int((g.get('fraction') or '0')[:6].ljust(6, '0'))
    dt = datetime(
        int(g['year']),
        int(g.get('month') or 1),
        int(g.get('day') or 1),
        int(g.get('hour') or 0),
        int(g.get('minute') or 0),
        int(g.get('second') or 0),
        micro,
    )
    if aware or assume_utc:
        dt = dt.replace(tzinfo=timezone(timedelta(seconds=offset)))
        if as_utc:
            dt = dt.astimezone(UTC)
    elif require_aware:
        raise ValueError('Date has no timezone')
    return dt, aware, offset


def parse_date(raw):
    return parse_iso8601(raw)[0]


def clean_xml_chars(s):
    return ''.join(c for c in s if c in '\t\n\r' or (' ' <= c <= '퟿') or ('' <= c <= '�') or c > '￿')


def barename(name):
    return name.split('}', 1)[1] if '}' in name else name


def namespace(name):
    return name.split('}', 1)[0][1:] if '}' in name else ''


def pdf_float(f):
    s = f'{f:.6f}'.rstrip('0').rstrip('.')
    return s or '0'


def get_num_of_significant_chars(elem):
    return len(''.join(elem.itertext()).strip())


def pread_all(fd, size, offset):
    import os

    return os.pread(fd, size, offset)


def websocket_mask(data, mask):
    out = bytearray(data)
    for i in range(len(out)):
        out[i] ^= mask[i % 4]
    return bytes(out)


def utf8_decode(data):
    return data.decode('utf-8')


def set_thread_name(name):
    pass


def create_texture(width, height, red, green, blue, blend_alpha=0.1, density=0.7, weight=3.0, radius=1, dither=True):
    raise ImportError("speedup.create_texture needs calibre's C extension")


__all__ = [
    'parse_iso8601',
    'parse_date',
    'clean_xml_chars',
    'barename',
    'namespace',
    'pdf_float',
    'get_num_of_significant_chars',
    'pread_all',
    'websocket_mask',
    'utf8_decode',
    'set_thread_name',
]
