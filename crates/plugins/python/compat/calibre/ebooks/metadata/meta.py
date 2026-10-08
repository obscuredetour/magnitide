"""Mirrors calibre's `calibre.ebooks.metadata.meta` (calibre 9.15): reading
metadata from a book file through the metadata reader plugins, with the
file name as a fallback, and writing it through the writer plugins."""

import os
import re

from calibre.customize.ui import get_file_type_metadata, set_file_type_metadata
from calibre.ebooks.metadata import MetaInformation, string_to_authors
from calibre.ebooks.metadata.book.base import Metadata
from calibre.utils.config_base import prefs

_METADATA_PRIORITIES = [
    'mobi',
    'azw',
    'azw3',
    'azw1',
    'azw4',
    'epub',
    'kepub',
    'lit',
    'odt',
    'rtf',
    'fb2',
    'html',
    'htm',
    'xhtml',
    'xhtm',
    'pdf',
    'prc',
    'txt',
    'txtz',
    'pdb',
    'zip',
    'cbz',
    'docx',
    'opf',
]

# The priorities for loading metadata from different file types
# Higher values should be used to update metadata from lower values
METADATA_PRIORITIES = {k: i for i, k in enumerate(_METADATA_PRIORITIES)}


def path_to_ext(path):
    return os.path.splitext(path)[1][1:].lower()


def metadata_from_formats(formats, force_read_metadata=False, pattern=None):
    """Metadata merged from several files of the same book, best format first."""
    try:
        return _metadata_from_formats(formats, force_read_metadata, pattern)
    except Exception:
        mi = metadata_from_filename(list(iter(formats))[0], pat=pattern)
        if not mi.authors:
            mi.authors = [_('Unknown')]
        return mi


def _metadata_from_formats(formats, force_read_metadata=False, pattern=None):
    mi = MetaInformation(None, None)
    formats.sort(key=lambda x: METADATA_PRIORITIES.get(path_to_ext(x), -1), reverse=True)
    extensions = list(map(path_to_ext, formats))
    if 'opf' in extensions:
        opf = formats[extensions.index('opf')]
        mi2 = opf_metadata(opf)
        if mi2 is not None and mi2.title:
            return mi2

    for path, ext in zip(formats, extensions):
        with open(path, 'rb') as stream:
            try:
                newmi = get_metadata(
                    stream,
                    stream_type=ext,
                    use_libprs_metadata=True,
                    force_read_metadata=force_read_metadata,
                    pattern=pattern,
                )
                mi.smart_update(newmi)
            except Exception:
                continue
            if getattr(mi, 'application_id', None) is not None:
                return mi

    if not mi.title:
        mi.title = _('Unknown')
    if not mi.authors:
        mi.authors = [_('Unknown')]

    return mi


def is_recipe(filename):
    return filename.startswith('calibre') and filename.rpartition('.')[0].endswith('_recipe_out')


def get_metadata(stream, stream_type='lrf', use_libprs_metadata=False, force_read_metadata=False, pattern=None):
    """Metadata for the book in `stream`, of type `stream_type` (an
    extension): what the readers for that type find, merged over what the
    file name says, as calibre does when adding books."""
    pos = 0
    if hasattr(stream, 'tell'):
        pos = stream.tell()
    try:
        return _get_metadata(stream, stream_type, use_libprs_metadata, force_read_metadata, pattern)
    finally:
        if hasattr(stream, 'seek'):
            stream.seek(pos)


def _get_metadata(stream, stream_type, use_libprs_metadata, force_read_metadata=False, pattern=None):
    if stream_type:
        stream_type = stream_type.lower()
    if stream_type in ('html', 'html', 'xhtml', 'xhtm', 'xml'):
        stream_type = 'html'
    if stream_type in ('mobi', 'prc', 'azw'):
        stream_type = 'mobi'
    if stream_type in ('odt', 'ods', 'odp', 'odg', 'odf'):
        stream_type = 'odt'

    opf = None
    if hasattr(stream, 'name'):
        c = os.path.splitext(stream.name)[0] + '.opf'
        if os.access(c, os.R_OK):
            opf = opf_metadata(os.path.abspath(c))

    if use_libprs_metadata and getattr(opf, 'application_id', None) is not None:
        return opf

    name = os.path.basename(getattr(stream, 'name', ''))
    # The fallback pattern matches the default filename format produced by calibre
    base = metadata_from_filename(name, pat=pattern, fallback_pat=re.compile(r'^(?P<title>.+) - (?P<author>[^-]+)$'))
    if force_read_metadata or prefs['read_file_metadata']:
        mi = get_file_type_metadata(stream, stream_type)
    else:
        mi = MetaInformation(None, None)
    if base.title == os.path.splitext(name)[0] and base.is_null('authors') and base.is_null('isbn'):
        # Assume that there was no metadata in the file and the user set pattern
        # to match meta info from the file name did not match.
        # The regex is meant to match the standard format filenames are written
        # in. The standard format is title - author.ext
        base.smart_update(mi)
    else:
        mi.smart_update(base)
    if not mi.title:
        mi.title = _('Unknown')
    if not mi.authors:
        mi.authors = [_('Unknown')]

    if opf is not None:
        mi.smart_update(opf)

    return mi


