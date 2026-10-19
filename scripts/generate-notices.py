"""Collect actual license texts for the locked Windows app/helper dependency graph.

Includes normal and build dependencies conservatively, excludes dev-only edges.
This generates distribution documentation; it does not change dependencies.
"""
import argparse
import concurrent.futures
import json
import pathlib
import subprocess
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]


def license_files(package):
    folder = pathlib.Path(package['manifest_path']).parent
    files = [p for p in folder.iterdir() if p.is_file() and p.name.lower().startswith(
        ('license', 'licence', 'copying', 'notice', 'copyright', 'unlicense'))]
    if package['license_file']:
        files.append((folder / package['license_file']).resolve())
    return sorted(set(files))


def fetch_missing(package, packages):
    supplied = ROOT / 'packaging/licenses' / (package['name'] + '-COPYRIGHT.txt')
    if supplied.exists():
        return [(supplied.name, supplied.read_text(encoding='utf-8'))]
    # Prefer the same upstream project's already-downloaded notices.
    for other in packages:
        if package['repository'] and other['repository'] == package['repository']:
            found = license_files(other)
            if found:
                return [(f.name, f.read_text(encoding='utf-8-sig')) for f in found]
    expression = package['license'] or ''
    if 'Apache-2.0' in expression and ' AND ' not in expression:
        return [('Apache-2.0 (selected permissive alternative)',
                 (ROOT / 'vendor/gpui-windows/LICENSE-APACHE').read_text())]
    folder = pathlib.Path(package['manifest_path']).parent
    vcs_file = folder / '.cargo_vcs_info.json'
    vcs = json.loads(vcs_file.read_text()) if vcs_file.exists() else {}
    revision = vcs.get('git', {}).get('sha1', 'HEAD')
    repo = (package['repository'] or '').removesuffix('.git').rstrip('/')
    if repo.startswith('https://github.com/'):
        base = repo.replace('https://github.com/', 'https://raw.githubusercontent.com/') + '/' + revision + '/'
    elif repo.startswith('https://gitlab.redox-os.org/'):
        base = repo + '/-/raw/' + revision + '/'
    else:
        raise RuntimeError('Missing license text for ' + package['name'])
    for name in ('LICENSE', 'LICENSE-MIT', 'LICENSE.txt', 'LICENSE.md', 'COPYING'):
        try:
            with urllib.request.urlopen(base + name, timeout=15) as response:
                text = response.read().decode('utf-8-sig')
            if 'Permission is hereby granted' in text or 'Apache License' in text:
                return [(name + ' from pinned upstream source', text)]
        except (OSError, UnicodeError):
            continue
    raise RuntimeError('Could not obtain complete license text: ' + package['name'])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--metadata', type=pathlib.Path)
    parser.add_argument('--output', type=pathlib.Path, default=ROOT / 'THIRD-PARTY-NOTICES.txt')
    args = parser.parse_args()
    if args.metadata:
        raw = args.metadata.read_bytes()
        metadata = json.loads(raw.decode('utf-16' if raw.startswith(b'\xff\xfe') else 'utf-8-sig'))
    else:
        raw = subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1',
                                       '--filter-platform', 'x86_64-pc-windows-msvc'], cwd=ROOT)
        metadata = json.loads(raw)
    packages = {p['id']: p for p in metadata['packages']}
    nodes = {n['id']: n for n in metadata['resolve']['nodes']}
    selected = set()

    def visit(key):
        if key in selected:
            return
        selected.add(key)
        for dep in nodes[key]['deps']:
            if any(k['kind'] != 'dev' for k in dep['dep_kinds']):
                visit(dep['pkg'])

    for key, package in packages.items():
        if package['name'] in ('seek', 'seek-helper'):
            visit(key)
    shipping = sorted((packages[k] for k in selected if packages[k]['name'] not in
                       ('seek', 'seek-helper')), key=lambda p: (p['name'], p['version']))
    texts = {}
    missing = []
    for p in shipping:
        files = license_files(p)
        if files:
            texts[p['id']] = [(f.name, f.read_text(encoding='utf-8-sig')) for f in files]
        else:
            missing.append(p)
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        futures = {pool.submit(fetch_missing, p, list(packages.values())): p for p in missing}
        for future in concurrent.futures.as_completed(futures):
            p = futures[future]
            texts[p['id']] = future.result()
    blocks = ['Seek 1.0.0 - Third-party notices\n\n'
              'This file includes complete license/notice texts and attribution from the locked\n'
              'Windows app/helper dependency graph, including build tools conservatively.\n'
              'Dependencies are not relicensed by Seek. For OR expressions either permissive\n'
              'alternative may be used; Apache-2.0 is selected where explicitly noted.\n'
              'fsearch-crossplatform: https://github.com/threehymns/fsearch-crossplatform\n'
              'Engine revision: 16eaebd89b8db161e4a5cac961f746565b878686 (MIT).\n'
              'GPUI revision: 279fe070bb389b79652e52065b2f001edcc0b11b (Apache-2.0).\n'
              'GPUI Kit revision: 1ccf011620da24d633f7cd6d3c11ca2e97798dcf (Apache-2.0).\n'
              'Local modifications: crates/engine/UPSTREAM.md and vendor/*/SEEK-PATCHES.md.\n'
              'For registry packages, corresponding source is available at\n'
              'https://crates.io/api/v1/crates/NAME/VERSION/download (substitute below).\n'
              'This includes MPL-2.0-covered source where listed; no MPL files are modified.\n']
    for p in shipping:
        blocks.append('\n' + '=' * 78 + '\n' + p['name'] + ' ' + p['version'] + '\n'
                      + 'License: ' + str(p['license']) + '\n'
                      + 'Upstream: ' + (p['repository'] or p['homepage'] or 'See Cargo.lock') + '\n'
                      + 'Authors: ' + ('; '.join(p['authors']) or 'See notices below') + '\n')
        for name, text in texts[p['id']]:
            blocks.append('\n--- ' + name + ' ---\n' + text.strip() + '\n')
    # Asset-specific attribution is additional to the crates' code licenses.
    f = ROOT / 'vendor/gpui-component/crates/assets/LICENSE-LUCIDE'
    blocks.append('\nLucide icon artwork\n' + f.read_text() + '\n')
    args.output.write_text(''.join(blocks), encoding='utf-8')
    print(f'Included {len(shipping)} packages; {args.output.stat().st_size} notice bytes')


if __name__ == '__main__':
    main()
