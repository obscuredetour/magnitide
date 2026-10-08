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
    root = os.path.dirname(here)  # python/host
    if root not in sys.path:
        sys.path.insert(0, root)
    # In Magnitide's runtime the compat package (our `calibre`, `qt`, …) sits
    # beside the host in python/compat; under an installed calibre it must
    # stay off the path, so calibre's own packages are used.
    compat = os.path.join(os.path.dirname(root), 'compat')
    if os.environ.get('MAGNITIDE_HOST_MODE', 'runtime') != 'calibre' and os.path.isdir(compat):
        if compat not in sys.path:
            sys.path.insert(1, compat)
        sys.resources_location = os.path.join(compat, 'resources')
    os.environ.setdefault(
        'CALIBRE_CONFIG_DIRECTORY', os.path.join(os.path.expanduser('~'), '.magnitide-calibre-plugins')
    )
    os.environ.setdefault('CALIBRE_CACHE_DIRECTORY', os.path.join(os.environ['CALIBRE_CONFIG_DIRECTORY'], 'caches'))
    # Packages a plugin needs that the runtime doesn't ship: the user puts
    # them in <plugins folder>/site-packages (`pip install --target` there).
    extras = os.path.join(os.path.dirname(os.environ['CALIBRE_CONFIG_DIRECTORY']), 'site-packages')
    if os.path.isdir(extras) and extras not in sys.path:
        sys.path.append(extras)
    if os.environ.get('MAGNITIDE_HOST_MODE', 'runtime') != 'calibre':
        # Our worker, for calibre code that runs jobs in another process
        # (a real calibre has calibre-parallel for that).
        sys.running_from_setup = True
        sys.setup_dir = here
    import calibre  # noqa: F401  (installs the built-ins)
