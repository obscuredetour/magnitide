"""Mirrors calibre's `calibre.library` package root."""


def db(path=None, read_only=False):
    """Open the library at `path` (the shadow library Magnitide gives
    plugins) as a `LibraryDatabase`."""
    from calibre.db.legacy import LibraryDatabase

    if path is None:
        from calibre.utils.config import prefs

        path = prefs['library_path']
    return LibraryDatabase(path, read_only=read_only)


def current_library_name():
    import os

    from calibre.utils.config import prefs

    path = prefs['library_path']
    if path:
        return os.path.basename(path)
