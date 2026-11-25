"""Generate the review shortcut sheet from the native UI's shared registry."""
import argparse
import re
from pathlib import Path

root = Path(__file__).resolve().parents[1]
source = (root / "app/src/shortcuts.rs").read_text(encoding="utf-8")
entries = re.findall(
    r'Shortcut\s*\{\s*group:\s*"([^"\n]+)",\s*keys:\s*"([^"\n]+)",\s*action:\s*"([^"\n]+)"\s*,?\s*\}',
    source,
)
if not entries:
    raise SystemExit("No shortcut entries found; the registry format needs review.")
lines = [
    "# Phase-two keyboard reference", "",
    "Generated from `app/src/shortcuts.rs`, the same registry used by Settings > Keyboard shortcuts. Run `python scripts/generate-shortcuts.py --check` to check for drift.", "",
    "These shortcuts act within Seek. The focused reader, text field or sheet owns its editing/navigation keys.", "",
]
group = None
for section, keys, action in entries:
    if section != group:
        group = section
        lines += [f"## {group}", "", "| Shortcut | Action |", "| --- | --- |"]
    lines.append(f"| {keys} | {action.replace('|', '/')} |")
    if entries[-1] == (section, keys, action):
        lines.append("")
    elif entries[entries.index((section, keys, action)) + 1][0] != group:
        lines.append("")
content = "\n".join(lines)
target = root / "docs/revamp/PHASE2-SHORTCUTS.md"
parser = argparse.ArgumentParser()
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
if args.check:
    if not target.exists() or target.read_text(encoding="utf-8") != content:
        raise SystemExit("Shortcut sheet differs from the native registry; regenerate it.")
else:
    target.write_text(content, encoding="utf-8")
print(f"{len(entries)} shortcut entries {'checked' if args.check else 'generated'}")
