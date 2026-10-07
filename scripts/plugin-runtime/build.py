#!/usr/bin/env python3
"""Assemble Magnitide's plugin runtime for one target.

A standalone CPython (python-build-standalone) with the wheels in
requirements.txt, stripped to what calibre plugins use, then either left in
target/plugin-runtime/ for development (--local) or packed into the archive
Magnitide downloads. See README.md.

Steps: fetch the CPython tarball (verified) → extract → install the wheels
with uv (hashes required) → strip → on macOS thin to one architecture and
re-sign → smoke test (native builds only) → link check → write
python/magnitide-runtime.json → pack.

Needs Python 3.14 (for compression.zstd; a `zstd` command is used otherwise)
and uv on PATH or in $UV.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import platform
import shutil
import subprocess
import sys
import tarfile
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
VERSION = (HERE / 'VERSION').read_text().strip()
PYTHON = json.loads((HERE / 'python.json').read_text())

TARGETS = ('aarch64-apple-darwin', 'x86_64-apple-darwin', 'x86_64-pc-windows-msvc',
           'x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu')

# Qt modules calibre's qt.core uses (plus what they link to). Everything
# else in the PyQt6-Qt6 wheel is deleted: WebEngine, Qml/Quick, Multimedia,
# Pdf, Positioning, Sensors, Sql, TextToSpeech, Xml, Bluetooth, Nfc, …
QT_KEEP = ('Core', 'Gui', 'Widgets', 'Network', 'PrintSupport', 'Svg', 'SvgWidgets',
           'OpenGL', 'OpenGLWidgets', 'DBus')
QT_PLUGIN_DIRS_KEEP = ('platforms', 'styles', 'imageformats', 'iconengines', 'tls', 'printsupport', 'generic',
                       'platforminputcontexts', 'platformthemes', 'xcbglintegrations', 'wayland-decoration-client',
                       'wayland-graphics-integration-client', 'wayland-shell-integration', 'networkinformation')
QT_PLATFORM_PLUGINS_KEEP = ('cocoa', 'windows', 'xcb', 'wayland', 'wayland-egl', 'minimal', 'offscreen')
# Qt frameworks/libraries that the kept ones may link to without a PyQt6
# module of their own: on Linux the xcb and Wayland platform plugins (and
# their integration plugins) link Qt's private platform libraries.
QT_LIB_EXTRA_KEEP = ('Concurrent', 'XcbQpa', 'WaylandClient', 'WlShellIntegration', 'WaylandEglClientHwIntegration')

MAX_INSTALLED_MB = 260  # x86_64 Linux is 243 MB (Qt's ICU data); the others 160–200
MAX_ARCHIVE_MB = 90


def log(msg: str) -> None:
    print(f'[plugin-runtime] {msg}', flush=True)


def fail(msg: str) -> None:
    print(f'[plugin-runtime] error: {msg}', file=sys.stderr, flush=True)
    sys.exit(1)


def native_target() -> str:
    m = platform.machine().lower()
    arch = 'aarch64' if m in ('arm64', 'aarch64') else 'x86_64'
    if sys.platform == 'darwin':
        return f'{arch}-apple-darwin'
    if sys.platform == 'win32':
        return 'x86_64-pc-windows-msvc'
    return f'{arch}-unknown-linux-gnu'


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open('rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def fetch(url: str, dest: Path, sha256: str) -> None:
    if dest.exists() and sha256_of(dest) == sha256:
        log(f'cached {dest.name}')
        return
    log(f'downloading {url}')
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_suffix(dest.suffix + '.part')
    with urllib.request.urlopen(url) as r, tmp.open('wb') as f:
        shutil.copyfileobj(r, f)
    got = sha256_of(tmp)
    if got != sha256:
        tmp.unlink()
        fail(f'{dest.name}: sha256 {got}, expected {sha256}')
    tmp.replace(dest)


def rmtree(p: Path) -> None:
    if p.is_symlink() or p.is_file():
        p.unlink()
    elif p.is_dir():
        shutil.rmtree(p)


def extract_python(tarball: Path, work: Path) -> Path:
    rmtree(work)
    work.mkdir(parents=True)
    log(f'extracting {tarball.name}')
    with tarfile.open(tarball) as t:
        t.extractall(work, filter='tar')
    py = work / 'python'
    if not py.is_dir():
        fail('the tarball has no python/ folder')
    return py


def interpreter(py: Path, target: str) -> Path:
    if 'windows' in target:
        return py / 'python.exe'
    return py / 'bin' / 'python3'


def site_packages(py: Path, target: str) -> Path:
    if 'windows' in target:
        return py / 'Lib' / 'site-packages'
    return py / 'lib' / f'python{PYTHON["version"].rsplit(".", 1)[0]}' / 'site-packages'


def stdlib_dir(py: Path, target: str) -> Path:
    if 'windows' in target:
        return py / 'Lib'
    return py / 'lib' / f'python{PYTHON["version"].rsplit(".", 1)[0]}'


def find_uv() -> str:
    uv = os.environ.get('UV') or shutil.which('uv')
    if not uv:
        fail('uv not found: install it (https://docs.astral.sh/uv/) or set UV=/path/to/uv')
    return uv


def install_wheels(py: Path, target: str, native: bool) -> None:
    uv = find_uv()
    cmd = [uv, 'pip', 'install', '--require-hashes', '--no-deps', '--only-binary', ':all:', '--no-cache',
           '-r', str(HERE / 'requirements.txt')]
    if native:
        cmd += ['--python', str(interpreter(py, target))]
    else:
        cmd += ['--python-platform', target, '--python-version', PYTHON['version'],
                '--target', str(site_packages(py, target))]
    log('installing wheels')
    subprocess.run(cmd, check=True, env={**os.environ, 'UV_PYTHON_DOWNLOADS': 'never'})


def strip_python(py: Path, target: str) -> None:
    log('stripping the standard library and tools')
    lib = stdlib_dir(py, target)
    for name in ('test', 'idlelib', 'tkinter', 'turtledemo', 'ensurepip', 'pydoc_data', 'lib2to3', '__phello__',
                 'site-packages/pip', 'site-packages/setuptools', 'site-packages/_distutils_hack',
                 'site-packages/pkg_resources'):
        rmtree(lib / name)
    for d in (py / 'share', py / 'include', py / 'libs', py / 'tcl', py / 'Doc'):
        rmtree(d)
    for p in list(py.rglob('__pycache__')):
        rmtree(p)
    for p in list(py.glob('bin/*')):
        if p.name.startswith(('pip', 'idle', 'pydoc', '2to3')):
            p.unlink()
    for p in list(lib.glob('config-*')):
        for a in p.glob('libpython*.a'):
            a.unlink()
    dynload = lib / 'lib-dynload'
    if dynload.is_dir():
        for p in dynload.iterdir():
            if p.name.startswith(('_tkinter', '_test', 'xxlimited', '_xx', '_ctypes_test')):
                p.unlink()
    if 'windows' in target:
        for p in list(py.glob('*.pdb')) + list(py.glob('DLLs/_test*')) + list(py.glob('DLLs/_tkinter*')) + \
                list(py.glob('DLLs/tcl*')) + list(py.glob('DLLs/tk*')):
            rmtree(p)


def qt_dir(py: Path, target: str) -> Path:
    return site_packages(py, target) / 'PyQt6' / 'Qt6'


def strip_qt(py: Path, target: str) -> None:
    """Keep only the Qt modules calibre's qt.core uses, and their plugins."""
    log('stripping Qt')
    pyqt = site_packages(py, target) / 'PyQt6'
    qt = pyqt / 'Qt6'
    if not qt.is_dir():
        fail('PyQt6/Qt6 not found after install')
    keep = set(QT_KEEP) | set(QT_LIB_EXTRA_KEEP)
    mac = 'apple' in target
    win = 'windows' in target
    # Libraries.
    libdir = qt / ('bin' if win else 'lib')
    for p in list(libdir.iterdir()):
        name = p.name
        mod = None
        if mac and name.startswith('Qt') and name.endswith('.framework'):
            mod = name[2:-len('.framework')]
        elif win and name.startswith('Qt6') and name.endswith('.dll'):
            mod = name[3:-4]
        elif not mac and not win and name.startswith('libQt6'):
            mod = name[6:].split('.so')[0]
        if mod is not None and mod not in keep:
            rmtree(p)
        elif win and name.lower() in ('opengl32sw.dll', 'd3dcompiler_47.dll'):
            rmtree(p)
        elif name.startswith(('libav', 'libsw', 'avcodec', 'avformat', 'avutil', 'swresample', 'swscale')):
            rmtree(p)  # ffmpeg, for QtMultimedia (removed)
    if win:
        for p in list(qt.glob('lib/*')):
            rmtree(p)
    # Bindings for removed modules, and tooling.
    for p in list(pyqt.iterdir()):
        n = p.name
        if n.startswith('Qt') and (n.endswith(('.abi3.so', '.pyd', '.pyi', '.so'))):
            mod = n[2:].split('.')[0]
            if mod not in set(QT_KEEP):
                rmtree(p)
        elif n in ('bindings', 'uic', 'lupdate', 'qml', 'pylupdate.abi3.so', 'pylupdate.pyd', 'pylupdate.pyi',
                   'pyrcc.abi3.so', 'pyrcc.pyd', 'pyrcc.pyi'):
            rmtree(p)
    for d in ('qml', 'translations', 'libexec', 'resources', 'include', 'mkspecs', 'metatypes', 'modules', 'sbom'):
        rmtree(qt / d)
    for p in list(qt.glob('bin/*')):
        if win and p.suffix.lower() == '.dll':
            continue
        rmtree(p)
    # Plugins.
    plugins = qt / 'plugins'
    if plugins.is_dir():
        for d in list(plugins.iterdir()):
            if d.name not in QT_PLUGIN_DIRS_KEEP:
                rmtree(d)
        platforms = plugins / 'platforms'
        if platforms.is_dir():
            for p in list(platforms.iterdir()):
                base = p.name
                for prefix in ('libq', 'q'):
                    if base.startswith(prefix):
                        base = base[len(prefix):]
                        break
                base = base.split('.')[0]
                if base not in QT_PLATFORM_PLUGINS_KEEP:
                    rmtree(p)
        # Image-format plugins for modules that were removed (the PDF one links QtPdf).
        formats = plugins / 'imageformats'
        if formats.is_dir():
            for p in list(formats.iterdir()):
                if 'pdf' in p.name.lower():
                    rmtree(p)
        themes = plugins / 'platformthemes'
        if themes.is_dir():
            for p in list(themes.iterdir()):
                if 'gtk' in p.name:
                    rmtree(p)


