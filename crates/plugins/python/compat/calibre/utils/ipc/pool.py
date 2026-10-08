"""Mirrors calibre's `calibre.utils.ipc.pool`: a pool of workers given
tasks by id; results come back on a queue."""

from collections import namedtuple
from queue import Queue
from threading import Thread

from calibre.utils.ipc.simple_worker import fork_job

Result = namedtuple('Result', 'value err traceback')
WorkerResult = namedtuple('WorkerResult', 'id result is_terminal_failure worker')


class Failure(Exception):
    def __init__(self, tf):
        Exception.__init__(self, tf.message)
        self.details = tf.details
        self.job_id = tf.job_id
        self.failure_message = tf.message


class Pool(Thread):
    def __init__(self, max_workers=None, name=None):
        Thread.__init__(self, name=name)
        self.daemon = True
        self.max_workers = max_workers or 2
        self.results = Queue()
        self.tasks = Queue()
        self.failed = False
        self.shutting_down = False
        self.start()

    def __call__(self, job_id, module, func, *args, **kwargs):
        self.tasks.put((job_id, module, func, args, kwargs))

    def run(self):
        while not self.shutting_down:
            try:
                task = self.tasks.get(timeout=0.2)
            except Exception:
                continue
            job_id, module, func, args, kwargs = task
            try:
                r = fork_job(module, func, args, kwargs)
                self.results.put(WorkerResult(job_id, Result(r['result'], None, None), False, None))
            except Exception as e:
                import traceback

                self.results.put(WorkerResult(job_id, Result(None, str(e), traceback.format_exc()), False, None))

    def wait_for_tasks(self, timeout=None):
        import time

        start = time.monotonic()
        while not self.tasks.empty():
            if timeout is not None and time.monotonic() - start > timeout:
                return False
            time.sleep(0.05)
        return True

    def shutdown(self, wait_time=0.1):
        self.shutting_down = True

    def join(self, timeout=None):
        Thread.join(self, timeout)


__all__ = ['Pool', 'Result', 'WorkerResult', 'Failure']
