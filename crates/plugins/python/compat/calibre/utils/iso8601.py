"""Mirrors calibre's `calibre.utils.iso8601`: parsing ISO 8601 date-times."""

import re
from datetime import UTC, datetime, timedelta, tzinfo

utc_tz = UTC

try:
    from dateutil.tz import tzlocal as _tzlocal

    local_tz = _tzlocal()
except Exception:  # pragma: no cover
    local_tz = datetime.now().astimezone().tzinfo

UNDEFINED_DATE = datetime(101, 1, 1, tzinfo=utc_tz)

_ISO = re.compile(
    r'^\s*(?P<year>\d{4})(?:-?(?P<month>\d{2})(?:-?(?P<day>\d{2}))?)?'
    r'(?:[T ](?P<hour>\d{2})(?::?(?P<minute>\d{2})(?::?(?P<second>\d{2})(?:[.,](?P<fraction>\d+))?)?)?'
    r'\s*(?P<tz>Z|[+-]\d{2}(?::?\d{2})?)?)?\s*$',
    re.IGNORECASE,
)


class _Fixed(tzinfo):
    def __init__(self, minutes):
        self.offset = timedelta(minutes=minutes)

    def utcoffset(self, dt):
        return self.offset

    def dst(self, dt):
        return timedelta(0)

    def tzname(self, dt):
        return 'UTC%+03d:%02d' % divmod(int(self.offset.total_seconds()) // 60, 60)


def _parse(date_string):
    m = _ISO.match(date_string)
    if m is None:
        raise ValueError(f'Not an ISO 8601 date: {date_string!r}')
    g = m.groupdict()
    year = int(g['year'])
    month = int(g['month'] or 1)
    day = int(g['day'] or 1)
    hour = int(g['hour'] or 0)
    minute = int(g['minute'] or 0)
    second = int(g['second'] or 0)
    micro = 0
    if g['fraction']:
        micro = int((g['fraction'] + '000000')[:6])
    tz = g['tz']
    aware = tz is not None
    tzsec = 0
    if tz and tz.upper() != 'Z':
        sign = -1 if tz[0] == '-' else 1
        digits = tz[1:].replace(':', '')
        hh, mm = int(digits[:2]), int(digits[2:4] or 0)
        tzsec = sign * (hh * 3600 + mm * 60)
    if hour == 24 and minute == 0 and second == 0:
        hour = 0
        dt = datetime(year, month, day, hour, minute, second, micro) + timedelta(days=1)
    else:
        dt = datetime(year, month, day, hour, minute, second, micro)
    return dt, aware, tzsec


def parse_iso8601(date_string, assume_utc=False, as_utc=True, require_aware=False):
    """Parse an ISO 8601 string. A naive one is taken as UTC if `assume_utc`,
    else local; the result is in UTC if `as_utc`, else local time."""
    if not date_string:
        return UNDEFINED_DATE
    dt, aware, tzseconds = _parse(date_string)
    tz = utc_tz if assume_utc else local_tz
    if aware:
        tz = utc_tz if tzseconds == 0 else _Fixed(tzseconds // 60)
    elif require_aware:
        raise ValueError(f'{date_string} does not specify a time zone')
    dt = dt.replace(tzinfo=tz)
    return dt.astimezone(utc_tz if as_utc else local_tz)


if __name__ == '__main__':
    import sys

    print(parse_iso8601(sys.argv[-1]))
