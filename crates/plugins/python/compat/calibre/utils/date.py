# Derived from calibre's src/calibre/utils/date.py (calibre 9.15).
# Copyright: 2010, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.utils.date` (calibre 9.15): parsing and
formatting dates the way calibre does, with calibre's special dates
(`UNDEFINED_DATE`, `DEFAULT_DATE`) and its `format_date` tokens.

Semantics kept from calibre: `parse_date` of a date without a time zone takes
it as local time unless `assume_utc`; missing parts come from a default of
the current year, month 1, day 15 (a month-only date doesn't shift to the
previous month in another time zone); results are UTC unless `as_utc` is
False.
"""

import re
import time
from datetime import date, datetime, timedelta, timezone

from calibre.utils.iso8601 import UNDEFINED_DATE, local_tz, parse_iso8601, utc_tz  # noqa: F401

_utc_tz = utc_tz
_local_tz = local_tz
EPOCH = datetime(1970, 1, 1, tzinfo=utc_tz)
DEFAULT_DATE = datetime(2000, 1, 1, tzinfo=utc_tz)
UNDEFINED_QDATETIME = None  # set by calibre.gui2 when Qt is around


def is_date_undefined(qt_or_dt):
    d = qt_or_dt
    if d is None:
        return True
    if hasattr(d, 'toString'):  # a QDate/QDateTime
        if hasattr(d, 'date'):
            d = d.date()
        try:
            return d.year() <= UNDEFINED_DATE.year or not d.isValid()
        except Exception:
            return True
    if hasattr(d, 'year'):
        return d.year <= UNDEFINED_DATE.year or (d.year == UNDEFINED_DATE.year and d.month == 1 and d.day == 1)
    return False


def parse_date(date_string, assume_utc=False, as_utc=True, default=None):
    """Parse a date or date-time in nearly any form. Returns a time-zone-aware
    datetime, UTC unless `as_utc` is False. An empty string is
    `UNDEFINED_DATE`."""
    from dateutil.parser import parse

    if not date_string:
        return UNDEFINED_DATE
    if isinstance(date_string, bytes):
        date_string = date_string.decode('utf-8', 'replace')
    date_string = date_string.strip()
    if not date_string:
        return UNDEFINED_DATE
    if default is None:
        func = datetime.utcnow if assume_utc else datetime.now
        default = func().replace(
            hour=0, minute=0, second=0, microsecond=0, month=1, day=15, tzinfo=utc_tz if assume_utc else local_tz
        )
    try:
        dt = parse_iso8601(date_string, assume_utc=assume_utc, as_utc=as_utc)
    except Exception:
        try:
            dt = parse(date_string, default=default)
        except Exception:
            # Just a year?
            m = re.match(r'^\s*(\d{4})\s*$', date_string)
            if m is None:
                raise
            dt = default.replace(year=int(m.group(1)))
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=utc_tz if assume_utc else local_tz)
    return dt.astimezone(utc_tz if as_utc else local_tz)


def parse_only_date(raw, assume_utc=True, as_utc=True):
    """Parse a date without a time; the time is noon so it stays on the
    same day in other time zones."""
    f = utcnow if assume_utc else now
    default = f().replace(hour=0, minute=0, second=0, microsecond=0, day=15)
    ans = parse_date(raw, default=default, assume_utc=assume_utc, as_utc=as_utc)
    n = ans.replace(hour=12, minute=0, second=0, microsecond=0)
    return n


def strptime(val, fmt, assume_utc=False, as_utc=True):
    dt = datetime.strptime(val, fmt)
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=utc_tz if assume_utc else local_tz)
    return dt.astimezone(utc_tz if as_utc else local_tz)


def dt_factory(time_t, assume_utc=False, as_utc=True):
    dt = datetime(*(time_t[0:6]))
    dt = dt.replace(tzinfo=utc_tz if assume_utc else local_tz)
    return dt.astimezone(utc_tz if as_utc else local_tz)


def qt_to_dt(qdate_or_qdatetime, as_utc=True):
    o = qdate_or_qdatetime
    if o is None:
        return UNDEFINED_DATE
    if hasattr(o, 'toUTC'):
        o = o.toUTC()
        d, t = o.date(), o.time()
        try:
            ans = datetime(d.year(), d.month(), d.day(), t.hour(), t.minute(), t.second(), t.msec() * 1000, utc_tz)
        except ValueError:
            ans = datetime(
                d.year(), d.month(), max(1, d.day()), t.hour(), t.minute(), t.second(), t.msec() * 1000, utc_tz
            )
        if not as_utc:
            ans = ans.astimezone(local_tz)
        return ans
    dt = datetime(o.year(), o.month(), o.day()).replace(tzinfo=local_tz)
    return dt.astimezone(utc_tz if as_utc else local_tz)


def qt_from_dt(d, as_utc=False, assume_utc=False):
    from qt.core import QDate, QDateTime, Qt, QTime, QTimeZone

    if d.tzinfo is None:
        d = d.replace(tzinfo=utc_tz if assume_utc else local_tz)
    d = d.astimezone(utc_tz if as_utc else local_tz)
    ans = QDateTime(
        QDate(d.year, d.month, d.day),
        QTime(d.hour, d.minute, d.second, int(d.microsecond / 1000)),
        QTimeZone(Qt.TimeSpec.UTC if as_utc else Qt.TimeSpec.LocalTime),
    )
    return ans


def fromtimestamp(ctime, as_utc=True):
    dt = datetime.fromtimestamp(ctime, utc_tz)
    if not as_utc:
        dt = dt.astimezone(local_tz)
    return dt


def fromordinal(day, as_utc=True):
    dt = datetime.fromordinal(day).replace(tzinfo=utc_tz)
    if not as_utc:
        dt = dt.astimezone(local_tz)
    return dt


def isoformat(date_time, assume_utc=False, as_utc=True, sep='T'):
    if not hasattr(date_time, 'tzinfo'):
        return str(date_time.isoformat())
    if date_time.tzinfo is None:
        date_time = date_time.replace(tzinfo=utc_tz if assume_utc else local_tz)
    date_time = date_time.astimezone(utc_tz if as_utc else local_tz)
    # str(sep) because isoformat barfs with unicode sep on python 2.x
    return str(date_time.isoformat(str(sep)))


def internal_iso_format_string():
    return 'yyyy-MM-ddThh:mm:ss'


def w3cdtf(date_time, assume_utc=False):
    if hasattr(date_time, 'tzinfo'):
        if date_time.tzinfo is None:
            date_time = date_time.replace(tzinfo=utc_tz if assume_utc else local_tz)
        date_time = date_time.astimezone(utc_tz)
    return str(date_time.strftime('%Y-%m-%dT%H:%M:%SZ'))


def as_local_time(date_time, assume_utc=True):
    if not hasattr(date_time, 'tzinfo'):
        return date_time
    if date_time.tzinfo is None:
        date_time = date_time.replace(tzinfo=utc_tz if assume_utc else local_tz)
    return date_time.astimezone(local_tz)


def as_utc(date_time, assume_utc=True):
    if not hasattr(date_time, 'tzinfo'):
        return date_time
    if date_time.tzinfo is None:
        date_time = date_time.replace(tzinfo=utc_tz if assume_utc else local_tz)
    return date_time.astimezone(utc_tz)


def now():
    return datetime.now().replace(tzinfo=local_tz)


def utcnow():
    return datetime.now(utc_tz)


def utcfromtimestamp(stamp):
    try:
        return datetime.fromtimestamp(stamp, utc_tz)
    except Exception:
        return utcnow()


def timestampfromdt(dt, assume_utc=True):
    return (as_utc(dt, assume_utc) - EPOCH).total_seconds()


def clean_date_for_sort(dt, fmt=None):
    """Zero out the parts of a date `fmt` doesn't show, so sorting by the
    shown date groups them."""
    if not hasattr(dt, 'tzinfo'):
        return dt
    if fmt is None:
        fmt = 'yyMMdd'
    elif fmt == 'iso':
        fmt = 'yyMMddhhmmss'
    if dt == UNDEFINED_DATE:
        return dt
    dt = as_local_time(dt)
    if 'yy' not in fmt:
        dt = dt.replace(year=UNDEFINED_DATE.year)
    if 'M' not in fmt:
        dt = dt.replace(month=1)
    if 'd' not in fmt:
        dt = dt.replace(day=1)
    if 'h' not in fmt:
        dt = dt.replace(hour=0)
    if 'm' not in fmt:
        dt = dt.replace(minute=0)
    if 's' not in fmt:
        dt = dt.replace(second=0)
    return dt.replace(microsecond=0)


# --- format_date, with calibre's Qt-style tokens --------------------------

_months = [
    'January',
    'February',
    'March',
    'April',
    'May',
    'June',
    'July',
    'August',
    'September',
    'October',
    'November',
    'December',
]
_days = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday']


def _lcdata():
    """Month and day names in the current locale (English if unknown)."""
    try:
        import calendar

        months = [calendar.month_name[i] for i in range(1, 13)]
        short_months = [calendar.month_abbr[i] for i in range(1, 13)]
        days = [calendar.day_name[i] for i in range(7)]
        short_days = [calendar.day_abbr[i] for i in range(7)]
        if all(months) and all(days):
            return months, short_months, days, short_days
    except Exception:
        pass
    return _months, [m[:3] for m in _months], _days, [d[:3] for d in _days]


def fd_format_hour(dt, ampm, hr):
    h = dt.hour
    if ampm:
        h = h % 12
        if h == 0:
            h = 12
    return '%d' % h if len(hr) == 1 else '%02d' % h


def fd_format_minute(dt, ampm, min):
    return '%d' % dt.minute if len(min) == 1 else '%02d' % dt.minute


def fd_format_second(dt, ampm, sec):
    return '%d' % dt.second if len(sec) == 1 else '%02d' % dt.second


def fd_format_ampm(dt, ampm, ap):
    res = 'AM' if dt.hour < 12 else 'PM'
    return res if ap == 'AP' else res.lower()


def fd_format_day(dt, ampm, dy):
    months, short_months, days, short_days = _lcdata()
    if len(dy) == 1:
        return '%d' % dt.day
    if len(dy) == 2:
        return '%02d' % dt.day
    if len(dy) == 3:
        return short_days[dt.weekday()]
    return days[dt.weekday()]


def fd_format_month(dt, ampm, mo):
    months, short_months, days, short_days = _lcdata()
    if len(mo) == 1:
        return '%d' % dt.month
    if len(mo) == 2:
        return '%02d' % dt.month
    if len(mo) == 3:
        return short_months[dt.month - 1]
    return months[dt.month - 1]


def fd_format_year(dt, ampm, yr):
    if len(yr) == 2:
        return '%02d' % (dt.year % 100)
    return '%04d' % dt.year


fd_function_index = {
    'd': fd_format_day,
    'M': fd_format_month,
    'y': fd_format_year,
    'h': fd_format_hour,
    'm': fd_format_minute,
    's': fd_format_second,
    'a': fd_format_ampm,
    'A': fd_format_ampm,
}


def fd_repl_func(dt, ampm, mo):
    s = mo.group(0)
    if not s:
        return ''
    return fd_function_index[s[0]](dt, ampm, s)


def format_date(dt, format, assume_utc=False, as_utc=False):
    """Format a date with Qt-style tokens: d, dd, ddd, dddd, M, MM, MMM, MMMM,
    yy, yyyy, h, hh, m, mm, s, ss, ap/AP, and 'iso'."""
    if not format:
        format = 'dd MMM yyyy'
    if not isinstance(dt, datetime):
        dt = datetime.combine(dt, datetime.min.time())
    if hasattr(dt, 'tzinfo'):
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=utc_tz if assume_utc else local_tz)
        dt = dt.astimezone(utc_tz if as_utc else local_tz)
    if format == 'iso':
        return isoformat(dt, assume_utc=assume_utc, as_utc=as_utc)
    if dt == UNDEFINED_DATE:
        return ''
    repl_func = partial_repl(dt, 'ap' in format.lower())
    return re.sub(r'(s{1,2})|(m{1,2})|(h{1,2})|(ap)|(AP)|(d{1,4}|M{1,4}|(?:yyyy|yy))', repl_func, format)


def partial_repl(dt, ampm):
    from functools import partial

    return partial(fd_repl_func, dt, ampm)


def replace_months(datestr, clang):
    """Replace month names in `datestr` (in language `clang`) with English ones, for parsing."""
    return datestr


def compute_locale_info_for_parse_date():
    pass


def format_date_for_qt(fmt):
    return fmt


def date_to_struct_time(dt):
    return time.struct_time((dt.year, dt.month, dt.day, 0, 0, 0, dt.weekday(), dt.timetuple().tm_yday, -1))


def datetime_to_string(dt):
    """ISO text for JSON (None stays None)."""
    if dt is None:
        return None
    return isoformat(dt, as_utc=True)


def string_to_datetime(src):
    if src is None or src == '':
        return None
    try:
        return parse_date(src, assume_utc=True)
    except Exception:
        return None


def timeit(func):
    import functools

    @functools.wraps(func)
    def wrapper(*args, **kwargs):
        st = time.monotonic()
        try:
            return func(*args, **kwargs)
        finally:
            print(func.__name__, 'took', time.monotonic() - st, 'seconds')

    return wrapper


__all__ = [
    'parse_date',
    'parse_only_date',
    'strptime',
    'dt_factory',
    'qt_to_dt',
    'qt_from_dt',
    'fromtimestamp',
    'fromordinal',
    'isoformat',
    'w3cdtf',
    'as_local_time',
    'as_utc',
    'now',
    'utcnow',
    'utcfromtimestamp',
    'timestampfromdt',
    'clean_date_for_sort',
    'format_date',
    'is_date_undefined',
    'UNDEFINED_DATE',
    'DEFAULT_DATE',
    'EPOCH',
    'utc_tz',
    'local_tz',
    'date',
    'datetime',
    'timedelta',
    'timezone',
    'internal_iso_format_string',
    'replace_months',
    'datetime_to_string',
    'string_to_datetime',
]
