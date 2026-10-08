"""Mirrors calibre's `calibre.utils.logging` (calibre 9.15): the `Log` that
calibre hands plugins (metadata sources get one per lookup), with its levels
and output streams."""

import io
import sys
import traceback
from functools import partial
from threading import Lock

from polyglot.builtins import as_unicode

DEBUG = 0
INFO = 1
WARN = 2
ERROR = 3


class Stream:
    def __init__(self, stream=None):
        if stream is None:
            stream = io.StringIO()
        self.stream = stream
        self._prints = partial(_prints, file=stream)

    def flush(self):
        try:
            self.stream.flush()
        except Exception:
            pass

    def prints(self, level, *args, **kwargs):
        self._prints(*args, **kwargs)


def _prints(*args, **kwargs):
    file = kwargs.get('file', sys.stdout)
    sep = kwargs.get('sep', ' ')
    end = kwargs.get('end', '\n')
    parts = []
    for a in args:
        if isinstance(a, bytes):
            a = a.decode('utf-8', 'replace')
        elif not isinstance(a, str):
            try:
                a = str(a)
            except Exception:
                a = repr(a)
        parts.append(a)
    try:
        file.write(sep.join(parts) + end)
    except Exception:
        try:
            file.write((sep.join(parts) + end).encode('utf-8', 'replace').decode('utf-8', 'replace'))
        except Exception:
            pass


class ANSIStream(Stream):
    def __init__(self, stream=sys.stdout):
        Stream.__init__(self, stream)
        self.color = {
            DEBUG: '\033[0;32m',
            INFO: '',
            WARN: '\033[0;33m',
            ERROR: '\033[0;31m',
        }
        self.reset = '\033[0m'
        self.use_color = hasattr(stream, 'isatty') and stream.isatty()

    def prints(self, level, *args, **kwargs):
        if self.use_color and self.color.get(level):
            self.stream.write(self.color[level])
        self._prints(*args, **kwargs)
        if self.use_color and self.color.get(level):
            self.stream.write(self.reset)

    def flush(self):
        self.stream.flush()


class FileStream(Stream):
    def __init__(self, stream=None):
        Stream.__init__(self, stream)

    def prints(self, level, *args, **kwargs):
        self._prints(*args, **kwargs)


class HTMLStream(Stream):
    color = {
        DEBUG: b'<span style="color:green">',
        INFO: b'<span>',
        WARN: b'<span style="color:blue">',
        ERROR: b'<span style="color:red">',
    }
    normal = b'</span>'

    def __init__(self, stream=sys.stdout):
        Stream.__init__(self, stream)

    def prints(self, level, *args, **kwargs):
        self.stream.write(
            self.color[level].decode('ascii') if isinstance(self.stream, io.TextIOBase) else self.color[level]
        )
        kwargs['file'] = self.stream
        self._prints(*args, **kwargs)
        self.stream.write(self.normal.decode('ascii') if isinstance(self.stream, io.TextIOBase) else self.normal)

    def flush(self):
        self.stream.flush()


class UnicodeHTMLStream(HTMLStream):
    color = {k: v.decode('ascii') for k, v in HTMLStream.color.items()}
    normal = HTMLStream.normal.decode('ascii')

    def __init__(self):
        self.clear()

    def flush(self):
        pass

    def prints(self, level, *args, **kwargs):
        col = self.color[level]
        if col != self.last_col:
            if self.data:
                self.data.append(self.normal)
            self.data.append(col)
            self.last_col = col

        sep = kwargs.get('sep', ' ')
        end = kwargs.get('end', '\n')

        for arg in args:
            arg = as_unicode(arg)
            self.data.append(arg + sep)
            self.plain_text.append(arg + sep)
        self.data.append(end)
        self.plain_text.append(end)

    def clear(self):
        self.data = []
        self.plain_text = []
        self.last_col = self.color[INFO]

    @property
    def html(self):
        from calibre import prepare_string_for_xml

        end = self.normal if self.data else ''
        return ''.join(self.data[:1] + [prepare_string_for_xml(x) for x in self.data[1:]]) + end

    def dump(self):
        return [self.data, self.plain_text, self.last_col]

    def load(self, dump):
        self.data, self.plain_text, self.last_col = dump

    def append_dump(self, dump):
        d, p, lc = dump
        self.data.extend(d)
        self.plain_text.extend(p)
        self.last_col = lc


class Log:
    """calibre's logger: `log('x')`, `log.info/warn/error/debug/exception`,
    with a level filter and any number of output streams."""

    DEBUG = DEBUG
    INFO = INFO
    WARN = WARN
    ERROR = ERROR

    def __init__(self, level=INFO):
        self.filter_level = level
        default_output = ANSIStream()
        self.outputs = [default_output]

        self.debug = partial(self.prints, DEBUG)
        self.info = partial(self.prints, INFO)
        self.warn = self.warning = partial(self.prints, WARN)
        self.error = partial(self.prints, ERROR)

    def prints(self, level, *args, **kwargs):
        if level < self.filter_level:
            return
        for output in self.outputs:
            output.prints(level, *args, **kwargs)

    def exception(self, *args, **kwargs):
        limit = kwargs.pop('limit', None)
        self.prints(ERROR, *args, **kwargs)
        self.prints(DEBUG, traceback.format_exc(limit))

    def __call__(self, *args, **kwargs):
        self.info(*args, **kwargs)

    def __enter__(self):
        self.orig_filter_level = self.filter_level
        self.filter_level = ERROR + 100

    def __exit__(self, *args):
        self.filter_level = self.orig_filter_level

    def flush(self):
        for o in self.outputs:
            if hasattr(o, 'flush'):
                o.flush()

    def close(self):
        for o in self.outputs:
            if hasattr(o, 'close'):
                o.close()


class DevNull(Log):
    def __init__(self):
        Log.__init__(self, level=Log.ERROR)
        self.outputs = []


class ThreadSafeLog(Log):
    exception_traceback_level = Log.DEBUG

    def __init__(self, level=Log.INFO):
        Log.__init__(self, level=level)
        self._lock = Lock()

    def prints(self, level, *args, **kwargs):
        with self._lock:
            Log.prints(self, level, *args, **kwargs)

    def exception(self, *args, **kwargs):
        limit = kwargs.pop('limit', None)
        with self._lock:
            Log.prints(self, ERROR, *args, **kwargs)
            Log.prints(self, self.exception_traceback_level, traceback.format_exc(limit))


class ThreadSafeWrapper(Log):
    def __init__(self, other_log):
        Log.__init__(self, level=other_log.filter_level)
        self.outputs = list(other_log.outputs)
        self._lock = Lock()

    def prints(self, *args, **kwargs):
        with self._lock:
            Log.prints(self, *args, **kwargs)


class GUILog(ThreadSafeLog):
    """A log kept in memory as HTML and plain text, for showing in a window
    (calibre's metadata download log, say)."""

    def __init__(self):
        ThreadSafeLog.__init__(self, level=self.DEBUG)
        self.outputs = [UnicodeHTMLStream()]

    def clear(self):
        self.outputs[0].clear()

    @property
    def html(self):
        return self.outputs[0].html

    @property
    def plain_text(self):
        return ''.join(self.outputs[0].plain_text)

    def dump(self):
        return self.outputs[0].dump()

    def load(self, dump):
        return self.outputs[0].load(dump)

    def append_dump(self, dump):
        return self.outputs[0].append_dump(dump)


default_log = Log()
