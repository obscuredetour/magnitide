"""Set up the process for the compat package before anything imports `calibre`:
where resources are, the folders calibre code expects, and calibre's built-ins."""

import os
import sys

_done = False


def setup():
    global _done
    if _done:
        return
    _done = True
    here = os.path.dirname(os.path.abspath(__file__))
    root = os.path.dirname(here)
    if root not in sys.path:
        sys.path.insert(0, root)
    sys.resources_location = os.path.join(root, 'resources')
    os.environ.setdefault(
        'CALIBRE_CONFIG_DIRECTORY', os.path.join(os.path.expanduser('~'), '.magnitide-calibre-plugins')
    )
    os.environ.setdefault('CALIBRE_CACHE_DIRECTORY', os.path.join(os.environ['CALIBRE_CONFIG_DIRECTORY'], 'caches'))
    # Our worker, for calibre code that runs jobs in another process.
    sys.running_from_setup = True
    sys.setup_dir = here
    import calibre  # noqa: F401  (installs the built-ins)
