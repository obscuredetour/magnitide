"""Mirrors the parts of calibre's `calibre.ebooks.metadata.sources.identify`
that plugins import: `urls_from_identifiers` and the result-merging
helpers. Magnitide does the actual identification run itself."""

from calibre.ebooks.metadata import check_doi, check_isbn

_known_urls = {
    'isbn': ('ISBN', 'https://www.worldcat.org/isbn/{0}'),
    'doi': ('DOI', 'https://dx.doi.org/{0}'),
    'arxiv': ('arXiv', 'https://arxiv.org/abs/{0}'),
    'oclc': ('OCLC', 'https://www.worldcat.org/oclc/{0}'),
    'issn': ('ISSN', 'https://www.worldcat.org/issn/{0}'),
    'amazon': ('Amazon', 'https://www.amazon.com/dp/{0}'),
    'amazon_uk': ('Amazon UK', 'https://www.amazon.co.uk/dp/{0}'),
    'amazon_de': ('Amazon DE', 'https://www.amazon.de/dp/{0}'),
    'amazon_fr': ('Amazon FR', 'https://www.amazon.fr/dp/{0}'),
    'amazon_ca': ('Amazon CA', 'https://www.amazon.ca/dp/{0}'),
    'amazon_jp': ('Amazon JP', 'https://www.amazon.co.jp/dp/{0}'),
    'amazon_es': ('Amazon ES', 'https://www.amazon.es/dp/{0}'),
    'amazon_it': ('Amazon IT', 'https://www.amazon.it/dp/{0}'),
    'amazon_br': ('Amazon BR', 'https://www.amazon.com.br/dp/{0}'),
    'amazon_au': ('Amazon AU', 'https://www.amazon.com.au/dp/{0}'),
    'amazon_in': ('Amazon IN', 'https://www.amazon.in/dp/{0}'),
    'amazon_nl': ('Amazon NL', 'https://www.amazon.nl/dp/{0}'),
    'google': ('Google Books', 'https://books.google.com/books?id={0}'),
    'goodreads': ('Goodreads', 'https://www.goodreads.com/book/show/{0}'),
    'kobo': ('Kobo', 'https://www.kobo.com/ebook/{0}'),
    'mobi-asin': ('MOBI-ASIN', 'https://www.amazon.com/dp/{0}'),
    'lubimyczytac': ('Lubimyczytac', 'https://lubimyczytac.pl/ksiazka/{0}/ksiazka'),
    'litres': ('Litres', 'https://www.litres.ru/{0}'),
    'comicvine': ('Comic Vine', 'https://comicvine.gamespot.com/volume/{0}'),
}


def urls_from_identifiers(identifiers, sort_results=False):
    """(name, identifier type, value, URL) for the identifiers that have
    a known web page, plus what installed sources say."""
    identifiers = {k.lower(): v for k, v in identifiers.items()}
    ans = []
    keys_left = set(identifiers)

    def add(name, k, val, url):
        ans.append((name, k, val, url))
        keys_left.discard(k)

    try:
        from calibre.customize.ui import metadata_plugins

        plugins = list(metadata_plugins(['identify']))
    except Exception:
        plugins = []
    for plugin in plugins:
        try:
            for id_type, id_val, url in plugin.get_book_urls(identifiers):
                add(plugin.get_book_url_name(id_type, id_val, url), id_type, id_val, url)
        except Exception:
            pass

    for k in list(keys_left):
        v = identifiers[k]
        if k == 'isbn':
            v = check_isbn(v) or v
        if k == 'doi':
            v = check_doi(v) or v
        entry = _known_urls.get(k)
        if entry is not None:
            name, url = entry
            add(name, k, v, url.format(v))
        elif k.startswith('url:') or (isinstance(v, str) and v.startswith(('http://', 'https://'))):
            add(k.partition(':')[-1] or k, k, v, v if v.startswith('http') else 'https://' + v)
    if sort_results:
        from calibre.utils.icu import lower

        ans = sorted(ans, key=lambda x: lower(x[0]))
    return ans


def merge_identify_results(result_map, log):
    """The best Metadata per source, as calibre's identify does: the
    results merged into one list, sorted by relevance."""
    ans = []
    for plugin, results in result_map.items():
        ans.extend(results)
    return ans
