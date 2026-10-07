"""Mirrors calibre's `calibre.db.locking`: the database's read/write locks
(here one re-entrant lock; calibre's is a shared/exclusive pair)."""

import threading
from contextlib import contextmanager


class LockingError(RuntimeError):
    def __init__(self, msg, extra=None, *args):
        RuntimeError.__init__(self, msg, *args)
        self.locking_debug_msg = extra


class DowngradeLockError(LockingError):
    pass


class RecordLock:
    def __init__(self, lock):
        self.lock = lock

    @contextmanager
    def lock(self):
        with self.lock:
            yield


def create_locks():
    lock = threading.RLock()
    return lock, lock


class SafeReadLock:
    def __init__(self, lock):
        self.lock = lock

    def __enter__(self):
        self.lock.acquire()

    def __exit__(self, *args):
        self.lock.release()