def macho_files(root: Path):
    for p in root.rglob('*'):
        if p.is_symlink() or not p.is_file():
            continue
        if p.suffix in ('.so', '.dylib') or (p.parent.name == 'bin') or ('.framework' in str(p) and p.suffix == ''):
            with p.open('rb') as f:
                magic = f.read(4)
            if magic in (b'\xcf\xfa\xed\xfe', b'\xca\xfe\xba\xbe', b'\xfe\xed\xfa\xcf'):
                yield p


def thin_and_sign_macos(py: Path, target: str) -> None:
    """PyQt6's macOS wheels are universal2: keep one architecture, then re-sign (arm64 refuses unsigned code)."""
    arch = 'arm64' if target.startswith('aarch64') else 'x86_64'
    log(f'thinning Mach-O files to {arch} and re-signing')
    for p in macho_files(site_packages(py, target)):
        info = subprocess.run(['lipo', '-info', str(p)], capture_output=True, text=True)
        if info.returncode == 0 and 'Architectures in the fat file' in info.stdout and arch in info.stdout:
            tmp = p.with_name(p.name + '.thin')
            subprocess.run(['lipo', '-thin', arch, str(p), '-output', str(tmp)], check=True)
            mode = p.stat().st_mode
            tmp.replace(p)
            p.chmod(mode)
            subprocess.run(['codesign', '-s', '-', '-f', str(p)], check=True, capture_output=True)


