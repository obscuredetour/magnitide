"""Mirrors calibre's `calibre.library.comments`: turning plain-text
comments into HTML, cleaning HTML comments, and merging two."""

import re

from calibre import prepare_string_for_xml
from calibre.constants import preferred_encoding

# Hackish - ignoring sentences ending or beginning in numbers to avoid
# confusion with decimal points.
lost_cr_pat = re.compile(r'([a-z])([\.\?!])([A-Z])')
lost_cr_exception_pat = re.compile(r'(Ph\.D)|(D\.Phil)|((Dr|Mr|Mrs|Ms)\.[A-Z])')
sanitize_pat = re.compile(r'<script|<table|<tr|<td|<th|<style|<iframe', re.IGNORECASE)


def comments_to_html(comments):
    """Plain text (paragraphs separated by blank lines, `\\n` for line
    breaks) to HTML; text that already is HTML is tidied, not escaped."""
    if not comments:
        return '<p></p>'
    if isinstance(comments, bytes):
        comments = comments.decode(preferred_encoding, 'replace')

    if comments.lstrip().startswith('<'):
        try:
            return sanitize_comments_html(comments)
        except Exception:
            pass

    if '<' not in comments:
        comments = prepare_string_for_xml(comments)
        parts = ['<p class="description">%s</p>' % x.replace('\n', '<br />') for x in comments.split('\n\n')]
        return '\n'.join(parts)

    if sanitize_pat.search(comments) is not None:
        try:
            return sanitize_comments_html(comments)
        except Exception:
            import traceback

            traceback.print_exc()
            return '<p></p>'

    # Explode lost CRs to \n\n
    comments = lost_cr_exception_pat.sub(lambda m: m.group().replace('.', '.\r'), comments)
    for lost_cr in lost_cr_pat.finditer(comments):
        comments = comments.replace(
            lost_cr.group(), '%s%s\n\n%s' % (lost_cr.group(1), lost_cr.group(2), lost_cr.group(3))
        )

    comments = comments.replace('\r', '')
    # Convert \n\n to <p>s
    comments = comments.replace('\n\n', '<p>')
    # Convert solo returns to <br />
    comments = comments.replace('\n', '<br />')
    # Convert two hyphens to emdash
    comments = comments.replace('--', '&mdash;')

    import html

    out = []
    for para in comments.split('<p>'):
        para = para.strip()
        if not para:
            continue
        if para.startswith('<'):
            out.append(para)
        else:
            out.append('<p class="description">%s</p>' % para)
    result = '\n'.join(out)
    try:
        return sanitize_comments_html(result)
    except Exception:
        return html.escape(comments)


def markdown(val):
    try:
        import markdown as md

        return md.markdown(val)
    except Exception:
        return comments_to_html(val)


def merge_comments(one, two):
    return comments_to_html(one) + '\n\n' + comments_to_html(two)


_dangerous = re.compile(r'<(script|style|iframe|object|embed|link|meta|form|input|textarea|button)\b', re.IGNORECASE)


def sanitize_comments_html(html):
    """Remove scripts, styles and event handlers, keeping the markup."""
    if not html:
        return ''
    if _dangerous.search(html) is None and ' on' not in html.lower() and 'javascript:' not in html.lower():
        return html.strip()
    try:
        from lxml import etree
        from lxml import html as lh

        root = lh.fromstring('<div>' + html + '</div>')
        for bad in root.xpath(
            './/script|.//style|.//iframe|.//object|.//embed|.//link|.//meta|.//form|.//input|.//textarea|.//button'
        ):
            bad.drop_tree()
        for el in root.iter():
            if not isinstance(el.tag, str):
                continue
            for attr in list(el.attrib):
                low = attr.lower()
                if low.startswith('on') or (
                    low in ('href', 'src') and el.attrib[attr].strip().lower().startswith('javascript:')
                ):
                    del el.attrib[attr]
        parts = []
        if root.text:
            parts.append(prepare_string_for_xml(root.text))
        for child in root:
            parts.append(etree.tostring(child, method='html', encoding='unicode'))
        return ''.join(parts).strip()
    except Exception:
        import html as hm

        return hm.escape(html)
