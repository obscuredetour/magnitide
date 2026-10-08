"""Mirrors calibre's `calibre.utils.html2text`: HTML to Markdown-ish text,
through the html2text library."""


def html2text(html, single_line_break=True, unicode_snob=True, wrap=False, **kw):
    import html2text as h2t

    h = h2t.HTML2Text()
    h.unicode_snob = unicode_snob
    h.single_line_break = single_line_break
    h.body_width = 0 if not wrap else 78
    h.ignore_images = kw.get('ignore_images', False)
    h.ignore_links = kw.get('ignore_links', False)
    return h.handle(html)