def smoke_test(py: Path, target: str) -> dict:
    log('smoke test')
    exe = interpreter(py, target)
    env = {**os.environ, 'QT_QPA_PLATFORM': 'offscreen', 'PYTHONDONTWRITEBYTECODE': '1'}
    code = (
        'import json, sys\n'
        'import lxml.etree, PIL, regex, apsw, msgpack, xxhash, html5lib, mechanize, css_parser, dateutil, tzlocal, '
        'bs4, charset_normalizer, pypdf, html2text, markdown\n'
        'from PyQt6 import QtCore, QtGui, QtWidgets, QtNetwork, QtPrintSupport, QtSvg, QtSvgWidgets\n'
        'app = QtWidgets.QApplication([])\n'
        'QtWidgets.QLabel("x"); QtSvg.QSvgRenderer()\n'
        'import importlib.util\n'
        'assert importlib.util.find_spec("PyQt6.QtWebEngineWidgets") is None\n'
        'print(json.dumps({"python": sys.version.split()[0], "qt": QtCore.qVersion(), "pyqt": QtCore.PYQT_VERSION_STR}))\n'
    )
    out = subprocess.run([str(exe), '-I', '-c', code], env=env, capture_output=True, text=True)
    if out.returncode != 0:
        fail(f'smoke test failed:\n{out.stdout}\n{out.stderr}')
    return json.loads(out.stdout.strip().splitlines()[-1])


