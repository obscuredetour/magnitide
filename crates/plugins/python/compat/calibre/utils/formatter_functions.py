"""Mirrors the commonly used functions of calibre's template language
(`calibre.utils.formatter_functions`) for the subset the formatter
supports: text and list helpers, conditionals and arithmetic, in calibre's
`{field:func(args)}` form, where the field's value is the first argument.
"""

import re


class FormatterFunction:
    name = 'no name provided'
    arg_count = 0
    category = 'Other'

    def evaluate(self, formatter, kwargs, mi, locals, *args):
        raise NotImplementedError()

    def eval_(self, formatter, kwargs, mi, locals, *args):
        return self.evaluate(formatter, kwargs, mi, locals, *args)


def _n(x):
    try:
        return float(x or 0)
    except Exception:
        return 0.0


def _fmt_num(x):
    return ('%d' % x) if float(x).is_integer() else str(x)


class _F(FormatterFunction):
    def __init__(self, name, func, arg_count=-1):
        self.name = name
        self.func = func
        self.arg_count = arg_count

    def evaluate(self, formatter, kwargs, mi, locals, *args):
        return self.func(formatter, mi, *args)


def _lookup(f, mi, val, *args):
    # lookup(val, pattern, field, ..., else_field)
    if len(args) == 1:
        return f.get_value(args[0], [], f.kwargs)
    pats = list(args[:-1])
    while len(pats) >= 2:
        pat, field = pats[0], pats[1]
        if re.search(pat, val, re.IGNORECASE):
            return f.get_value(field, [], f.kwargs)
        pats = pats[2:]
    return f.get_value(args[-1], [], f.kwargs)


def _switch(f, mi, val, *args):
    pats = list(args)
    while len(pats) >= 2:
        if re.search(pats[0], val, re.IGNORECASE):
            return pats[1]
        pats = pats[2:]
    return pats[0] if pats else ''


def _select(f, mi, val, key):
    for item in val.split(','):
        k, _, v = item.strip().partition(':')
        if k == key:
            return v
    return ''


def _sublist(f, mi, val, start, end, sep):
    items = [x.strip() for x in val.split(sep)] if val else []
    s, e = int(start), int(end)
    if e == 0:
        e = len(items)
    return sep.join(items[s:e])


def _list_item(f, mi, val, index, sep):
    items = [x.strip() for x in val.split(sep)] if val else []
    try:
        return items[int(index)]
    except IndexError:
        return ''


def _format_date(f, mi, val, fmt):
    if not val:
        return ''
    from calibre.utils.date import format_date, parse_date

    try:
        d = parse_date(val)
        return format_date(d, fmt)
    except Exception:
        return 'BAD DATE'


def _format_number(f, mi, val, template):
    if not val:
        return ''
    try:
        v = float(val)
        if template.startswith('{'):
            return template.format(v)
        return template % v
    except Exception:
        try:
            return (template % int(float(val))) if '%' in template else template.format(int(float(val)))
        except Exception:
            return ''


def _shorten(f, mi, val, left, middle, right):
    l, r = int(left), int(right)
    if len(val) > l + len(middle) + r:
        return val[:l] + middle + (val[-r:] if r > 0 else '')
    return val


