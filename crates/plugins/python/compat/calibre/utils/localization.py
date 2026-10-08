"""Mirrors calibre's `calibre.utils.localization` (calibre 9.15): the user
interface language, translations, and language codes and names.

Translations: Magnitide ships none of calibre's, so `_()` returns its
argument. Plugins' own translations work: `load_translations()` (in
`calibre.customize.zipplugin`) installs a plugin's `.mo` into its modules.

Language codes come from `resources/localization/iso639.json`, built from
the iso-codes project's ISO 639-2/3 data: `canonicalize_lang` turns names
("English"), two-letter codes ("en") and bibliographic codes ("ger") into
calibre's three-letter terminology codes ("eng", "deu").
"""

import builtins
import json
import locale
import os
import sys
from functools import lru_cache

_lang_trans = None
_lcase_lang_map = None


def get_lang():
    """The UI language, as 'en' or 'pt_BR'."""
    lang = os.environ.get('CALIBRE_OVERRIDE_LANG')
    if lang:
        return lang.replace('-', '_')
    try:
        from calibre.utils.config_base import prefs

        lang = prefs['language']
    except Exception:
        lang = None
    if lang:
        return lang
    try:
        lang = locale.getlocale()[0]
    except Exception:
        lang = None
    if not lang:
        for var in ('LC_ALL', 'LC_MESSAGES', 'LANG', 'LANGUAGE'):
            v = os.environ.get(var)
            if v:
                lang = v.split(':')[0].split('.')[0]
                break
    if not lang or lang in ('C', 'POSIX'):
        lang = 'en'
    return lang.replace('-', '_')


def is_rtl():
    return get_lang()[:2] in {'he', 'ar'}


def get_lc_messages_path(lang):
    return None


def available_translations():
    return []


def get_translator(bcp_47_code):
    import gettext

    return True, 'en', gettext.NullTranslations()


def set_translators():
    """Install `_` and `ngettext` built-ins: identity translations."""
    import gettext

    t = gettext.NullTranslations()
    builtins.__dict__['_'] = t.gettext
    builtins.__dict__['ngettext'] = t.ngettext
    builtins.__dict__['pgettext'] = t.pgettext
    builtins.__dict__['npgettext'] = t.npgettext
    global _lang_trans
    _lang_trans = t


def set_qt_translator(translator):
    return False


def ngettext(singular, plural, n):
    return singular if n == 1 else plural


def pgettext(context, msg):
    return msg


def translate(lang, text):
    return text


def _(s):
    return builtins.__dict__.get('_', lambda x: x)(s)


# --- Language codes -------------------------------------------------------


@lru_cache(2)
def _iso_data():
    from calibre.utils.resources import get_path

    path = get_path('localization/iso639.json', allow_user_override=False)
    try:
        with open(path, 'rb') as f:
            return json.load(f)
    except OSError:
        # The bare minimum, so the code works without the resource.
        return {
            'by_3': {
                'eng': 'English',
                'deu': 'German',
                'fra': 'French',
                'spa': 'Spanish',
                'ita': 'Italian',
                'por': 'Portuguese',
                'rus': 'Russian',
                'jpn': 'Japanese',
                'zho': 'Chinese',
                'nld': 'Dutch',
                'und': 'Undetermined',
            },
            '2_to_3': {
                'en': 'eng',
                'de': 'deu',
                'fr': 'fra',
                'es': 'spa',
                'it': 'ita',
                'pt': 'por',
                'ru': 'rus',
                'ja': 'jpn',
                'zh': 'zho',
                'nl': 'nld',
            },
            'bib_to_term': {'ger': 'deu', 'fre': 'fra', 'dut': 'nld', 'chi': 'zho'},
        }


def lang_map():
    """3-letter code → English name."""
    return _iso_data()['by_3']


def lang_map_for_ui():
    """3-letter code → name in the UI language (English here)."""
    ans = dict(lang_map())
    ans.pop('und', None)
    return ans