def link_check(py: Path, target: str) -> None:
    """No kept Qt library may reference a deleted one."""
    qt = qt_dir(py, target)
    if 'windows' in target:
        return  # dumpbin isn't on every runner; the smoke test loads the modules.
    log('checking library links')
    present = {p.name for p in qt.rglob('*') if p.is_file() or p.is_symlink()}
    present |= {p.name for p in qt.glob('lib/*.framework')}
    missing = set()
    for p in list(qt.rglob('*.dylib')) + list(qt.rglob('*.so*')) + \
            [f for f in qt.glob('lib/*.framework/Versions/A/*') if f.is_file() and f.suffix == '']:
        if p.is_symlink() or not p.is_file():
            continue
        if sys.platform == 'darwin':
            out = subprocess.run(['otool', '-L', str(p)], capture_output=True, text=True).stdout
            refs = [line.strip().split(' (')[0] for line in out.splitlines()[1:]]
            for r in refs:
                if '@rpath/Qt' in r or '/Qt6/lib/' in r:
                    name = r.split('/')[-1]
                    fw = [s for s in r.split('/') if s.endswith('.framework')]
                    if fw and fw[0] not in present:
                        missing.add(f'{p.name} → {fw[0]}')
                    elif not fw and name not in present:
                        missing.add(f'{p.name} → {name}')
        else:
            out = subprocess.run(['ldd', str(p)], capture_output=True, text=True).stdout
            for line in out.splitlines():
                if 'libQt6' in line and 'not found' in line:
                    missing.add(f'{p.name} → {line.split()[0]}')
    if missing:
        fail('kept libraries reference deleted ones:\n  ' + '\n  '.join(sorted(missing)))


def wheels_from_lock() -> dict:
    wheels = {}
    for line in (HERE / 'requirements.txt').read_text().splitlines():
        line = line.strip()
        if line and not line.startswith(('#', '--hash')) and '==' in line:
            name, _, version = line.rstrip(' \\').partition('==')
            wheels[name.strip()] = version.split(';')[0].strip().rstrip(' \\')
    return wheels


def dir_size(p: Path) -> int:
    return sum(f.stat().st_size for f in p.rglob('*') if f.is_file() and not f.is_symlink())


def write_manifest(py: Path, target: str, smoke: dict | None) -> dict:
    info = {
        'runtime': VERSION,
        'python': PYTHON['version'],
        'pythonBuild': PYTHON['tag'],
        'target': target,
        'qt': smoke.get('qt') if smoke else None,
        'pyqt': smoke.get('pyqt') if smoke else wheels_from_lock().get('pyqt6'),
        'wheels': wheels_from_lock(),
        'builtAt': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
    }
    (py / 'magnitide-runtime.json').write_text(json.dumps(info, indent=2) + '\n')
    return info


def pack(py: Path, target: str, out: Path) -> Path:
    out.mkdir(parents=True, exist_ok=True)
    name = f'magnitide-plugin-runtime-{VERSION}-{target}.tar.zst'
    archive = out / name
    log(f'packing {name}')
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode='w:') as t:
        for p in sorted(py.rglob('*'), key=lambda p: str(p.relative_to(py.parent))):
            arcname = str(p.relative_to(py.parent))
            ti = t.gettarinfo(str(p), arcname)
            ti.uid = ti.gid = 0
            ti.uname = ti.gname = ''
            ti.mtime = 1_700_000_000
            if ti.isreg():
                with p.open('rb') as f:
                    t.addfile(ti, f)
            else:
                t.addfile(ti)
        root = tarfile.TarInfo('python')
        root.type = tarfile.DIRTYPE
        root.mode = 0o755
        root.mtime = 1_700_000_000
    raw = buf.getvalue()
    try:
        from compression import zstd  # Python 3.14

        data = zstd.compress(raw, level=19)
        archive.write_bytes(data)
    except ImportError:
        subprocess.run(['zstd', '-19', '-q', '-f', '-o', str(archive)], input=raw, check=True)
    digest = sha256_of(archive)
    (out / (name + '.sha256')).write_text(f'{digest}  {name}\n')
    size = archive.stat().st_size
    log(f'{name}: {size / 1e6:.1f} MB, sha256 {digest}')
    if size > MAX_ARCHIVE_MB * 1e6:
        fail(f'archive is {size / 1e6:.0f} MB, over the {MAX_ARCHIVE_MB} MB limit')
    (out / f'{name}.json').write_text(json.dumps({
        'target': target, 'file': name, 'sha256': digest, 'size': size, 'installedSize': dir_size(py),
    }, indent=2) + '\n')
    return archive


