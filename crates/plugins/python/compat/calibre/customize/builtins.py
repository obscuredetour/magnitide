"""Mirrors the part of calibre's `calibre.customize.builtins` that Magnitide
provides: metadata readers and writers for common formats, the input and
output profiles, and the stand-ins for calibre's own actions (Edit Metadata,
Remove Books, Convert Books, Add Books, Choose Library, View) that plugins
reach through `gui.iactions`. No device drivers, conversion plugins, stores
or catalogs."""

from calibre.customize import InterfaceActionBase, MetadataReaderPlugin, MetadataWriterPlugin
from calibre.customize.profiles import input_profiles, output_profiles

plugins = []


# --- Metadata readers -----------------------------------------------------


class EPUBMetadataReader(MetadataReaderPlugin):
    name = 'Read EPUB metadata'
    file_types = {'epub', 'kepub'}
    description = 'Read metadata from EPUB files'

    def get_metadata(self, stream, ftype):
        from calibre.ebooks.metadata.epub import get_metadata, get_quick_metadata

        if self.quick:
            return get_quick_metadata(stream)
        return get_metadata(stream)


class MOBIMetadataReader(MetadataReaderPlugin):
    name = 'Read MOBI metadata'
    file_types = {'mobi', 'prc', 'azw', 'azw3', 'azw4', 'pobi'}
    description = 'Read metadata from Mobipocket and Kindle files'

    def get_metadata(self, stream, ftype):
        from calibre.ebooks.metadata.mobi import get_metadata

        return get_metadata(stream)


class OPFMetadataReader(MetadataReaderPlugin):
    name = 'Read OPF metadata'
    file_types = {'opf'}
    description = 'Read metadata from OPF files'

    def get_metadata(self, stream, ftype):
        from calibre.ebooks.metadata.opf2 import OPF

        return OPF(stream, populate_spine=False).to_book_metadata()


class PDFMetadataReader(MetadataReaderPlugin):
    name = 'Read PDF metadata'
    file_types = {'pdf'}
    description = 'Read metadata from PDF files'

    def get_metadata(self, stream, ftype):
        from calibre.ebooks.metadata.pdf import get_metadata

        return get_metadata(stream, cover=not self.quick)


class TXTMetadataReader(MetadataReaderPlugin):
    name = 'Read TXT metadata'
    file_types = {'txt', 'md', 'textile', 'markdown'}
    description = 'Read metadata from TXT files'

    def get_metadata(self, stream, ftype):
        from calibre.ebooks.metadata.txt import get_metadata

        return get_metadata(stream)


class FB2MetadataReader(MetadataReaderPlugin):
    name = 'Read FB2 metadata'
    file_types = {'fb2', 'fbz'}
    description = 'Read metadata from FictionBook files'

    def get_metadata(self, stream, ftype):
        from calibre.ebooks.metadata.fb2 import get_metadata

        return get_metadata(stream)


class HTMLMetadataReader(MetadataReaderPlugin):
    name = 'Read HTML metadata'
    file_types = {'html', 'htm', 'xhtml', 'xhtm', 'shtml'}
    description = 'Read metadata from HTML files'

    def get_metadata(self, stream, ftype):
        from calibre.ebooks.metadata.html import get_metadata

        return get_metadata(stream)


class ComicMetadataReader(MetadataReaderPlugin):
    name = 'Read comic metadata'
    file_types = {'cbz'}
    description = 'Extract cover from comic files'

    def get_metadata(self, stream, ftype):
        from calibre.ebooks.metadata.archive import get_comic_metadata

        return get_comic_metadata(stream, ftype, series_index=None)


class ZipMetadataReader(MetadataReaderPlugin):
    name = 'Read ZIP metadata'
    file_types = {'zip', 'oebzip'}
    description = 'Read metadata from e-books in ZIP archives'

    def get_metadata(self, stream, ftype):
        from calibre.ebooks.metadata.zip import get_metadata

        return get_metadata(stream)


# --- Metadata writers -----------------------------------------------------


class EPUBMetadataWriter(MetadataWriterPlugin):
    name = 'Set EPUB metadata'
    file_types = {'epub', 'kepub'}
    description = 'Set metadata in EPUB files'

    def set_metadata(self, stream, mi, ftype):
        from calibre.ebooks.metadata.epub import set_metadata

        q = self.site_customization or ''
        apply_null = self.apply_null or 'apply_null' in q
        set_metadata(stream, mi, apply_null=apply_null, force_identifiers=False)


class MOBIMetadataWriter(MetadataWriterPlugin):
    name = 'Set MOBI metadata'
    file_types = {'mobi', 'prc', 'azw', 'azw3', 'azw4'}
    description = 'Set metadata in Mobipocket and Kindle files'

    def set_metadata(self, stream, mi, ftype):
        from calibre.ebooks.metadata.mobi import set_metadata

        set_metadata(stream, mi)


class TXTMetadataWriter(MetadataWriterPlugin):
    name = 'Set TXT metadata'
    file_types = {'txt'}
    description = 'Set metadata in TXT files (title only, as the first line)'

    def set_metadata(self, stream, mi, ftype):
        from calibre.ebooks.metadata.txt import set_metadata

        set_metadata(stream, mi)


plugins += [
    EPUBMetadataReader,
    MOBIMetadataReader,
    OPFMetadataReader,
    PDFMetadataReader,
    TXTMetadataReader,
    FB2MetadataReader,
    HTMLMetadataReader,
    ComicMetadataReader,
    ZipMetadataReader,
    EPUBMetadataWriter,
    MOBIMetadataWriter,
    TXTMetadataWriter,
]

plugins += input_profiles + output_profiles


# --- calibre's own actions, as Magnitide's --------------------------------


class ActionEditMetadata(InterfaceActionBase):
    name = 'Edit Metadata'
    actual_plugin = 'magnitide_host.gui.actions:EditMetadataAction'
    description = "calibre's Edit Metadata: Magnitide's editor"


class ActionDelete(InterfaceActionBase):
    name = 'Remove Books'
    actual_plugin = 'magnitide_host.gui.actions:RemoveBooksAction'
    description = "calibre's Remove Books: Magnitide asks, then moves books to the Trash"


class ActionConvert(InterfaceActionBase):
    name = 'Convert Books'
    actual_plugin = 'magnitide_host.gui.actions:ConvertBooksAction'
    description = "calibre's Convert Books: conversions run in Magnitide"


class ActionAdd(InterfaceActionBase):
    name = 'Add Books'
    actual_plugin = 'magnitide_host.gui.actions:AddBooksAction'
    description = "calibre's Add Books: books are added to the library"


class ActionChooseLibrary(InterfaceActionBase):
    name = 'Choose Library'
    actual_plugin = 'magnitide_host.gui.actions:ChooseLibraryAction'
    description = "calibre's Choose Library (Magnitide's library stays open)"


class ActionView(InterfaceActionBase):
    name = 'View'
    actual_plugin = 'magnitide_host.gui.actions:ViewAction'
    description = "calibre's View: opens the book in the default app"


plugins += [ActionEditMetadata, ActionDelete, ActionConvert, ActionAdd, ActionChooseLibrary, ActionView]

__all__ = ['plugins']
