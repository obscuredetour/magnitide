"""Mirrors calibre's `calibre.utils.ipc` package: running work in other
processes (`simple_worker.fork_job`, `server.Server` for the GUI's jobs)."""

import errno
import os
import sys


def eintr_retry_call(func, *args, **kwargs):
    while True:
        try:
            return func(*args, **kwargs)
        except OSError as e:
            if getattr(e, 'errno', None) in (errno.EINTR, errno.EAGAIN):
                continue
            raise


def gui_socket_address():
    return None


def worker_script():
    """The script Estuary's interpreter runs as a worker."""
    here = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    return os.path.join(os.path.dirname(here), 'magnitide_host', 'run-calibre-worker.py')


def worker_command():
    """The command to start a worker: the interpreter, isolated, the worker script."""
    return [sys.executable, '-I', '-X', 'utf8', worker_script()]


RC = None
