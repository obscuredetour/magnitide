"""Mirrors calibre's old `calibre.utils.magick` (the ImageMagick-style
`Image` class plugins like Generate Cover still use), over Qt's QImage."""

from calibre.utils.img import image_from_data, image_to_data, resize_image
from qt.core import QColor, QImage, QPainter, Qt


class Image:
    def __init__(self):
        self.img = QImage()
        self.format = 'png'
        self.quality = 90

    # -- loading and saving --

    def open(self, path):
        with open(path, 'rb') as f:
            self.load(f.read())
        self.format = path.rpartition('.')[-1].lower() or self.format

    def load(self, data):
        self.img = image_from_data(data)

    def create_canvas(self, width, height, bgcolor):
        self.img = QImage(int(width), int(height), QImage.Format.Format_ARGB32)
        self.img.fill(QColor(bgcolor))

    def export(self, fmt):
        fmt = fmt.lower()
        return image_to_data(
            self.img, compression_quality=self.quality, fmt='JPEG' if fmt in ('jpg', 'jpeg') else fmt.upper()
        )

    def save(self, path, fmt=None):
        fmt = fmt or path.rpartition('.')[-1]
        with open(path, 'wb') as f:
            f.write(self.export(fmt))

    def copy(self):
        ans = Image()
        ans.img = QImage(self.img)
        ans.format = self.format
        return ans

    # -- geometry --

    @property
    def size(self):
        return self.img.width(), self.img.height()

    @size.setter
    def size(self, value):
        self.img = resize_image(self.img, value[0], value[1])

    def thumbnail(self, width, height):
        from calibre import fit_image

        scaled, w, h = fit_image(self.img.width(), self.img.height(), width, height)
        if scaled:
            self.img = resize_image(self.img, w, h)

    def crop(self, width, height, left, top):
        self.img = self.img.copy(int(left), int(top), int(width), int(height))

    def rotate(self, degrees):
        from qt.core import QTransform

        t = QTransform()
        t.rotate(degrees)
        self.img = self.img.transformed(t)

    def trim(self, fuzz):
        pass

    def set_border_color(self, color):
        self.border_color = color

    def normalize(self):
        pass

    def despeckle(self):
        pass

    def quantize(self, number_colors, colorspace='RGB', treedepth=0, dither=True, measure_error=False):
        pass

    def set_compression_quality(self, quality):
        self.quality = quality

    def set_page(self, *a):
        pass

    def flip(self, vertical=False):
        self.img = self.img.mirrored(not vertical, vertical)

    @property
    def colorspace(self):
        return 'RGB'

    @colorspace.setter
    def colorspace(self, v):
        pass

    @property
    def type(self):
        return 'TrueColorMatte' if self.img.hasAlphaChannel() else 'TrueColor'

    @type.setter
    def type(self, v):
        pass

    def identify(self, data):
        self.load(data)
        return self.size

    def compose(self, img, left=0, top=0, operation='OverCompositeOp'):
        """Draw `img` (an Image) onto this one at (left, top)."""
        src = img.img if isinstance(img, Image) else image_from_data(img)
        if self.img.format() != QImage.Format.Format_ARGB32:
            self.img = self.img.convertToFormat(QImage.Format.Format_ARGB32)
        p = QPainter(self.img)
        p.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform)
        p.drawImage(int(left), int(top), src)
        p.end()

    def texture(self, img):
        pass

    def to_qimage(self):
        return QImage(self.img)

    def from_qimage(self, qimage):
        self.img = QImage(qimage)


def create_canvas(width, height, bgcolor='#ffffff'):
    canvas = Image()
    canvas.create_canvas(int(width), int(height), bgcolor)
    return canvas


class PixelWand:
    pass


class DrawingWand:
    def __init__(self):
        self.font = None
        self.font_size = 12
        self.stroke_color = '#000000'
        self.fill_color = '#000000'
        self.text_antialias = True
        self.gravity = 'NorthWestGravity'


__all__ = ['Image', 'create_canvas', 'PixelWand', 'DrawingWand', 'Qt']
