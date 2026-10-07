"""Pure-Python stand-ins for calibre's C extensions, for the vendoring
fallback (scripts/vendor-calibre.py). Each module mirrors the functions of
the extension of the same name that calibre's Python calls; the rest raise
ImportError naming the extension. Not used by the compat package."""