def get_metadata_from_stream_plain(stream, ftype):
    """Metadata from the file name alone (when no reader handles the type)."""
    name = os.path.basename(getattr(stream, 'name', ''))
    mi = metadata_from_filename(name)
    if not mi.authors:
        mi.authors = [_('Unknown')]
    return mi


def set_metadata(stream, mi, stream_type='lrf', report_error=None):
    if stream_type:
        stream_type = stream_type.lower()
    set_file_type_metadata(stream, mi, stream_type, report_error=report_error)


def metadata_from_filename(name, pat=None, fallback_pat=None):
    """Metadata guessed from a file name, with the `filename_pattern`
    preference (a regex with named groups: title, author, series,
    series_index, isbn, publisher, published, comments)."""
    if isinstance(name, bytes):
        name = name.decode('utf-8', 'replace')
    name = name.rpartition('.')[0] if '.' in name else name
    name = name.replace('_', ' ')
    mi = MetaInformation(None, None)
    if pat is None:
        pat = re.compile(prefs.get('filename_pattern'))
    name = name.replace('_', ' ')
    match = pat.search(name)
    if match is None and fallback_pat is not None:
        match = fallback_pat.search(name)

    if match is not None:
        try:
            mi.title = match.group('title')
        except IndexError:
            pass
        try:
            au = match.group('author')
            aus = string_to_authors(au)
            if aus:
                mi.authors = aus
                if prefs['swap_author_names'] and mi.authors:

                    def swap(a):
                        if ',' in a:
                            parts = a.split(',', 1)
                        else:
                            parts = a.split(None, 1)
                        if len(parts) > 1:
                            t = parts[-1]
                            parts = parts[:-1]
                            parts.insert(0, t)
                        return ' '.join(parts)

                    mi.authors = [swap(x) for x in mi.authors]
        except IndexError, ValueError:
            pass
        try:
            mi.series = match.group('series')
        except IndexError:
            pass
        try:
            si = match.group('series_index')
            mi.series_index = float(si)
        except IndexError, ValueError, TypeError:
            pass
        try:
            si = match.group('isbn')
            mi.isbn = si
        except IndexError, ValueError:
            pass
        try:
            publisher = match.group('publisher')
            mi.publisher = publisher
        except IndexError, ValueError:
            pass
        try:
            pubdate = match.group('published')
            if pubdate:
                from calibre.utils.date import parse_only_date

                mi.pubdate = parse_only_date(pubdate)
        except Exception:
            pass
        try:
            comments = match.group('comments')
            mi.comments = comments
        except IndexError, ValueError:
            pass

    if mi.is_null('title'):
        mi.title = name
    return mi


def opf_metadata(opfpath):
    if hasattr(opfpath, 'read'):
        f = opfpath
        opfpath = getattr(f, 'name', os.getcwd())
    else:
        f = open(opfpath, 'rb')
    try:
        from calibre.ebooks.metadata.opf2 import OPF

        opf = OPF(f, os.path.dirname(opfpath))
        if opf.application_id is not None:
            mi = opf.to_book_metadata()
            if hasattr(opf, 'cover') and opf.cover:
                cpath = os.path.join(os.path.dirname(opfpath), opf.cover)
                if os.access(cpath, os.R_OK):
                    from calibre.utils.imghdr import what

                    with open(cpath, 'rb') as cf:
                        data = cf.read()
                    fmt = what(None, data)
                    mi.cover_data = (fmt, data)
            return mi
    except Exception:
        import traceback

        traceback.print_exc()
    finally:
        if f is not opfpath:
            f.close()


def forked_read_metadata(original_path, tdir):
    """Read metadata into `tdir/metadata.opf` (and `cover.jpg`), as
    calibre's worker does; here in-process."""
    from calibre.ebooks.metadata.opf2 import metadata_to_opf

    with open(original_path, 'rb') as f:
        fmt = path_to_ext(original_path)
        f.seek(0, 2)
        sz = f.tell()
        with open(os.path.join(tdir, 'size.txt'), 'wb') as s:
            s.write(str(sz).encode('ascii'))
        f.seek(0)
        mi = get_metadata(f, fmt)
    if mi.cover_data and mi.cover_data[1]:
        with open(os.path.join(tdir, 'cover.jpg'), 'wb') as f:
            f.write(mi.cover_data[1])
        mi.cover_data = (None, None)
        mi.cover = 'cover.jpg'
    opf = metadata_to_opf(mi, default_lang='und')
    with open(os.path.join(tdir, 'metadata.opf'), 'wb') as f:
        f.write(opf)


__all__ = [
    'get_metadata',
    'set_metadata',
    'metadata_from_filename',
    'metadata_from_formats',
    'opf_metadata',
    'path_to_ext',
    'get_metadata_from_stream_plain',
    'forked_read_metadata',
    'Metadata',
    'is_recipe',
    'METADATA_PRIORITIES',
]
