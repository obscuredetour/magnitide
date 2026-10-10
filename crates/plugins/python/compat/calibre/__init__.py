# Derived from calibre's src/calibre/__init__.py (calibre 9.15).
# Copyright: 2008, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Magnitide's compatibility package for calibre plugins: the `calibre`
package plugins import, written for Magnitide, mirroring calibre 9.15's
public API where plugins use it. Each module's docstring says what it mirrors
and what it leaves out; modules that reuse calibre's own code say so at the
top (see NOTICE).

This module mirrors `calibre/__init__.py`: the odds and ends plugins import
from the top level (`prints`, `force_unicode`, `browser`, `fit_image`, …).
"""

import codecs
import os
import re
import sys
import time
from contextlib import suppress

from calibre.constants import (  # noqa: F401
    DEBUG,
    __appname__,
    __author__,
    __version__,
    filesystem_encoding,
    ismacos,
    isosx,
    iswindows,
    plugins,
    preferred_encoding,
)
from calibre.startup import initialize_calibre

initialize_calibre()

_mt_inited = False


def _init_mimetypes():
    global _mt_inited
    if not _mt_inited:
        import mimetypes

        _mt_inited = True
        mimetypes.init()
        for ext, mt in (
            ('epub', 'application/epub+zip'),
            ('kepub', 'application/epub+zip'),
            ('mobi', 'application/x-mobipocket-ebook'),
            ('prc', 'application/x-mobipocket-ebook'),
            ('azw', 'application/x-mobipocket-ebook'),
            ('azw3', 'application/x-mobi8-ebook'),
            ('kfx', 'application/x-kfx-ebook'),
            ('azw4', 'application/x-mobipocket-ebook'),
            ('fb2', 'text/fb2+xml'),
            ('lit', 'application/x-ms-reader'),
            ('lrf', 'application/x-sony-bbeb'),
            ('pdb', 'application/vnd.palm'),
            ('cbz', 'application/vnd.comicbook+zip'),
            ('cbr', 'application/vnd.comicbook-rar'),
            ('cb7', 'application/x-cb7'),
            ('cbc', 'application/x-cbc'),
            ('djvu', 'image/vnd.djvu'),
            ('opf', 'application/oebps-package+xml'),
            ('ncx', 'application/x-dtbncx+xml'),
            ('xhtml', 'application/xhtml+xml'),
            ('htmlz', 'application/x-htmlz'),
            ('txtz', 'application/x-txtz'),
            ('rb', 'application/x-rocketbook'),
            ('snb', 'application/snb'),
            ('tcr', 'application/x-tcr'),
            ('pml', 'application/x-pml'),
            ('pmlz', 'application/x-pmlz'),
            ('webp', 'image/webp'),
            ('svg', 'image/svg+xml'),
            ('md', 'text/markdown'),
            ('txt', 'text/plain'),
            ('rtf', 'application/rtf'),
            ('docx', 'application/vnd.openxmlformats-officedocument.wordprocessingml.document'),
            ('odt', 'application/vnd.oasis.opendocument.text'),
            ('zip', 'application/zip'),
            ('rar', 'application/vnd.rar'),
            ('json', 'application/json'),
            ('css', 'text/css'),
            ('js', 'text/javascript'),
            ('otf', 'font/otf'),
            ('ttf', 'font/ttf'),
            ('woff', 'font/woff'),
            ('woff2', 'font/woff2'),
            ('jpg', 'image/jpeg'),
            ('jpeg', 'image/jpeg'),
            ('png', 'image/png'),
            ('gif', 'image/gif'),
            ('bmp', 'image/bmp'),
            ('tif', 'image/tiff'),
            ('tiff', 'image/tiff'),
            ('xml', 'application/xml'),
            ('html', 'text/html'),
            ('htm', 'text/html'),
            ('mp3', 'audio/mpeg'),
            ('m4a', 'audio/mp4'),
            ('m4b', 'audio/mp4'),
            ('mp4', 'video/mp4'),
            ('smil', 'application/smil+xml'),
        ):
            mimetypes.add_type(mt, '.' + ext)


def guess_type(*args, **kwargs):
    import mimetypes

    _init_mimetypes()
    return mimetypes.guess_type(*args, **kwargs)


def guess_all_extensions(*args, **kwargs):
    import mimetypes

    _init_mimetypes()
    return mimetypes.guess_all_extensions(*args, **kwargs)


def guess_extension(*args, **kwargs):
    import mimetypes

    _init_mimetypes()
    ext = mimetypes.guess_extension(*args, **kwargs)
    if not ext and args and args[0] == 'application/x-palmreader':
        ext = '.pdb'
    return ext


def get_types_map():
    import mimetypes

    _init_mimetypes()
    return mimetypes.types_map


def to_unicode(raw, encoding='utf-8', errors='strict'):
    if isinstance(raw, str):
        return raw
    return raw.decode(encoding, errors)


def patheq(p1, p2):
    p = os.path
    d = lambda x: p.normcase(p.normpath(p.realpath(p.normpath(x))))  # noqa: E731
    if not p1 or not p2:
        return False
    return d(p1) == d(p2)


def unicode_path(path, abs=False):
    if isinstance(path, bytes):
        path = path.decode(filesystem_encoding)
    if abs:
        path = os.path.abspath(path)
    return path


def osx_version():
    from calibre.constants import get_osx_version

    return get_osx_version()


def confirm_config_name(name):
    return name + '_again'


_filename_sanitize_unicode = frozenset(('\\', '|', '?', '*', '<', '"', ':', '>', '+', '/') + tuple(map(chr, range(32))))


def sanitize_file_name(name, substitute='_'):
    """Remove the characters file systems don't allow, and leading/trailing
    dots and spaces (as calibre does), keeping the name usable on every OS."""
    if isinstance(name, bytes):
        name = name.decode(filesystem_encoding, 'ignore')
    one = ''.join(substitute if x in _filename_sanitize_unicode else x for x in name)
    one = re.sub(r'\s', ' ', one).strip()
    bname, ext = os.path.splitext(one)
    one = re.sub(r'^\.+$', '_', bname)
    one = one.replace('..', substitute)
    one += ext
    # Windows doesn't like trailing dots or spaces.
    one = one.rstrip('. ')
    if not one:
        one = substitute
    return one


sanitize_file_name_unicode = sanitize_file_name
sanitize_file_name2 = sanitize_file_name


def prints(*args, **kwargs):
    """Print, never failing on an encoding error (calibre's `prints`)."""
    file = kwargs.get('file', sys.stdout) or sys.stdout
    sep = kwargs.get('sep', ' ')
    end = kwargs.get('end', '\n')
    enc = kwargs.get('encoding') or getattr(file, 'encoding', None) or preferred_encoding
    safe_encode = kwargs.get('safe_encode', False)
    parts = []
    for arg in args:
        if isinstance(arg, bytes):
            arg = arg.decode(enc, 'replace')
        elif not isinstance(arg, str):
            try:
                arg = str(arg)
            except Exception:
                arg = repr(arg)
        if safe_encode:
            arg = arg.encode(enc, 'replace').decode(enc, 'replace')
        parts.append(arg)
    try:
        file.write(sep.join(parts) + end)
    except UnicodeEncodeError:
        file.write((sep.join(parts) + end).encode(enc, 'replace').decode(enc, 'replace'))
    except Exception:
        pass
    if kwargs.get('flush'):
        with suppress(Exception):
            file.flush()


class CurrentDir:
    def __init__(self, path):
        self.path = path
        self.cwd = None

    def __enter__(self, *args):
        self.cwd = os.getcwd()
        os.chdir(self.path)
        return self.cwd

    def __exit__(self, *args):
        with suppress(OSError):
            os.chdir(self.cwd)


def detect_ncpus():
    try:
        return max(1, os.cpu_count() or 1)
    except Exception:
        return 1


relpath = os.path.relpath


def walk(dir):
    """Every file under `dir`, as calibre's walk."""
    for record in os.walk(dir):
        for f in record[-1]:
            yield os.path.join(record[0], f)


def strftime(fmt, t=None):
    """strftime that handles pre-1900 dates and Unicode formats."""
    if t is None:
        t = time.localtime()
    if hasattr(t, 'timetuple'):
        t = t.timetuple()
    early_year = t[0] < 1900
    if early_year:
        replacement = 1900 if t[0] % 4 == 0 else 1901
        fmt = fmt.replace('%Y', '_early year hack##')
        t = list(t)
        orig_year = t[0]
        t[0] = replacement
        t = time.struct_time(t)
    ans = time.strftime(fmt, t)
    if early_year:
        ans = ans.replace('_early year hack##', str(orig_year))
    return ans


def my_unichr(num):
    try:
        return chr(num)
    except ValueError, OverflowError:
        return '?'


def entity_to_unicode(match, exceptions=(), encoding='cp1252', result_exceptions={}):
    """Replace an HTML entity match (`&name;`, `&#nn;`, `&#xhh;`) with its
    character. Entities in `exceptions` are kept; `result_exceptions` maps
    a character to what to return instead."""
    from html.entities import name2codepoint

    def check(ch):
        return result_exceptions.get(ch, ch)

    ent = match.group(1)
    if ent in exceptions:
        return '&' + ent + ';'
    if ent in {'apos', 'squot'}:
        return check("'")
    if ent == 'hellips':
        ent = 'hellip'
    if ent.startswith('#'):
        try:
            if ent[1] in 'xX':
                num = int(ent[2:], 16)
            else:
                num = int(ent[1:])
        except Exception:
            return '&' + ent + ';'
        if encoding is None or num > 255:
            return check(my_unichr(num))
        try:
            return check(bytes(bytearray((num,))).decode(encoding))
        except UnicodeDecodeError:
            return check(my_unichr(num))
    try:
        return check(my_unichr(name2codepoint[ent]))
    except KeyError:
        return '&' + ent + ';'


_ent_pat = re.compile(r'&(\S+?);')
xml_entity_to_unicode = lambda match: entity_to_unicode(match, exceptions=('amp', 'lt', 'gt', 'quot', 'apos'))  # noqa: E731


def replace_entities(raw, encoding='cp1252'):
    return _ent_pat.sub(lambda m: entity_to_unicode(m, encoding=encoding), raw)


def xml_replace_entities(raw, encoding='cp1252'):
    return _ent_pat.sub(
        lambda m: entity_to_unicode(m, exceptions=('amp', 'lt', 'gt', 'quot', 'apos'), encoding=encoding), raw
    )


def prepare_string_for_xml(raw, attribute=False):
    raw = _ent_pat.sub(xml_entity_to_unicode, raw)
    raw = raw.replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;')
    if attribute:
        raw = raw.replace('"', '&quot;').replace("'", '&apos;')
    return raw


def isbytestring(obj):
    return isinstance(obj, bytes)


def force_unicode(obj, enc=preferred_encoding):
    if isinstance(obj, bytes):
        try:
            obj = obj.decode(enc)
        except Exception:
            try:
                obj = obj.decode(filesystem_encoding if enc == preferred_encoding else preferred_encoding)
            except Exception:
                try:
                    obj = obj.decode('utf-8')
                except Exception:
                    obj = repr(obj)
    return obj


def as_unicode(obj, enc=preferred_encoding):
    if not isinstance(obj, bytes):
        try:
            obj = str(obj)
        except Exception:
            try:
                obj = repr(obj)
            except Exception:
                obj = '<unrepresentable object>'
    return force_unicode(obj, enc=enc)


def fit_image(width, height, pwidth, pheight):
    """Scale (width, height) to fit in (pwidth, pheight), keeping the aspect
    ratio: (scaled, new_width, new_height)."""
    scaled = height > pheight or width > pwidth
    if height > pheight:
        corrf = pheight / float(height)
        width, height = floor(corrf * width), pheight
    if width > pwidth:
        corrf = pwidth / float(width)
        width, height = pwidth, floor(corrf * height)
    if height > pheight:
        corrf = pheight / float(height)
        width, height = floor(corrf * width), pheight
    return scaled, int(width), int(height)


def floor(x):
    import math

    return math.floor(x)


def human_readable(size, sep=' '):
    """Bytes as a short string: 1.2 MB."""
    divisor, suffix = 1, 'B'
    for i, candidate in enumerate(('B', 'KB', 'MB', 'GB', 'TB', 'PB', 'EB')):
        if size < (1 << ((i + 1) * 10)):
            divisor, suffix = (1 << (i * 10)), candidate
            break
    size = str(float(size) / divisor)
    if size.find('.') > -1:
        size = size[: size.find('.') + 2]
    if size.endswith('.0'):
        size = size[:-2]
    return size + sep + suffix


def remove_bracketed_text(src, brackets=None):
    if brackets is None:
        brackets = {'(': ')', '[': ']', '{': '}'}
    from collections import Counter

    counts = Counter()
    buf = []
    src = force_unicode(src)
    rmap = {v: k for k, v in brackets.items()}
    for char in src:
        if char in brackets:
            counts[char] += 1
        elif char in rmap:
            idx = rmap[char]
            if counts[idx] > 0:
                counts[idx] -= 1
        elif sum(counts.values()) < 1:
            buf.append(char)
    return ''.join(buf)


def get_proxies(debug=True):
    from polyglot.urllib import getproxies

    proxies = getproxies()
    for key, proxy in list(proxies.items()):
        if not proxy or '..' in proxy or key == 'auto':
            del proxies[key]
            continue
        if proxy.startswith(key + '://'):
            proxy = proxy[len(key) + 3 :]
        if key == 'https' and proxy.startswith('http://'):
            proxy = proxy[7:]
        if proxy.endswith('/'):
            proxy = proxy[:-1]
        if len(proxy) > 4:
            proxies[key] = proxy
        else:
            del proxies[key]
    if proxies and debug:
        prints('Using proxies:', proxies)
    return proxies


def get_proxy_info(proxy_scheme, proxy_string):
    """Parse 'user:pass@host:port' into a dict with scheme, user, pass,
    hostname and port."""
    from polyglot.urllib import urlparse

    try:
        proxy_url = f'{proxy_scheme}://{proxy_string}'
        urlinfo = urlparse(proxy_url)
        ans = {
            'scheme': urlinfo.scheme,
            'hostname': urlinfo.hostname,
            'port': urlinfo.port,
            'username': urlinfo.username,
            'password': urlinfo.password,
        }
    except Exception:
        return None
    return ans


def get_parsed_proxy(typ='http', debug=True):
    """The proxy for `typ` from the environment, parsed (see
    `get_proxy_info`), or None."""
    proxies = get_proxies(debug)
    proxy = proxies.get(typ, None)
    if proxy:
        pattern = re.compile(r'(?:(?P<user>\S+?):(?P<pass>\S+?)@)?(?P<host>[^:]+)(?::(?P<port>\d+))?')
        match = pattern.match(proxy)
        if match:
            try:
                ans = {
                    'host': match.group('host'),
                    'port': match.group('port'),
                    'user': match.group('user'),
                    'pass': match.group('pass'),
                }
                if ans['port']:
                    ans['port'] = int(ans['port'])
            except Exception:
                if debug:
                    import traceback

                    traceback.print_exc()
            else:
                if debug:
                    prints('Using http proxy', str(ans))
                return ans
    return None


USER_AGENT = 'Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0'
USER_AGENT_MOBILE = 'Mozilla/5.0 (Windows; U; Windows CE 5.1; rv:1.8.1a3) Gecko/20060610 Minimo/0.016'


def is_mobile_ua(ua):
    return 'Mobile/' in ua or 'Mobile ' in ua


def random_user_agent(choose=None, allow_ie=True):
    from calibre.utils.random_ua import random_user_agent as rua

    return rua(choose=choose, allow_ie=allow_ie)


def browser(
    honor_time=True,
    max_time=2,
    mobile_browser=False,
    user_agent=None,
    verify_ssl_certificates=True,
    handle_refresh=True,
    **kw,
):
    """A mechanize browser set up as calibre's: robots ignored, a browser-like
    user agent, gzip accepted, proxies from the environment."""
    from calibre.utils.browser import Browser

    opener = Browser(verify_ssl=verify_ssl_certificates)
    opener.set_handle_refresh(handle_refresh, max_time=max_time, honor_time=honor_time)
    opener.set_handle_robots(False)
    if user_agent is None:
        user_agent = USER_AGENT_MOBILE if mobile_browser else USER_AGENT
    opener.addheaders = [('User-agent', user_agent)]
    proxies = get_proxies(debug=False)
    to_add = {}
    http_proxy = proxies.get('http', None)
    if http_proxy:
        to_add['http'] = http_proxy
    https_proxy = proxies.get('https', None)
    if https_proxy:
        to_add['https'] = https_proxy
    if to_add:
        opener.set_proxies(to_add)
    return opener


def url_slash_cleaner(url):
    """Remove redundant slashes from the path of an URL."""
    return re.sub(r'(?<!:)/{2,}', '/', url)


def get_download_filename_from_response(response):
    from polyglot.urllib import unquote as urllib_unquote

    filename = ''
    content_disp = response.headers.get('Content-Disposition', '')
    if content_disp:
        for p in content_disp.split(';'):
            p = p.strip()
            if p.startswith('filename='):
                filename = p[9:].strip('"\'')
                break
    if not filename:
        filename = os.path.basename(response.geturl().split('?')[0])
    return urllib_unquote(filename)


def get_download_filename(url, cookie_file=None):
    br = browser()
    with br.open(url) as r:
        return get_download_filename_from_response(r)


def extract(path, dir):
    """Unpack a zip (or rar, if Estuary can) into `dir`."""
    extractor = None
    with open(path, 'rb') as f:
        head = f.read(4)
    if head.startswith(b'PK'):
        from calibre.utils.zipfile import ZipFile

        extractor = lambda p, d: ZipFile(p).extractall(d)  # noqa: E731
    elif head.startswith(b'Rar'):
        from calibre.utils.unrar import extract as rarextract

        extractor = rarextract
    if extractor is None:
        raise Exception('Unknown archive type')
    extractor(path, dir)


def setup_cli_handlers(logger, level):
    import logging

    logger.setLevel(level)
    if not logger.handlers:
        handler = logging.StreamHandler(sys.stderr)
        handler.setFormatter(logging.Formatter('%(levelname)s: %(message)s'))
        logger.addHandler(handler)


def load_library(name, cdll):
    """Load a shared library by name through ctypes."""
    if iswindows:
        return cdll.LoadLibrary(name)
    if ismacos:
        name += '.dylib'
        return cdll.LoadLibrary(name)
    return cdll.LoadLibrary(name + '.so')


def ipython(user_ns=None):
    import code

    code.interact(local=user_ns or {})


def getcwd():
    return os.getcwd()


def utf8_open(path, mode='r'):
    return codecs.open(path, mode, 'utf-8')
