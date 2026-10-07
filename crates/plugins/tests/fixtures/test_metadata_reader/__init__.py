"""A metadata reader for Magnitide's plugin host tests: TXT files with a
header of "Field: value" lines. Priority 2, so it beats a built-in reader."""

from calibre.customize import MetadataReaderPlugin
from calibre.ebooks.metadata.book.base import Metadata


class TestReader(MetadataReaderPlugin):
    name = 'Magnitide Test Reader'
    description = 'Reads a "Field: value" header from TXT files'
    author = 'Magnitide'
    version = (1, 0, 0)
    minimum_calibre_version = (5, 0, 0)
    supported_platforms = ['windows', 'osx', 'linux']
    file_types = {'txt'}
    priority = 2

    def get_metadata(self, stream, type):
        text = stream.read().decode('utf-8', 'replace')
        mi = Metadata(_('Unknown'))
        for line in text.splitlines():
            field, sep, value = line.partition(': ')
            if not sep:
                break
            value = value.strip()
            if field == 'Title':
                mi.title = value
            elif field == 'Author':
                mi.authors = [a.strip() for a in value.split('&')]
            elif field == 'ISBN':
                mi.set_identifier('isbn', value)
            elif field == 'Tags':
                mi.tags = [t.strip() for t in value.split(',')]
            elif field == 'Crash':
                raise ValueError('crashed on purpose')
        return mi
