"""Three uncached release scans of current-user Known Folders; no Seek profile.

All source folders are read only. Path-bearing diagnostics remain in .references.
"""
import ctypes as c
from ctypes import wintypes as w
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import uuid

exe = Path('target/x86_64-pc-windows-msvc/release/seek.exe').resolve()
base = Path(tempfile.mkdtemp(prefix='seek-content-retry-'))


def known(value):
    guid = (c.c_ubyte * 16).from_buffer_copy(uuid.UUID(value).bytes_le)
    path = c.c_void_p()
    shell = c.WinDLL('shell32')
    shell.SHGetKnownFolderPath.argtypes = [c.c_void_p, w.DWORD, c.c_void_p, c.POINTER(c.c_void_p)]
    hr = shell.SHGetKnownFolderPath(c.byref(guid), 0, None, c.byref(path))
    if hr < 0:
        raise OSError(hr, 'Known Folder unavailable')
    try:
        return c.wstring_at(path)
    finally:
        c.WinDLL('ole32').CoTaskMemFree(c.c_void_p(path.value))


roots = [known('374DE290-123F-4565-9164-39C4925E467B'), known('FDD39AD0-238F-46AF-ADB4-6C85480369C7')]
manifest = base / 'manifest.json'
manifest.write_text(json.dumps([{'query': 'note', 'locations': roots, 'include_paths': True}]), encoding='utf-8')
reports = []
truth = None
for n in range(3):
    result = subprocess.run([str(exe), '--content-audit', str(manifest)], capture_output=True, check=True, timeout=240)
    Path(f'.references/retry-real-{n+1}.json').write_bytes(result.stdout)
    report = json.loads(result.stdout)[0]
    reports.append({k: report[k] for k in ['checked', 'total', 'unreadable', 'retried', 'recovered', 'first_result_ms', 'elapsed_ms']} |
                   {'matches': len(report['paths']), 'first_failures': len(report['first_failures']),
                    'document_candidates': sum(report['formats'].get(ext, [0])[0] for ext in ['pdf', 'docx']) - report['retried']})
    assert report['finished'] and not report['cancelled'] and report['checked'] == report['total']
    assert len(report['paths']) == len(set(report['paths'])) == 150, reports[-1]
    assert report['retried'] == len(report['first_failures'])
    assert len(report['failures']) == report['unreadable']
    assert all(f['attempts'] == 2 for f in report['failures'] if Path(f['path']).suffix.lower() in ['.pdf', '.docx'])
    if truth is None:
        truth = set(report['paths'])
    assert set(report['paths']) == truth
    print(json.dumps(reports[-1]), flush=True)
summary = {'sha256': hashlib.sha256(exe.read_bytes()).hexdigest().upper(), 'runs': reports,
           'first_failures': sum(r['first_failures'] for r in reports),
           'document_candidates': sum(r['document_candidates'] for r in reports),
           'recovered': sum(r['recovered'] for r in reports), 'all_150_and_equal_paths': True}
Path('docs/benchmarks/content-retry-summary.json').write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
Path('.references/retry-real-context.json').write_text(json.dumps({'base': str(base), 'roots': roots}), encoding='utf-8')
