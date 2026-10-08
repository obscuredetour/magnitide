"""Magnitide's host for calibre plugins, run in the plugin runtime.

The calibre API level this compat package provides; plugins see it as
calibre's version (their `minimum_calibre_version` is checked against it).
"""

import os

COMPAT_VERSION = '9.15.0'

#: 'runtime' (Magnitide's plugin runtime with the compat package) or
#: 'calibre' (inside an installed calibre, on its own code).
MODE = os.environ.get('MAGNITIDE_HOST_MODE', 'runtime')
IN_CALIBRE = MODE == 'calibre'
