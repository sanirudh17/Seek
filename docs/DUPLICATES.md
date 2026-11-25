# Find Duplicates

How Seek finds duplicate files and folders on the Storage page, and why it works this way. Code: `app/src/duplicates.rs` (finding) and `app/src/duplicates_view.rs` (choosing and moving).

## What other tools do

| Tool | Matching | Safety |
| --- | --- | --- |
| Monodrive (`docs/storage.md`) | Same size, then BLAKE3 of the first, middle and last 64 KiB, then a full BLAKE3; each file's identity, length and modification time must match before and after reading | No automatic rules: "nothing is chosen by age, folder or name". The person names exact copies; a plan that would trash every copy of a group is refused; the owner re-checks each copy's stamp before moving |
| fclones | Size, prefix and suffix hashes, then full hash; hard links and reflinks are one file | Refuses to remove the last copy; checks that files weren't modified since grouping |
| czkawka | Size, a pre-hash of the start, then a full hash (BLAKE3, CRC32 or XXH3) | Excluded folders, reference folders, minimum size, system files ignored |
| dupeGuru | Contents or names | Reference folders are never touched; a Folders mode reports folders whose whole contents match |
| Duplicate Cleaner | Contents | A duplicate-folders mode lists only folders that are 100% identical |

## What Seek does

**Finding files.** Files that share a size are candidates. A BLAKE3 hash of three 64 KB samples splits most of them without reading them whole; the rest are hashed in full, and a group exists only when the full hashes match. A file whose length or modification time changed while it was read is left out. Hard links to one file are one file. Links, junctions and cloud placeholders are never opened (opening a placeholder would download it). System files and folder clutter (`desktop.ini`, `Thumbs.db`, `.DS_Store`) are left out.

**Search everywhere** compares files of 1 MB or more that the index knows (or, when the index doesn't cover the profile, a walk of it). It leaves out places where copies are needed where they are:

- Windows, Program Files, ProgramData and the Recycle Bin;
- app data: AppData and the profile's hidden dot-folders (`.codex`, `.local`, `.gemini`);
- package caches and build output (`node_modules`, `.cargo`, `target`, `.venv`);
- projects: any folder inside a Git repository, whose copies (vendored binaries, assets) belong to each project;
- document bundles: folders an app keeps as one document, named with an extension (`Clip.glint-studio`).

**In a folder…** checks one folder completely, without those exclusions, because the person chose it.

**Identical folders.** Two folders count only when every file and subfolder matches by relative name and size, and every file's contents match (already-grouped files are known to match; others are hashed, within a budget). A folder is never paired with one inside it. A match is widened to the highest parents that still match (stopping at the profile, a drive, or where the two meet), so a copied project shows once. Folders holding links or placeholders, or more than 20,000 entries, aren't compared.

**Choosing.** Nothing is selected for you. Each copy has a checkbox; Select extras ticks all but one copy per group by a rule (Oldest, Newest, Shortest path) only when pressed, and Clear undoes the choice. The last copy of a file can't be ticked. In Identical folders, ticking one side unticks the other.

**Moving.** The plan refuses a move that would leave a file with no copy, counting copies inside folders being moved. Then, off the interface thread, each ticked file must still have the size and modification time it had when compared, each ticked folder must still list exactly as compared, and a copy that stays must be intact. Everything goes to the Recycle Bin in one journaled operation; Undo on the page (Ctrl+Z in the file list) puts it back.

## Measured on the development PC

Search everywhere, debug build, profile walk (no index): before the exclusions above it reported 683 groups (6.4 GB) and pairs of tool folders that are each a tool's own install; with them, 18 groups, all copies in Downloads and Videos (re-downloaded PDFs and slides, an exported video), 89 MB, in 2.1 s cold.
