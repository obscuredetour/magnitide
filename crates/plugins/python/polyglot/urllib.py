"""Mirrors calibre's `polyglot.urllib`: the urllib names under one roof."""

from urllib.error import HTTPError, URLError  # noqa: F401
from urllib.parse import (  # noqa: F401
    parse_qs,
    quote,
    quote_plus,
    unquote,
    unquote_plus,
    urlencode,
    urljoin,
    urlparse,
    urlsplit,
    urlunparse,
    urlunsplit,
)
from urllib.request import (  # noqa: F401
    Request,
    build_opener,
    getproxies,
    install_opener,
    url2pathname,
    urlopen,
)


def unquote(x, encoding='utf-8', errors='replace'):  # noqa: F811
    from urllib.parse import unquote as uq

    binary = isinstance(x, bytes)
    if binary:
        x = x.decode(encoding, errors)
    ans = uq(x, encoding, errors)
    if binary:
        ans = ans.encode(encoding, errors)
    return ans
