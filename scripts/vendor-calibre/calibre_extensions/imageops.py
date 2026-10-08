"""calibre_extensions.imageops: image operations over QImage (no-ops where Qt has no equivalent)."""

from PyQt6.QtGui import QImage, QTransform


def despeckle(img):
    return QImage(img)


def normalize(img):
    return QImage(img)


def quantize(img, max_colors, dither, palette):
    return img.convertToFormat(QImage.Format.Format_Indexed8)


def remove_borders(img, fuzz):
    return QImage(img)


def oil_paint(img, radius, high_quality):
    return QImage(img)


def gaussian_blur(img, radius, sigma):
    return QImage(img)


def gaussian_sharpen(img, radius, sigma, high_quality):
    return QImage(img)


def set_opacity(img, alpha):
    from PyQt6.QtCore import Qt
    from PyQt6.QtGui import QPainter

    out = QImage(img.size(), QImage.Format.Format_ARGB32)
    out.fill(Qt.GlobalColor.transparent)
    p = QPainter(out)
    p.setOpacity(alpha)
    p.drawImage(0, 0, img)
    p.end()
    return out


def overlay(img, canvas, left, top):
    from PyQt6.QtGui import QPainter

    p = QPainter(canvas)
    p.drawImage(left, top, img)
    p.end()


def texture_image(canvas, texture):
    return canvas


def ordered_dither(img):
    return img.convertToFormat(QImage.Format.Format_Mono)


def scale_image(img, w, h):
    return img.scaled(w, h)


def rotate(img, degrees):
    t = QTransform()
    t.rotate(degrees)
    return img.transformed(t)
