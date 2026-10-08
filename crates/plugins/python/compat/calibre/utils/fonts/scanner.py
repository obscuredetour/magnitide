"""Mirrors calibre's `font_scanner` over Qt's font database: families and
the faces in each. File paths of font files aren't known through Qt, so
`get_font_data` raises `NoFonts` unless a path was found."""


class NoFonts(ValueError):
    pass


class Scanner:
    def __init__(self):
        self._families = None

    def find_font_families(self):
        from qt.core import QFontDatabase

        if self._families is None:
            self._families = sorted(QFontDatabase.families())
        return list(self._families)

    def fonts_for_family(self, family):
        from qt.core import QFontDatabase

        ans = []
        for style in QFontDatabase.styles(family):
            weight = QFontDatabase.weight(family, style)
            ans.append(
                {
                    'font-family': family,
                    'font-weight': 'bold' if weight >= 700 else 'normal',
                    'font-style': 'italic' if QFontDatabase.italic(family, style) else 'normal',
                    'font-stretch': 'normal',
                    'full_name': f'{family} {style}',
                    'path': None,
                }
            )
        if not ans:
            raise NoFonts(f'No fonts found for the family {family}')
        return ans

    def legacy_fonts_for_family(self, family):
        return self.fonts_for_family(family)

    def alt_font_family(self, family):
        return None

    def get_font_data(self, font_or_path):
        path = font_or_path.get('path') if isinstance(font_or_path, dict) else font_or_path
        if not path:
            raise NoFonts('The font file is not known')
        with open(path, 'rb') as f:
            return f.read()

    def find_font_for_text(self, text, allowed_families=None, preferred_families=None, preferred_bold_weight=700):
        return None

    def do_scan(self):
        self._families = None
        self.find_font_families()


font_scanner = Scanner()

__all__ = ['font_scanner', 'NoFonts', 'Scanner']