def build(args) -> None:
    target = args.target or native_target()
    if target not in TARGETS:
        fail(f'unknown target {target}; one of {", ".join(TARGETS)}')
    native = target == native_target()
    if not native and args.local:
        fail('--local builds for this machine only')
    spec = PYTHON['targets'].get(target) or fail(f'python.json has no entry for {target}')
    cache = Path(args.cache or (Path.home() / '.cache' / 'magnitide-plugin-runtime'))
    tarball = cache / spec['file']
    fetch(PYTHON['base'] + PYTHON['tag'] + '/' + spec['file'], tarball, spec['sha256'])
    work = Path(args.work or (ROOT / 'target' / 'plugin-runtime' / 'work')) / target
    py = extract_python(tarball, work)
    install_wheels(py, target, native)
    strip_python(py, target)
    strip_qt(py, target)
    if 'apple' in target and sys.platform == 'darwin':
        thin_and_sign_macos(py, target)
    smoke = smoke_test(py, target) if native else None
    if not native:
        log('cross build: smoke test skipped (run it on the target platform)')
    link_check(py, target)
    for p in list(py.rglob('__pycache__')):
        rmtree(p)
    info = write_manifest(py, target, smoke)
    size = dir_size(py)
    log(f'installed size {size / 1e6:.0f} MB')
    if size > MAX_INSTALLED_MB * 1e6:
        fail(f'installed size {size / 1e6:.0f} MB is over the {MAX_INSTALLED_MB} MB limit')
    if args.local:
        dest = ROOT / 'target' / 'plugin-runtime' / target
        rmtree(dest)
        dest.mkdir(parents=True)
        shutil.move(str(py), str(dest / 'python'))
        (dest / 'installed.json').write_text(json.dumps({
            'version': VERSION, 'python': info['python'], 'qt': info['qt'], 'sha256': None,
            'installedAt': info['builtAt'],
        }, indent=2) + '\n')
        current = ROOT / 'target' / 'plugin-runtime' / 'current'
        rmtree(current)
        if sys.platform == 'win32':
            shutil.copytree(dest, current)
        else:
            current.symlink_to(dest.name)
        log(f'ready: {dest} (target/plugin-runtime/current points at it)')
    else:
        pack(py, target, Path(args.out or (ROOT / 'dist')))
    rmtree(work)


def manifest(dist: Path, out: Path | None) -> None:
    """Merge the per-archive .json files into manifest.json and sha256sums.txt."""
    entries = [json.loads(p.read_text()) for p in sorted(dist.glob('magnitide-plugin-runtime-*.tar.zst.json'))]
    if not entries:
        fail(f'no archives in {dist}')
    m = {'version': VERSION, 'python': PYTHON['version'], 'targets': {e['target']: e for e in entries}}
    (dist / 'manifest.json').write_text(json.dumps(m, indent=2) + '\n')
    sums = ''.join(f'{e["sha256"]}  {e["file"]}\n' for e in entries)
    (out or (HERE / 'sha256sums.txt')).write_text(sums)
    print(json.dumps(m, indent=2))


def rust_table(path: Path) -> None:
    m = json.loads(path.read_text())
    print('pub const RELEASES: &[Release] = &[')
    for target, e in sorted(m['targets'].items()):
        print(f'    Release {{\n        target: "{target}",\n        file: "{e["file"]}",\n'
              f'        sha256: "{e["sha256"]}",\n        size: {e["size"]},\n'
              f'        installed_size: {e["installedSize"]},\n    }},')
    print('];')


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--target', help='target triple (default: this machine)')
    ap.add_argument('--out', help='where the archive goes (default: dist/)')
    ap.add_argument('--local', action='store_true', help='leave the runtime in target/plugin-runtime/ instead of packing')
    ap.add_argument('--work', help='scratch directory')
    ap.add_argument('--cache', help='download cache (default: ~/.cache/magnitide-plugin-runtime)')
    ap.add_argument('--manifest', metavar='DIST', help='merge DIST/*.tar.zst.json into manifest.json and sha256sums.txt')
    ap.add_argument('--sha256sums', metavar='FILE', help='with --manifest: where to write the sums')
    ap.add_argument('--rust-table', metavar='MANIFEST', help='print the RELEASES table for runtime.rs')
    args = ap.parse_args()
    if args.manifest:
        manifest(Path(args.manifest), Path(args.sha256sums) if args.sha256sums else None)
    elif args.rust_table:
        rust_table(Path(args.rust_table))
    else:
        if sys.version_info < (3, 14) and not shutil.which('zstd'):
            fail('Python 3.14 or a zstd command is needed to pack the archive')
        build(args)


if __name__ == '__main__':
    main()
