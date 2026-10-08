"""Mirrors the part of calibre's template language (`calibre.utils.formatter`)
that metadata code and plugins use: `{field}`, `{field:prefix|suffix}`,
`{field:func(args)}` with the common functions, and `TemplateFormatter`'s
`safe_format` / `unsafe_format` interface.

calibre's full language ('program:' templates with variables, `if`, `for`,
and over a hundred functions) isn't provided; such a template gives an
error string rather than a result.
"""

import re
import traceback

from calibre.utils.formatter_functions import formatter_functions


class _Parser:
    """`{name}` and `{name:format}` fields with calibre's `|prefix|suffix`
    and `func(args)` extensions."""

    _field = re.compile(r'\{([^{}]*)\}')


class TemplateFormatter:
    def __init__(self):
        self.book = None
        self.kwargs = None
        self.strip_results = True
        self.column_name = None
        self.template_cache = None
        self.global_vars = {}
        self.composite_values = {}
        self.funcs = formatter_functions().get_functions()
        self.locals = {}

    def _do_format(self, val, fmt):
        if not fmt or not val:
            return val
        if val == self.NOT_SET:
            return ''
        if fmt[0] == ',':
            fmt = fmt[1:]
        try:
            if fmt and fmt[-1] in 'dxXo':
                val = int(val)
            elif fmt and fmt[-1] in 'eEfFgGn%':
                val = float(val)
        except Exception:
            pass
        try:
            return ('{0:' + fmt + '}').format(val)
        except Exception:
            try:
                return ('%' + fmt) % val
            except Exception:
                return val

    NOT_SET = object()

    def get_value(self, key, args, kwargs):
        """The value of a field; subclasses (SafeFormat) look in a Metadata."""
        if key in kwargs:
            return kwargs[key]
        raise ValueError('Value: unknown field ' + key)

    def _eval_field(self, spec):
        """One `{...}` reference: name, optional `:func(args)` or
        `:prefix|suffix` or `:format`."""
        if ':' in spec:
            name, _, rest = spec.partition(':')
        else:
            name, rest = spec, ''
        name = name.strip()
        if name.startswith('$'):
            name = name[1:]
        try:
            val = self.get_value(name, [], self.kwargs or {})
        except Exception:
            val = ''
        if val is None:
            val = ''
        if isinstance(val, (list, tuple)):
            val = ', '.join(str(x) for x in val)
        if not isinstance(val, str):
            val = str(val)
        if not rest:
            return val
        # prefix|suffix form: {field:|prefix|suffix} or {field:format|prefix|suffix}
        if '|' in rest and '(' not in rest.split('|')[0]:
            parts = rest.split('|')
            fmt = parts[0]
            prefix = parts[1] if len(parts) > 1 else ''
            suffix = parts[2] if len(parts) > 2 else ''
            val = self._do_format(val, fmt)
            if not val:
                return ''
            return prefix + val + suffix
        # function form: name(args)
        m = re.match(r'^\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*\((.*)\)\s*$', rest, re.DOTALL)
        if m is not None:
            fname, argstr = m.group(1), m.group(2)
            args = self._split_args(argstr)
            func = self.funcs.get(fname)
            if func is None:
                raise ValueError(f'Unknown function {fname}')
            try:
                return func.evaluate(self, self.kwargs, self.book, self.locals, val, *args)
            except TypeError:
                return func.evaluate(self, self.kwargs, self.book, self.locals, val)
        # a plain Python format spec
        return self._do_format(val, rest)

    @staticmethod
    def _split_args(argstr):
        args, cur, depth, quote = [], [], 0, None
        i = 0
        while i < len(argstr):
            c = argstr[i]
            if quote:
                if c == '\\' and i + 1 < len(argstr):
                    cur.append(argstr[i + 1])
                    i += 2
                    continue
                if c == quote:
                    quote = None
                else:
                    cur.append(c)
            elif c in '\'"':
                quote = c
            elif c == '(':
                depth += 1
                cur.append(c)
            elif c == ')':
                depth -= 1
                cur.append(c)
            elif c == ',' and depth == 0:
                args.append(''.join(cur).strip())
                cur = []
            else:
                cur.append(c)
            i += 1
        if cur or args:
            args.append(''.join(cur).strip())
        return [a.strip() for a in args]

    def evaluate(self, fmt, args, kwargs, global_vars, break_reporter=None):
        self.kwargs = kwargs
        self.global_vars = global_vars
        if fmt.startswith('program:') or fmt.startswith('python:'):
            raise ValueError("'program:' templates aren't supported by Magnitide's compatibility package")
        out = []
        pos = 0
        for m in _Parser._field.finditer(fmt):
            out.append(fmt[pos : m.start()].replace('{{', '{').replace('}}', '}'))
            out.append(self._eval_field(m.group(1)))
            pos = m.end()
        out.append(fmt[pos:].replace('{{', '{').replace('}}', '}'))
        return ''.join(out)

    def safe_format(
        self,
        fmt,
        kwargs,
        error_value,
        book,
        column_name=None,
        template_cache=None,
        strip_results=True,
        template_functions=None,
        global_vars=None,
        break_reporter=None,
        python_context_object=None,
    ):
        """Format `fmt` against `book`; on any error return `error_value`
        (with the error, as calibre does)."""
        self.strip_results = strip_results
        self.column_name = column_name
        self.template_cache = template_cache
        self.book = book
        self.composite_values = {}
        self.locals = {}
        try:
            ans = self.evaluate(fmt, [], kwargs, global_vars or {})
        except Exception as e:
            if getattr(e, 'is_locking_error', False):
                raise
            ans = error_value + ' ' + str(e)
        return ans.strip() if strip_results and isinstance(ans, str) else ans

    def unsafe_format(
        self,
        fmt,
        kwargs,
        book,
        strip_results=True,
        global_vars=None,
        template_functions=None,
        python_context_object=None,
    ):
        self.strip_results = strip_results
        self.book = book
        self.composite_values = {}
        self.locals = {}
        ans = self.evaluate(fmt, [], kwargs, global_vars or {})
        return ans.strip() if strip_results and isinstance(ans, str) else ans

    def validate(self, fmt):
        try:
            self.evaluate(fmt, [], {}, {})
        except Exception:
            return traceback.format_exc()
        return None


class ValidateFormatter(TemplateFormatter):
    def get_value(self, key, args, kwargs):
        return self._validation_string

    def validate(self, x):
        from calibre.ebooks.metadata.book.base import Metadata

        self._validation_string = 'This Is Some Text THAT SHOULD be LONG Enough.%^&*'
        return self.safe_format(x, {}, 'VALIDATE ERROR', Metadata(''))


class EvalFormatter(TemplateFormatter):
    """A template formatter that substitutes values from a dict (kwargs)."""

    def get_value(self, key, args, kwargs):
        if key == '':
            return ''
        key = key.lower()
        return kwargs.get(key, 'NO VALUE')


validation_formatter = ValidateFormatter()
eval_formatter = EvalFormatter()


__all__ = ['TemplateFormatter', 'ValidateFormatter', 'EvalFormatter', 'validation_formatter', 'eval_formatter']
