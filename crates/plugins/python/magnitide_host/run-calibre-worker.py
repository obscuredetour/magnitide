"""The worker calibre code starts for jobs in another process (`fork_job`,
`ParallelJob`), as calibre's own `calibre-parallel` would be: the same
interpreter and compat package as the host."""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from magnitide_host import bootstrap  # noqa: E402

bootstrap.setup()

from calibre.utils.ipc.worker import main  # noqa: E402

sys.exit(main())
