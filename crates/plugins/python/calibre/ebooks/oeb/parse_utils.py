"""Mirrors `parse_html` and `NotHTML` from calibre's
`calibre.ebooks.oeb.parse_utils`: HTML or XHTML bytes to an lxml tree in the
XHTML namespace, forgiving of broken markup."""

import re

from lxml import etree, html

from calibre.ebooks.chardet import strip_encoding_declarations, xml_to_unicode
from calibre.ebooks.oeb.base import XHTML, XHTML_NS, XML_NS, barename, namespace

RECOVER_PARSER = etree.XMLParser(recover=True, no_network=True, resolve_entities=False)
XMLNS_ATTR = '{%s}xmlns' % XML_NS


class NotHTML(Exception):
    def __init__(self, root_tag):
        Exception.__init__(self, 'Data is not HTML')
        self.root_tag = root_tag


def merge_multiple_html_heads_and_bodies(root, log=None):
    heads, bodies = root.findall(XHTML('head')), root.findall(XHTML('body'))
    if len(heads) < 2 and len(bodies) < 2:
        return root
    head = etree.SubElement(root, XHTML('head'))
    body = etree.SubElement(root, XHTML('body'))
    for h in heads:
        for x in list(h):
            head.append(x)
    for b in bodies:
        for x in list(b):
            body.append(x)
    for x in heads + bodies:
        root.remove(x)
    return root


def _html_to_xhtml(root):
    """Give every element the XHTML namespace."""
    for elem in root.iter(etree.Element):
        tag = elem.tag
        if isinstance(tag, str) and not namespace(tag):
            elem.tag = XHTML(tag)
    return root


def clean_word_doc(data, log=None):
    return data


def parse_html(data, log=None, decoder=None, preprocessor=None, filename='<string>', non_html_file_tags=frozenset()):
    """Parse `data` (bytes or str) as (X)HTML into an lxml tree whose
    elements are in the XHTML namespace."""
    if isinstance(data, bytes):
        data = xml_to_unicode(data, strip_encoding_pats=True, resolve_entities=True)[0]
    data = strip_encoding_declarations(data)
    data = data.replace('\0', '')
    data = re.sub(r'^\s*<\?xml[^>]*\?>', '', data)
    root = None
    try:
        root = etree.fromstring(
            data.strip(), parser=etree.XMLParser(recover=False, no_network=True, resolve_entities=False)
        )
    except etree.XMLSyntaxError:
        root = None
    if root is None or barename(root.tag).lower() != 'html':
        try:
            root = html.fromstring(data)
        except Exception:
            root = etree.fromstring(data, parser=RECOVER_PARSER)
        if root is None:
            raise NotHTML('')
        # lxml.html may hand back a fragment: wrap it.
        if barename(root.tag).lower() != 'html':
            doc = html.fromstring('<html><head></head><body></body></html>')
            doc.find('body').append(root)
            root = doc
        root = _html_to_xhtml(root)
    tag = barename(root.tag).lower()
    if tag in non_html_file_tags or tag != 'html':
        raise NotHTML(root.tag)
    if root.nsmap.get(None) != XHTML_NS:
        root = _html_to_xhtml(root)
    if root.find(XHTML('head')) is None:
        root.insert(0, etree.Element(XHTML('head')))
    if root.find(XHTML('body')) is None:
        etree.SubElement(root, XHTML('body'))
    return merge_multiple_html_heads_and_bodies(root, log)


def html5_parse(data, log=None):
    return parse_html(data, log)


__all__ = ['parse_html', 'html5_parse', 'NotHTML', 'merge_multiple_html_heads_and_bodies', 'clean_word_doc']
