# Derived from calibre's src/calibre/utils/img.py (calibre 9.15).
# Copyright: 2015-2019, Kovid Goyal <kovid at kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.utils.img`: image helpers over Qt's QImage
(calibre uses its own C extension for some; here it's all Qt)."""

import os

from calibre import fit_image
from qt.core import QBuffer, QByteArray, QColor, QImage, QImageReader, QIODevice, QPainter, Qt, QTransform


class NotImage(ValueError):
    pass


def null_image():
    return QImage()


def image_from_data(data):
    if isinstance(data, QImage):
        return data
    i = QImage()
    if not i.loadFromData(data):
        raise NotImage('Not a valid image (failed to load with Qt)')
    return i


def image_from_path(path):
    with open(path, 'rb') as f:
        return image_from_data(f.read())


def image_from_x(x):
    if isinstance(x, (bytes, bytearray)):
        return image_from_data(bytes(x))
    if isinstance(x, str):
        return image_from_path(x)
    if isinstance(x, QImage):
        return x
    if hasattr(x, 'read'):
        return image_from_data(x.read())
    raise TypeError('Unknown image src type: %s' % type(x))


def image_and_format_from_data(data):
    ba = QByteArray(data)
    buf = QBuffer(ba)
    buf.open(QIODevice.OpenModeFlag.ReadOnly)
    r = QImageReader(buf)
    fmt = bytes(r.format()).decode('utf-8', 'replace')
    return r.read(), fmt


def image_to_data(
    img, compression_quality=95, fmt='JPEG', png_compression_level=9, jpeg_optimized=True, jpeg_progressive=False
):
    fmt = fmt.upper()
    if fmt == 'JPG':
        fmt = 'JPEG'
    if fmt == 'JPEG' and img.hasAlphaChannel():
        img = blend_image(img)
    ba = QByteArray()
    buf = QBuffer(ba)
    buf.open(QIODevice.OpenModeFlag.WriteOnly)
    quality = compression_quality if fmt == 'JPEG' else (100 - png_compression_level * 10 if fmt == 'PNG' else -1)
    if not img.save(buf, fmt, quality):
        raise ValueError('Failed to export image as ' + fmt)
    return bytes(ba)


def resize_image(img, width, height):
    return img.scaled(
        int(width), int(height), Qt.AspectRatioMode.IgnoreAspectRatio, Qt.TransformationMode.SmoothTransformation
    )


def resize_to_fit(img, width, height):
    img = image_from_data(img)
    resize_needed, nw, nh = fit_image(img.width(), img.height(), width, height)
    if resize_needed:
        img = resize_image(img, nw, nh)
    return resize_needed, img


def clone_image(img):
    return QImage(img)


def scale_image(data, width=60, height=80, compression_quality=70, as_png=False, preserve_aspect_ratio=True):
    """Scale an image, returning (width, height, data)."""
    img = image_from_data(data)
    if preserve_aspect_ratio:
        scaled, nwidth, nheight = fit_image(img.width(), img.height(), width, height)
        if scaled:
            img = resize_image(img, nwidth, nheight)
    else:
        if img.width() != width or img.height() != height:
            img = resize_image(img, width, height)
    fmt = 'PNG' if as_png else 'JPEG'
    return img.width(), img.height(), image_to_data(img, compression_quality=compression_quality, fmt=fmt)


def crop_image(img, x, y, width, height):
    return image_from_data(img).copy(int(x), int(y), int(width), int(height))


def blend_image(img, bgcolor='#ffffff'):
    img = image_from_data(img)
    nimg = QImage(img.size(), QImage.Format.Format_RGB32)
    nimg.fill(QColor(bgcolor))
    p = QPainter(nimg)
    p.drawImage(0, 0, img)
    p.end()
    return nimg


def overlay_image(img, canvas=None, left=0, top=0):
    if canvas is None:
        canvas = QImage(img.size(), QImage.Format.Format_RGB32)
        canvas.fill(Qt.GlobalColor.white)
    left, top = int(left), int(top)
    p = QPainter(canvas)
    p.drawImage(left, top, img)
    p.end()
    return canvas


def add_borders_to_image(img_data, left=0, top=0, right=0, bottom=0, border_color='#ffffff', fmt='jpg'):
    img = image_from_data(img_data)
    if not (left > 0 or right > 0 or top > 0 or bottom > 0):
        return img_data
    canvas = QImage(img.width() + left + right, img.height() + top + bottom, QImage.Format.Format_RGB32)
    canvas.fill(QColor(border_color))
    overlay_image(img, canvas, left, top)
    return image_to_data(canvas, fmt=fmt)


def remove_borders_from_image(img, fuzz=None):
    return image_from_data(img)


def rotate_image(img, degrees):
    t = QTransform()
    t.rotate(degrees)
    return image_from_data(img).transformed(t)


def flip_image(img, horizontal=False, vertical=False):
    return image_from_data(img).mirrored(horizontal, vertical)


def grayscale_image(img):
    return image_from_data(img).convertToFormat(QImage.Format.Format_Grayscale8)


def despeckle_image(img):
    return image_from_data(img)


def normalize_image(img):
    return image_from_data(img)


def quantize_image(img, max_colors=256, dither=True, palette=''):
    return image_from_data(img)


def gaussian_sharpen_image(img, radius=0, sigma=3, high_quality=True):
    return image_from_data(img)


def gaussian_blur_image(img, radius=-1, sigma=3):
    return image_from_data(img)


def oil_paint_image(img, radius=-1, high_quality=True):
    return image_from_data(img)


def set_image_opacity(img, alpha=0.5):
    img = image_from_data(img).convertToFormat(QImage.Format.Format_ARGB32)
    out = QImage(img.size(), QImage.Format.Format_ARGB32)
    out.fill(Qt.GlobalColor.transparent)
    p = QPainter(out)
    p.setOpacity(alpha)
    p.drawImage(0, 0, img)
    p.end()
    return out


def texture_image(canvas, texture):
    return canvas


def save_image(img, path, **kw):
    fmt = path.rpartition('.')[-1].upper() or 'JPEG'
    with open(path, 'wb') as f:
        f.write(image_to_data(image_from_data(img), fmt=fmt, **kw))


def save_cover_data_to(
    data,
    path=None,
    bgcolor='#ffffff',
    resize_to=None,
    compression_quality=90,
    minify_to=None,
    grayscale=False,
    eink=False,
    letterbox=False,
    letterbox_color='#000000',
    data_fmt='jpeg',
):
    """Save (or return) cover data as JPEG/PNG, resized and letterboxed as
    asked; `resize_to` and `minify_to` are (width, height)."""
    fmt = (path.rpartition('.')[-1] if path else data_fmt).lower()
    fmt = 'JPEG' if fmt in ('jpg', 'jpeg') else fmt.upper()
    img = image_from_data(data)
    orig_fmt = None
    if resize_to is not None:
        img = resize_image(img, resize_to[0], resize_to[1])
    elif minify_to is not None:
        scaled, w, h = fit_image(img.width(), img.height(), minify_to[0], minify_to[1])
        if scaled:
            img = resize_image(img, w, h)
    if grayscale:
        img = grayscale_image(img)
    if letterbox and resize_to is not None:
        canvas = QImage(int(resize_to[0]), int(resize_to[1]), QImage.Format.Format_RGB32)
        canvas.fill(QColor(letterbox_color))
        img = overlay_image(img, canvas, (canvas.width() - img.width()) // 2, (canvas.height() - img.height()) // 2)
    if img.hasAlphaChannel() and fmt == 'JPEG':
        img = blend_image(img, bgcolor)
    out = image_to_data(img, compression_quality=compression_quality, fmt=fmt)
    if path is None:
        return out
    with open(path, 'wb') as f:
        f.write(out)
    return orig_fmt


def identify(src):
    """(fmt, width, height) of an image; (None, -1, -1) when it isn't one."""
    if isinstance(src, str) and os.path.exists(src):
        with open(src, 'rb') as f:
            src = f.read()
    elif hasattr(src, 'read'):
        src = src.read()
    try:
        img, fmt = image_and_format_from_data(src)
    except Exception:
        return None, -1, -1
    if img.isNull():
        return None, -1, -1
    return fmt.lower() or None, img.width(), img.height()


def encode_jpeg(file_path, quality=80):
    with open(file_path, 'rb') as f:
        data = f.read()
    return image_to_data(image_from_data(data), compression_quality=quality, fmt='JPEG')


def optimize_jpeg(file_path):
    return None


def optimize_png(file_path, level=7):
    return None


__all__ = [n for n in dir() if not n.startswith('_')]
