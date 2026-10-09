# Derived from calibre's src/calibre/ebooks/metadata/sources/prefs.py (calibre 9.15).
# Copyright: 2013, Kovid Goyal <kovid at kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.ebooks.metadata.sources.prefs`: the settings
shared by all metadata sources (`msprefs`)."""

from calibre.utils.config import JSONConfig

msprefs = JSONConfig('metadata_sources/global.json')
msprefs.defaults['txt_comments'] = False
msprefs.defaults['ignore_fields'] = []
msprefs.defaults['user_default_ignore_fields'] = []
msprefs.defaults['max_tags'] = 20
msprefs.defaults['wait_after_first_identify_result'] = 30  # seconds
msprefs.defaults['wait_after_first_cover_result'] = 60  # seconds
msprefs.defaults['swap_author_names'] = False
msprefs.defaults['fewer_tags'] = True
msprefs.defaults['find_first_edition_date'] = False
msprefs.defaults['append_comments'] = False
msprefs.defaults['tag_map_rules'] = []
msprefs.defaults['author_map_rules'] = []
msprefs.defaults['publisher_map_rules'] = []
msprefs.defaults['series_map_rules'] = []
msprefs.defaults['cover_priorities'] = {'Google': 2, 'Google Images': 2, 'Big Book Search': 2}
msprefs.defaults['keep_dups'] = False
