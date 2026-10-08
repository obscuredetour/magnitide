"""Mirrors calibre's `calibre.utils.ipc.simple_worker` (calibre 9.15):
`fork_job`, which runs a function in a worker process and returns its
result, and the worker-side `arbitrary` functions."""

import os
import subprocess
import sys
import time
import traceback
from threading import Thread

from calibre.constants import iswindows
from calibre.ptempfile import base_dir
from calibre.utils.ipc import worker_command
from calibre.utils.ipc.launch import Worker


class WorkerError(Exception):
    def __init__(self, msg, orig_tb='', log_path=None):
        Exception.__init__(self, msg)
        self.orig_tb = orig_tb
        self.log_path = log_path


class ConnectedWorker(Thread):
    """Reads a worker's stdout for progress lines while it runs."""

    def __init__(self, process, notifications=None):
        Thread.__init__(self, daemon=True)
        self.process = process
        self.notifications = notifications
        self.lines = []

    def run(self):
        try:
            for raw in self.process.stdout:
                line = raw.decode('utf-8', 'replace').rstrip('\n')
                if line.startswith('NOTIFY '):
                    try:
                        import json

                        percent, msg = json.loads(line[7:])
                        if self.notifications is not None:
                            self.notifications.put((percent / 100.0 if percent > 1 else percent, msg))
                    except Exception:
                        pass
                else:
                    self.lines.append(line)
        except Exception:
            pass


def compile_code(src):
    if not isinstance(src, str):
        src = src.decode('utf-8')
    namespace = {}
    exec(src, namespace)
    return namespace


def _spec_files(prefix='fork_job'):
    d = base_dir()
    base = os.path.join(d, '%s_%d_%d' % (prefix, os.getpid(), int(time.time() * 1000)))
    n = 0
    while os.path.exists(base + '_%d.spec' % n):
        n += 1
    base += '_%d' % n
    return base + '.spec', base + '.result'


def _write_spec(path, spec):
    from calibre.utils.serialize import pickle_dumps

    with open(path, 'wb') as f:
        f.write(pickle_dumps(spec))


def _read_result(path):
    from calibre.utils.serialize import pickle_loads

    with open(path, 'rb') as f:
        return pickle_loads(f.read())


def fork_job(
    mod_name,
    func_name,
    args=(),
    kwargs=None,
    timeout=300,
    cwd=None,
    priority='normal',
    env={},
    no_output=False,
    heartbeat=None,
    abort=None,
    module_is_source_code=False,
    notifications=None,
):
    """Run `mod_name.func_name(*args, **kwargs)` in a worker process, waiting
    at most `timeout` seconds. Returns {'result': …, 'stdout_stderr': path
    of the worker's output (None when `no_output`)}. A failure raises
    `WorkerError` with the worker's traceback in `orig_tb`."""
    kwargs = kwargs or {}
    spec_path, result_path = _spec_files()
    _write_spec(
        spec_path,
        {
            'name': 'arbitrary',
            'args': (mod_name, func_name, args, kwargs, module_is_source_code),
            'kwargs': {},
            'result': result_path,
        },
    )
    w = Worker(env, job_name=func_name)
    w.spec_path = spec_path
    try:
        w(redirect_output=not no_output, cwd=cwd, priority=priority)
    except Exception as e:
        raise WorkerError(f'Failed to start the worker: {e}')
    reader = ConnectedWorker(w.child, notifications)
    reader.start()
    start = time.monotonic()
    aborted = False
    while w.is_alive:
        if timeout and time.monotonic() - start > timeout:
            w.kill()
            raise WorkerError('Worker appears to have hung', log_path=w.log_path)
        if abort is not None and abort.is_set():
            w.kill()
            aborted = True
            break
        if heartbeat is not None:
            try:
                heartbeat()
            except Exception:
                pass
        time.sleep(0.05)
    reader.join(2)
    w.close_log_file()
    if aborted:
        raise WorkerError('Worker aborted', log_path=w.log_path)
    try:
        ans = _read_result(result_path)
    except Exception:
        raise WorkerError(
            'Worker failed to produce a result (exit code %r)' % w.returncode,
            log_path=w.log_path,
            orig_tb='\n'.join(reader.lines),
        )
    finally:
        for p in (spec_path, result_path):
            try:
                os.remove(p)
            except OSError:
                pass
    if 'error' in ans:
        raise WorkerError(
            'Worker failed with error: ' + ans['error'], orig_tb=ans.get('traceback', ''), log_path=w.log_path
        )
    return {'result': ans.get('result'), 'stdout_stderr': w.log_path}


def two_part_fork_job(env=None, priority='normal', cwd=None):
    """calibre's: a worker you start, then give a job to. Here `fork_job`
    does both at once."""

    class _Deferred:
        def __call__(self, mod_name, func_name, args=(), kwargs=None, timeout=300, module_is_source_code=False, **kw):
            return fork_job(
                mod_name,
                func_name,
                args,
                kwargs,
                timeout=timeout,
                cwd=cwd,
                priority=priority,
                env=env or {},
                module_is_source_code=module_is_source_code,
            )

    return _Deferred()


def offload_worker(env={}, priority='normal', cwd=None):
    """An object to call `(mod_name, func_name, *args)` on, running each in a
    worker; `shutdown()` when done."""

    class _Offload:
        def __call__(self, mod_name, func_name, *args, **kwargs):
            r = fork_job(mod_name, func_name, args, kwargs, env=env, priority=priority, cwd=cwd, no_output=True)
            return r['result']

        def shutdown(self):
            pass

        def is_alive(self):
            return True

    return _Offload()


def start_pipe_worker(command, env=None, priority='normal', **process_args):
    """A worker running `command` (Python source) with pipes to it."""
    args = {'stdout': subprocess.PIPE, 'stdin': subprocess.PIPE}
    args.update(process_args)
    full_env = os.environ.copy()
    full_env.update(env or {})
    from polyglot.binary import as_hex_unicode

    full_env['CALIBRE_WORKER_TEMP_DIR'] = as_hex_unicode(base_dir().encode('utf-8'))
    if iswindows:
        args['creationflags'] = subprocess.CREATE_NO_WINDOW
    exe = worker_command()[:-1]  # the interpreter, isolated, without the worker script
    boot = (
        'import sys, os; sys.path.insert(0, %r); from magnitide_host import bootstrap; bootstrap.setup(); '
        % os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
    )
    return subprocess.Popen(exe + ['-c', boot + command], env=full_env, **args)


def arbitrary(module_name, func_name, args, kwargs={}, module_is_source_code=False):
    """Worker side: import `module_name` (or compile it, when it's source)
    and call its `func_name`."""
    if module_is_source_code:
        mod = compile_code(module_name)
        func = mod[func_name]
    else:
        import importlib

        mod = importlib.import_module(module_name)
        func = getattr(mod, func_name)
    return func(*args, **kwargs)


def arbitrary_n(module_name, func_name, args, kwargs={}, module_is_source_code=False, notification=lambda x, y: y):
    """Like `arbitrary`, with a `notification(percent, message)` passed in."""
    if module_is_source_code:
        mod = compile_code(module_name)
        func = mod[func_name]
    else:
        import importlib

        mod = importlib.import_module(module_name)
        func = getattr(mod, func_name)
    kwargs = dict(kwargs)
    kwargs['notification'] = notification
    return func(*args, **kwargs)


__all__ = [
    'fork_job',
    'WorkerError',
    'compile_code',
    'two_part_fork_job',
    'offload_worker',
    'start_pipe_worker',
    'arbitrary',
    'arbitrary_n',
    'sys',
    'traceback',
]
