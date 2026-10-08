"""Mirrors calibre's `CreateNewCustomColumn`, the API plugins use to add a
custom column (Count Pages, Goodreads Sync). calibre needs a restart after;
Magnitide creates the column in the shadow library at once, and it reaches
the library when the plugin is done, so `must_restart()` is False."""

from enum import Enum


class CreateNewCustomColumn:
    class Result(Enum):
        COLUMN_ADDED = 0
        CANCELED = 1
        INVALID_KEY = 2
        DUPLICATE_KEY = 3
        INVALID_TYPE = 4
        INVALID_IS_MULTIPLE = 5
        INVALID_DISPLAY = 6
        EXCEPTION_RAISED = 7
        MUST_RESTART = 8
        BAD_COLUMN_HEADING = 9

    column_types = {
        'bool': 'Yes/No',
        'comments': 'Long text',
        'composite': 'Column built from other columns',
        'datetime': 'Date',
        'enumeration': 'Text, but with a fixed set of permitted values',
        'float': 'Floating point numbers',
        'int': 'Integers',
        'rating': 'Ratings',
        'series': 'Text column for keeping series-like information',
        'text': 'Text',
        '*text': 'Comma separated text, like tags',
        'text*': 'Comma separated text, like tags',
    }

    def __init__(self, gui):
        self.gui = gui
        self.restart_required = False
        self.db = gui.library_view.model().db
        self.created_columns = {}

    def create_column(
        self,
        lookup_name,
        column_heading,
        datatype,
        is_multiple,
        display={},
        generate_unused_lookup_name=False,
        freeze_lookup_name=True,
    ):
        """Create the column (no dialog: the arguments are the column).
        Returns (Result, lookup name or message)."""
        import re

        if not lookup_name or not lookup_name.startswith('#'):
            return self.Result.INVALID_KEY, _('The lookup name must begin with a "#"')
        if not re.match(r'^#[a-z0-9_]+$', lookup_name) or re.match(r'^#\d', lookup_name):
            return self.Result.INVALID_KEY, _('The lookup name contains invalid characters')
        existing = set(self.db.field_metadata.custom_field_keys(include_composites=True))
        if lookup_name in existing:
            if not generate_unused_lookup_name:
                return self.Result.DUPLICATE_KEY, _('The custom column %s already exists') % lookup_name
            base, n = lookup_name, 1
            while lookup_name in existing:
                n += 1
                lookup_name = f'{base}_{n}'
        if not column_heading:
            return self.Result.BAD_COLUMN_HEADING, _('The column heading must not be empty')
        if datatype not in self.column_types:
            return self.Result.INVALID_TYPE, _("The custom column type %s isn't valid") % datatype
        if datatype == '*text':
            datatype, is_multiple = 'text', True
        if is_multiple and datatype not in ('text', 'composite'):
            return self.Result.INVALID_IS_MULTIPLE, _('Only text and composite columns can hold several values')
        display = dict(display or {})
        if datatype == 'composite' and not display.get('composite_template'):
            return self.Result.INVALID_DISPLAY, _('A composite column needs a template')
        if datatype == 'enumeration' and not display.get('enum_values'):
            return self.Result.INVALID_DISPLAY, _('An enumeration column needs its values')
        try:
            self.db.new_api.create_custom_column(
                lookup_name[1:], column_heading, datatype, bool(is_multiple), display=display
            )
            # The window's field metadata learns of it now, as after calibre's restart.
            try:
                self.db.field_metadata = self.db.new_api.field_metadata
            except Exception:
                pass
            try:
                self.gui.library_view.model().refresh(reset=True)
            except Exception:
                pass
        except Exception as e:
            return self.Result.EXCEPTION_RAISED, str(e)
        self.created_columns[lookup_name] = (column_heading, datatype, is_multiple, display)
        return self.Result.COLUMN_ADDED, lookup_name

    def current_columns(self):
        """The custom columns, by lookup name, with their metadata."""
        ans = {}
        fm = self.db.field_metadata
        for key in fm.custom_field_keys(include_composites=True):
            m = fm[key]
            ans[key] = {
                'column_heading': m['name'],
                'datatype': m['datatype'],
                'is_multiple': bool(m['is_multiple']),
                'display': m.get('display', {}),
            }
        return ans

    def current_columns_restart_state(self):
        return self.current_columns()

    def must_restart(self):
        return self.restart_required
