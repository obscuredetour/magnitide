"""Magnitide's host for calibre plugins, run in a calibre or in Estuary.

The calibre API level this compat package provides; plugins see it as
calibre's version (their `minimum_calibre_version` is checked against it).
"""

import os

COMPAT_VERSION = '9.15.0'

#: 'estuary' (Estuary, Magnitide's compatibility layer, with the compat package) or
#: 'calibre' (inside an installed calibre, on its own code).
MODE = os.environ.get('MAGNITIDE_HOST_MODE', 'estuary')
IN_CALIBRE = MODE == 'calibre'
