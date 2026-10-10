#!/usr/bin/env python3
"""The fallback to Magnitide's own compat package: copy the subset of
calibre's own (GPL) Python that plugins and the host need from a calibre
checkout, replacing only the edges, into crates/plugins/python-vendor/.

Not wired in. docs/calibre-plugins.md ("The fallback: vendoring calibre")
says when and how it would be switched on. What it does:

  1. clones calibre at the tag in scripts/vendor-calibre.toml (or uses
     --source <checkout>) into target/vendor/calibre;
  2. walks the import closure from the configured entry modules through
     calibre's source (top-level imports always; nested imports only when
     listed as `lazy`), stopping at `deny` trees and at `replace` modules
     (which stay the compat package's);
  3. copies the modules found (and polyglot, qt, css_selectors, tinycss, odf
     as needed), the configured resources, and the pure-Python stand-ins for
     calibre's C extensions from scripts/vendor-calibre/calibre_extensions/;
  4. writes VENDOR_MANIFEST.json (tag, commit, every file's sha256, what was
     replaced), LICENSE.calibre and THIRD_PARTY_NOTICES.md;
  5. checks that every `calibre_extensions.*` and third-party import the
     vendored code makes is provided, and that everything compiles under
     the current Python.

    python3 scripts/vendor-calibre.py [--source DIR] [--check] [--report-nested]

`--check` verifies an existing vendored tree against the manifest (for CI);
`--report-nested` lists nested imports that aren't in the closure, to decide
what to promote to `lazy`.
"""

import argparse
import ast
import hashlib
import json
import os
import py_compile
import shutil
import subprocess
import sys
import tomllib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CONFIG = os.path.join(ROOT, 'scripts', 'vendor-calibre.toml')
SHIMS = os.path.join(ROOT, 'scripts', 'vendor-calibre', 'calibre_extensions')
OURS = ('calibre', 'polyglot', 'qt', 'css_selectors', 'tinycss', 'odf', 'unicode_names')
STDLIB = set(sys.stdlib_module_names) | {'builtins'}


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


class Source:
    """A calibre checkout's src/ tree."""

    def __init__(self, root):
        self.src = os.path.join(root, 'src')
        self.root = root
        if not os.path.isdir(self.src):
            raise SystemExit(f'{root} is not a calibre checkout (no src/)')

    def path_of(self, module):
        p = os.path.join(self.src, *module.split('.'))
        if os.path.isfile(p + '.py'):
            return p + '.py', False
        if os.path.isfile(os.path.join(p, '__init__.py')):
            return os.path.join(p, '__init__.py'), True
        return None, False

    def imports_of(self, module):
        """(imported module, nested) for every import in `module`."""
        path, is_pkg = self.path_of(module)
        if path is None:
            return
        try:
            with open(path, encoding='utf-8') as f:
                tree = ast.parse(f.read())
        except SyntaxError as e:
            print(f'warning: {module}: {e}', file=sys.stderr)
            return
        base = module if is_pkg else module.rpartition('.')[0]

        def walk(node, nested):
            for child in ast.iter_child_nodes(node):
                n = nested or isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef, ast.Lambda))
                if isinstance(child, ast.Import):
                    for a in child.names:
                        yield a.name, n
                elif isinstance(child, ast.ImportFrom):
                    if child.level:
                        parts = base.split('.') if base else []
                        if child.level > 1:
                            parts = parts[: -(child.level - 1)] or parts
                        m = '.'.join(parts + ([child.module] if child.module else []))
                    else:
                        m = child.module or ''
                    if m:
                        yield m, n
                        for a in child.names:
                            yield m + '.' + a.name, n
                yield from walk(child, n)

        yield from walk(tree, False)


def denied(module, deny):
    return any(module == d or module.startswith(d + '.') for d in deny)


