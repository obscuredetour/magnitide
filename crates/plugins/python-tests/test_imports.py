"""Every module of the compat package imports (with Qt offscreen); the
modules standing in for parts of calibre Magnitide doesn't provide raise
an ImportError that says so."""

import importlib
import pathlib

import pytest
from conftest import COMPAT

MODULES = []
for path in sorted(COMPAT.rglob('*.py')):
    rel = path.relative_to(COMPAT)
    if rel.parts[0] == 'resources':
        continue
    parts = list(rel.with_suffix('').parts)
    if parts[-1] == '__init__':
        parts = parts[:-1]
    MODULES.append('.'.join(parts))


def _is_stub(name):
    path = COMPAT / pathlib.Path(*name.split('.'))
    path = path.with_suffix('.py') if path.with_suffix('.py').exists() else path / '__init__.py'
    return "isn't available in Magnitide" in path.read_text(encoding='utf-8')[:4000]


@pytest.mark.parametrize('name', MODULES)
def test_module_imports(name, qapp):
    if _is_stub(name):
        with pytest.raises(ImportError) as e:
            importlib.import_module(name)
        assert 'Magnitide' in str(e.value)
    else:
        importlib.import_module(name)


def test_qt_core_has_the_usual_names(qapp):
    from qt.core import QApplication, QDialog, QIcon, QNetworkAccessManager, Qt, QToolButton

    assert QApplication is not None and QDialog is not None and QIcon is not None and QNetworkAccessManager
    # PyQt5-style unscoped enum members, as plugins written for calibre 5 use them.
    assert QToolButton.MenuButtonPopup == QToolButton.ToolButtonPopupMode.MenuButtonPopup
    assert Qt.AlignCenter == Qt.AlignmentFlag.AlignCenter
    # PyQt5's exec_() and QFontMetrics.width(), on subclasses too.
    from qt.core import QEventLoop, QFontMetrics, QMenu, QMessageBox

    class MyDialog(QDialog):
        pass

    for cls in (QDialog, MyDialog, QMenu, QMessageBox, QEventLoop, QApplication):
        assert cls.exec_ is not None, cls
    loop = QEventLoop()
    from qt.core import QTimer

    QTimer.singleShot(0, loop.quit)
    assert loop.exec_() == 0
    assert QFontMetrics.width is not None


def test_pyqt5_is_redirected(qapp):
    from PyQt5.Qt import QDialog as D5  # noqa: I001
    from PyQt5.QtWidgets import QDialog as D5w
    from qt.core import QDialog

    assert D5 is QDialog and D5w is QDialog


def test_webengine_is_a_readable_error():
    with pytest.raises(ImportError) as e:
        import qt.webengine  # noqa: F401
    assert 'WebEngine' in str(e.value)


def test_version_claims():
    from calibre.constants import MAGNITIDE, numeric_version

    assert MAGNITIDE is True
    assert numeric_version >= (9, 15, 0)
