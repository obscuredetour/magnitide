"""Mirrors calibre's `calibre.gui2.threaded_jobs`: jobs that run on a thread
in the GUI process (`ThreadedJob`), and the server that runs them."""

import os
import time
import traceback
from queue import Empty, Queue
from threading import Event, RLock, Thread

from calibre.ptempfile import PersistentTemporaryFile
from calibre.utils.ipc.job import BaseJob
from calibre.utils.logging import GUILog


class ThreadedJob(BaseJob):
    def __init__(
        self, type_, description, func, args, kwargs, callback, max_concurrent_count=1, killable=True, log=None
    ):
        """`func(*args, **kwargs, notifications=queue, abort=event, log=log)`
        runs on a thread; `callback(job)` afterwards. `type_` groups jobs
        that must not run together more than `max_concurrent_count` at a time."""
        BaseJob.__init__(self, description)

        self.type = type_
        self.max_concurrent_count = max_concurrent_count
        self.killable = killable
        self.callback = callback
        self.abort = Event()
        self.exception = None
        self.log = GUILog() if log is None else log
        self.func = func
        self.args = args
        self.kwargs = kwargs
        self.consolidated_log = None
        self.notifications = Queue()

    def start_work(self):
        self.start_time = time.time()
        self.log('Starting job:', self.description)
        try:
            self.result = self.func(
                *self.args, notifications=self.notifications, abort=self.abort, log=self.log, **self.kwargs
            )
        except Exception as e:
            self.exception = e
            self.failed = True
            self.log.exception('Job: "%s" failed with error:' % self.description)
            self.log.debug('Called with args:', self.args, self.kwargs)
        self.duration = time.time() - self.start_time
        try:
            self.callback(self)
        except Exception:
            traceback.print_exc()
        self._cleanup()

    def _cleanup(self):
        try:
            self.consolidate_log()
        except Exception:
            if self.log is not None:
                self.log.exception('Log consolidation failed')
        self.args = self.kwargs = self.notifications = None

    def kill(self):
        if self.start_time is None:
            self.start_time = time.time()
            self.duration = 0.0001
        else:
            self.duration = time.time() - self.start_time
            self.abort.set()
        self.log('Aborted job:', self.description)
        self.killed = True
        self.failed = True
        self._cleanup()

    def consolidate_log(self):
        logs = [self.log.html, self.log.plain_text]
        self.consolidated_log = PersistentTemporaryFile('_log')
        from calibre.utils.serialize import pickle_dumps

        self.consolidated_log.write(pickle_dumps(logs))
        self.consolidated_log.close()
        self.consolidated_log = self.consolidated_log.name
        self.log_path = self.consolidated_log

    def read_consolidated_log(self):
        from calibre.utils.serialize import pickle_loads

        with open(self.consolidated_log, 'rb') as f:
            return pickle_loads(f.read())

    @property
    def details(self):
        if self.consolidated_log is None:
            return self.log.plain_text
        return self.read_consolidated_log()[1]

    @property
    def html_details(self):
        if self.consolidated_log is None:
            return self.log.html
        return self.read_consolidated_log()[0]

    @property
    def is_finished(self):
        return self.duration is not None


class ThreadedJobServer(Thread):
    def __init__(self):
        Thread.__init__(self)
        self.daemon = True
        self.lock = RLock()
        self.queued_jobs = []
        self.running_jobs = set()
        self.changed_jobs_queue = Queue()
        self.keep_going = True
        self.started = False

    def close(self):
        self.keep_going = False

    def add_job(self, job):
        with self.lock:
            if not self.started:
                self.start()
                self.started = True
            self.queued_jobs.append(job)

    def run(self):
        while self.keep_going:
            try:
                self.run_once()
            except Exception:
                traceback.print_exc()
            time.sleep(0.1)

    def run_once(self):
        with self.lock:
            remove = set()
            for job in self.running_jobs:
                if job.is_finished:
                    remove.add(job)
                    self.changed_jobs_queue.put(job)
            self.running_jobs -= remove
            if self.queued_jobs:
                counts = {}
                for job in self.running_jobs:
                    counts[job.type] = counts.get(job.type, 0) + 1
                for job in list(self.queued_jobs):
                    if counts.get(job.type, 0) < job.max_concurrent_count:
                        self.queued_jobs.remove(job)
                        self.running_jobs.add(job)
                        self.run_job(job)
                        counts[job.type] = counts.get(job.type, 0) + 1

    def run_job(self, job):
        t = Thread(target=job.start_work, name=job.description)
        t.daemon = True
        t.start()
        self.changed_jobs_queue.put(job)

    def kill_job(self, job):
        with self.lock:
            if job in self.queued_jobs:
                self.queued_jobs.remove(job)
            elif job in self.running_jobs:
                self.running_jobs.remove(job)
        job.kill()
        self.changed_jobs_queue.put(job)


__all__ = ['ThreadedJob', 'ThreadedJobServer', 'Empty', 'os']
