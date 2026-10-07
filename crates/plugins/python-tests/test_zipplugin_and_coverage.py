"""calibre's plugin loader on the fixture plugins, and the coverage scanner."""

import zipfile

from calibre.customize import InterfaceActionBase, MetadataReaderPlugin
from calibre.customize.zipplugin import get_resources, loader
from calibre.ebooks.metadata.sources.base import Source
from magnitide_host.coverage import scan_zip
from magnitide_host.plugins import plugin_kind


def test_loads_fixture_plugins(fixture_zip):
    source = loader.load(fixture_zip('test_source'))
    assert issubclass(source, Source) and source.name == 'Magnitide Test Source'
    assert plugin_kind(source) == 'metadataSource'
    action = loader.load(fixture_zip('test_action'))
    assert issubclass(action, InterfaceActionBase) and plugin_kind(action) == 'interfaceAction'
    reader = loader.load(fixture_zip('test_metadata_reader'))
    assert issubclass(reader, MetadataReaderPlugin) and reader.file_types == {'txt'}


def test_namespace_import_and_resources(fixture_zip):
    path = fixture_zip('test_action')
    loader.load(path)
    import calibre_plugins.magnitide_test_action.action as action  # noqa: I001

    assert hasattr(action, 'TestAction')
    assert b'TestAction' in get_resources(path, 'action.py')
    assert get_resources(path, ['action.py', 'missing.txt'], print_tracebacks_for_missing_resources=False) == {
        'action.py': get_resources(path, 'action.py')
    }


def _zip_with(tmp_path, source):
    path = tmp_path / 'p.zip'
    with zipfile.ZipFile(path, 'w') as zf:
        zf.writestr('plugin-import-name-p.txt', '')
        zf.writestr('__init__.py', source)
    return str(path)


def test_coverage_scanner(tmp_path):
    found = scan_zip(
        _zip_with(
            tmp_path,
            'from calibre.ebooks.conversion.plumber import Plumber\nfrom calibre.utils.config import JSONConfig\n',
        )
    )
    assert any(f.startswith('calibre.ebooks.conversion.plumber') for f in found)
    assert not any('utils.config' in f for f in found)
    # A fallback the plugin copes without isn't a gap.
    guarded = scan_zip(
        _zip_with(
            tmp_path, 'try:\n    from qt.core import QDialog\nexcept ImportError:\n    from PyQt5.Qt import QDialog\n'
        )
    )
    assert guarded == []
    # Nor a redirected PyQt5 import.
    assert scan_zip(_zip_with(tmp_path, 'from PyQt5.Qt import QDialog\nfrom PyQt5.QtWidgets import QWidget\n')) == []
    # A stub that says what's missing counts as missing.
    assert any('podofo' in f for f in scan_zip(_zip_with(tmp_path, 'from calibre.utils.podofo import get_podofo\n')))
