"""calibre's `qt.webengine` (QtWebEngine). Estuary doesn't
ship WebEngine (it alone is larger than the rest of Estuary), so a plugin
that needs it stops here with a clear message."""

raise ImportError(
    "qt.webengine isn't available in Magnitide: this plugin needs Qt WebEngine (a web browser inside calibre), "
    "which Estuary, Magnitide's compatibility layer for calibre plugins, doesn't include"
)
