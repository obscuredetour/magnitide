"""Mirrors calibre's `calibre.utils.ipc.worker`: what runs in a worker
process. The job (a spec file: the function to run, its arguments) comes
from the parent; the result goes back to a file; progress lines go to
stdout as `NOTIFY <json>`; everything else printed goes to the log."""

import importlib
import io
import json
import os
import sys
import traceback

# name → (module, function, notification keyword or None)
PARALLEL_FUNCS = {
    'arbitrary': ('calibre.utils.ipc.simple_worker', 'arbitrary', None),
    'arbitrary_n': ('calibre.utils.ipc.simple_worker', 'arbitrary_n', 'notification'),
    'read_metadata': ('calibre.ebooks.metadata.worker', 'read_metadata', 'notification'),
    'gui_convert': ('calibre.gui2.convert.gui_conversion', 'gui_convert', 'notification'),
    'gui_convert_override': ('calibre.gui2.convert.gui_conversion', 'gui_convert_override', 'notification'),
    'gui_convert_recipe': ('calibre.gui2.convert.gui_conversion', 'gui_convert_recipe', 'notification'),
    'gui_polish': ('calibre.ebooks.oeb.polish.main', 'gui_polish', None),
    'gui_catalog': ('calibre.gui2.convert.gui_conversion', 'gui_catalog', 'notification'),
    'save_book': ('calibre.library.save_to_disk', 'save_book', None),
}


class Progress:
    """Progress callback: `notification(percent, message)` to the parent."""

    def __init__(self, stream):
        self.stream = stream

    def __call__(self, percent, msg=''):
        try:
            self.stream.write('NOTIFY ' + json.dumps([float(percent), str(msg)]) + '\n')
            self.stream.flush()
        except Exception:
            pass


def _load(spec_path):
    from calibre.utils.serialize import pickle_loads

    with open(spec_path, 'rb') as f:
        return pickle_loads(f.read())


def _dump(path, obj):
    from calibre.utils.serialize import pickle_dumps

    with open(path, 'wb') as f:
        f.write(pickle_dumps(obj))


def main():
    """`run-calibre-worker.py <spec file>`: run the job the spec names."""
    args = [a for a in sys.argv[1:] if a not in ('calibre-debug', 'calibre-parallel', 'calibre')]
    spec_path = args[0] if args else os.environ.get('MAGNITIDE_WORKER_SPEC')
    if not spec_path:
        print('no job spec', file=sys.stderr)
        return 1
    spec = _load(spec_path)
    name = spec['name']
    args, kwargs = spec.get('args', ()), spec.get('kwargs', {}) or {}
    result_path = spec['result']
    notify = Progress(os.fdopen(os.dup(1), 'w', encoding='utf-8', buffering=1))
    # What the job prints is the log, not the protocol.
    os.dup2(2, 1)
    sys.stdout = sys.stderr
    try:
        if name in PARALLEL_FUNCS:
            mod, func, notification = PARALLEL_FUNCS[name]
            fn = getattr(importlib.import_module(mod), func)
            if notification:
                kwargs = dict(kwargs)
                kwargs[notification] = notify
            result = fn(*args, **kwargs)
        else:
            mod, _, func = name.rpartition('.')
            fn = getattr(importlib.import_module(mod), func)
            result = fn(*args, **kwargs)
        _dump(result_path, {'result': result})
    except BaseException as e:
        tb = traceback.format_exc()
        print(tb, file=sys.stderr, flush=True)
        _dump(result_path, {'error': f'{type(e).__name__}: {e}', 'traceback': tb})
        return 1
    return 0


__all__ = ['main', 'PARALLEL_FUNCS', 'Progress', 'io']
