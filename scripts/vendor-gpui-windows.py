"""Reproduce the Windows backend baseline from Seek's pinned official GPUI checkout."""
import pathlib
import shutil
import sys
import tomllib

source = pathlib.Path(sys.argv[1]).resolve()
destination = pathlib.Path(__file__).resolve().parents[1] / "vendor" / "gpui-windows"
if destination.exists() and '--manifest-only' not in sys.argv:
    raise SystemExit("Destination exists; do not overwrite local backend changes")
if not destination.exists():
    shutil.copytree(source / "crates/gpui_windows", destination)
shutil.copyfile(source / "LICENSE-APACHE", destination / "LICENSE-APACHE")
workspace = tomllib.loads((source / "Cargo.toml").read_text())['workspace']['dependencies']
pin = "279fe070bb389b79652e52065b2f001edcc0b11b"


def value(item):
    if isinstance(item, str):
        return '"' + item + '"'
    if isinstance(item, bool):
        return str(item).lower()
    if isinstance(item, list):
        return '[' + ', '.join(value(v) for v in item) + ']'
    return '{ ' + ', '.join(f'{k} = {value(v)}' for k, v in item.items()) + ' }'


manifest = destination / "Cargo.toml"
lines = (source / 'crates/gpui_windows/Cargo.toml').read_text().replace("edition.workspace = true", 'edition = "2024"')
lines = lines.replace("publish.workspace = true", "publish = false")
lines = lines.replace("[lints]\nworkspace = true\n", "")
parsed = tomllib.loads(lines)
for table in [parsed['dependencies'], parsed['target']['cfg(target_os = "windows")']['dependencies'],
              parsed['target']['cfg(target_os = "windows")']['build-dependencies']]:
    for name, dep in table.items():
        if not isinstance(dep, dict) or not dep.get('workspace'):
            continue
        resolved = workspace[name]
        if isinstance(resolved, str):
            resolved = {'version': resolved}
        else:
            resolved = resolved.copy()
        if 'path' in resolved:
            del resolved['path']
            resolved.update(git="https://github.com/zed-industries/zed", rev=pin)
            if name == 'gpui':
                resolved['version'] = '=0.2.2'
        if dep.get('features'):
            resolved['features'] = resolved.get('features', []) + dep['features']
        if dep.get('optional'):
            resolved['optional'] = True
        # Replace the one-line workspace reference (including explicit features).
        import re
        lines = re.sub(rf'^{re.escape(name)} = .*workspace.*$', name + ' = ' + value(resolved), lines, flags=re.M)
        lines = re.sub(rf'^{re.escape(name)}\.workspace = true$', name + ' = ' + value(resolved), lines, flags=re.M)

# The optional scap dependency is expressed as a subtable upstream.
scap = workspace['scap']
lines = lines.replace('workspace = true\noptional = true',
                      '\n'.join(f'{k} = {value(v)}' for k, v in scap.items()) + '\noptional = true')
manifest.write_text(lines, encoding='utf-8')
print(destination)
