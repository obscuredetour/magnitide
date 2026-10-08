"""Mirrors calibre's `calibre.ebooks.metadata.book`: the names of the
metadata fields a `Metadata` object holds."""

SOCIAL_METADATA_FIELDS = frozenset(
    [
        'tags',  # Ordered list
        'rating',  # A floating point number between 0 and 10
        'comments',  # A simple HTML enabled string
        'series',  # A simple string
        'series_index',  # A floating point number
        # Of the form { 'isbn':'123456789', 'doi':'xxxx', ... }
        'identifiers',
    ]
)

TOP_LEVEL_IDENTIFIERS = frozenset(['isbn'])

PUBLICATION_METADATA_FIELDS = frozenset(
    [
        'title',  # title must never be None. Should be _('Unknown')
        # Pseudo field that can be set, but if not set is auto generated
        # from title and languages
        'title_sort',
        'authors',  # Ordered list. Must never be None, can be [_('Unknown')]
        'author_sort_map',  # Map of sort strings for each author
        # Pseudo field that can be set, but if not set is auto generated
        # from authors and languages
        'author_sort',
        'book_producer',
        'timestamp',  # Dates and times must be timezone aware
        'pubdate',
        'last_modified',
        'rights',
        # So far only known publication type is periodical:calibre
        # If None, means book
        'publication_type',
        'uuid',  # A UUID usually of type 4
        'languages',  # ordered list of languages in this publication
        'publisher',  # Simple string, no special semantics
        # Absolute path to image file encoded in filesystem_encoding
        'cover',
        # Image data in (format, data) form
        'cover_data',
        # thumbnail
        'thumbnail',
    ]
)

BOOK_STRUCTURE_FIELDS = frozenset(
    [
        # These are used by code, Null values are None.
        'toc',
        'spine',
        'guide',
        'manifest',
    ]
)

USER_METADATA_FIELDS = frozenset(
    [
        # A dict of dicts similar to field_metadata. Each field description dict
        # also contains a value field with the key #value#.
        'user_metadata',
    ]
)

DEVICE_METADATA_FIELDS = frozenset(
    [
        'device_collections',  # Ordered list of strings
        'lpath',  # Unicode, / separated
        'size',  # In bytes
        'mime',  # Mimetype of the book file being represented
    ]
)

CALIBRE_METADATA_FIELDS = frozenset(
    [
        'application_id',  # An application id, currently set to the db_id.
        'db_id',  # the calibre primary key of the item.
        'formats',  # list of formats (extensions) for this book
        # a dict of user category names, where the value is a list of item names
        # from the book that are in that category
        'user_categories',
        # a dict of items to associated hyperlink
        'link_maps',
        'author_link_map',
    ]
)

ALL_METADATA_FIELDS = (
    SOCIAL_METADATA_FIELDS.union(PUBLICATION_METADATA_FIELDS)
    .union(BOOK_STRUCTURE_FIELDS)
    .union(USER_METADATA_FIELDS)
    .union(DEVICE_METADATA_FIELDS)
    .union(CALIBRE_METADATA_FIELDS)
)

# All fields except custom fields
STANDARD_METADATA_FIELDS = (
    SOCIAL_METADATA_FIELDS.union(PUBLICATION_METADATA_FIELDS)
    .union(BOOK_STRUCTURE_FIELDS)
    .union(DEVICE_METADATA_FIELDS)
    .union(CALIBRE_METADATA_FIELDS)
)

# Metadata fields that smart update must do special processing to copy.
SC_FIELDS_NOT_COPIED = frozenset(
    [
        'title',
        'title_sort',
        'authors',
        'author_sort',
        'author_sort_map',
        'cover_data',
        'tags',
        'languages',
        'identifiers',
    ]
)

# Metadata fields that smart update should copy only if the source is not None
SC_FIELDS_COPY_NOT_NULL = frozenset(['device_collections', 'lpath', 'size', 'comments', 'thumbnail'])

# Metadata fields that smart update should copy without special handling
SC_COPYABLE_FIELDS = SOCIAL_METADATA_FIELDS.union(PUBLICATION_METADATA_FIELDS).union(BOOK_STRUCTURE_FIELDS).union(
    DEVICE_METADATA_FIELDS
).union(CALIBRE_METADATA_FIELDS) - SC_FIELDS_NOT_COPIED.union(SC_FIELDS_COPY_NOT_NULL)

SERIALIZABLE_FIELDS = SOCIAL_METADATA_FIELDS.union(USER_METADATA_FIELDS).union(PUBLICATION_METADATA_FIELDS).union(
    CALIBRE_METADATA_FIELDS
).union(DEVICE_METADATA_FIELDS) - frozenset(['device_collections', 'formats', 'cover_data'])
