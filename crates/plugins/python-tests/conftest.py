"""The compat package (crates/plugins/python) on the path, calibre's
builtins installed, settings in a temporary folder and Qt offscreen, as
the host sets things up (magnitide_host.bootstrap)."""

import os
import pathlib
import shutil
import sys
import tempfile
import zipfile

import pytest

HERE = pathlib.Path(__file__).resolve().parent
COMPAT = HERE.parent / 'python'
FIXTURES = HERE.parent / 'tests' / 'fixtures'
REPO = HERE.parents[2]

sys.path.insert(0, str(COMPAT))
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
_TMP = tempfile.mkdtemp(prefix='magnitide-compat-tests-')
os.environ['CALIBRE_CONFIG_DIRECTORY'] = os.path.join(_TMP, 'config')
os.environ['CALIBRE_CACHE_DIRECTORY'] = os.path.join(_TMP, 'cache')
os.makedirs(os.environ['CALIBRE_CONFIG_DIRECTORY'])
os.makedirs(os.environ['CALIBRE_CACHE_DIRECTORY'])

from magnitide_host import bootstrap  # noqa: E402

bootstrap.setup()


@pytest.fixture(scope='session')
def qapp():
    from PyQt6.QtWidgets import QApplication

    app = QApplication.instance() or QApplication(['magnitide-compat-tests'])
    yield app


@pytest.fixture
def fixture_zip(tmp_path):
    """A fixture plugin folder (crates/plugins/tests/fixtures/<name>) as the zip calibre expects."""

    def make(name):
        src = FIXTURES / name
        path = tmp_path / f'{name}.zip'
        with zipfile.ZipFile(path, 'w') as zf:
            for f in sorted(src.iterdir()):
                if f.is_file():
                    zf.write(f, f.name)
        return str(path)

    return make


@pytest.fixture
def shadow_library(tmp_path):
    """A calibre library folder with the calibre-compat crate's fixture database."""
    lib = tmp_path / 'library'
    lib.mkdir()
    shutil.copy(
        REPO / 'crates' / 'calibre-compat' / 'tests' / 'fixtures' / 'calibre_test_metadata.db', lib / 'metadata.db'
    )
    return str(lib)
