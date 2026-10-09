#!/usr/bin/env python
# Derived from calibre's resources/default_tweaks.py (calibre 9.15).
# Copyright: 2010, Kovid Goyal <kovid at kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.

# The defaults for calibre's "tweaks", as Magnitide's compatibility package
# provides them (calibre keeps its own at resources/default_tweaks.py). A
# plugin or user may override any of them in <config_dir>/tweaks.json.
# Only the tweaks that calibre's library and metadata code consult are here.

# How an author's sort name is made from the name: 'invert' (Last, First),
# 'copy', 'comma' (Last, First for names without a comma) or 'nocomma'.
author_sort_copy_method = 'comma'

# Words that aren't part of a surname when inverting: suffixes, prefixes and
# "copy words" (names with one of these are copied as they are).
author_name_suffixes = ('Jr', 'Sr', 'Inc', 'Ph.D', 'Phd', 'MD', 'M.D', 'I', 'II', 'III', 'IV', 'Junior', 'Senior')
author_name_prefixes = ('Mr', 'Mrs', 'Ms', 'Dr', 'Prof')
author_name_copywords = (
    'Agency',
    'Corporation',
    'Company',
    'Co.',
    'Council',
    'Committee',
    'Inc.',
    'Institute',
    'National',
    'Society',
    'Club',
    'Team',
    'Software',
    'Games',
    'Entertainment',
    'Media',
    'Studios',
)
author_use_surname_prefixes = False
author_surname_prefixes = ('da', 'de', 'di', 'la', 'le', 'van', 'von')

# How "Author One & Author Two" is split into authors.
authors_split_regex = r'(?i),?\s+(and|with)\s+'

# Title sorting: 'library_order' moves leading articles to the end ("The
# Title" → "Title, The"), 'strictly_alphabetic' leaves titles alone.
title_series_sorting = 'library_order'
save_template_title_series_sorting = 'library_order'
per_language_title_sort_articles = {
    'eng': (r'A\s+', r'The\s+', r'An\s+'),
    'spa': (r'El\s+', r'La\s+', r'Lo\s+', r'Los\s+', r'Las\s+', r'Un\s+', r'Una\s+', r'Unos\s+', r'Unas\s+'),
    'fra': (
        r'Le\s+',
        r'La\s+',
        r"L'",
        r'L’',
        r'Les\s+',
        r'Un\s+',
        r'Une\s+',
        r'Des\s+',
        r'De\s+La\s+',
        r'De\s+',
        r"D'",
        r'D’',
    ),
    'ita': (
        r'Lo\s+',
        r'Il\s+',
        r"L'",
        r'L’',
        r'La\s+',
        r'Gli\s+',
        r'I\s+',
        r'Le\s+',
        r'Uno\s+',
        r'Un\s+',
        r'Una\s+',
        r"Un'",
        r'Un’',
    ),
    'por': (r'A\s+', r'O\s+', r'Os\s+', r'As\s+', r'Um\s+', r'Uns\s+', r'Uma\s+', r'Umas\s+'),
    'ron': (r'Un\s+', r'O\s+', r'Nişte\s+'),
    'deu': (
        r'Der\s+',
        r'Die\s+',
        r'Das\s+',
        r'Den\s+',
        r'Ein\s+',
        r'Eine\s+',
        r'Einen\s+',
        r'Dem\s+',
        r'Des\s+',
        r'Einem\s+',
        r'Eines\s+',
    ),
    'nld': (
        r'De\s+',
        r'Het\s+',
        r'Een\s+',
        r"'n\s+",
        r"'s\s+",
        r'Ene\s+',
        r'Ener\s+',
        r'Enes\s+',
        r'Den\s+',
        r'Der\s+',
        r'Des\s+',
        r"'t\s+",
    ),
    'swe': (r'En\s+', r'Ett\s+', r'Det\s+', r'Den\s+', r'De\s+'),
    'tur': (r'Bir\s+',),
    'afr': (r"'n\s+", r'Die\s+'),
    'ell': (
        r'O\s+',
        r'I\s+',
        r'To\s+',
        r'Ta\s+',
        r'Tus\s+',
        r'Tis\s+',
        r"'Enas\s+",
        r"'Mia\s+",
        r"'Ena\s+",
        r"'Enan\s+",
    ),
    'hun': (r'A\s+', r'Az\s+', r'Egy\s+'),
}
default_language_for_title_sort = None

# The series index given to a book added to a series: 'next', 'first_free',
# 'next_free', 'last_free', 'const', 'no_change'.
series_index_auto_increment = 'next'
use_series_auto_increment_tweak_when_importing = False

# Date formats in the book list.
gui_pubdate_display_format = 'MMM yyyy'
gui_timestamp_display_format = 'dd MMM yyyy'
gui_last_modified_display_format = 'dd MMM yyyy'

# Sorting: digits in names compared as numbers ("Book 2" before "Book 10").
numeric_collation = False

# How to sort by language.
language_sort_by = 'name'

# Tag browser and categories.
categories_use_field_for_author_name = 'author'
categories_collapsed_name_template = r'{first.sort:shorten(4,,0)} - {last.sort:shorten(4,,0)}'
categories_collapsed_rating_template = r'{first.avg_rating:4.2f:ifempty(0)} - {last.avg_rating:4.2f:ifempty(0)}'
categories_collapsed_popularity_template = r'{first.count:d} - {last.count:d}'
categories_collapse_more_than = 20
categories_collapse_model = 'disable'
categories_sort_by = 'name'
maximum_resort_levels = 5
sort_dates_using_visible_fields = False
sort_columns_at_startup = None

# Searching.
limit_search_columns = False
limit_search_columns_to = ['title', 'authors', 'tags', 'series', 'publisher']
search_as_you_type = False
case_sensitive = False
use_primary_find_in_search = True
allow_template_database_functions_in_composites = False

# Metadata.
max_content_server_tags_shown = 5
metadata_single_use_2_cols_for_custom_fields = True
metadata_edit_custom_column_order = []
metadata_edit_bulk_cc_label_length = 25
metadata_edit_elision_point = 'right'
metadata_edit_elide_labels = True
metadata_edit_title_sort_as_author_sort = False
show_saved_search_box = False
public_smtp_relay_delay = 301
public_smtp_relay_host_suffixes = ['gmail.com', 'live.com']
content_server_will_display = ['*']
content_server_wont_display = []
content_server_thumbnail_compression_quality = 75
max_books_to_reorder_in_series = 100

# Comments and templates.
comments_columns_to_display = []
comments_entry_height = 4
show_series_index_in_file_names = False
save_original_format = True
save_original_format_when_polishing = True
compile_gpm_templates = True
template_functions_on_other_thread = True
draw_hidden_section_indicators = True

# Files.
file_dialog_disallow_cookie = False
doubleclick_on_library_view = 'open_viewer'
enter_key_behavior = 'do_nothing'
locale_for_sorting = ''
default_tweak_format = None
save_template_default_format = None
preselect_first_completion = False
completion_mode = 'prefix'
completion_change_to_ascii_sorting = 'auto'
completer_append_separator = False
author_completer_append_separator = False
bool_custom_columns_are_tristate = 'yes'
booklist_transparent_background = False
restrict_output_formats = None
auto_connect_to_folder = ''
highlight_virtual_library_book_count = True
auto_add_path = None
auto_add_check_for_duplicates = False
auto_add_auto_convert = True
dedupe_on_import = False
