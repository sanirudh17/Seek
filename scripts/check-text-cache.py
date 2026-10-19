"""Owned-profile fixtures; separate process per audit. Never opens Seek's profile.

Usage: python scripts/check-text-cache.py [path/to/seek.exe]
"""
import json
import subprocess
import sys
import tempfile
from pathlib import Path
from docx import Document

exe = Path(sys.argv[1] if len(sys.argv) > 1 else
           'target/x86_64-pc-windows-msvc/debug/seek.exe').resolve()
base = Path(tempfile.mkdtemp(prefix='seek-text-cache-check-'))
files, data = base / 'files', base / 'data'
data.mkdir()
subprocess.run([sys.executable, 'scripts/make-content-accuracy-fixture.py', str(files)], check=True)
manifest = base / 'accuracy-manifest.json'
cases = json.loads(manifest.read_text(encoding='utf-8'))
for case in cases:
    case['include_paths'] = True
manifest.write_text(json.dumps(cases), encoding='utf-8')
checks = []


def audit(label, cached, cases_path=manifest):
    args = [str(exe), '--content-audit', str(cases_path)]
    if cached:
        args += ['--text-cache-data', str(data)]
    result = subprocess.run(args, capture_output=True, timeout=120, check=True)
    (base / (label + '.json')).write_bytes(result.stdout)
    rows = json.loads(result.stdout)
    assert all(r['finished'] and r['checked'] == r['total'] and not r['cache']['error'] for r in rows), rows
    assert all(not r['accuracy'] or r['accuracy']['passed'] for r in rows), rows
    return rows


def paths(rows):
    return [sorted(r['paths']) for r in rows]


plain = audit('uncached', False)
cold = audit('cold', True)
warm = audit('restart-warm', True)
assert paths(plain) == paths(cold) == paths(warm)
assert warm[0]['cache']['hits'] >= 4, warm[0]['cache']
checks.append({'name': '10 known-answer PDF/DOCX cases equal uncached, cold and process-restarted warm', 'passed': True,
               'warm_cache': warm[0]['cache']})


def compare(label, roots=None):
    path = base / 'mutation-manifest.json'
    path.write_text(json.dumps([{'query': 'quartzmarker', 'locations': [str(p) for p in (roots or [files])],
                                'include_paths': True}]), encoding='utf-8')
    plain, cached = audit(label + '-plain', False, path), audit(label + '-cache', True, path)
    assert paths(plain) == paths(cached), label
    checks.append({'name': label, 'passed': True, 'cache': cached[0]['cache'], 'matches': len(cached[0]['paths'])})
    return cached


document = Document()
document.add_paragraph('changed content without the requested marker')
document.save(files / 'Structured.docx')
compare('modified-source')
(files / 'Structured.pdf').rename(files / 'Renamed.pdf')
compare('renamed-source')
sub = files / 'subfolder'
sub.mkdir()
(files / 'Renamed.pdf').rename(sub / 'Moved.pdf')
compare('moved-source')
(sub / 'Moved.pdf').unlink()  # Disposable fixture only, never user documents.
compare('deleted-source')
empty = files / 'empty-scope'
empty.mkdir()
compare('scope-removed', [empty])
assert len(list((data / 'document-text-v1').glob('*.bin'))) == 1  # encrypted empty manifest
checks.append({'name': 'removed scopes purge all extracted payloads while source documents still exist', 'passed': True})
summary = {'executable': str(exe), 'fixture': str(base), 'checks': checks}
(base / 'summary.json').write_text(json.dumps(summary, indent=2), encoding='utf-8')
Path('.references/text-cache-fixture-latest.txt').write_text(str(base), encoding='utf-8')
print(json.dumps(summary, indent=2))
