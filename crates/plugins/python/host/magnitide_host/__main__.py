"""Magnitide's host for calibre plugins: `python -I -m magnitide_host`.

Runs in Magnitide's plugin runtime with the compat package on sys.path (the
directory holding this package), so plugins import `calibre` and `qt` from
it. Talks JSON lines with Magnitide (see rpc.py). `python -m magnitide_host
doctor` instead checks that the runtime (Qt included) works here.
"""

import os
import sys


def main():
    from magnitide_host import bootstrap

    bootstrap.setup()
    if len(sys.argv) > 1 and sys.argv[1] == 'doctor':
        from magnitide_host.doctor import main as doctor

        return doctor()
    # Qt records the thread that first loads it as the main thread; make sure
    # that's this one, not a request thread that imports a plugin.
    from PyQt6 import QtCore  # noqa: F401

    from magnitide_host import hooks, plugins, rpc, sources  # noqa: F401  (register methods)
    from magnitide_host.gui import session  # noqa: F401

    if os.environ.get('MAGNITIDE_TEST_HOOKS'):
        from magnitide_host import testing  # noqa: F401

    plugins.install_registry_listener()
    rpc.serve()


if __name__ == '__main__':
    sys.exit(main())
