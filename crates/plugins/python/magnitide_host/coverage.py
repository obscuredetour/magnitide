"""What a plugin needs from calibre that Magnitide's compat package doesn't
provide: a static scan of the zip's imports, so Settings can warn at
install time ("uses calibre.ebooks.conversion, which Magnitide doesn't
provide") rather than the plugin failing later."""

import ast
import importlib.util
import sys
import zipfile

# Whole trees calibre has and Magnitide doesn't, with why.
KNOWN_UNSUPPORTED = {
    'calibre.ebooks.conversion': "calibre's conversion pipeline",
    'calibre.ebooks.oeb.transforms': "calibre's conversion pipeline",
    'calibre.devices': "calibre's device drivers",
    'calibre.srv': "calibre's content server",
    'calibre.gui2.tweak_book': "calibre's book editor",
    'calibre.gui2.viewer': "calibre's viewer",
    'calibre.gui2.store': "calibre's Get Books stores",
    'calibre.gui2.tts': "calibre's read-aloud",
    'calibre.utils.podofo': 'PDF manipulation (PoDoFo)',
    'calibre.ebooks.pdf': 'PDF rendering',
    'calibre.web': "calibre's news recipes",
    'calibre.library.catalogs': "calibre's catalog generation",
    'calibre.utils.smtp': "calibre's email sending",
    'calibre.ai': "calibre's AI features",
    'calibre_extensions': "calibre's C extensions",
    'qt.webengine': 'Qt WebEngine',
    'PyQt6.QtWebEngineWidgets': 'Qt WebEngine',
    'PyQt6.QtWebEngineCore': 'Qt WebEngine',
    'PyQt6.QtMultimedia': 'Qt Multimedia',
}

_ROOTS = ('calibre', 'qt', 'polyglot', 'PyQt6', 'PyQt5', 'css_selectors', 'tinycss', 'odf')


_GUARD_NAMES = {'ImportError', 'ModuleNotFoundError', 'Exception', 'BaseException'}


def _guarded(tree):
    """Import nodes inside a `try` whose handlers catch ImportError (or
    everything): the plugin copes without them, so they aren't missing."""
    guarded = set()
    for node in ast.walk(tree):
        if not isinstance(node, ast.Try):
            continue
        catches = False
        for h in node.handlers:
            if h.type is None:
                catches = True
            else:
                names = [h.type] if not isinstance(h.type, ast.Tuple) else list(h.type.elts)
                for n in names:
                    name = n.id if isinstance(n, ast.Name) else getattr(n, 'attr', None)
                    if name in _GUARD_NAMES:
                        catches = True
        if not catches:
            continue
        for stmt in list(node.body) + [s for h in node.handlers for s in h.body]:
            for sub in ast.walk(stmt):
                if isinstance(sub, (ast.Import, ast.ImportFrom)):
                    guarded.add(id(sub))
    return guarded


def _imports(src):
    try:
        tree = ast.parse(src)
    except SyntaxError:
        return
    guarded = _guarded(tree)
    for node in ast.walk(tree):
        if id(node) in guarded:
            continue
        if isinstance(node, ast.Import):
            for a in node.names:
                yield a.name, None
        elif isinstance(node, ast.ImportFrom) and node.level == 0 and node.module:
            for a in node.names:
                yield node.module, a.name


def _provided(module):
    try:
        return importlib.util.find_spec(module) is not None
    except ImportError, ValueError, AttributeError:
        return False


def _stub(module):
    """A module of the compat package that only raises ImportError (it
    says what's missing); counts as not provided."""
    try:
        spec = importlib.util.find_spec(module)
        origin = spec.origin if spec else None
        if not origin or not origin.endswith('.py'):
            return False
        with open(origin, encoding='utf-8') as f:
            head = f.read(4000)
        return 'raise ImportError(' in head and "isn't available in Magnitide" in head
    except Exception:
        return False


def classify(module, name=None):
    """None if provided; else a reason it isn't."""
    root = module.split('.')[0]
    if root not in _ROOTS:
        return None  # a third-party or stdlib module: the plugin's own concern
    if root == 'PyQt5':
        # Redirected to PyQt6 by qt/__init__.py; `PyQt5.Qt` is qt.core.
        module = 'qt.core' if module in ('PyQt5.Qt', 'PyQt5.QtCore.Qt') else 'PyQt6' + module[5:]
    if _provided(module) and not _stub(module):
        return None
    for prefix, why in KNOWN_UNSUPPORTED.items():
        if module == prefix or module.startswith(prefix + '.'):
            return why
    if _provided(module):
        if name and name[0].isupper() is False and name != '*':
            # `from calibre.x import y` where y is a submodule or a name; a
            # missing name shows only when imported, so check submodules.
            if _provided(module + '.' + name):
                return None
        return None
    if name and _provided(module.rpartition('.')[0]) and module.rpartition('.')[2]:
        pass
    return 'not provided by Magnitide'


def scan_zip(path):
    """Unsupported imports in a plugin zip: ['calibre.ebooks.conversion (calibre's conversion pipeline)', …]."""
    found = {}
    try:
        with zipfile.ZipFile(path) as zf:
            for n in zf.namelist():
                if not n.endswith('.py'):
                    continue
                try:
                    src = zf.read(n).decode('utf-8', 'replace')
                except Exception:
                    continue
                for module, name in _imports(src):
                    if module.startswith('calibre_plugins.'):
                        continue
                    why = classify(module, name)
                    if why:
                        found.setdefault(module, why)
    except zipfile.BadZipFile:
        return []
    return [f'{m} ({why})' for m, why in sorted(found.items())]


if __name__ == '__main__':
    for p in sys.argv[1:]:
        print(p, scan_zip(p))
