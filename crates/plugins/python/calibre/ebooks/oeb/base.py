"""Mirrors the constants and helpers of calibre's `calibre.ebooks.oeb.base`
that plugins use: namespaces, `XPath`, `xml2str`, `barename`, `namespace`,
`urlnormalize`, mime types. calibre's `OEBBook` object model (the conversion
pipeline's) isn't here."""

import re
from urllib.parse import quote, unquote, urlparse, urlunparse

from lxml import etree

XML_NS = 'http://www.w3.org/XML/1998/namespace'
XHTML_NS = 'http://www.w3.org/1999/xhtml'
OEB_DOC_NS = 'http://openebook.org/namespaces/oeb-document/1.0/'
OPF1_NS = 'http://openebook.org/namespaces/oeb-package/1.0/'
OPF2_NS = 'http://www.idpf.org/2007/opf'
OPF_NS = OPF2_NS
OPF_NSES = {OPF1_NS, OPF2_NS}
DC09_NS = 'http://purl.org/metadata/dublin_core'
DC10_NS = 'http://purl.org/dc/elements/1.0/'
DC11_NS = 'http://purl.org/dc/elements/1.1/'
DC_NSES = {DC09_NS, DC10_NS, DC11_NS}
XSI_NS = 'http://www.w3.org/2001/XMLSchema-instance'
DCTERMS_NS = 'http://purl.org/dc/terms/'
NCX_NS = 'http://www.daisy.org/z3986/2005/ncx/'
SVG_NS = 'http://www.w3.org/2000/svg'
XLINK_NS = 'http://www.w3.org/1999/xlink'
CALIBRE_NS = 'http://calibre.kovidgoyal.net/2009/metadata'
RE_NS = 'http://exslt.org/regular-expressions'
MBP_NS = 'http://www.mobipocket.com'
EPUB_NS = 'http://www.idpf.org/2007/ops'
MATHML_NS = 'http://www.w3.org/1998/Math/MathML'

XPNSMAP = {
    'h': XHTML_NS,
    'o1': OPF1_NS,
    'o2': OPF2_NS,
    'd09': DC09_NS,
    'd10': DC10_NS,
    'd11': DC11_NS,
    'xsi': XSI_NS,
    'dt': DCTERMS_NS,
    'ncx': NCX_NS,
    'svg': SVG_NS,
    'xl': XLINK_NS,
    're': RE_NS,
    'mathml': MATHML_NS,
    'mbp': MBP_NS,
    'calibre': CALIBRE_NS,
    'epub': EPUB_NS,
}

OPF_MIME = 'application/oebps-package+xml'
OEB_DOC_MIME = 'text/x-oeb1-document'
OEB_CSS_MIME = 'text/x-oeb1-css'
OPENTYPE_MIME = 'application/vnd.ms-opentype'
GIF_MIME = 'image/gif'
JPEG_MIME = 'image/jpeg'
PNG_MIME = 'image/png'
SVG_MIME = 'image/svg+xml'
WEBP_MIME = 'image/webp'
BINARY_MIME = 'application/octet-stream'
XHTML_MIME = 'application/xhtml+xml'
CSS_MIME = 'text/css'
NCX_MIME = 'application/x-dtbncx+xml'
PAGE_MAP_MIME = 'application/oebps-page-map+xml'
JS_MIME = 'application/javascript'
OEB_STYLES = {CSS_MIME, OEB_CSS_MIME, 'text/x-oeb-css', 'xhtml/css'}
OEB_DOCS = {XHTML_MIME, 'application/xhtml+xml', OEB_DOC_MIME, 'text/x-oeb1-document'}
OEB_RASTER_IMAGES = {GIF_MIME, JPEG_MIME, PNG_MIME, WEBP_MIME}
OEB_IMAGES = {GIF_MIME, JPEG_MIME, PNG_MIME, SVG_MIME, WEBP_MIME}
OEB_FONTS = {
    'application/x-font-ttf',
    'application/x-font-otf',
    OPENTYPE_MIME,
    'font/otf',
    'font/ttf',
    'font/woff',
    'font/woff2',
    'application/font-woff',
    'application/font-woff2',
}
MS_COVER_TYPE = 'other.ms-coverimage-standard'
ENTITY_RE = re.compile(r'&([a-zA-Z_:][a-zA-Z0-9.-_:]+);')
COLLAPSE_RE = re.compile(r'[^\S\u00ad\u2000-\u200f]+')
QNAME_RE = re.compile(r'^[{][^{}]+[}][^{}]+$')
PREFIXNAME_RE = re.compile(r'^[^:]+[:][^:]+')
XMLDECL_RE = re.compile(r'^\s*<[?]xml.*?[?]>')
CSSURL_RE = re.compile(r"""url[(](?P<q>["']?)(?P<url>[^)]+)(?P=q)[)]""")


def element(parent, *args, **kwargs):
    if parent is not None:
        return etree.SubElement(parent, *args, **kwargs)
    return etree.Element(*args, **kwargs)


def XPath(expr):
    return etree.XPath(expr, namespaces=XPNSMAP)


def xpath(elem, expr):
    return elem.xpath(expr, namespaces=XPNSMAP)


