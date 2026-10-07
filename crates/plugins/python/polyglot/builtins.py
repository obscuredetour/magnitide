"""Mirrors calibre's `polyglot.builtins` (calibre 9.15): the names plugins
written for calibre 3–5 still import. Everything is the Python 3 built-in."""

import os
import sys
from functools import reduce  # noqa: F401 (re-exported)

is_py3 = True
ispy3 = True
unicode_type = str
string_or_bytes = (str, bytes)
string_or_unicode = (str, bytes)
long_type = int
native_string_type = str
raw_input = input
codepoint_to_chr = chr
unichr = chr
range = range
zip = zip
map = map
filter = filter
getcwd = os.getcwd
cmp = lambda a, b: (a > b) - (a < b)  # noqa: E731
iterkeys = lambda d: iter(d.keys())  # noqa: E731
itervalues = lambda d: iter(d.values())  # noqa: E731
iteritems = lambda d: iter(d.items())  # noqa: E731


def as_bytes(x, encoding='utf-8'):
    if isinstance(x, str):
        return x.encode(encoding)
    if isinstance(x, bytes):
        return x
    if isinstance(x, bytearray):
        return bytes(x)
    if isinstance(x, memoryview):
        return x.tobytes()
    ans = str(x)
    if isinstance(ans, str):
        ans = ans.encode(encoding)
    return ans


def as_unicode(x, encoding='utf-8', errors='strict'):
    if isinstance(x, bytes):
        return x.decode(encoding, errors)
    return str(x)


def reraise(tp, value, tb=None):
    try:
        if value is None:
            value = tp()
        if value.__traceback__ is not tb:
            raise value.with_traceback(tb)
        raise value
    finally:
        value = None
        tb = None


def environ_item(x):
    if isinstance(x, bytes):
        x = x.decode('utf-8')
    return x


def hasenv(x):
    return os.environ.get(x) is not None


def exec_path(path, ctx=None):
    ctx = ctx or {}
    with open(path, 'rb') as f:
        code = f.read()
    code = compile(code, path, 'exec')
    exec(code, ctx)


def getenv(x, default=None):
    return os.environ.get(x, default)


def print_to_binary_file(fileobj, encoding='utf-8'):
    def print(*a, **kw):
        kw['file'] = fileobj
        sep, end = kw.get('sep', ' '), kw.get('end', '\n')
        fileobj.write(sep.join(as_bytes(x, encoding) for x in a) + as_bytes(end, encoding))

    return print


def __getattr__(name):
    raise AttributeError(f'polyglot.builtins has no {name!r}')


__all__ = [n for n in dir(sys.modules[__name__]) if not n.startswith('_')]
