"""Mirrors calibre's `calibre.ebooks.oeb.iterator`: `EbookIterator` walks an
EPUB's spine (Count Pages reads every spine file through it)."""

from calibre.ebooks.oeb.iterator.book import EbookIterator, SpineItem

__all__ = ['EbookIterator', 'SpineItem']
