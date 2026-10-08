"""Mirrors calibre's `calibre.utils.imghdr`: recognizing image formats and
reading their sizes without decoding them."""

import struct
from io import BytesIO

HSIZE = 120


def what(file, h=None):
    """The format of an image file or bytes: 'jpeg', 'png', 'gif', 'bmp',
    'webp', 'tiff', 'svg', 'ico' or None."""
    if h is None:
        if isinstance(file, (str, bytes)) and not (isinstance(file, bytes) and len(file) > HSIZE):
            try:
                with open(file, 'rb') as f:
                    h = f.read(HSIZE)
            except Exception:
                return None
        elif isinstance(file, bytes):
            h = file[:HSIZE]
        else:
            location = file.tell()
            h = file.read(HSIZE)
            file.seek(location)
    if isinstance(h, str):
        h = h.encode('utf-8', 'replace')
    for tf in tests:
        res = tf(h)
        if res:
            return res
    if h.lstrip()[:5].lower() in (b'<?xml', b'<svg '):
        return 'svg'
    return None


def test_jpeg(h):
    if h[6:10] in (b'JFIF', b'Exif') or h[:2] == b'\xff\xd8':
        return 'jpeg'


def test_png(h):
    if h[:8] == b'\211PNG\r\n\032\n':
        return 'png'


def test_gif(h):
    if h[:6] in (b'GIF87a', b'GIF89a'):
        return 'gif'


def test_tiff(h):
    if h[:2] in (b'MM', b'II') and h[2:4] in (b'\x00\x2a', b'\x2a\x00'):
        return 'tiff'


def test_webp(h):
    if h[:4] == b'RIFF' and h[8:12] == b'WEBP':
        return 'webp'


def test_bmp(h):
    if h[:2] == b'BM':
        return 'bmp'


def test_ico(h):
    if h[:4] == b'\x00\x00\x01\x00':
        return 'ico'


def test_jxr(h):
    if h[:3] == b'II\xbc':
        return 'jxr'


tests = [test_jpeg, test_png, test_gif, test_tiff, test_webp, test_bmp, test_ico, test_jxr]


def identify(src):
    """(format, width, height) of an image file, path or bytes; -1 for
    unknown dimensions."""
    if isinstance(src, str):
        with open(src, 'rb') as f:
            data = f.read()
    elif isinstance(src, bytes):
        data = src
    else:
        pos = src.tell()
        data = src.read()
        src.seek(pos)
    fmt = what(None, data)
    width = height = -1
    try:
        if fmt == 'png':
            width, height = struct.unpack('>II', data[16:24])
        elif fmt == 'gif':
            width, height = struct.unpack('<HH', data[6:10])
        elif fmt == 'bmp':
            width, height = struct.unpack('<ii', data[18:26])
            height = abs(height)
        elif fmt == 'webp':
            if data[12:16] == b'VP8 ':
                width, height = struct.unpack('<HH', data[26:30])
                width &= 0x3FFF
                height &= 0x3FFF
            elif data[12:16] == b'VP8L':
                b = data[21:25]
                width = 1 + (((b[1] & 0x3F) << 8) | b[0])
                height = 1 + (((b[3] & 0xF) << 10) | (b[2] << 2) | ((b[1] & 0xC0) >> 6))
            elif data[12:16] == b'VP8X':
                width = 1 + int.from_bytes(data[24:27], 'little')
                height = 1 + int.from_bytes(data[27:30], 'little')
        elif fmt == 'jpeg':
            s = BytesIO(data)
            s.read(2)
            b = s.read(1)
            while b and ord(b) != 0xDA:
                while ord(b) != 0xFF:
                    b = s.read(1)
                while ord(b) == 0xFF:
                    b = s.read(1)
                if 0xC0 <= ord(b) <= 0xC3 or 0xC5 <= ord(b) <= 0xC7 or 0xC9 <= ord(b) <= 0xCB or 0xCD <= ord(b) <= 0xCF:
                    s.read(3)
                    height, width = struct.unpack('>HH', s.read(4))
                    break
                s.read(int(struct.unpack('>H', s.read(2))[0]) - 2)
                b = s.read(1)
        elif fmt == 'tiff':
            from PIL import Image

            with Image.open(BytesIO(data)) as im:
                width, height = im.size
    except Exception:
        width = height = -1
    return fmt, int(width), int(height)
