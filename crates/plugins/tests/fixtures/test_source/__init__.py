"""A metadata source for Magnitide's plugin host tests: answers without the
network, with every field filled in, and a cover."""

from calibre.ebooks.metadata.book.base import Metadata
from calibre.ebooks.metadata.sources.base import Source
from calibre.utils.date import parse_date

# A 1×1 PNG.
PNG = bytes.fromhex(
    '89504e470d0a1a0a0000000d4948445200000001000000010806000000'
    '1f15c4890000000d49444154789c6360f8cfc0f01f0005000201e2b4a1'
    '1f0000000049454e44ae426082'
)


class TestSource(Source):
    name = 'Magnitide Test Source'
    description = 'Answers every lookup with the same test book'
    author = 'Magnitide'
    version = (1, 2, 3)
    minimum_calibre_version = (5, 0, 0)
    supported_platforms = ['windows', 'osx', 'linux']
    capabilities = frozenset(['identify', 'cover'])
    touched_fields = frozenset(
        ['title', 'authors', 'identifier:test', 'identifier:isbn', 'publisher', 'pubdate', 'comments', 'tags', 'series', 'languages']
    )

    def get_book_url(self, identifiers):
        test_id = identifiers.get('test')
        if test_id:
            return ('test', test_id, f'https://example.invalid/book/{test_id}')

    def get_cached_cover_url(self, identifiers):
        return self.cached_identifier_to_cover_url(identifiers.get('test'))

    def identify(self, log, result_queue, abort, title=None, authors=None, identifiers={}, timeout=30):
        log.info('Test source asked for', title, authors, identifiers)
        print('a plugin printing to stdout must not break the protocol')
        if title == 'Fail':
            return 'This source failed on purpose.'
        if title == 'Crash':
            raise RuntimeError('the plugin crashed')
        if title == 'Slow':
            abort.wait(timeout + 30)
            return
        mi = Metadata(title or 'Untitled', authors or ['Anonymous'])
        mi.set_identifier('test', '42')
        mi.set_identifier('isbn', '9780593135204')
        mi.publisher = 'Ballantine Books'
        mi.pubdate = parse_date('2021-05-04')
        mi.comments = 'A plain-text description.\n\nWith two paragraphs & an ampersand.'
        mi.tags = ['Science Fiction', 'Space']
        mi.series = 'Test Series'
        mi.series_index = 2
        # A name, as some sources give: the host turns it into a code.
        mi.languages = ['English']
        mi.source_relevance = 0
        self.cache_identifier_to_cover_url('42', 'https://example.invalid/cover.png')
        result_queue.put(mi)

    def download_cover(self, log, result_queue, abort, title=None, authors=None, identifiers={}, timeout=30, get_best_cover=False):
        if self.get_cached_cover_url(identifiers):
            result_queue.put((self, PNG))