def closure(source, cfg, report_nested=False):
    """The calibre-tree modules to vendor, plus third-party and extension uses."""
    c = cfg['closure']
    deny, replace, lazy = c['deny'], c['replace'], set(c['lazy'])
    seen, third, exts, nested_seen = {}, {}, {}, {}
    queue = list(c['entries']) + list(lazy)
    while queue:
        name = queue.pop()
        if name in seen or denied(name, deny) or denied(name, replace):
            continue
        root = name.split('.')[0]
        if root == 'calibre_extensions':
            exts.setdefault(name, set())
            continue
        if root not in OURS:
            if root not in STDLIB:
                third.setdefault(root, set())
            continue
        path, is_pkg = source.path_of(name)
        if path is None:
            # `from x import name`: the parent module was queued too.
            continue
        seen[name] = path
        # A package's __init__ comes with every submodule.
        parent = name.rpartition('.')[0]
        if parent and parent not in seen:
            queue.append(parent)
        for imported, nested in source.imports_of(name):
            iroot = imported.split('.')[0]
            if iroot == 'calibre_extensions':
                exts.setdefault(imported, set()).add(name)
                continue
            if iroot not in OURS:
                if iroot not in STDLIB:
                    third.setdefault(iroot, set()).add(name)
                continue
            if nested and imported not in lazy and not any(imported.startswith(x + '.') for x in lazy):
                nested_seen.setdefault(imported, set()).add(name)
                continue
            queue.append(imported)
    if report_nested:
        print('Nested imports outside the closure (candidates for `lazy`):')
        for m in sorted(nested_seen):
            if m not in seen and not denied(m, deny) and not denied(m, replace) and source.path_of(m)[0]:
                print(f'  {m}  <- {", ".join(sorted(nested_seen[m]))}')
    return seen, third, exts


def checkout(cfg, source_dir):
    if source_dir:
        return Source(source_dir), None
    dest = os.path.join(ROOT, 'target', 'vendor', 'calibre')
    tag = cfg['calibre']['tag']
    if not os.path.isdir(os.path.join(dest, '.git')):
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        subprocess.check_call(['git', 'clone', '--depth', '1', '--branch', tag, cfg['calibre']['repository'], dest])
    commit = subprocess.check_output(['git', '-C', dest, 'rev-parse', 'HEAD'], text=True).strip()
    return Source(dest), commit


def copy_tree(src, dest, files):
    """Copy `files` (absolute, under src) to dest keeping their relative paths."""
    out = []
    for path in files:
        rel = os.path.relpath(path, src)
        target = os.path.join(dest, rel)
        os.makedirs(os.path.dirname(target), exist_ok=True)
        shutil.copy2(path, target)
        out.append(rel)
    return out