def _prefixname(name, nsmap):
    if not isqname(name):
        return name
    ns = namespace(name)
    if ns not in nsmap:
        return name
    prefix = nsmap[ns]
    if not prefix:
        return barename(name)
    return ':'.join((prefix, barename(name)))


def prefixname(name, nsmap):
    return _prefixname(name, nsmap)


def isprefixname(name):
    return name and PREFIXNAME_RE.match(name) is not None


def qname(name, nsmap):
    if not isprefixname(name):
        return name
    prefix, local = name.split(':', 1)
    if prefix not in nsmap:
        return name
    return '{%s}%s' % (nsmap[prefix], local)


def isqname(name):
    return name and QNAME_RE.match(name) is not None


def XHTML(name):
    return '{%s}%s' % (XHTML_NS, name)


def OPF(name):
    return '{%s}%s' % (OPF2_NS, name)


def DC(name):
    return '{%s}%s' % (DC11_NS, name)


def NCX(name):
    return '{%s}%s' % (NCX_NS, name)


def SVG(name):
    return '{%s}%s' % (SVG_NS, name)


def XLINK(name):
    return '{%s}%s' % (XLINK_NS, name)


def CALIBRE(name):
    return '{%s}%s' % (CALIBRE_NS, name)


def XML(name):
    return '{%s}%s' % (XML_NS, name)


def EPUB(name):
    return '{%s}%s' % (EPUB_NS, name)


def namespace(name):
    if '}' in name:
        return name.split('}', 1)[0][1:]
    return ''


def barename(name):
    if '}' in name:
        return name.split('}', 1)[1]
    return name


def xml2str(root, pretty_print=False, strip_comments=False, with_tail=True):
    if hasattr(root, 'xpath'):
        ans = etree.tostring(
            root, encoding='utf-8', xml_declaration=True, pretty_print=pretty_print, with_tail=with_tail
        )
    else:
        ans = etree.tostring(root, encoding='utf-8', xml_declaration=True, pretty_print=pretty_print)
    if strip_comments:
        ans = re.compile(rb'<!--.*?-->', re.DOTALL).sub(b'', ans)
    return ans


def xml2text(elem, pretty_print=False, method='text'):
    return etree.tostring(elem, method=method, encoding='unicode', with_tail=False, pretty_print=pretty_print)


def xml2unicode(root, pretty_print=False):
    return etree.tostring(root, pretty_print=pretty_print, encoding='unicode')


def serialize(data, media_type, pretty_print=False):
    if isinstance(data, etree._Element):
        is_oeb_doc = media_type in OEB_DOCS
        if is_oeb_doc:
            return xml2str(data, pretty_print=pretty_print)
        return xml2str(data, pretty_print=pretty_print)
    if isinstance(data, str):
        return data.encode('utf-8')
    return bytes(data)


def urlquote(href):
    return quote(href, safe="/:@&=+$,;!*'()%#?")


def urlunquote(href, error_handling='strict'):
    return unquote(href, errors=error_handling)


def urlnormalize(href):
    """Normalize a URL for comparison: quoted the way calibre quotes."""
    try:
        parts = urlparse(href)
    except ValueError:
        return href
    if not parts.scheme or parts.scheme == 'file':
        path, frag = href.partition('#')[::2]
        return urlquote(urlunquote(path)) + ('#' + frag if frag else '')
    return urlunparse(parts)


def urldefrag(href):
    path, _, frag = href.partition('#')
    return path, frag


def rewrite_links(root, link_repl_func, resolve_base_href=False):
    for el in root.iter():
        for attr in ('href', 'src', 'xlink:href'):
            v = el.get(attr)
            if v:
                new = link_repl_func(v)
                if new is not None and new != v:
                    el.set(attr, new)


def iterlinks(root, find_links_in_css=True):
    for el in root.iter():
        for attr in ('href', 'src'):
            v = el.get(attr)
            if v:
                yield el, attr, v, 0


class TOC:
    """A table-of-contents tree node, as plugins build one."""

    def __init__(
        self,
        title=None,
        href=None,
        klass=None,
        id=None,
        play_order=None,
        author=None,
        description=None,
        toc_thumbnail=None,
    ):
        self.title = title
        self.href = href
        self.klass = klass
        self.id = id
        self.nodes = []
        self.play_order = play_order
        self.author = author
        self.description = description
        self.toc_thumbnail = toc_thumbnail

    def add(self, *args, **kwargs):
        node = TOC(*args, **kwargs)
        self.nodes.append(node)
        return node

    def iter(self):
        yield self
        for node in self.nodes:
            yield from node.iter()

    def __iter__(self):
        return iter(self.nodes)

    def __len__(self):
        return len(self.nodes)

    def count(self):
        return len(list(self.iter())) - 1

    def iterdescendants(self):
        for node in self.nodes:
            yield from node.iter()

    def depth(self):
        return 1 + max((n.depth() for n in self.nodes), default=0)


class Metadata:
    """calibre's OEB metadata container isn't needed outside the pipeline; this keeps `isinstance` checks working."""

    def __init__(self, oeb=None):
        self.oeb = oeb
        self.items = {}

    def add(self, term, value, attrib={}, nsmap={}, **kwargs):
        self.items.setdefault(term, []).append(value)


__all__ = [n for n in dir() if not n.startswith('_')]
