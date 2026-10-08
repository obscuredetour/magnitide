"""Mirrors calibre's `calibre.ebooks.metadata.mobi` (calibre 9.15): reading
metadata from Mobipocket and Kindle files (MOBI, PRC, AZW, AZW3) through
their EXTH header, and writing it back.

The file is a PalmDB: record 0 holds the MOBI header, then the EXTH
metadata records, then the full title. Writing rebuilds record 0 and
shifts the record offsets that follow it.
"""

import io
import struct

from calibre.ebooks.metadata import MetaInformation, string_to_authors
from calibre.utils.date import parse_date
from calibre.utils.imghdr import what
from calibre.utils.localization import canonicalize_lang

EXTH_AUTHOR, EXTH_PUBLISHER, EXTH_IMPRINT, EXTH_DESCRIPTION, EXTH_ISBN, EXTH_SUBJECT = 100, 101, 102, 103, 104, 105
EXTH_PUBDATE, EXTH_REVIEW, EXTH_CONTRIBUTOR, EXTH_RIGHTS, EXTH_SUBJECTCODE, EXTH_TYPE = 106, 107, 108, 109, 110, 111
EXTH_SOURCE, EXTH_ASIN, EXTH_VERSION, EXTH_SAMPLE, EXTH_START, EXTH_ADULT = 112, 113, 114, 115, 116, 117
EXTH_KF8_BOUNDARY, EXTH_COVER_OFFSET, EXTH_THUMB_OFFSET, EXTH_HAS_FAKE_COVER = 121, 201, 202, 203
EXTH_CREATOR_SOFTWARE, EXTH_CDE_TYPE, EXTH_UPDATED_TITLE, EXTH_ASIN2, EXTH_LANGUAGE = 204, 501, 503, 504, 524

_TEXT_EXTH = {
    EXTH_AUTHOR,
    EXTH_PUBLISHER,
    EXTH_IMPRINT,
    EXTH_DESCRIPTION,
    EXTH_ISBN,
    EXTH_SUBJECT,
    EXTH_PUBDATE,
    EXTH_REVIEW,
    EXTH_CONTRIBUTOR,
    EXTH_RIGHTS,
    EXTH_SUBJECTCODE,
    EXTH_TYPE,
    EXTH_SOURCE,
    EXTH_ASIN,
    EXTH_VERSION,
    EXTH_UPDATED_TITLE,
    EXTH_ASIN2,
    EXTH_LANGUAGE,
    100,
    517,
    522,
    523,
    525,
    527,
    528,
    529,
    534,
    535,
}


class PalmDB:
    def __init__(self, raw):
        if len(raw) < 78:
            raise ValueError('Not a PalmDB file')
        self.raw = raw
        self.name = raw[:32].rstrip(b'\0')
        self.ident = raw[60:68]
        self.num_records = struct.unpack('>H', raw[76:78])[0]
        self.offsets = []
        for i in range(self.num_records):
            off = struct.unpack('>I', raw[78 + i * 8 : 82 + i * 8])[0]
            self.offsets.append(off)

    def record(self, i):
        start = self.offsets[i]
        end = self.offsets[i + 1] if i + 1 < self.num_records else len(self.raw)
        return self.raw[start:end]

    def records(self):
        return [self.record(i) for i in range(self.num_records)]


