# Derived from calibre's src/calibre/utils/ipc/job.py (calibre 9.15).
# Copyright: 2009, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.utils.ipc.job`: the job objects the GUI's
JobManager holds (`BaseJob`, `ParallelJob`)."""

import os
import time
from queue import Empty, Queue


class BaseJob:
    STATUS_TEXTS = {
        'running': 'Working',
        'done': 'Finished',
        'failed': 'Error',
        'killed': 'Stopped',
        'waiting': 'Waiting',
    }
    _counter = [0]

    def __init__(self, description, done=lambda x: x):
        BaseJob._counter[0] += 1
        self.id = BaseJob._counter[0]
        self.description = description
        self.done = done
        self.done2 = None
        self.killed = False
        self.failed = False
        self.kill_on_start = False
        self.start_time = None
        self.result = None
        self.duration = None
        self.log_path = None
        self.notifications = Queue()

        self._run_state = None
        self._status_text = 'Waiting...'
        self._done_called = False
        self.core_usage = 1
        self.timed_out = False

    def update(self, consume_notifications=True):
        if self.duration is not None:
            self._run_state = 'done'
        elif self.start_time is not None:
            self._run_state = 'running'
        else:
            self._run_state = 'waiting'
        if consume_notifications:
            self.consume_notifications()
        if self.is_finished:
            if self.killed:
                self._status_text = self.STATUS_TEXTS['killed']
            elif self.failed:
                self._status_text = self.STATUS_TEXTS['failed']
            else:
                self._status_text = self.STATUS_TEXTS['done']
            if not self._done_called:
                self._done_called = True
                try:
                    self.done(self)
                except Exception:
                    import traceback

                    traceback.print_exc()
                try:
                    if callable(self.done2):
                        self.done2(self)
                except Exception:
                    import traceback

                    traceback.print_exc()

    def consume_notifications(self):
        got_notification = False
        while self.notifications is not None:
            try:
                self.percent, self._message = self.notifications.get_nowait()
                self.percent *= 100.0
                got_notification = True
            except Empty:
                break
        return got_notification

    percent = 0
    _message = None

    @property
    def status_text(self):
        if self._run_state == 'running' and self._message is not None:
            return self._message
        return self._status_text

    @property
    def is_finished(self):
        return self.duration is not None

    @property
    def is_started(self):
        return self.start_time is not None

    @property
    def is_running(self):
        return self.is_started and not self.is_finished

    @property
    def running_time(self):
        if self.start_time is None:
            return ''
        d = self.duration if self.duration is not None else time.time() - self.start_time
        return '%d:%02d' % divmod(int(d), 60)

    def __lt__(self, other):
        return self.id < getattr(other, 'id', 0)

    @property
    def details(self):
        return self.log_path and self.read_log() or ''

    def read_log(self):
        if self.log_path and os.path.exists(self.log_path):
            with open(self.log_path, 'rb') as f:
                return f.read().decode('utf-8', 'replace')
        return ''

    @property
    def log_file(self):
        return self.log_path


class ParallelJob(BaseJob):
    """A job run in a worker process: `name` picks the worker function
    ('arbitrary', 'arbitrary_n', …), `args`/`kwargs` go to it."""

    def __init__(self, name, description, done, args=[], kwargs={}):
        self.name, self.args, self.kwargs = name, args, kwargs
        BaseJob.__init__(self, description, done)
        self.returncode = None
        self.output = None
        self.process = None

    def read_log(self):
        if self.log_path is None:
            return ''
        return BaseJob.read_log(self)

    @property
    def details(self):
        if self.log_path is None:
            return 'Job not started yet' if self.start_time is None else ''
        return self.read_log()

    @property
    def log(self):
        return self.read_log()


__all__ = ['BaseJob', 'ParallelJob']
