"""Mirrors calibre's `calibre.gui2.jobs` (calibre 9.15): the `JobManager`
plugins run background work through (`run_job` for another process,
`run_threaded_job` for a thread), with its signals. Jobs are reported to
Magnitide's job bar by the host."""

import time

from calibre.gui2 import Dispatcher
from calibre.gui2.threaded_jobs import ThreadedJobServer
from calibre.utils.ipc.job import ParallelJob
from calibre.utils.ipc.server import Server
from qt.core import QAbstractTableModel, QModelIndex, Qt, QTimer, pyqtSignal


class JobManager(QAbstractTableModel):
    """Runs and watches jobs. `jobs` are the known jobs, newest first;
    `job_added(row)` and `job_done(row)` fire as they come and go."""

    job_added = pyqtSignal(int)
    job_done = pyqtSignal(int)
    has_jobs = pyqtSignal(bool)
    job_count_changed = pyqtSignal(int)

    def __init__(self):
        QAbstractTableModel.__init__(self)
        self.wait_icon = None
        self.running_icon = None
        self.error_icon = None
        self.done_icon = None

        self.jobs = []
        self.add_job = Dispatcher(self._add_job)
        from calibre.gui2 import config

        try:
            limit = int(config['worker_limit'] or 6)
        except Exception:
            limit = 6
        self.server = Server(limit=limit, enforce_cpu_limit=True)
        self.threaded_server = ThreadedJobServer()
        self.changed_queue = self.server.changed_jobs_queue
        self.timer = QTimer(self)
        self.timer.timeout.connect(self.update, type=Qt.ConnectionType.QueuedConnection)
        self.timer.start(1000)

    def columnCount(self, parent=QModelIndex()):
        return 5

    def rowCount(self, parent=QModelIndex()):
        return len(self.jobs)

    def headerData(self, section, orientation, role):
        if role != Qt.ItemDataRole.DisplayRole:
            return None
        if orientation == Qt.Orientation.Horizontal:
            return ('Job', 'Status', 'Progress', 'Running time', 'Start time')[section] if section < 5 else None
        return section + 1

    def data(self, index, role):
        if role not in (Qt.ItemDataRole.DisplayRole,):
            return None
        row, col = index.row(), index.column()
        try:
            job = self.jobs[row]
        except Exception:
            return None
        if col == 0:
            return job.description
        if col == 1:
            return job.status_text
        if col == 2:
            return job.percent
        if col == 3:
            return job.running_time
        if col == 4:
            return time.strftime('%H:%M:%S', time.localtime(job.start_time)) if job.start_time else ''
        return None

    def update(self):
        """Poll the servers for changes; emit `job_done` for finished ones."""
        try:
            self._update()
        except BaseException:
            import traceback

            traceback.print_exc()

    def _update(self):
        changed = set()
        while not self.changed_queue.empty():
            try:
                changed.add(self.changed_queue.get_nowait())
            except Exception:
                break
        while not self.threaded_server.changed_jobs_queue.empty():
            try:
                changed.add(self.threaded_server.changed_jobs_queue.get_nowait())
            except Exception:
                break
        for job in list(self.jobs):
            if job in changed or (not job.is_finished and getattr(job, 'is_running', False)):
                pass
        rows_done = []
        for row, job in enumerate(self.jobs):
            if job.is_finished and not getattr(job, '_reported', False):
                job._reported = True
                rows_done.append(row)
        for _row in rows_done:
            self.job_done.emit(len(self.unfinished_jobs()))
        if self.jobs:
            self.dataChanged.emit(self.index(0, 0), self.index(len(self.jobs) - 1, 4))
        self.has_jobs.emit(bool(self.unfinished_jobs()))
        self.job_count_changed.emit(len(self.unfinished_jobs()))

    def _add_job(self, job):
        self.beginInsertRows(QModelIndex(), 0, 0)
        self.jobs.insert(0, job)
        self.endInsertRows()
        self.job_added.emit(len(self.jobs))
        if isinstance(job, ParallelJob):
            self.server.add_job(job)
        else:
            self.threaded_server.add_job(job)
        self.has_jobs.emit(True)

    def run_job(self, done, name, args=[], kwargs={}, description='', core_usage=1):
        """Run `name` (a worker function: 'arbitrary', 'arbitrary_n', …) in
        another process; `done(job)` is called when it finishes."""
        job = ParallelJob(name, description, done, args=args, kwargs=kwargs)
        job.core_usage = core_usage
        self.add_job(job)
        return job

    def run_threaded_job(self, job):
        self.add_job(job)
        return job

    def launch_gui_app(self, name, args=[], kwargs={}, description=''):
        job = ParallelJob(name, description, lambda x: x, args=args, kwargs=kwargs)
        self.server.run_job(job, gui=True, redirect_output=False)

    def unfinished_jobs(self):
        return [j for j in self.jobs if not j.is_finished]

    def has_device_jobs(self, queued_also=False):
        return False

    def row_to_job(self, row):
        return self.jobs[row]

    def rows_to_jobs(self, rows):
        return [self.jobs[r] for r in rows]

    def abort_job(self, job):
        self.kill_job(job)

    def kill_job(self, job):
        if isinstance(job, int):
            job = self.jobs[job]
        if isinstance(job, ParallelJob):
            self.server.kill_job(job)
        else:
            self.threaded_server.kill_job(job)

    def kill_multiple_jobs(self, jobs):
        for j in jobs:
            self.kill_job(j)

    def kill_all_jobs(self):
        for job in self.unfinished_jobs():
            self.kill_job(job)

    def terminate_all_jobs(self):
        self.kill_all_jobs()
        self.server.close()

    def abort_jobs(self):
        self.kill_all_jobs()

    def abort_job_by_id(self, i):
        pass


__all__ = ['JobManager']