class MobiHeader:
    def __init__(self, rec0):
        if rec0[16:20] != b'MOBI':
            raise ValueError('Not a MOBI file (no MOBI header in record 0)')
        self.compression, _, self.text_length, self.text_records, self.record_size = struct.unpack('>HHIHH', rec0[:12])
        self.length = struct.unpack('>I', rec0[20:24])[0]
        self.type = struct.unpack('>I', rec0[24:28])[0]
        self.encoding = struct.unpack('>I', rec0[28:32])[0]
        self.codec = 'utf-8' if self.encoding == 65001 else ('cp1252' if self.encoding == 1252 else 'latin1')
        self.version = struct.unpack('>I', rec0[36:40])[0]
        self.title_offset, self.title_length = struct.unpack('>II', rec0[84:92])
        self.locale = struct.unpack('>I', rec0[92:96])[0]
        self.first_image_index = struct.unpack('>I', rec0[108:112])[0] if len(rec0) >= 112 else 0
        self.exth_flags = struct.unpack('>I', rec0[128:132])[0] if len(rec0) >= 132 else 0
        self.has_exth = bool(self.exth_flags & 0x40)
        self.exth_offset = 16 + self.length
        self.exth = {}
        self.exth_items = []
        self.exth_end = self.exth_offset
        if self.has_exth and rec0[self.exth_offset : self.exth_offset + 4] == b'EXTH':
            exth_len, count = struct.unpack('>II', rec0[self.exth_offset + 4 : self.exth_offset + 12])
            pos = self.exth_offset + 12
            for _ in range(count):
                if pos + 8 > len(rec0):
                    break
                rtype, rlen = struct.unpack('>II', rec0[pos : pos + 8])
                data = rec0[pos + 8 : pos + rlen]
                self.exth_items.append((rtype, data))
                self.exth.setdefault(rtype, []).append(data)
                pos += rlen
            self.exth_end = self.exth_offset + exth_len
        self.title = rec0[self.title_offset : self.title_offset + self.title_length].decode(self.codec, 'replace')

    def text(self, rtype):
        vals = self.exth.get(rtype)
        if not vals:
            return None
        return [v.decode(self.codec, 'replace').strip() for v in vals]

    def first(self, rtype):
        v = self.text(rtype)
        return v[0] if v else None

    def number(self, rtype):
        vals = self.exth.get(rtype)
        if not vals or len(vals[0]) != 4:
            return None
        return struct.unpack('>I', vals[0])[0]


_LOCALE_LANGS = {
    1: 'ara',
    2: 'bul',
    3: 'cat',
    4: 'zho',
    5: 'ces',
    6: 'dan',
    7: 'deu',
    8: 'ell',
    9: 'eng',
    10: 'spa',
    11: 'fin',
    12: 'fra',
    13: 'heb',
    14: 'hun',
    15: 'isl',
    16: 'ita',
    17: 'jpn',
    18: 'kor',
    19: 'nld',
    20: 'nor',
    21: 'pol',
    22: 'por',
    24: 'ron',
    25: 'rus',
    26: 'hrv',
    27: 'slk',
    28: 'sqi',
    29: 'swe',
    30: 'tha',
    31: 'tur',
    34: 'ukr',
    36: 'fas',
    37: 'lav',
    38: 'lit',
    39: 'est',
    42: 'vie',
    54: 'hin',
}


def get_metadata(stream):
    """The metadata (and cover) of a MOBI/AZW/AZW3 file."""
    stream.seek(0)
    raw = stream.read()
    db = PalmDB(raw)
    rec0 = db.record(0)
    mh = MobiHeader(rec0)
    # KF8 joint files have a second MOBI header; the first carries the metadata.
    mi = MetaInformation(mh.title or db.name.decode('cp1252', 'replace') or 'Unknown', ['Unknown'])
    title = mh.first(EXTH_UPDATED_TITLE)
    if title:
        mi.title = title
    authors = []
    for a in mh.text(EXTH_AUTHOR) or []:
        authors.extend(string_to_authors(a))
    if authors:
        mi.authors = authors
    pub = mh.first(EXTH_PUBLISHER)
    if pub:
        mi.publisher = pub
    comments = mh.first(EXTH_DESCRIPTION)
    if comments:
        mi.comments = comments
    isbn = mh.first(EXTH_ISBN)
    if isbn:
        mi.isbn = isbn
    tags = mh.text(EXTH_SUBJECT)
    if tags:
        mi.tags = [t for t in tags if t]
    pubdate = mh.first(EXTH_PUBDATE)
    if pubdate:
        try:
            mi.pubdate = parse_date(pubdate, assume_utc=True)
        except Exception:
            pass
    rights = mh.first(EXTH_RIGHTS)
    if rights:
        mi.rights = rights
    asin = mh.first(EXTH_ASIN) or mh.first(EXTH_ASIN2)
    if asin:
        mi.set_identifier('mobi-asin', asin)
    lang = mh.first(EXTH_LANGUAGE)
    code = canonicalize_lang(lang) if lang else None
    if not code and mh.locale:
        code = _LOCALE_LANGS.get(mh.locale & 0xFF)
    if code:
        mi.languages = [code]
    # Cover: EXTH 201 is the image's offset from the first image record.
    offset = mh.number(EXTH_COVER_OFFSET)
    if offset is None:
        offset = mh.number(EXTH_THUMB_OFFSET)
    if offset is not None and mh.first_image_index and mh.first_image_index != 0xFFFFFFFF:
        idx = mh.first_image_index + offset
        if idx < db.num_records:
            data = db.record(idx)
            fmt = what(None, data)
            if fmt:
                mi.cover_data = (fmt, data)
    return mi


