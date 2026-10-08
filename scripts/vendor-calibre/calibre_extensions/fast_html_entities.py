"""calibre_extensions.fast_html_entities: `replace_all_entities`."""

import html
import re

_ENTITY = re.compile(r'&(#\d+|#[xX][0-9a-fA-F]+|[a-zA-Z][a-zA-Z0-9]*);')


def replace_all_entities(raw, keep_xml_entities=False):
    def sub(m):
        text = m.group(0)
        if keep_xml_entities and m.group(1) in ('lt', 'gt', 'amp', 'quot', 'apos'):
            return text
        out = html.unescape(text)
        return out if out != text else text

    return _ENTITY.sub(sub, raw)
