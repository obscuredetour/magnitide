"""A catalog plugin for Magnitide's plugin host tests: a kind Magnitide
doesn't run, using a part of calibre it doesn't provide."""

from calibre.customize import CatalogPlugin


class TestCatalog(CatalogPlugin):
    name = 'Magnitide Test Catalog'
    description = 'Makes a catalog (not in Magnitide)'
    author = 'Magnitide'
    version = (1, 0, 0)
    minimum_calibre_version = (5, 0, 0)
    supported_platforms = ['windows', 'osx', 'linux']
    file_types = {'csv'}

    def run(self, path_to_output, opts, db, notification=None):
        from calibre.ebooks.conversion.plumber import Plumber  # noqa: F401
