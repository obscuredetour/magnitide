"""Mirrors calibre's `calibre.db.constants`."""

from collections import namedtuple

COVER_FILE_NAME = 'cover.jpg'
METADATA_FILE_NAME = 'metadata.opf'
DEFAULT_TRASH_EXPIRY_TIME_SECONDS = 14 * 86400
TRASH_DIR_NAME = '.caltrash'
NOTES_DIR_NAME = '.calnotes'
NOTES_DB_NAME = 'notes.db'
DATA_DIR_NAME = 'data'
DATA_FILE_PATTERN = f'{DATA_DIR_NAME}/**/*'
BOOK_ID_PATH_TEMPLATE = ' ({})'
RESOURCE_URL_SCHEME = 'calres'
TEMPLATE_ERROR = 'TEMPLATE ERROR '

TrashEntry = namedtuple('TrashEntry', 'book_id title author cover_path mtime formats')


def resource_url_to_name(url):
    return url.partition('://')[-1]
