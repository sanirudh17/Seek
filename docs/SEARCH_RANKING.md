# Search ranking

How Seek orders name-search results, and how to change where files rank.

## Order

1. **How cleanly the name matches** (`name_tier` in `crates/engine/src/engine.rs`), for one-word queries:
   - the whole name, or the name without its extension (`report` finds `report` and `Report.docx` first);
   - the start of the name or of a word in it: after a space or punctuation, at a lower-to-upper case change (`projectConfig`), or where digits meet letters (`report2026`);
   - everything else: inside a word, across words, or with a typo.

   A typo or a scattered (fuzzy) match never ranks above a clean one.
2. **Score**, within a tier: match quality from the matcher (consecutive letters, word starts, shorter names), plus the **location prior** of the file's folder, plus small nudges for recently modified files and files Seek has opened (capped).

Demoting never hides a file. A folder's weight only moves its files within their tier, so a file you name exactly is found wherever it lives.

## Location prior

Each folder gets a prior: the sum of the weights of the folder names on its path, clamped to -100..60. A folder under your user profile (`%USERPROFILE%`) gets +15 more. Folder names match without regard to letter case.

| Folder | Weight | Where |
| --- | ---: | --- |
| Downloads | +16 | anywhere |
| Desktop | +14 | anywhere |
| Documents | +12 | anywhere |
| Pictures | +8 | anywhere |
| Videos, Music, OneDrive, source, repos | +6 | anywhere |
| Windows | -45 | directly under a drive |
| ProgramData | -30 | directly under a drive |
| Program Files, Program Files (x86) | -12 | directly under a drive |
| $Recycle.Bin, System Volume Information | -70 | directly under a drive |
| $WinREAgent, Recovery | -40 | directly under a drive |
| AppData | -25 | anywhere |
| WinSxS, node_modules, \_\_pycache\_\_, site-packages, venv, bower_components | -30 | anywhere |
| Temp, Cache, Caches, INetCache | -20 | anywhere |
| tmp, Logs | -15 | anywhere |
| target, obj | -12 | anywhere |
| build, dist, vendor, Packages | -10 | anywhere |
| out | -8 | anywhere |
| any folder whose name starts with a dot (.git, .cargo, .vs, .venv) | -25 | anywhere |

So `C:\Users\you\Downloads` is +31, `C:\Users\you\AppData\Local\Temp` is -30, and `C:\Windows\WinSxS` is -75.

## Changing the weights

Settings > Files and search > Search ranking > **Edit** opens `%LOCALAPPDATA%\Seek\ranking.json`. If the file doesn't exist yet, Edit creates it with the table above. It is a JSON list:

```json
[
  { "name": "Downloads", "weight": 16 },
  { "name": "Windows", "weight": -45, "top": true },
  { "name": "Clients", "weight": 20 }
]
```

- `name`: a folder name (letter case doesn't matter);
- `weight`: how far everything under it moves, positive up, negative down;
- `top`: optional; `true` applies the weight only to a folder directly under a drive.

The list replaces the defaults entirely, so keep the entries you want. Changes apply the next time Seek starts: the saved index is rebuilt from itself with the new weights, which takes about 2 s on a 3.5 million entry index; the disk is not scanned again. An unreadable file means the defaults. **Reset** deletes the file.

## Query language

The `?` button at the end of the search field lists these.

| Syntax | Meaning |
| --- | --- |
| `budget report` | names containing both words |
| `'budget.xlsx` | exact name |
| `^read` / `pdf$` | names starting / ending with |
| `!draft` | leave out names with |
| `ext:pdf,docx` | extensions |
| `type:image` | image, video, audio, doc, code, archive, font, app |
| `kind:folder` | file, folder or link |
| `in:C:\Projects` | only inside a folder |
| `-in:C:\Projects\old` | leave out a folder |
| `size:>100mb`, `size:1mb..5mb` | size |
| `mod:<7d` (also `mtime:`, `modified:`) | modified within |
| `name:"t3"` | name contains this exact text |
| `re:^IMG_\d+` | name matches a regular expression |
| `path:invoices` | full path matches a regular expression |

Queries are put in Unicode normalisation form C before matching, so text pasted from a Mac (an `e` followed by a combining accent) finds the composed `é` Windows uses in file names. A file whose own name is stored decomposed is only found by a decomposed query; such names are rare on Windows.