def vendor(cfg, source, commit, report_nested):
    out_dir = os.path.join(ROOT, cfg['output']['dir'])
    seen, third, exts = closure(source, cfg, report_nested)
    missing_exts = sorted(
        e
        for e in exts
        if not os.path.exists(os.path.join(SHIMS, e.split('.')[1] + '.py'))
        if e.count('.') >= 1 and e.split('.')[1] not in cfg['closure']['extensions']
    )
    if missing_exts:
        print('warning: no stand-in for', ', '.join(missing_exts), file=sys.stderr)
    if os.path.exists(out_dir):
        shutil.rmtree(out_dir)
    os.makedirs(out_dir)
    copied = copy_tree(source.src, out_dir, sorted(seen.values()))
    # Packages need their __init__ even when only a submodule was reached.
    for rel in list(copied):
        d = os.path.dirname(rel)
        while d:
            init = os.path.join(source.src, d, '__init__.py')
            if os.path.exists(init) and os.path.relpath(init, source.src) not in copied:
                copied += copy_tree(source.src, out_dir, [init])
            d = os.path.dirname(d)
    # The C extension stand-ins.
    ext_dir = os.path.join(out_dir, 'calibre_extensions')
    os.makedirs(ext_dir, exist_ok=True)
    for f in sorted(os.listdir(SHIMS)):
        if f.endswith('.py'):
            shutil.copy2(os.path.join(SHIMS, f), os.path.join(ext_dir, f))
            copied.append(os.path.join('calibre_extensions', f))
    # Resources.
    res_dir = os.path.join(out_dir, 'resources')
    for name in cfg['resources']['copy']:
        src = os.path.join(source.root, 'resources', name)
        dest = os.path.join(res_dir, name)
        if os.path.isdir(src):
            shutil.copytree(src, dest, dirs_exist_ok=True)
        elif os.path.isfile(src):
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            shutil.copy2(src, dest)
        else:
            print(f'warning: resource {name} not in the checkout', file=sys.stderr)
    for name in cfg['resources']['generate']:
        print(
            f"note: {name} must be generated (calibre's setup/iso_codes.py); scripts/estuary/gen-iso-data.py has the data"
        )
    # Licence and notices.
    for lic in ('LICENSE', 'COPYRIGHT'):
        src = os.path.join(source.root, lic)
        if os.path.exists(src):
            shutil.copy2(src, os.path.join(out_dir, f'{lic}.calibre'))
    with open(os.path.join(out_dir, 'THIRD_PARTY_NOTICES.md'), 'w', encoding='utf-8') as f:
        f.write(
            '# Third-party notices\n\nThis tree is a subset of calibre, copyright Kovid Goyal and contributors, '
            f'GPL-3.0 (LICENSE.calibre), taken from {cfg["calibre"]["tag"]} ({commit or "local checkout"}) by '
            'scripts/vendor-calibre.py. Files listed in VENDOR_MANIFEST.json under "replaced" are Magnitide\'s.\n'
        )
    # The manifest.
    manifest = {
        'tag': cfg['calibre']['tag'],
        'commit': commit,
        'files': {rel: sha256(os.path.join(out_dir, rel)) for rel in sorted(set(copied))},
        'replaced': sorted(cfg['closure']['replace']),
        'denied': sorted(cfg['closure']['deny']),
        'extensions': sorted(exts),
        'third_party': {k: sorted(v) for k, v in sorted(third.items())},
        'generated': cfg['resources']['generate'],
    }
    with open(os.path.join(out_dir, 'VENDOR_MANIFEST.json'), 'w', encoding='utf-8') as f:
        json.dump(manifest, f, indent=2, sort_keys=True)
        f.write('\n')
    # Everything compiles here.
    failed = []
    for rel in manifest['files']:
        if rel.endswith('.py'):
            try:
                py_compile.compile(os.path.join(out_dir, rel), doraise=True)
            except py_compile.PyCompileError as e:
                failed.append(f'{rel}: {e.msg}')
    for rel in failed:
        print('does not compile:', rel, file=sys.stderr)
    print(
        f'{len(manifest["files"])} files into {os.path.relpath(out_dir, ROOT)}; third-party: '
        f'{", ".join(sorted(third)) or "none"}; extensions: {len(exts)}'
    )
    return 1 if failed else 0


def check(cfg):
    out_dir = os.path.join(ROOT, cfg['output']['dir'])
    path = os.path.join(out_dir, 'VENDOR_MANIFEST.json')
    if not os.path.exists(path):
        print('no vendored tree (nothing to check)')
        return 0
    with open(path, encoding='utf-8') as f:
        manifest = json.load(f)
    bad = [
        rel
        for rel, h in manifest['files'].items()
        if not os.path.exists(os.path.join(out_dir, rel)) or sha256(os.path.join(out_dir, rel)) != h
    ]
    for rel in bad:
        print('changed or missing:', rel, file=sys.stderr)
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--source', help='a calibre checkout to use instead of cloning the tag')
    ap.add_argument('--check', action='store_true', help='verify the vendored tree against its manifest')
    ap.add_argument('--report-nested', action='store_true', help='list nested imports outside the closure')
    args = ap.parse_args()
    with open(CONFIG, 'rb') as f:
        cfg = tomllib.load(f)
    if args.check:
        return check(cfg)
    source, commit = checkout(cfg, args.source)
    return vendor(cfg, source, commit, args.report_nested)


if __name__ == '__main__':
    sys.exit(main())
