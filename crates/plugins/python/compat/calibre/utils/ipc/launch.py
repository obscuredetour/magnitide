"""Mirrors calibre's `calibre.utils.ipc.launch`: starting a worker process."""

import os
import subprocess
import time

from calibre.constants import iswindows
from calibre.ptempfile import base_dir
from calibre.utils.ipc import worker_command


def exe_path(exe_name):
    """What calibre runs for `calibre-parallel`, `calibre-debug` and friends:
    here always Estuary's interpreter with our worker."""
    return worker_command()


class Worker:
    """A worker process: build it, call it to start, watch it."""

    exe_name = 'calibre-parallel'

    @property
    def executable(self):
        return exe_path(self.exe_name)

    @property
    def gui_executable(self):
        return self.executable

    @property
    def env(self):
        env = os.environ.copy()
        env.update(self._env)
        env['CALIBRE_WORKER'] = '1'
        from polyglot.binary import as_hex_unicode

        td = base_dir()
        env['CALIBRE_WORKER_TEMP_DIR'] = as_hex_unicode(td.encode('utf-8'))
        env['ORIGWD'] = as_hex_unicode(os.path.abspath(os.getcwd()).encode('utf-8'))
        return env

    @property
    def is_alive(self):
        return hasattr(self, 'child') and self.child.poll() is None

    @property
    def returncode(self):
        if not hasattr(self, 'child'):
            return None
        self.child.poll()
        return self.child.returncode

    @property
    def pid(self):
        if not hasattr(self, 'child'):
            return None
        return getattr(self.child, 'pid', None)

    def close_log_file(self):
        try:
            self._file.close()
        except Exception:
            pass

    def kill(self):
        self.close_log_file()
        try:
            if self.is_alive:
                self.child.kill()
                try:
                    self.child.wait(5)
                except Exception:
                    pass
        except Exception:
            pass

    def __init__(self, env, gui=False, job_name=None):
        self._env = {}
        self.gui = gui
        self.job_name = job_name
        self._env = dict(env or {})

    def __call__(self, redirect_output=True, cwd=None, priority='normal', pass_fds=()):
        """Start the worker; returns the Popen. With `redirect_output` its
        output goes to a log file (`self.log_path`)."""
        exe = list(self.executable)
        env = self.env
        env['ORIGWD'] = env.get('ORIGWD', '')
        args = {'env': env, 'cwd': cwd}
        if iswindows:
            args['creationflags'] = subprocess.CREATE_NO_WINDOW
        if redirect_output:
            self._file = open(os.path.join(base_dir(), 'worker_%d_%d.log' % (os.getpid(), time.time())), 'wb')
            args['stdout'] = subprocess.PIPE
            args['stderr'] = self._file
            args['stdin'] = subprocess.DEVNULL
            self.log_path = self._file.name
        else:
            self.log_path = None
        if self.spec_path:
            exe.append(self.spec_path)
        self.child = subprocess.Popen(exe, **args)
        return self.child

    spec_path = None


__all__ = ['Worker', 'exe_path']
