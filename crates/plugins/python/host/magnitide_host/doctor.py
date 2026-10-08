"""`python -m magnitide_host doctor`: can the runtime start Qt here? Prints
JSON: {"ok": bool, "python": ..., "qt": ..., "missing": [...], "error": ...}.
On Linux, a Qt that can't load its platform plugin usually lacks a system
library; the missing ones are listed from `ldd` so Settings can name them."""

import json
import os
import subprocess
import sys


def _missing_libs():
    missing = []
    if not sys.platform.startswith('linux'):
        return missing
    try:
        import PyQt6

        base = os.path.join(os.path.dirname(PyQt6.__file__), 'Qt6')
        targets = [
            os.path.join(base, 'lib', n) for n in os.listdir(os.path.join(base, 'lib')) if n.startswith('libQt6Gui')
        ]
        plat = os.path.join(base, 'plugins', 'platforms')
        if os.path.isdir(plat):
            targets += [os.path.join(plat, n) for n in os.listdir(plat)]
        for t in targets:
            out = subprocess.run(['ldd', t], capture_output=True, text=True).stdout
            for line in out.splitlines():
                if 'not found' in line:
                    name = line.split()[0]
                    if name not in missing:
                        missing.append(name)
    except Exception:
        pass
    return missing


def main():
    ans = {'ok': False, 'python': sys.version.split()[0], 'qt': None, 'missing': [], 'error': None}
    try:
        os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
        from PyQt6 import QtCore, QtWidgets

        app = QtWidgets.QApplication([])  # noqa: F841
        ans['qt'] = QtCore.qVersion()
        import calibre  # noqa: F401
        from calibre.customize import ui  # noqa: F401

        ans['ok'] = True
    except Exception as e:
        ans['error'] = f'{type(e).__name__}: {e}'
        ans['missing'] = _missing_libs()
    print(json.dumps(ans))
    return 0 if ans['ok'] else 1