def langnames_to_langcodes(names):
    """Names → 3-letter codes ('English' → 'eng'); unknown names map to None."""
    lm = lang_map()
    rmap = {v.lower(): k for k, v in lm.items()}
    return {n: rmap.get(n.lower()) for n in names}


def lang_as_iso639_1(name_or_code):
    """The 2-letter code for a language, or None."""
    code = canonicalize_lang(name_or_code)
    if code is not None:
        for k, v in _iso_data()['2_to_3'].items():
            if v == code:
                return k
    return None


def get_iso_language(lang_trans, lang):
    return lang_map().get(lang) or lang


def get_language(lang, gettext_func=None):
    """A language's name, from any code."""
    translate = gettext_func or (lambda x: x)
    lang = _lcase_map().get(lang.lower(), lang) if isinstance(lang, str) else lang
    return translate(lang_map().get(lang, lang))


def calibre_langcode_to_name(lc, localize=True):
    return lang_map().get(lc, lc)


def _lcase_map():
    global _lcase_lang_map
    if _lcase_lang_map is None:
        data = _iso_data()
        m = {}
        for code in data['by_3']:
            m[code] = code
        for two, three in data['2_to_3'].items():
            m[two] = three
        for bib, term in data.get('bib_to_term', {}).items():
            m[bib] = term
        for code, name in data['by_3'].items():
            m.setdefault(name.lower(), code)
            for part in name.lower().split(';'):
                m.setdefault(part.strip(), code)
        _lcase_lang_map = m
    return _lcase_lang_map


def canonicalize_lang(raw):
    """calibre's three-letter code for any way of naming a language, or None."""
    if not raw:
        return None
    if not isinstance(raw, str):
        raw = raw.decode('utf-8', 'ignore')
    raw = raw.lower().strip()
    if not raw:
        return None
    raw = raw.replace('_', '-').partition('-')[0].strip()
    if not raw:
        return None
    m = _lcase_map()
    ans = m.get(raw)
    if ans is not None:
        return ans
    if len(raw) == 2:
        return m.get(raw)
    return None


def get_udc():
    """calibre's Unidecode: text → ASCII. Ours uses Unicode decomposition."""
    return _UDC()


class _UDC:
    def decode(self, text):
        import unicodedata

        if isinstance(text, bytes):
            text = text.decode('utf-8', 'replace')
        out = []
        for ch in text:
            if ord(ch) < 128:
                out.append(ch)
                continue
            base = unicodedata.normalize('NFKD', ch)
            ascii_only = ''.join(c for c in base if ord(c) < 128)
            if ascii_only:
                out.append(ascii_only)
            else:
                # A few common ones Unicode doesn't decompose.
                out.append(
                    {
                        'ß': 'ss',
                        'æ': 'ae',
                        'Æ': 'AE',
                        'ø': 'o',
                        'Ø': 'O',
                        'œ': 'oe',
                        'Œ': 'OE',
                        'đ': 'd',
                        'Đ': 'D',
                        'ł': 'l',
                        'Ł': 'L',
                        'þ': 'th',
                        'Þ': 'Th',
                        'ð': 'd',
                        'Ð': 'D',
                        '–': '-',
                        '—': '-',
                        '‘': "'",
                        '’': "'",
                        '“': '"',
                        '”': '"',
                        '…': '...',
                        '«': '"',
                        '»': '"',
                    }.get(ch, '')
                )
        return ''.join(out)


def localize_user_manual_link(url):
    return url


def localize_website_link(url):
    return url


def user_manual_stats():
    return {}


def website_languages():
    return ('en',)


def lang_code_for_user_manual():
    return 'en'


def is_english(lang=None):
    return (lang or get_lang()).startswith('en')


def get_system_locale():
    try:
        lang = locale.getlocale()[0]
    except Exception:
        lang = None
    return lang or 'en'


__all__ = [n for n in dir(sys.modules[__name__]) if not n.startswith('_')] + ['_']