_FUNCS = {
    'lookup': _lookup,
    'switch': _switch,
    'select': _select,
    'sublist': _sublist,
    'list_item': _list_item,
    'format_date': _format_date,
    'format_number': _format_number,
    'shorten': _shorten,
    'ifempty': lambda f, mi, val, text: val if val else text,
    'test': lambda f, mi, val, yes, no: yes if val else no,
    'contains': lambda f, mi, val, pat, yes, no: yes if re.search(pat, val, re.IGNORECASE) else no,
    're': lambda f, mi, val, pat, repl: re.sub(pat, repl, val, flags=re.IGNORECASE),
    're_group': lambda f, mi, val, pat, *tmpls: ''.join(tmpls) if re.search(pat, val) else '',
    'uppercase': lambda f, mi, val: val.upper(),
    'lowercase': lambda f, mi, val: val.lower(),
    'titlecase': lambda f, mi, val: __import__('calibre.utils.titlecase', fromlist=['titlecase']).titlecase(val),
    'capitalize': lambda f, mi, val: val[:1].upper() + val[1:].lower(),
    'strcat': lambda f, mi, val, *args: ''.join(args),
    'strlen': lambda f, mi, val, *args: str(len(args[0] if args else val)),
    'substr': lambda f, mi, val, start, end: val[int(start) : (int(end) or None)],
    'count': lambda f, mi, val, sep: str(len([x for x in val.split(sep) if x.strip()])) if val else '0',
    'list_count': lambda f, mi, val, sep: str(len([x for x in val.split(sep) if x.strip()])) if val else '0',
    'add': lambda f, mi, val, *args: _fmt_num(sum(_n(a) for a in args)),
    'subtract': lambda f, mi, val, a, b: _fmt_num(_n(a) - _n(b)),
    'multiply': lambda f, mi, val, *args: _fmt_num(
        __import__('functools').reduce(lambda x, y: x * y, (_n(a) for a in args), 1.0)
    ),
    'divide': lambda f, mi, val, a, b: _fmt_num(_n(a) / _n(b)) if _n(b) else '',
    'cmp': lambda f, mi, val, x, y, lt, eq, gt: lt if _n(x) < _n(y) else (eq if _n(x) == _n(y) else gt),
    'strcmp': lambda f, mi, val, x, y, lt, eq, gt: (
        lt if x.lower() < y.lower() else (eq if x.lower() == y.lower() else gt)
    ),
    'first_non_empty': lambda f, mi, val, *args: next((a for a in args if a), ''),
    'raw_field': lambda f, mi, val, name, *rest: str(f.get_value(name, [], f.kwargs)),
    'field': lambda f, mi, val, name: str(f.get_value(name, [], f.kwargs)),
    'template': lambda f, mi, val, tmpl: f.evaluate(
        tmpl.replace('[[', '{').replace(']]', '}'), [], f.kwargs, f.global_vars
    ),
    'eval': lambda f, mi, val, tmpl: f.evaluate(
        tmpl.replace('[[', '{').replace(']]', '}'), [], f.kwargs, f.global_vars
    ),
    'assign': lambda f, mi, val, name, value: f.locals.__setitem__(name, value) or value,
    'print': lambda f, mi, val, *args: '',
    'swap_around_comma': lambda f, mi, val: (
        ' '.join(reversed([x.strip() for x in val.split(',', 1)])) if ',' in val else val
    ),
    'swap_around_articles': lambda f, mi, val, sep: val,
    'in_list': lambda f, mi, val, sep, pat, yes, no: (
        yes if any(re.search(pat, x.strip(), re.IGNORECASE) for x in val.split(sep)) else no
    ),
    'str_in_list': lambda f, mi, val, sep, s, yes, no: (
        yes if s.lower() in [x.strip().lower() for x in val.split(sep)] else no
    ),
    'subitems': lambda f, mi, val, start, end: ', '.join(
        '.'.join(x.strip().split('.')[int(start) : (int(end) or None)]) for x in val.split(',')
    ),
    'booksize': lambda f, mi, val: str(getattr(mi, 'size', '') or ''),
    'ondevice': lambda f, mi, val: '',
    'current_library_name': lambda f, mi, val: '',
    'current_library_path': lambda f, mi, val: '',
    'has_cover': lambda f, mi, val: (
        'Yes' if getattr(mi, 'cover', None) or (getattr(mi, 'cover_data', (None, None))[1]) else ''
    ),
    'approximate_formats': lambda f, mi, val: ','.join(getattr(mi, 'formats', None) or []),
    'formats_modtimes': lambda f, mi, val, fmt: '',
    'formats_sizes': lambda f, mi, val: '',
    'formats_paths': lambda f, mi, val: '',
    'identifier_in_list': lambda f, mi, val, idtype, yes, no: (
        yes if any(x.strip().split(':')[0] == idtype for x in val.split(',')) else no
    ),
    'language_strings': lambda f, mi, val, localize: ', '.join(
        __import__('calibre.utils.localization', fromlist=['get_language']).get_language(x.strip())
        for x in val.split(',')
        if x.strip()
    ),
    'language_codes': lambda f, mi, val: ', '.join(
        __import__('calibre.utils.localization', fromlist=['canonicalize_lang']).canonicalize_lang(x.strip()) or ''
        for x in val.split(',')
    ),
    'rating_to_stars': lambda f, mi, val, half: __import__(
        'calibre.ebooks.metadata', fromlist=['rating_to_stars']
    ).rating_to_stars(int(_n(val)), half == '1'),
    'today': lambda f, mi, val: __import__('calibre.utils.date', fromlist=['format_date']).format_date(
        __import__('calibre.utils.date', fromlist=['now']).now(), 'iso'
    ),
    'days_between': lambda f, mi, val, a, b: _days_between(a, b),
    'human_readable': lambda f, mi, val: __import__('calibre', fromlist=['human_readable']).human_readable(
        int(_n(val))
    ),
    'trim': lambda f, mi, val: val.strip(),
    'or': lambda f, mi, val, *args: '1' if any(args) else '',
    'and': lambda f, mi, val, *args: '1' if all(args) else '',
    'not': lambda f, mi, val, x: '' if x else '1',
    'check_yes_no': lambda f, mi, val, field, is_undefined, is_false, is_true: _check_yes_no(
        f, field, is_undefined, is_false, is_true
    ),
}


def _check_yes_no(f, field, is_undefined, is_false, is_true):
    v = f.get_value(field, [], f.kwargs)
    if v in (None, '') and is_undefined == '1':
        return 'Yes'
    if v in ('No', 'False', False, 'false', 'no') and is_false == '1':
        return 'Yes'
    if v in ('Yes', 'True', True, 'true', 'yes') and is_true == '1':
        return 'Yes'
    return ''


def _days_between(a, b):
    from calibre.utils.date import parse_date

    try:
        return _fmt_num((parse_date(a) - parse_date(b)).total_seconds() / 86400)
    except Exception:
        return ''


class _Registry:
    def __init__(self):
        self._functions = {name: _F(name, func) for name, func in _FUNCS.items()}
        self._builtins = dict(self._functions)

    def get_functions(self):
        return self._functions

    def get_builtins(self):
        return self._builtins

    def get_builtins_and_aliases(self):
        return self._builtins

    def register_function(self, function):
        self._functions[function.name] = function

    def unregister_function(self, name):
        self._functions.pop(name, None)

    def load_user_template_functions(self, library_uuid, funcs, precompiled_user_functions=None):
        pass

    def unload_user_template_functions(self, library_uuid):
        pass

    def register_functions(self, library_uuid, funcs):
        pass


_registry = _Registry()


def formatter_functions():
    return _registry


def load_user_template_functions(*a, **k):
    pass


def unload_user_template_functions(*a, **k):
    pass


def compile_user_template_functions(*a, **k):
    return {}


__all__ = [
    'FormatterFunction',
    'formatter_functions',
    'load_user_template_functions',
    'unload_user_template_functions',
    'compile_user_template_functions',
]