def _build_exth(items, codec):
    body = b''.join(struct.pack('>II', t, len(d) + 8) + d for t, d in items)
    length = 12 + len(body)
    pad = (4 - length % 4) % 4
    return b'EXTH' + struct.pack('>II', length + pad, len(items)) + body + b'\0' * pad


def set_metadata(stream, mi):
    """Write `mi` into the file's EXTH header: title, authors, publisher,
    comments, ISBN, tags, published date, language and rights."""
    stream.seek(0)
    raw = stream.read()
    db = PalmDB(raw)
    rec0 = db.record(0)
    mh = MobiHeader(rec0)
    codec = mh.codec
    replaced = {
        EXTH_AUTHOR,
        EXTH_PUBLISHER,
        EXTH_DESCRIPTION,
        EXTH_ISBN,
        EXTH_SUBJECT,
        EXTH_PUBDATE,
        EXTH_RIGHTS,
        EXTH_LANGUAGE,
        EXTH_UPDATED_TITLE,
    }
    items = [(t, d) for t, d in mh.exth_items if t not in replaced]

    def enc(x):
        return x.encode(codec, 'replace')

    for a in mi.authors or []:
        if a and a != 'Unknown':
            items.append((EXTH_AUTHOR, enc(a)))
    if mi.publisher:
        items.append((EXTH_PUBLISHER, enc(mi.publisher)))
    if mi.comments:
        items.append((EXTH_DESCRIPTION, enc(mi.comments)))
    if mi.isbn:
        items.append((EXTH_ISBN, enc(mi.isbn)))
    for t in mi.tags or []:
        items.append((EXTH_SUBJECT, enc(t)))
    if mi.pubdate is not None and not mi.is_null('pubdate'):
        items.append((EXTH_PUBDATE, enc(mi.pubdate.date().isoformat())))
    if mi.rights:
        items.append((EXTH_RIGHTS, enc(mi.rights)))
    if mi.languages:
        items.append((EXTH_LANGUAGE, enc(mi.languages[0])))
    title = mi.title or mh.title
    items.append((EXTH_UPDATED_TITLE, enc(title)))
    exth = _build_exth(items, codec)
    title_bytes = enc(title)
    head = bytearray(rec0[: mh.exth_offset])
    # Full name follows EXTH; pad to a 4-byte boundary and leave room.
    title_offset = mh.exth_offset + len(exth)
    new_rec0 = bytes(head) + exth + title_bytes + b'\0' * (2 + (4 - (len(title_bytes) + 2) % 4) % 4)
    new_rec0 = bytearray(new_rec0)
    struct.pack_into('>II', new_rec0, 84, title_offset, len(title_bytes))
    struct.pack_into('>I', new_rec0, 128, mh.exth_flags | 0x40)
    new_rec0 = bytes(new_rec0)
    stream.seek(0)
    stream.truncate()
    stream.write(_with_record0(raw, db, new_rec0))
    stream.flush()


