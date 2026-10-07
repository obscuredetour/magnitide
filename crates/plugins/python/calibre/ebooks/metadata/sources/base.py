"""Mirrors calibre's `calibre.ebooks.metadata.sources.base` (calibre 9.15):
the `Source` base class of metadata download plugins, and its helpers.

Magnitide's Find Metadata runs a source's `identify()` and
`download_cover()` as calibre does: on a thread, with a log, a result
queue, an abort flag and a timeout.
"""

import re
import threading
from collections import namedtuple
from functools import total_ordering

from calibre import browser, random_user_agent
from calibre.customize import Plugin
from calibre.ebooks.metadata import check_isbn
from calibre.ebooks.metadata.author_mapper import cap_author_token
from calibre.utils.config import JSONConfig
from calibre.utils.localization import canonicalize_lang, get_lang
from calibre.utils.logging import FileStream, ThreadSafeLog
from calibre.utils.titlecase import titlecase

_Option = namedtuple('Option', 'name type default label desc choices')


def Option(name, type_, default, label, desc, choices=None):
    """A configurable setting of a source: shown in its config widget and
    read from `self.prefs[name]`. `type_` is 'number', 'string', 'bool' or
    'choices' (with `choices` a dict of value → label)."""
    return _Option(name, type_, default, label, desc, choices)


def create_log(ostream=None):
    from calibre.utils.logging import ThreadSafeLog

    log = ThreadSafeLog(level=ThreadSafeLog.DEBUG)
    log.outputs = [FileStream(ostream)]
    return log


# Comparing Metadata objects for relevance {{{
words = ('the', 'a', 'an', 'of', 'and')
prefix_pat = re.compile(r'^(%s)\s+' % ('|'.join(words)))
trailing_paren_pat = re.compile(r'\(.*\)$')
whitespace_pat = re.compile(r'\s+')


def cleanup_title(s):
    if not s:
        s = _('Unknown')
    s = s.strip().lower()
    s = prefix_pat.sub(' ', s)
    s = trailing_paren_pat.sub('', s)
    s = whitespace_pat.sub(' ', s)
    return s.strip()


@total_ordering
class InternalMetadataCompareKeyGen:
    """A sort key for metadata results: identifier matches, a cover, how
    many fields are filled, language, an exact title, the comments' length,
    and the source's own relevance, in that order."""

    def __init__(self, mi, source_plugin, title, authors, identifiers):
        same_identifier = 2
        idents = mi.get_identifiers()
        for k, v in identifiers.items():
            if idents.get(k) == v:
                same_identifier = 1
                break

        all_fields = 1 if source_plugin.test_fields(mi) is None else 2

        exact_title = 1 if title and cleanup_title(title) == cleanup_title(mi.title) else 2

        language = 1
        if mi.language:
            mil = canonicalize_lang(mi.language)
            if mil != 'und' and mil != canonicalize_lang(get_lang()):
                language = 2

        has_cover = (
            2
            if (
                not source_plugin.cached_cover_url_is_reliable
                or source_plugin.get_cached_cover_url(mi.identifiers) is None
            )
            else 1
        )

        self.base = (same_identifier, has_cover, all_fields, language, exact_title)
        self.comments_len = len((mi.comments or '').strip())
        self.extra = getattr(mi, 'source_relevance', 0)

    def compare_to_other(self, other):
        a = self.base
        b = other.base
        if a != b:
            return (a > b) - (a < b)
        a = self.comments_len
        b = other.comments_len
        if a != b:
            return (b > a) - (b < a)
        return (self.extra > other.extra) - (self.extra < other.extra)

    def __eq__(self, other):
        return self.compare_to_other(other) == 0

    def __ne__(self, other):
        return self.compare_to_other(other) != 0

    def __lt__(self, other):
        return self.compare_to_other(other) < 0

    def __le__(self, other):
        return self.compare_to_other(other) <= 0

    def __gt__(self, other):
        return self.compare_to_other(other) > 0

    def __ge__(self, other):
        return self.compare_to_other(other) >= 0


# }}}


