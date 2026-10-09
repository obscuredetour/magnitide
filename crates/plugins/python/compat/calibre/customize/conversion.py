# Derived from calibre's src/calibre/customize/conversion.py (calibre 9.15).
# Copyright: Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors the class shells of calibre's `calibre.customize.conversion`:
`InputFormatPlugin`, `OutputFormatPlugin` and `OptionRecommendation`, so
conversion plugins load and are recognized. Magnitide doesn't run calibre's
conversion pipeline, so these plugins are installed as "not supported"."""

from calibre.customize import Plugin


class ConversionOption:
    def __init__(self, name=None, help=None, long_switch=None, short_switch=None, choices=None):
        self.name = name
        self.help = help
        self.long_switch = long_switch
        self.short_switch = short_switch
        self.choices = choices
        if self.long_switch is None:
            self.long_switch = self.name.replace('_', '-')
        self.validate_parameters()

    def validate_parameters(self):
        if not self.name:
            raise ValueError('Option must have a name')
        if not self.help:
            raise ValueError('Option must have a help text')

    def __hash__(self):
        return hash(self.name)

    def __eq__(self, other):
        return hash(self) == hash(other)

    def clone(self):
        return ConversionOption(
            name=self.name,
            help=self.help,
            long_switch=self.long_switch,
            short_switch=self.short_switch,
            choices=self.choices,
        )


class OptionRecommendation:
    LOW = 1
    MED = 2
    HIGH = 3

    def __init__(self, recommended_value=None, level=LOW, **kwargs):
        self.level = level
        self.recommended_value = recommended_value
        self.option = kwargs.pop('option', None)
        if self.option is None:
            self.option = ConversionOption(**kwargs)
        self.validate_parameters()

    @property
    def help(self):
        return self.option.help

    def clone(self):
        return OptionRecommendation(
            recommended_value=self.recommended_value, level=self.level, option=self.option.clone()
        )

    def validate_parameters(self):
        if self.option.choices and self.recommended_value not in self.option.choices:
            raise ValueError('OpRec: %s: Recommended value not in choices' % self.option.name)
        if not (isinstance(self.recommended_value, (int, float, str, bool, type(None)))):
            raise ValueError(
                'OpRec: %s:' % self.option.name + repr(self.recommended_value) + ' is not a string or a number'
            )


class DummyReporter:
    def __init__(self):
        self.cancel_requested = False

    def __call__(self, percent, msg=''):
        pass


class InputFormatPlugin(Plugin):
    """A conversion input plugin. Not run by Magnitide."""

    type = 'Conversion input'
    can_be_disabled = False
    supported_platforms = ['windows', 'osx', 'linux']
    file_types = set()
    is_image_collection = False
    core_usage = 1
    for_viewer = False
    output_encoding = 'utf-8'
    common_options = set()
    options = set()
    recommendations = set()

    def __init__(self, *args):
        Plugin.__init__(self, *args)
        self.report_progress = DummyReporter()

    def get_images(self):
        raise NotImplementedError()

    def convert(self, stream, options, file_ext, log, accelerators):
        raise NotImplementedError("Magnitide doesn't run calibre's conversion pipeline")

    def __call__(self, stream, options, file_ext, log, accelerators, output_dir):
        raise NotImplementedError("Magnitide doesn't run calibre's conversion pipeline")

    def postprocess_book(self, oeb, opts, log):
        pass

    def specialize(self, oeb, opts, log, output_fmt):
        pass

    def gui_configuration_widget(self, parent, get_option_by_name, get_option_help, db, book_id=None):
        return None


class OutputFormatPlugin(Plugin):
    """A conversion output plugin. Not run by Magnitide."""

    type = 'Conversion output'
    can_be_disabled = False
    supported_platforms = ['windows', 'osx', 'linux']
    file_type = None
    common_options = set()
    options = set()
    recommendations = set()

    def __init__(self, *args):
        Plugin.__init__(self, *args)
        self.report_progress = DummyReporter()

    def convert(self, oeb_book, output, input_plugin, opts, log):
        raise NotImplementedError("Magnitide doesn't run calibre's conversion pipeline")

    @property
    def is_periodical(self):
        return False

    def specialize_options(self, log, opts, input_fmt):
        pass

    def specialize_css_for_output(self, log, opts, item, stylizer):
        pass

    def gui_configuration_widget(self, parent, get_option_by_name, get_option_help, db, book_id=None):
        return None