def _with_record0(raw, db, new_rec0):
    """The PalmDB bytes with record 0 replaced (the offsets after it shift)."""
    records = db.records()
    records[0] = new_rec0
    header = bytearray(raw[:78])
    out = io.BytesIO()
    offset = 78 + 8 * len(records) + 2
    entries = bytearray()
    for i, rec in enumerate(records):
        attrs = raw[82 + i * 8 : 86 + i * 8]
        entries += struct.pack('>I', offset) + attrs
        offset += len(rec)
    out.write(bytes(header))
    out.write(bytes(entries))
    out.write(b'\0\0')
    for rec in records:
        out.write(rec)
    return out.getvalue()


class MetadataUpdater:
    """Mirrors calibre's `MetadataUpdater`: a MOBI file open for update,
    with its EXTH records (`original_exth_records`, by id) and the ability
    to write a new EXTH block (`create_exth`) or new metadata (`update`).
    Plugins subclass it to set an ASIN, say."""

    def __init__(self, stream):
        self.stream = stream
        stream.seek(0)
        self.data = stream.read()
        self.db = PalmDB(self.data)
        self.type = self.db.ident
        self.nrecs = self.db.num_records
        self.record0 = self.db.record(0)
        if self.type != b'BOOKMOBI' and self.record0[16:20] != b'MOBI':
            raise ValueError('Not a MOBI file')
        self.header = MobiHeader(self.record0)
        self.codec = self.header.codec
        self.title = self.header.title
        self.exth_offset = self.header.exth_offset
        self.have_exth = self.header.has_exth
        self.exth = self.record0[self.exth_offset : self.header.exth_end] if self.have_exth else None
        self.original_exth_records = {}
        self.fetchEXTHFields()

    def fetchEXTHFields(self):
        self.original_exth_records = {}
        for rtype, data in self.header.exth_items:
            self.original_exth_records[rtype] = data
        self.thumbnail_offset = self.header.number(EXTH_THUMB_OFFSET)
        self.cover_offset = self.header.number(EXTH_COVER_OFFSET)

    def record(self, n):
        return self.db.record(n)

    def hexdump(self, src, length=16):
        return ' '.join('%02x' % b for b in src[:length])

    def create_exth(self, new_title=None, exth=None):
        """Rewrite record 0 with `exth` (a complete EXTH block) and the
        title, and write the file back through the stream."""
        title = new_title if new_title is not None else self.title
        if isinstance(title, str):
            title_bytes = title.encode(self.codec, 'replace')
        else:
            title_bytes = title
        if exth is None:
            exth = _build_exth(sorted(self.original_exth_records.items()), self.codec)
        head = bytearray(self.record0[: self.exth_offset])
        title_offset = self.exth_offset + len(exth)
        new_rec0 = bytearray(bytes(head) + exth + title_bytes + b'\0' * (2 + (4 - (len(title_bytes) + 2) % 4) % 4))
        struct.pack_into('>II', new_rec0, 84, title_offset, len(title_bytes))
        struct.pack_into('>I', new_rec0, 128, self.header.exth_flags | 0x40)
        self.record0 = bytes(new_rec0)
        self.data = _with_record0(self.data, self.db, self.record0)
        self.db = PalmDB(self.data)
        self.header = MobiHeader(self.record0)
        self.exth = exth
        self.stream.seek(0)
        self.stream.truncate()
        self.stream.write(self.data)
        self.stream.flush()

    def update(self, mi, asin=None):
        """Write `mi` (and an ASIN, when given) into the file."""
        if asin:
            mi.set_identifier('mobi-asin', asin) if hasattr(mi, 'set_identifier') else None
        set_metadata(self.stream, mi)
        self.stream.seek(0)
        self.__init__(self.stream)


__all__ = ['get_metadata', 'set_metadata', 'PalmDB', 'MobiHeader', 'MetadataUpdater']
