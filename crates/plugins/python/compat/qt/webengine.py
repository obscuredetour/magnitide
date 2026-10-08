"""calibre's `qt.webengine` (QtWebEngine). Magnitide's plugin runtime doesn't
ship WebEngine (it alone is larger than the rest of the runtime), so a plugin
that needs it stops here with a clear message."""

raise ImportError(
    "qt.webengine isn't available in Magnitide: this plugin needs Qt WebEngine (a web browser inside calibre), "
    "which Magnitide's plugin runtime doesn't include"
)
