"""Mirrors calibre's `calibre.customize.profiles`: the input and output
profiles (device screen sizes and so on) as data; conversion isn't run."""

from calibre.customize import Plugin


class Profile(Plugin):
    supported_platforms = ['windows', 'osx', 'linux']
    can_be_disabled = False
    type = 'Profile'

    screen_size = (1600, 1200)
    dpi = 100
    fbase = 12
    fsizes = [5, 7, 9, 12, 13.5, 17, 20]
    short_name = 'default'
    comic_screen_size = (584, 754)
    width = 1600
    height = 1200
    ratio = 1.333
    is_generic = False

    @classmethod
    def description(cls):
        return cls.__doc__ or ''


class InputProfile(Profile):
    """This profile tries to provide reasonable defaults and is useful if you know nothing about the input document."""

    author = 'Kovid Goyal'
    name = 'Default Input Profile'
    short_name = 'default'


class OutputProfile(Profile):
    """This profile tries to provide reasonable defaults and is useful if you want to produce a document intended to be read at a computer or on a range of devices."""

    author = 'Kovid Goyal'
    name = 'Default Output Profile'
    short_name = 'default'
    touchscreen = False
    touchscreen_news_css = ''
    supports_mobi_indexing = False
    periodical_date_in_title = True
    missing_char = '?'
    ratings_char = '*'
    empty_ratings_char = ' '
    read_char = '+'
    unsupported_unicode_chars = []
    mobi_ems_per_blockquote = 1.0

    @classmethod
    def tags_to_string(cls, tags):
        from calibre import prepare_string_for_xml

        return prepare_string_for_xml(', '.join(tags))


def _profile(base, name, short, size, dpi=None, fbase=None, fsizes=None, doc=''):
    attrs = dict(
        name=name,
        short_name=short,
        screen_size=size,
        width=size[0],
        height=size[1],
        __doc__=doc,
        ratio=round(size[0] / size[1], 3) if size[1] else 1.333,
    )
    if dpi:
        attrs['dpi'] = dpi
    if fbase:
        attrs['fbase'] = fbase
    if fsizes:
        attrs['fsizes'] = fsizes
    return type(name.replace(' ', ''), (base,), attrs)


input_profiles = [
    InputProfile,
    _profile(
        InputProfile,
        'Sony Reader',
        'sony',
        (584, 754),
        168.451,
        12,
        [7.5, 9, 10, 12, 15.5, 20, 22, 24],
        'This profile is intended for the SONY PRS line.',
    ),
    _profile(
        InputProfile,
        'Kindle',
        'kindle',
        (525, 640),
        168.451,
        16,
        [12, 12, 14, 16, 18, 20, 22, 24],
        'This profile is intended for the Amazon Kindle.',
    ),
    _profile(
        InputProfile,
        'Mobipocket Books',
        'mobipocket',
        (600, 800),
        96,
        18,
        [14, 14, 16, 18, 20, 22, 24, 26],
        'This profile is intended for the Mobipocket books.',
    ),
    _profile(
        InputProfile,
        'Kobo Reader',
        'kobo',
        (536, 710),
        168.451,
        12,
        [7.5, 9, 10, 12, 15.5, 20, 22, 24],
        'This profile is intended for the Kobo Reader.',
    ),
    _profile(
        InputProfile,
        'Nook',
        'nook',
        (600, 770),
        167,
        16,
        [12, 12, 14, 16, 18, 20, 22, 24],
        'This profile is intended for the B&N Nook.',
    ),
]

