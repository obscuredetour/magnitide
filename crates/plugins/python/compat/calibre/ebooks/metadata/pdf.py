"""Mirrors calibre's `calibre.ebooks.metadata.pdf`: reading a PDF's
document information (through pypdf; calibre uses PoDoFo). No cover is
rendered, as Estuary has no PDF renderer."""

from calibre.ebooks.metadata import MetaInformation, check_isbn, string_to_authors
from calibre.utils.date import parse_date


def get_metadata(stream, cover=True):
    stream.seek(0)
    mi = MetaInformation('Unknown', ['Unknown'])
    try:
        from pypdf import PdfReader

        reader = PdfReader(stream)
        info = reader.metadata or {}
    except Exception:
        return mi
    title = (info.get('/Title') or '').strip()
    if title:
        mi.title = str(title)
    author = (info.get('/Author') or '').strip()
    if author:
        mi.authors = string_to_authors(str(author).replace(';', '&').replace(',', '&')) or ['Unknown']
    subject = (info.get('/Subject') or '').strip()
    if subject:
        mi.comments = str(subject)
    keywords = (info.get('/Keywords') or '').strip()
    if keywords:
        tags = [t.strip() for t in str(keywords).replace(';', ',').split(',') if t.strip()]
        if tags:
            mi.tags = tags
        isbn = check_isbn(str(keywords))
        if isbn:
            mi.isbn = isbn
    creator = (info.get('/Creator') or info.get('/Producer') or '').strip()
    if creator:
        mi.book_producer = str(creator)
    created = info.get('/CreationDate')
    if created:
        try:
            raw = str(created)
            if raw.startswith('D:'):
                raw = raw[2:]
            mi.pubdate = parse_date(raw[:4] + '-' + raw[4:6] + '-' + raw[6:8] if len(raw) >= 8 else raw)
        except Exception:
            pass
    return mi


def set_metadata(stream, mi):
    stream.seek(0)
    try:
        from pypdf import PdfReader, PdfWriter
    except Exception:
        return
    reader = PdfReader(stream)
    writer = PdfWriter()
    writer.append(reader)
    md = {}
    if mi.title:
        md['/Title'] = mi.title
    if mi.authors:
        md['/Author'] = ', '.join(mi.authors)
    if mi.comments:
        md['/Subject'] = mi.comments
    if mi.tags:
        md['/Keywords'] = ', '.join(mi.tags)
    writer.add_metadata(md)
    stream.seek(0)
    stream.truncate()
    writer.write(stream)
    stream.flush()
