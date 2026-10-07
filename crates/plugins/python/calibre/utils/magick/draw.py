"""Mirrors calibre's old `calibre.utils.magick.draw`: canvases, cover
thumbnails and saving, over `calibre.utils.img`."""

from calibre.utils.img import identify, image_from_data, image_to_data, save_cover_data_to, scale_image
from calibre.utils.magick import Image, create_canvas


def _identify_data(data):
    fmt, width, height = identify(data)
    if fmt is None:
        raise ValueError('Not an image')
    return width, height, fmt


def identify_data(data):
    return _identify_data(data)


def identify_path(path):
    with open(path, 'rb') as f:
        return _identify_data(f.read())


def thumbnail(
    data, width=120, height=120, bgcolor='#ffffff', fmt='jpg', preserve_aspect_ratio=True, compression_quality=70
):
    fmt = 'JPEG' if fmt.lower() in ('jpg', 'jpeg') else fmt.upper()
    w, h, out = scale_image(
        data,
        width=width,
        height=height,
        compression_quality=compression_quality,
        as_png=fmt == 'PNG',
        preserve_aspect_ratio=preserve_aspect_ratio,
    )
    return w, h, out


def add_borders_to_image(img_data, left=0, top=0, right=0, bottom=0, border_color='#ffffff', fmt='jpg'):
    from calibre.utils.img import add_borders_to_image as _add

    return _add(img_data, left=left, top=top, right=right, bottom=bottom, border_color=border_color, fmt=fmt)


def minify_image(data, minify_to=(1200, 1600), preserve_aspect_ratio=True):
    _w, _h, out = scale_image(
        data, width=minify_to[0], height=minify_to[1], preserve_aspect_ratio=preserve_aspect_ratio
    )
    return out


def draw_centered_text(img, dw, text, top_margin=10):
    return img


__all__ = [
    'Image',
    'create_canvas',
    'identify_data',
    'identify_path',
    'thumbnail',
    'add_borders_to_image',
    'save_cover_data_to',
    'minify_image',
    'image_from_data',
    'image_to_data',
    'draw_centered_text',
]