output_profiles = [
    OutputProfile,
    _profile(
        OutputProfile,
        'Generic e-ink',
        'generic_eink',
        (590, 775),
        168.451,
        12,
        [7.5, 9, 10, 12, 15.5, 20, 22, 24],
        'Suitable for use with any e-ink device',
    ),
    _profile(
        OutputProfile,
        'Generic e-ink large',
        'generic_eink_large',
        (600, 999),
        168.451,
        12,
        [7.5, 9, 10, 12, 15.5, 20, 22, 24],
        'Suitable for use with any large screen e-ink device',
    ),
    _profile(
        OutputProfile,
        'Generic e-ink HD',
        'generic_eink_hd',
        (1072, 1430),
        300,
        12,
        [7.5, 9, 10, 12, 15.5, 20, 22, 24],
        'Suitable for use with any modern high resolution e-ink device',
    ),
    _profile(
        OutputProfile,
        'Sony Reader',
        'sony',
        (590, 775),
        168.451,
        12,
        [7.5, 9, 10, 12, 15.5, 20, 22, 24],
        'This profile is intended for the SONY PRS line.',
    ),
    _profile(
        OutputProfile,
        'Kindle',
        'kindle',
        (525, 640),
        168.451,
        16,
        [12, 12, 14, 16, 18, 20, 22, 24],
        'This profile is intended for the Amazon Kindle.',
    ),
    _profile(
        OutputProfile,
        'Kindle Fire',
        'kindle_fire',
        (570, 1016),
        169,
        16,
        [12, 12, 14, 16, 18, 20, 22, 24],
        'This profile is intended for the Amazon Kindle Fire.',
    ),
    _profile(
        OutputProfile,
        'Kindle PaperWhite',
        'kindle_pw',
        (658, 940),
        212,
        16,
        [12, 12, 14, 16, 18, 20, 22, 24],
        'This profile is intended for the Amazon Kindle PaperWhite 1 and 2',
    ),
    _profile(
        OutputProfile,
        'Kindle PaperWhite 3',
        'kindle_pw3',
        (1072, 1430),
        300,
        16,
        [12, 12, 14, 16, 18, 20, 22, 24],
        'This profile is intended for the Amazon Kindle PaperWhite 3 and above',
    ),
    _profile(
        OutputProfile,
        'Kindle Voyage',
        'kindle_voyage',
        (1080, 1430),
        300,
        16,
        [12, 12, 14, 16, 18, 20, 22, 24],
        'This profile is intended for the Amazon Kindle Voyage',
    ),
    _profile(
        OutputProfile,
        'Kindle Oasis',
        'kindle_oasis',
        (1264, 1680),
        300,
        16,
        [12, 12, 14, 16, 18, 20, 22, 24],
        'This profile is intended for the Amazon Kindle Oasis 2017 and above',
    ),
    _profile(
        OutputProfile,
        'Kindle Scribe',
        'kindle_scribe',
        (1860, 2480),
        300,
        16,
        [12, 12, 14, 16, 18, 20, 22, 24],
        'This profile is intended for the Amazon Kindle Scribe',
    ),
    _profile(
        OutputProfile,
        'Kobo Reader',
        'kobo',
        (536, 710),
        168.451,
        12,
        [7.5, 9, 10, 12, 15.5, 20, 22, 24],
        'This profile is intended for the Kobo Reader.',
    ),
    _profile(
        OutputProfile,
        'Nook',
        'nook',
        (600, 730),
        167,
        16,
        [12, 12, 14, 16, 18, 20, 22, 24],
        'This profile is intended for the B&N Nook.',
    ),
    _profile(
        OutputProfile,
        'Nook HD+',
        'nook_hd_plus',
        (1280, 1920),
        299,
        16,
        [12, 12, 14, 16, 18, 20, 22, 24],
        'Profile for the Nook HD+',
    ),
    _profile(
        OutputProfile,
        'Tablet',
        'tablet',
        (1280, 1920),
        299,
        16,
        [12, 12, 14, 16, 18, 20, 22, 24],
        'Intended for generic tablet devices, does no resizing of images',
    ),
    _profile(
        OutputProfile,
        'iPad',
        'ipad',
        (768, 1024),
        132,
        16,
        [12, 12, 14, 16, 18, 20, 22, 24],
        'Intended for the iPad and similar devices',
    ),
    _profile(
        OutputProfile,
        'iPad 3',
        'ipad3',
        (1536, 2048),
        264,
        16,
        [12, 12, 14, 16, 18, 20, 22, 24],
        'Intended for the iPad 3 and similar devices',
    ),
    _profile(
        OutputProfile,
        'Mobipocket Books',
        'mobipocket',
        (600, 800),
        96,
        18,
        [14, 14, 16, 18, 20, 22, 24, 26],
        'This profile is intended for the Mobipocket books.',
    ),
]
for _p in output_profiles:
    if _p.short_name in (
        'kindle',
        'kindle_fire',
        'kindle_pw',
        'kindle_pw3',
        'kindle_voyage',
        'kindle_oasis',
        'kindle_scribe',
    ):
        _p.supports_mobi_indexing = True
        _p.ratings_char = '★'
        _p.empty_ratings_char = '☆'
del _p
