"""Mirrors calibre's `calibre.utils.ipc.server`: the `Server` that runs the
GUI's `ParallelJob`s in worker processes, a few at a time."""

import os
import time
import traceback
from queue import Queue
from threading import RLock, Thread

from calibre.utils.ipc.launch import Worker
from calibre.utils.ipc.simple_worker import ConnectedWorker, _read_result, _spec_files, _write_spec


class CriticalError(Exception):
    pass


class Server(Thread):
    def __init__(self, notify_on_job_done=lambda x: x, pool_size=None, limit=None, enforce_cpu_limit=True):
        Thread.__init__(self)
        self.daemon = True
        self.pool_size = limit or pool_size or max(1, (os.cpu_count() or 2))
        self.notify_on_job_done = notify_on_job_done
        self.lock = RLock()
        self.waiting_jobs = []
        self.workers = []  # (worker, job, reader)
        self.changed_jobs_queue = Queue()
        self.killed = False
        self.start()

    def add_job(self, job):
        job.done2 = self.notify_on_job_done
        with self.lock:
            self.waiting_jobs.append(job)

    def run_job(self, job, gui=True, redirect_output=False):
        """Start `job` at once, outside the pool (calibre uses this for GUI
        helper apps)."""
        self._start(job, redirect_output=redirect_output)

    def _start(self, job, redirect_output=True):
        spec_path, result_path = _spec_files('job')
        _write_spec(spec_path, {'name': job.name, 'args': job.args, 'kwargs': job.kwargs, 'result': result_path})
        w = Worker({}, job_name=job.name)
        w.spec_path = spec_path
        w.result_path = result_path
        try:
            w(redirect_output=redirect_output)
        except Exception:
            job.failed = True
            job.duration = 0.0001
            job.start_time = time.time()
            job.log_path = None
            self.changed_jobs_queue.put(job)
            traceback.print_exc()
            return
        job.start_time = time.time()
        job.log_path = w.log_path
        job.process = w.child
        reader = ConnectedWorker(w.child, job.notifications)
        reader.start()
        with self.lock:
            self.workers.append((w, job, reader))
        self.changed_jobs_queue.put(job)

    def run(self):
        while not self.killed:
            try:
                self._step()
            except Exception:
                traceback.print_exc()
            time.sleep(0.1)

    def _step(self):
        with self.lock:
            running = [x for x in self.workers if x[0].is_alive]
            for w, job, reader in self.workers:
                if not w.is_alive and job.duration is None:
                    self._finish(w, job, reader)
            self.workers = [x for x in self.workers if x[1].duration is None]
            while self.waiting_jobs and len(running) < self.pool_size:
                job = self.waiting_jobs.pop(0)
                if job.kill_on_start:
                    job.killed = True
                    job.duration = 0.0001
                    job.start_time = time.time()
                    self.changed_jobs_queue.put(job)
                    continue
                self._start(job)
                running.append(job)
            for w, job, reader in self.workers:
                if job.consume_notifications():
                    self.changed_jobs_queue.put(job)

    def _finish(self, w, job, reader):
        reader.join(2)
        w.close_log_file()
        job.returncode = w.returncode
        try:
            ans = _read_result(w.result_path)
        except Exception:
            ans = {'error': 'Worker produced no result (exit code %r)' % w.returncode}
        for p in (w.spec_path, w.result_path):
            try:
                os.remove(p)
            except OSError:
                pass
        if 'error' in ans:
            job.failed = True
            job.output = ans.get('traceback', '') or ans['error']
            job.exception = ans['error']
        else:
            job.result = ans.get('result')
        job.duration = time.time() - job.start_time
        self.changed_jobs_queue.put(job)

    def kill_job(self, job):
        with self.lock:
            if job in self.waiting_jobs:
                self.waiting_jobs.remove(job)
                job.kill_on_start = True
                job.killed = True
                job.duration = 0.0001
                job.start_time = job.start_time or time.time()
                self.changed_jobs_queue.put(job)
                return
            for w, j, reader in self.workers:
                if j is job:
                    w.kill()
                    job.killed = True
                    job.failed = True
                    job.duration = time.time() - (job.start_time or time.time())
                    self.changed_jobs_queue.put(job)

    def killall(self):
        with self.lock:
            for w, job, reader in self.workers:
                w.kill()
                job.killed = job.failed = True
                job.duration = time.time() - (job.start_time or time.time())
                self.changed_jobs_queue.put(job)
            self.workers = []
            self.waiting_jobs = []

    def close(self):
        self.killed = True
        self.killall()

    def split(self, tasks):
        """Split `tasks` into groups, one per worker slot."""
        n = max(1, self.pool_size)
        ans = [[] for _ in range(n)]
        for i, t in enumerate(tasks):
            ans[i % n].append(t)
        return [x for x in ans if x]


__all__ = ['Server', 'CriticalError']