def get_cached_cover_urls(mi):
    from calibre.customize.ui import metadata_plugins

    plugins = list(metadata_plugins(['identify']))
    for p in plugins:
        url = p.get_cached_cover_url(mi.identifiers)
        if url:
            yield (p, url)


def dump_caches():
    from calibre.customize.ui import metadata_plugins

    return {p.name: p.dump_caches() for p in metadata_plugins(['identify'])}


def load_caches(dump):
    from calibre.customize.ui import metadata_plugins

    plugins = list(metadata_plugins(['identify']))
    for p in plugins:
        cache = dump.get(p.name, None)
        if cache:
            p.load_caches(cache)


def fixauthors(authors):
    if not authors:
        return authors
    ans = []
    for x in authors:
        ans.append(' '.join(map(cap_author_token, x.split())))
    return ans


def fixcase(x):
    if x:
        x = titlecase(x)
    return x


class Source(Plugin):
    """A metadata source. Subclass it, set `capabilities` and
    `touched_fields`, and implement `identify()` (and `download_cover()`)."""

    type = 'Metadata source'
    author = 'Kovid Goyal'

    supported_platforms = ['windows', 'osx', 'linux']

    #: Set of capabilities supported by this plugin.
    #: Useful capabilities are: 'identify', 'cover'
    capabilities = frozenset()

    #: List of metadata fields that can potentially be download by this plugin
    #: during the identify phase
    touched_fields = frozenset()

    #: Set this to True if your plugin returns HTML formatted comments
    has_html_comments = False

    #: Setting this to True means that the browser object will indicate
    #: that it supports gzip transfer encoding.
    supports_gzip_transfer_encoding = False

    #: Set this to True to ignore HTTPS certificate errors when connecting
    #: to this source.
    ignore_ssl_errors = False

    #: Cached cover URLs can sometimes be unreliable (i.e. the download could
    #: fail or the returned image could be bogus).
    cached_cover_url_is_reliable = True

    #: A list of :class:`Option` objects. They will be used to automatically
    #: construct the configuration widget for this plugin
    options = ()

    #: A string that is displayed at the top of the config widget for this
    #: plugin
    config_help_message = None

    #: If True this source can return multiple covers for a given query
    can_get_multiple_covers = False

    #: If set to True covers downloaded by this plugin are automatically trimmed.
    auto_trim_covers = False

    #: If set to True, and this source returns multiple results for a query,
    #: some of which have ISBNs and some of which do not, the results without
    #: ISBNs will be ignored
    prefer_results_with_isbn = True

    def __init__(self, *args, **kwargs):
        Plugin.__init__(self, *args, **kwargs)
        self.running_a_test = False  # Set to True when using identify_test()
        self._isbn_to_identifier_cache = {}
        self._identifier_to_cover_url_cache = {}
        self.cache_lock = threading.RLock()
        self._config_obj = None
        self._browser = None
        self.prefs.defaults['ignore_fields'] = []
        for opt in self.options:
            self.prefs.defaults[opt.name] = opt.default

    # Configuration {{{

    def is_configured(self):
        """Return False if your plugin needs to be configured before it can
        be used."""
        return True

    def is_customizable(self):
        return True

    def customization_help(self):
        return 'This plugin can only be customized using the GUI'

    def config_widget(self):
        from calibre.gui2.metadata.config import ConfigWidget

        return ConfigWidget(self)

    def save_settings(self, config_widget):
        config_widget.commit()

    @property
    def prefs(self):
        if self._config_obj is None:
            self._config_obj = JSONConfig('metadata_sources/%s.json' % self.name)
        return self._config_obj

    # }}}

    # Browser {{{

    @property
    def user_agent(self):
        # Pass in an index to random_user_agent() to test with a particular
        # user agent
        return random_user_agent()

    @property
    def browser(self):
        if self._browser is None:
            self._browser = browser(user_agent=self.user_agent, verify_ssl_certificates=not self.ignore_ssl_errors)
            if self.supports_gzip_transfer_encoding:
                self._browser.set_handle_gzip(True)
        return self._browser.clone_browser()

    # }}}

    # Caching {{{

    def get_related_isbns(self, id_):
        with self.cache_lock:
            for isbn, q in self._isbn_to_identifier_cache.items():
                if q == id_:
                    yield isbn

    def cache_isbn_to_identifier(self, isbn, id_):
        with self.cache_lock:
            self._isbn_to_identifier_cache[isbn] = id_

    def cached_isbn_to_identifier(self, isbn):
        with self.cache_lock:
            return self._isbn_to_identifier_cache.get(isbn, None)

    def cache_identifier_to_cover_url(self, id_, url):
        with self.cache_lock:
            self._identifier_to_cover_url_cache[id_] = url

    def cached_identifier_to_cover_url(self, id_):
        with self.cache_lock:
            return self._identifier_to_cover_url_cache.get(id_, None)

    def dump_caches(self):
        with self.cache_lock:
            return {
                'isbn_map': self._isbn_to_identifier_cache.copy(),
                'cover_map': self._identifier_to_cover_url_cache.copy(),
            }

    def load_caches(self, dump):
        with self.cache_lock:
            self._isbn_to_identifier_cache.update(dump['isbn_map'])
            self._identifier_to_cover_url_cache.update(dump['cover_map'])

    # }}}

    # Utility functions {{{

    def get_author_tokens(self, authors, only_first_author=True):
        """Take a list of authors and return a list of tokens useful for an
        AND search query. Removes initials and brackets."""
        if authors:
            # Leave ' in there for Irish names
            remove_pat = re.compile(r'[!@#$%^&*(){}`~"\s\[\]/]')
            replace_pat = re.compile(r'[-+.:;,，。；：]')
            if only_first_author:
                authors = authors[:1]
            for au in authors:
                has_comma = ',' in au
                au = replace_pat.sub(' ', au)
                parts = au.split()
                if has_comma:
                    # au probably in ln, fn form
                    parts = parts[1:] + parts[:1]
                for tok in parts:
                    tok = remove_pat.sub('', tok).strip()
                    if len(tok) > 2 and tok.lower() not in ('von', 'van', _('Unknown').lower()):
                        yield tok

    def get_title_tokens(self, title, strip_joiners=True, strip_subtitle=False):
        """Take a title and return a list of tokens useful for an AND search
        query. Excludes connectives(optionally) and punctuation."""
        if title:
            # strip sub-titles
            if strip_subtitle:
                subtitle = re.compile(r'([\(\[\{].*?[\)\]\}]|[/:\\].*$)')
                if len(subtitle.sub('', title)) > 1:
                    title = subtitle.sub('', title)

            title_patterns = [
                (re.compile(pat, re.IGNORECASE), repl)
                for pat, repl in [
                    # Remove things like: (2010) (Omnibus) etc.
                    (
                        r'(?i)[({\[](\d{4}|omnibus|anthology|hardcover|audiobook|audio\scd|paperback|turtleback|mass\s*market|edition|ed\.)[\])}]',
                        '',
                    ),
                    # Remove any strings that contain the substring edition inside parentheses
                    (r'(?i)[({\[].*?(edition|ed.).*?[\]})]', ''),
                    # Remove commas used a separators in numbers
                    (r'(\d+),(\d+)', r'\1\2'),
                    # Remove hyphens only if they have whitespace before them
                    (r'(\s-)', ' '),
                    # Replace other special chars with a space
                    (r'[:,;!@$%^&*(){}.`~"\s\[\]/]', ' '),
                ]
            ]

            for pat, repl in title_patterns:
                title = pat.sub(repl, title)

            tokens = title.split()
            for token in tokens:
                token = token.strip().strip('"').strip("'")
                if token and (not strip_joiners or token.lower() not in ('a', 'and', 'the', '&')):
                    yield token

    def split_jobs(self, jobs, num):
        """Split a list of jobs into at most num groups, as evenly as possible."""
        groups = [[] for i in range(num)]
        jobs = list(jobs)
        while jobs:
            for gr in groups:
                try:
                    job = jobs.pop()
                except IndexError:
                    break
                gr.append(job)
        return [g for g in groups if g]

    def test_fields(self, mi):
        """Return the first field from self.touched_fields that is null on
        the mi object."""
        for key in self.touched_fields:
            if key.startswith('identifier:'):
                key = key.partition(':')[-1]
                if not mi.has_identifier(key):
                    return 'identifier: ' + key
            elif mi.is_null(key):
                return key

    def clean_downloaded_metadata(self, mi):
        """Call this method in your plugin's identify method to normalize
        metadata before putting the Metadata object into result_queue. You
        can of course, use a custom algorithm suited to your metadata source."""
        docase = mi.language == 'eng' or (mi.is_null('language') and get_lang().startswith('en'))
        if docase and mi.title:
            mi.title = fixcase(mi.title)
        mi.authors = fixauthors(mi.authors)
        if mi.tags and docase:
            mi.tags = list(map(fixcase, mi.tags))
        mi.isbn = check_isbn(mi.isbn)
        if mi.series_index is None:
            mi.series_index = 1.0

    def download_multiple_covers(
        self, title, authors, urls, get_best_cover, timeout, result_queue, abort, log, prefs_name='max_covers'
    ):
        if not urls:
            log('No images found for, title: %r and authors: %r' % (title, authors))
            return
        import time
        from threading import Thread

        if prefs_name:
            urls = urls[: self.prefs[prefs_name]]
        if get_best_cover:
            urls = urls[:1]
        workers = [Thread(target=self.download_image, args=(u, timeout, log, result_queue)) for u in urls]
        for w in workers:
            w.daemon = True
            w.start()
        alive = True
        start_time = time.monotonic()
        while alive and not abort.is_set() and time.monotonic() - start_time < timeout:
            alive = False
            for w in workers:
                if w.is_alive():
                    alive = True
                    break
            abort.wait(0.1)

    def download_image(self, url, timeout, log, result_queue):
        try:
            ans = self.browser.open_novisit(url, timeout=timeout).read()
            result_queue.put((self, ans))
            log('Downloaded cover from: %s' % url)
        except Exception:
            self.log.exception('Failed to download cover from: %r' % url)

    # }}}

    # Metadata API {{{

    def get_book_url(self, identifiers):
        """A 3-tuple (identifier_type, identifier_value, URL) for this
        source's page about the book, or None."""
        return None

    def get_book_url_name(self, idtype, idval, url):
        """The name to show for the URL (the plugin's name by default)."""
        return self.name

    def get_book_urls(self, identifiers):
        """Override for sources with more than one URL per book."""
        data = self.get_book_url(identifiers)
        if data is None:
            return ()
        return (data,)

    def get_cached_cover_url(self, identifiers):
        """A cached cover URL for the book identified by `identifiers`, or None."""
        return None

    def id_from_url(self, url):
        """Parse a URL and return a tuple of the form: (identifier_type, identifier_value), or None."""
        return None

    def identify_results_keygen(self, title=None, authors=None, identifiers={}):
        """Return a function that is used to generate a key that can sort
        Metadata objects by their relevance given a search query."""

        def keygen(mi):
            return InternalMetadataCompareKeyGen(mi, self, title, authors, identifiers)

        return keygen

    def identify(self, log, result_queue, abort, title=None, authors=None, identifiers={}, timeout=30):
        """Identify a book by its title/author/isbn/etc. Put Metadata objects
        into `result_queue`; return None, or an error string."""
        return None

    def download_cover(
        self, log, result_queue, abort, title=None, authors=None, identifiers={}, timeout=30, get_best_cover=False
    ):
        """Download a cover; put `(self, cover_data)` into `result_queue`."""
        pass

    # }}}


__all__ = [
    'Source',
    'Option',
    'create_log',
    'fixcase',
    'fixauthors',
    'cap_author_token',
    'cleanup_title',
    'InternalMetadataCompareKeyGen',
    'get_cached_cover_urls',
    'dump_caches',
    'load_caches',
    'ThreadSafeLog',
]
