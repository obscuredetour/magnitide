# Derived from calibre's src/calibre/db/categories.py (calibre 9.15).
# Copyright: 2013, Kovid Goyal <kovid at kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.db.categories`: the Tag browser's data, as
`db.get_categories()` gives it: field → list of `Tag`."""

from calibre.utils.icu import sort_key


class Tag:
    __slots__ = (
        'name',
        'original_name',
        'id',
        'count',
        'state',
        'is_hierarchical',
        'is_editable',
        'is_searchable',
        'id_set',
        'avg_rating',
        'sort',
        'use_sort_as_name',
        'tooltip',
        'icon',
        'category',
        'search_expression',
        'original_categories',
    )

    def __init__(
        self,
        name,
        id=None,
        count=0,
        state=0,
        avg=0,
        sort=None,
        tooltip=None,
        icon=None,
        category=None,
        id_set=None,
        search_expression=None,
        is_editable=True,
        is_searchable=True,
        use_sort_as_name=False,
        original_categories=None,
    ):
        self.name = self.original_name = name
        self.id = id
        self.count = count
        self.state = state
        self.is_hierarchical = ''
        self.is_editable = is_editable
        self.is_searchable = is_searchable
        self.id_set = id_set if id_set is not None else set()
        self.avg_rating = avg / 2.0 if avg is not None else 0
        self.sort = sort
        self.use_sort_as_name = use_sort_as_name
        self.tooltip = tooltip
        self.icon = icon
        self.category = category
        self.search_expression = search_expression
        self.original_categories = original_categories

    @property
    def string_representation(self):
        return '%s:%s:%s:%s:%s' % (self.name, self.count, self.id, self.state, self.category)

    def __str__(self):
        return self.string_representation

    def __repr__(self):
        return str(self)


def get_categories(dbcache, sort='name', book_ids=None, first_letter_sort=False):
    """Every category field's items with counts: {'tags': [Tag, …], …}."""
    fm = dbcache.field_metadata
    ans = {}
    book_ids = None if book_ids is None else frozenset(book_ids)
    for category in fm:
        meta = fm[category]
        if not meta.get('is_category') or meta['kind'] != 'field':
            continue
        if category == 'formats':
            tags = []
            counts = {}
            for b in book_ids if book_ids is not None else dbcache.all_book_ids():
                for f in dbcache.field_for('formats', b, ()) or ():
                    counts[f] = counts.get(f, 0) + 1
            for name, count in counts.items():
                tags.append(Tag(name, count=count, category=category, icon=None))
        elif category == 'identifiers':
            counts = {}
            for b in book_ids if book_ids is not None else dbcache.all_book_ids():
                for k in dbcache.field_for('identifiers', b, {}) or {}:
                    counts[k] = counts.get(k, 0) + 1
            tags = [Tag(name, count=count, category=category) for name, count in counts.items()]
        else:
            id_map = dbcache.get_id_map(category)
            usage = dbcache.get_usage_count_by_id(category)
            tags = []
            for item_id, name in id_map.items():
                books = dbcache.books_for_field(category, item_id)
                if book_ids is not None:
                    books = books & book_ids
                    if not books:
                        continue
                count = len(books) if book_ids is not None else usage.get(item_id, 0)
                if count == 0:
                    continue
                rating = 0
                if category != 'rating':
                    ratings = [dbcache.field_for('rating', b) or 0 for b in books]
                    ratings = [r for r in ratings if r]
                    rating = sum(ratings) / len(ratings) if ratings else 0
                name = str(name) if category != 'rating' else name
                tags.append(
                    Tag(
                        name if category != 'rating' else '★' * (name // 2),
                        id=item_id,
                        count=count,
                        avg=rating,
                        category=category,
                        id_set=set(books),
                    )
                )
        if sort == 'popularity':
            tags.sort(key=lambda t: t.count, reverse=True)
        elif sort == 'rating':
            tags.sort(key=lambda t: t.avg_rating, reverse=True)
        else:
            tags.sort(key=lambda t: sort_key(t.sort or t.name))
        ans[category] = tags
    return ans


def is_standard_category(key):
    return not (key.startswith('@') or key == 'search')


__all__ = ['Tag', 'get_categories', 'is_standard_category']
