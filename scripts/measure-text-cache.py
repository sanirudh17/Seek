"""Read-only real Known Folder RELEASE measurement, using an isolated profile cache.

No normal Seek profile is loaded. Reports with paths stay in .references.
"""
import ctypes as c
from ctypes import wintypes as w
import json
import subprocess
import tempfile
import time
import uuid
import argparse
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('--control-existing', type=Path,
                    help='Reuse an uncached JSON audit from the same executable and unchanged source folders')
options = parser.parse_args()
exe = Path('target/x86_64-pc-windows-msvc/release/seek.exe').resolve()
base = Path(tempfile.mkdtemp(prefix='seek-text-cache-release-'))
data = base / 'data'
data.mkdir()


def known_folder(value):
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


roots = [known_folder('374DE290-123F-4565-9164-39C4925E467B'),
         known_folder('FDD39AD0-238F-46AF-ADB4-6C85480369C7')]
manifest = base / 'manifest.json'
manifest.write_text(json.dumps([{'query': q, 'locations': roots, 'include_paths': True}
                                for q in ['note', 'rudkin']]), encoding='utf-8')


class Counters(c.Structure):
    _fields_ = [('cb', w.DWORD), ('PageFaultCount', w.DWORD)] + [(n, c.c_size_t) for n in
               ['PeakWorkingSetSize', 'WorkingSetSize', 'QuotaPeakPagedPoolUsage',
                'QuotaPagedPoolUsage', 'QuotaPeakNonPagedPoolUsage', 'QuotaNonPagedPoolUsage',
                'PagefileUsage', 'PeakPagefileUsage', 'PrivateUsage']]


get_memory = c.WinDLL('psapi').GetProcessMemoryInfo
get_memory.argtypes = [w.HANDLE, c.POINTER(Counters), w.DWORD]
reports = {}


def run(label, cached):
    args = [str(exe), '--content-audit', str(manifest)]
    if cached:
        args += ['--text-cache-data', str(data)]
    proc = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    started = time.perf_counter()
    peak_private = peak_ws = 0
    while True:
        memory = Counters()
        memory.cb = c.sizeof(memory)
        if get_memory(w.HANDLE(int(proc._handle)), c.byref(memory), memory.cb):
            peak_private = max(peak_private, memory.PrivateUsage)
            peak_ws = max(peak_ws, memory.PeakWorkingSetSize)
        try:
            out, err = proc.communicate(timeout=0.02)
            break
        except subprocess.TimeoutExpired:
            if time.perf_counter() - started > 240:
                proc.kill()
                raise RuntimeError('Measurement exceeded 240 seconds')
    if proc.returncode:
        raise RuntimeError(err.decode(errors='replace'))
    Path('.references/text-cache-release-' + label + '.json').write_bytes(out)
    rows = json.loads(out)
    assert all(r['finished'] and r['checked'] == r['total'] and not r['cache']['error'] for r in rows), rows
    reports[label] = {'rows': rows, 'parent_peak_private_bytes_sampled': peak_private,
                      'parent_peak_working_set_bytes': peak_ws,
                      'process_wall_seconds': time.perf_counter() - started}
    print(label, [(r['query'], len(r['paths']), round(r['first_result_ms'], 2),
                   round(r['elapsed_ms'], 2), r['cache']) for r in rows], flush=True)


run('empty', True)  # note fills the cache; rudkin in this process already reuses it.
run('full-restart', True)
if options.control_existing:
    reports['uncached-control'] = {'rows': json.loads(options.control_existing.read_text(encoding='utf-8')),
                                 'parent_peak_private_bytes_sampled': None,
                                 'parent_peak_working_set_bytes': None,
                                 'process_wall_seconds': None, 'reused': True}
else:
    run('uncached-control', False)
Path('.references/text-cache-release-raw.json').write_text(json.dumps({'data': str(data), 'reports': reports}, indent=2), encoding='utf-8')
for i in range(2):
    truth = set(reports['uncached-control']['rows'][i]['paths'])
    assert all(set(report['rows'][i]['paths']) == truth for report in reports.values()), 'Unequal final paths'
cache = data / 'document-text-v1'
disk = list(cache.iterdir())
summary = {'data': str(data), 'roots': roots, 'logical_cache_bytes': sum(p.stat().st_size for p in disk),
           'cache_files': len(disk), 'reports': reports, 'path_sets_equal': True,
           'note_matches': len(reports['empty']['rows'][0]['paths']),
           'rudkin_matches': len(reports['empty']['rows'][1]['paths'])}
Path('.references/text-cache-release-measurements.json').write_text(json.dumps(summary, indent=2), encoding='utf-8')
Path('.references/text-cache-release-data.txt').write_text(str(data), encoding='utf-8')
print('Exact final path sets equal. Data:', data, flush=True)
