"""calibre's built-in icon names, drawn with Magnitide's icon set (Lucide).

The mapping is resources/images/calibre-icons.json (calibre name without
`.png` → Lucide name) and the SVGs are beside it in images/lucide/,
fetched by scripts/fetch-lucide-icons.py. Two ways in:

- `render(name, size)` gives a QImage, used by `QIcon.ic()` / `icon_from_name`;
- `png_path(name)` gives a PNG file rendered once into the cache folder,
  so `I('config.png')` is a path that exists, as in calibre (plugins do
  `QIcon(I('config.png'))`, `QPixmap(I(...))` and `open(I(...))`).

Icons are drawn in the application palette's text colour (black before
Qt starts), so they follow light and dark themes.
"""

import json
import os

from calibre.constants import cache_dir, resources_location

_mapping = None


def mapping():
    global _mapping
    if _mapping is None:
        _mapping = {}
        try:
            with open(os.path.join(resources_location(), 'images', 'calibre-icons.json'), encoding='utf-8') as f:
                _mapping = json.load(f).get('icons', {})
        except Exception:
            _mapping = {}
    return _mapping


def lucide_name(name):
    """The Lucide icon for a calibre icon name ('config.png', 'config'), or None."""
    if not name:
        return None
    base = name.replace('__', '/')
    base = base.rsplit('/', 1)[-1]
    if base.endswith('.png'):
        base = base[:-4]
    return mapping().get(base)


def svg_path(name):
    lucide = lucide_name(name)
    if not lucide:
        return None
    path = os.path.join(resources_location(), 'images', 'lucide', lucide + '.svg')
    return path if os.path.exists(path) else None


def _colour():
    try:
        from qt.core import QApplication, QGuiApplication, QPalette

        app = QGuiApplication.instance() or QApplication.instance()
        if app is not None:
            return app.palette().color(QPalette.ColorRole.WindowText).name()
    except Exception:
        pass
    return '#000000'


def render(name, size=64, colour=None):
    """A QImage of the icon at `size` pixels, or None."""
    path = svg_path(name)
    if path is None:
        return None
    try:
        from qt.core import QImage, QPainter, QSvgRenderer, Qt

        with open(path, encoding='utf-8') as f:
            svg = f.read()
        renderer = QSvgRenderer(svg.replace('currentColor', colour or _colour()).encode('utf-8'))
        if not renderer.isValid():
            return None
        img = QImage(size, size, QImage.Format.Format_ARGB32)
        img.fill(Qt.GlobalColor.transparent)
        p = QPainter(img)
        renderer.render(p)
        p.end()
        return img
    except Exception:
        return None


def png_path(name, size=64):
    """A PNG of the icon in the cache folder (rendered once per colour), or None."""
    lucide = lucide_name(name)
    if not lucide or svg_path(name) is None:
        return None
    colour = _colour()
    folder = os.path.join(cache_dir(), 'icons')
    path = os.path.join(folder, f'{lucide}-{colour.lstrip("#")}-{size}.png')
    if os.path.exists(path):
        return path
    img = render(name, size, colour)
    if img is None:
        return None
    try:
        os.makedirs(folder, exist_ok=True)
        tmp = path + '.part'
        if not img.save(tmp, 'PNG'):
            return None
        os.replace(tmp, path)
    except Exception:
        return None
    return path


__all__ = ['mapping', 'lucide_name', 'svg_path', 'render', 'png_path']
