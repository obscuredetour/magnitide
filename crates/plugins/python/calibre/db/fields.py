"""Mirrors the class names of calibre's `calibre.db.fields`, which plugins
use in `isinstance` checks against `db.new_api.fields[...]`. Magnitide's
cache keeps its data elsewhere, so these carry no values."""


class Field:
    is_many = False
    is_many_many = False
    is_composite = False

    def __init__(self, name, table=None, bools_are_tristate=True, get_template_functions=None):
        self.name = name
        self.table = table
        self.metadata = getattr(table, 'metadata', {}) if table is not None else {}

    def for_book(self, book_id, default_value=None):
        return default_value


class OneToOneField(Field):
    pass


class CompositeField(OneToOneField):
    is_composite = True

    def __init__(self, name, table=None, bools_are_tristate=True, get_template_functions=None):
        OneToOneField.__init__(self, name, table, bools_are_tristate, get_template_functions)
        self.composite_sort = self.metadata.get('display', {}).get('composite_sort', 'text')
        self.is_multiple = bool(self.metadata.get('is_multiple'))


class OnDeviceField(OneToOneField):
    pass


class LazySortMap:
    pass


class ManyToOneField(Field):
    is_many = True


class ManyToManyField(Field):
    is_many = True
    is_many_many = True


class IdentifiersField(ManyToManyField):
    pass


class AuthorsField(ManyToManyField):
    pass


class FormatsField(ManyToManyField):
    pass


class LazySeriesIndexMap:
    pass


class SeriesField(ManyToOneField):
    pass


class TagsField(ManyToManyField):
    pass


def create_field(name, table, bools_are_tristate, get_template_functions):
    return Field(name, table, bools_are_tristate, get_template_functions)


__all__ = [
    'Field',
    'OneToOneField',
    'CompositeField',
    'OnDeviceField',
    'ManyToOneField',
    'ManyToManyField',
    'IdentifiersField',
    'AuthorsField',
    'FormatsField',
    'SeriesField',
    'TagsField',
    'create_field',
]
