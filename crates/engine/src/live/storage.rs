//! Storage overview (Phase 9.1): one pass over the index, never the disk.
//! Each folder takes a category from its own name (Developer caches, Apps,
//! Windows, Temp and caches, Cloud), inherited by everything inside it; other
//! files are sorted by extension. Cloud placeholders count as Cloud with
//! nothing on disk. Bounded by a deadline and a cancel flag.
use super::*;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

pub const CATEGORIES: [&str; 10] = [
    "Developer",
    "Apps",
    "Windows",
    "Documents",
    "Pictures",
    "Videos",
    "Music",
    "Cloud",
    "Temp and caches",
    "Other",
];
const DEVELOPER: u8 = 0;
const APPS: u8 = 1;
const WINDOWS: u8 = 2;
const DOCUMENTS: u8 = 3;
const PICTURES: u8 = 4;
const VIDEOS: u8 = 5;
const MUSIC: u8 = 6;
const CLOUD: u8 = 7;
const TEMP: u8 = 8;
const OTHER: u8 = 9;
/// No category from the folder; the file's extension decides.
const NONE: u8 = u8::MAX;

#[derive(Clone, Debug, Default)]
pub struct Storage {
    pub bytes: u64,
    /// Rounded up to whole clusters; placeholders take nothing.
    pub on_disk: u64,
    pub files: u64,
    /// (bytes, files) per entry of `CATEGORIES`.
    pub categories: [(u64, u64); 10],
    /// The scope's direct child folders, largest first: (path, bytes).
    pub children: Vec<(std::path::PathBuf, u64)>,
    /// Bytes in files directly inside the scope.
    pub here: u64,
    /// Why measuring stopped early, if it did.
    pub stopped: Option<String>,
}

fn lower(name: &[u8]) -> Vec<u8> {
    name.to_ascii_lowercase()
}

/// A folder's own category, from its name.
fn folder_category(name: &[u8], top_of_drive: bool) -> u8 {
    let n = lower(name);
    let n = n.as_slice();
    match n {
        b".cargo" | b".rustup" | b".gradle" | b".npm" | b".nuget" | b".android" | b".m2"
        | b".pnpm-store" | b".yarn" | b"node_modules" | b".venv" | b"__pycache__"
        | b".dotnet" | b".vscode-server" | b".cache" => DEVELOPER,
        b"windows" if top_of_drive => WINDOWS,
        b"program files" | b"program files (x86)" | b"programdata" if top_of_drive => APPS,
        b"temp" | b"tmp" | b"cache" | b"caches" | b"inetcache" | b"crashdumps"
        | b"$recycle.bin" | b"code cache" | b"gpucache" => TEMP,
        b"icloud drive" | b"icloudphotos" | b"dropbox" | b"google drive" => CLOUD,
        _ if n.starts_with(b"onedrive") => CLOUD,
        _ => NONE,
    }
}

/// A file's category from its extension, when its folder gives none.
fn file_category(name: &[u8]) -> u8 {
    let n = lower(name);
    let ext = n.rsplit(|b| *b == b'.').next().filter(|e| e.len() < n.len());
    match ext.unwrap_or(b"") {
        b"jpg" | b"jpeg" | b"png" | b"gif" | b"webp" | b"bmp" | b"heic" | b"heif" | b"tif"
        | b"tiff" | b"raw" | b"cr2" | b"nef" | b"arw" | b"dng" | b"svg" | b"psd" => PICTURES,
        b"mp4" | b"mov" | b"mkv" | b"avi" | b"wmv" | b"webm" | b"m4v" | b"mpg" | b"mpeg" => {
            VIDEOS
        }
        b"mp3" | b"wav" | b"flac" | b"m4a" | b"aac" | b"ogg" | b"opus" | b"wma" => MUSIC,
        b"pdf" | b"doc" | b"docx" | b"xls" | b"xlsx" | b"ppt" | b"pptx" | b"txt" | b"md"
        | b"rtf" | b"odt" | b"ods" | b"odp" | b"csv" | b"epub" | b"pages" | b"key" => DOCUMENTS,
        b"exe" | b"msi" | b"dll" => APPS,
        _ => OTHER,
    }
}

fn on_disk(size: u64, cluster: u64, kind: u8) -> u64 {
    if kind & walk::FLAG_CLOUD != 0 || size == 0 {
        0
    } else {
        size.div_ceil(cluster.max(1)) * cluster.max(1)
    }
}

impl Live {
    pub(crate) fn storage(
        &self,
        scope: &[u8],
        cluster: u64,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Storage {
        let mut out = Storage::default();
        let mut scope = normalize(scope);
        while scope.len() > 1 && scope.last() == Some(&b'/') {
            scope.pop();
        }
        let base_dir = self
            .base
            .lookup(&scope)
            .filter(|e| !self.is_dead(*e))
            .and_then(|e| self.base.dir_of(e));
        let mut path = Vec::new();
        let canonical_scope = if let Some(scope_dir) = base_dir {
            self.base
                .path(self.base.dir_entry()[scope_dir as usize] as usize, &mut path);
            path.clone()
        } else if self
            .over
            .get(&scope)
            .is_some_and(|e| e.kind & 3 == walk::KIND_DIR)
        {
            scope.clone()
        } else if let Some((found, _)) = self
            .over
            .iter()
            .find(|(p, e)| e.kind & 3 == walk::KIND_DIR && crate::paths::cmp_ci(p, &scope).is_eq())
        {
            found.clone()
        } else {
            out.stopped = Some("This folder isn't in the index yet".into());
            return out;
        };
        // The scope's own ancestry decides its category (C:\Windows\x).
        let mut inherited = NONE;
        let mut parent_is_drive = false;
        for part in canonical_scope.split(|b| *b == b'/').filter(|p| !p.is_empty()) {
            let own = folder_category(part, parent_is_drive);
            if own != NONE {
                inherited = own;
            }
            parent_is_drive = part.ends_with(b":");
        }
        let mut children: Vec<(Vec<u8>, u64)> = vec![];
        if let Some(scope_dir) = base_dir {
            let d_count = self.base.d;
            let start = self.base.dir_start()[scope_dir as usize] as usize;
            let end = self.base.dir_end()[scope_dir as usize] as usize;
            // Folder categories and which direct child of the scope each folder
            // is under, top-down (a parent folder always comes before its children).
            let mut category = vec![NONE; d_count];
            let mut top = vec![u32::MAX; d_count];
            let mut drive = vec![false; d_count];
            category[scope_dir as usize] = inherited;
            drive[scope_dir as usize] = parent_is_drive;
            let mut child_bytes: Vec<u64> = vec![0; d_count];
            for d in 1..d_count {
                let parent = self.base.dir_parent()[d] as usize;
                let entry = self.base.dir_entry()[d] as usize;
                if entry < start || entry >= end {
                    continue;
                }
                let name = self.base.name(entry);
                drive[d] = name.ends_with(b":");
                let own = folder_category(name, drive[parent]);
                category[d] = if own != NONE { own } else { category[parent] };
                top[d] = if parent == scope_dir as usize {
                    d as u32
                } else {
                    top[parent]
                };
            }
            for (n, i) in (start..end).enumerate() {
                if n % 65_536 == 0 {
                    if cancel.load(Ordering::Relaxed) {
                        out.stopped = Some("Stopped".into());
                        break;
                    }
                    if Instant::now() > deadline {
                        out.stopped = Some("Took too long; showing what was measured".into());
                        break;
                    }
                }
                let (kind, size, _) = self.attributes(i);
                if kind & 3 != walk::KIND_FILE
                    || self.is_dead(i as u32)
                    || kind & walk::FLAG_SYNTH != 0
                {
                    continue;
                }
                let dir = self.base.parent()[i] as usize;
                let mut cat = category[dir];
                if kind & walk::FLAG_CLOUD != 0 {
                    cat = CLOUD;
                }
                if cat == NONE {
                    cat = file_category(self.base.name(i));
                }
                out.bytes += size;
                out.files += 1;
                out.on_disk += on_disk(size, cluster, kind);
                out.categories[cat as usize].0 += size;
                out.categories[cat as usize].1 += 1;
                if dir == scope_dir as usize {
                    out.here += size;
                } else if top[dir] != u32::MAX {
                    child_bytes[top[dir] as usize] += size;
                }
            }
            for d in self.base.children(scope_dir) {
                if let Some(dir) = self.base.dir_of(d as u32)
                    && !self.is_dead(d as u32)
                {
                    self.base.path(d, &mut path);
                    children.push((path.clone(), child_bytes[dir as usize]));
                }
            }
        }
        // Recent changes (or a folder / drive whose subtree was moved into the
        // overlay by a recursive relist or watcher overflow) under this scope.
        let mut overlay_children = std::collections::HashMap::<Vec<u8>, u64>::new();
        if out.stopped.is_none() {
            let (lo, hi) = subtree_bounds(&canonical_scope);
            let prefix_len = canonical_scope.len();
            for (n, (p, e)) in self.over.range(lo..hi).enumerate() {
                if n % 65_536 == 0 {
                    if cancel.load(Ordering::Relaxed) {
                        out.stopped = Some("Stopped".into());
                        break;
                    }
                    if Instant::now() > deadline {
                        out.stopped = Some("Took too long; showing what was measured".into());
                        break;
                    }
                }
                if e.kind & walk::FLAG_SYNTH != 0 {
                    continue;
                }
                let rest = &p[prefix_len..];
                if !rest.starts_with(b"/") {
                    continue;
                }
                let parts: Vec<&[u8]> = rest
                    .split(|b| *b == b'/')
                    .filter(|c| !c.is_empty())
                    .collect();
                if parts.is_empty() {
                    continue;
                }
                if e.kind & 3 == walk::KIND_DIR {
                    if parts.len() == 1 {
                        overlay_children.entry(p.clone()).or_default();
                    }
                    continue;
                }
                if e.kind & 3 != walk::KIND_FILE {
                    continue;
                }
                let mut cat = inherited;
                let mut above_drive = parent_is_drive;
                for part in parts.iter().take(parts.len().saturating_sub(1)) {
                    let own = folder_category(part, above_drive);
                    if own != NONE {
                        cat = own;
                    }
                    above_drive = part.ends_with(b":");
                }
                if e.kind & walk::FLAG_CLOUD != 0 {
                    cat = CLOUD;
                }
                if cat == NONE {
                    cat = file_category(parts.last().copied().unwrap_or(b""));
                }
                out.bytes += e.size;
                out.files += 1;
                out.on_disk += on_disk(e.size, cluster, e.kind);
                out.categories[cat as usize].0 += e.size;
                out.categories[cat as usize].1 += 1;
                if parts.len() > 1 {
                    let mut child = canonical_scope.clone();
                    child.push(b'/');
                    child.extend_from_slice(parts[0]);
                    *overlay_children.entry(child).or_default() += e.size;
                } else {
                    out.here += e.size;
                }
            }
        }
        for (child, bytes) in overlay_children {
            match children
                .iter_mut()
                .find(|(p, _)| crate::paths::cmp_ci(p, &child).is_eq())
            {
                Some((_, total)) => *total += bytes,
                None => children.push((child, bytes)),
            }
        }
        children.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| crate::paths::cmp_ci(&a.0, &b.0)));
        out.children = children
            .into_iter()
            .map(|(p, b)| (crate::paths::native(&p), b))
            .collect();
        out
    }

    /// Zero-allocation fast byte total for a folder subtree (used by Worktrees
    /// and Cleanup sizing without allocating drive-wide directory tables).
    pub(crate) fn subtree_bytes(&self, scope: &[u8], deadline: Instant, cancel: &AtomicBool) -> u64 {
        let mut scope = normalize(scope);
        while scope.len() > 1 && scope.last() == Some(&b'/') {
            scope.pop();
        }
        let base_dir = self
            .base
            .lookup(&scope)
            .filter(|e| !self.is_dead(*e))
            .and_then(|e| self.base.dir_of(e));
        let mut path = Vec::new();
        let canonical_scope = if let Some(scope_dir) = base_dir {
            self.base
                .path(self.base.dir_entry()[scope_dir as usize] as usize, &mut path);
            path.clone()
        } else if self
            .over
            .get(&scope)
            .is_some_and(|e| e.kind & 3 == walk::KIND_DIR)
        {
            scope
        } else if let Some((found, _)) = self
            .over
            .iter()
            .find(|(p, e)| e.kind & 3 == walk::KIND_DIR && crate::paths::cmp_ci(p, &scope).is_eq())
        {
            found.clone()
        } else {
            return 0;
        };
        let mut total = 0u64;
        if let Some(scope_dir) = base_dir {
            let start = self.base.dir_start()[scope_dir as usize] as usize;
            let end = self.base.dir_end()[scope_dir as usize] as usize;
            for (n, i) in (start..end).enumerate() {
                if n.is_multiple_of(65_536)
                    && (cancel.load(Ordering::Relaxed) || Instant::now() > deadline)
                {
                    return total;
                }
                let (kind, size, _) = self.attributes(i);
                if kind & 3 == walk::KIND_FILE
                    && !self.is_dead(i as u32)
                    && kind & walk::FLAG_SYNTH == 0
                {
                    total = total.saturating_add(size);
                }
            }
        }
        let (lo, hi) = subtree_bounds(&canonical_scope);
        let prefix_len = canonical_scope.len();
        for (n, (p, e)) in self.over.range(lo..hi).enumerate() {
            if n.is_multiple_of(65_536)
                && (cancel.load(Ordering::Relaxed) || Instant::now() > deadline)
            {
                return total;
            }
            if e.kind & walk::FLAG_SYNTH != 0 || !p[prefix_len..].starts_with(b"/") {
                continue;
            }
            if e.kind & 3 == walk::KIND_FILE {
                total = total.saturating_add(e.size);
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk::{Listing, RawEnt};

    fn listing(id: u32, entries: &[(&str, u8, u64, u32)]) -> Listing {
        let mut l = Listing {
            id,
            names: vec![],
            ents: vec![],
        };
        let mut sorted = entries.to_vec();
        sorted.sort_by(|a, b| crate::paths::cmp_ci(a.0.as_bytes(), b.0.as_bytes()));
        for (name, kind, size, child) in sorted {
            l.ents.push(RawEnt {
                name_off: l.names.len() as u32,
                name_len: name.len() as u16,
                kind,
                size,
                mtime: 0,
                child,
            });
            l.names.extend(name.as_bytes());
        }
        l
    }

    #[test]
    fn categories_follow_folders_then_extensions() {
        const N: u32 = crate::walk::NONE;
        let d = walk::KIND_DIR;
        let f = walk::KIND_FILE;
        let live = Live::new(crate::index::Index::build(
            vec![
                listing(0, &[("C:", d, 0, 1)]),
                listing(1, &[("Windows", d, 0, 2), ("Users", d, 0, 3), ("Program Files", d, 0, 4)]),
                listing(2, &[("a.dll", f, 100, N)]),
                listing(3, &[("me", d, 0, 5)]),
                listing(4, &[("app.exe", f, 40, N)]),
                listing(
                    5,
                    &[
                        ("photo.jpg", f, 50, N),
                        ("notes.pdf", f, 30, N),
                        (".cargo", d, 0, 6),
                        ("cloud.docx", f | walk::FLAG_CLOUD, 20, N),
                    ],
                ),
                listing(6, &[("x.rlib", f, 70, N)]),
            ],
            0,
            0,
            &[],
        ));
        let never = Instant::now() + std::time::Duration::from_secs(60);
        let s = live.storage(b"/C:", 4096, never, &AtomicBool::new(false));
        assert_eq!(s.stopped, None);
        assert_eq!(s.bytes, 310);
        assert_eq!(s.files, 6);
        assert_eq!(s.categories[WINDOWS as usize], (100, 1));
        assert_eq!(s.categories[APPS as usize], (40, 1));
        assert_eq!(s.categories[PICTURES as usize], (50, 1));
        assert_eq!(s.categories[DOCUMENTS as usize], (30, 1));
        assert_eq!(s.categories[DEVELOPER as usize], (70, 1));
        assert_eq!(s.categories[CLOUD as usize], (20, 1));
        // Five files on disk at one 4 KiB cluster each; the placeholder takes none.
        assert_eq!(s.on_disk, 5 * 4096);
        let names: Vec<_> = s
            .children
            .iter()
            .map(|(p, b)| (p.file_name().unwrap().to_string_lossy().into_owned(), *b))
            .collect();
        assert_eq!(
            names,
            [("Users".into(), 170), ("Windows".into(), 100), ("Program Files".into(), 40)]
        );
        // Drilling into a folder keeps its inherited category.
        let windows = live.storage(b"/C:/Windows", 4096, never, &AtomicBool::new(false));
        assert_eq!(windows.categories[WINDOWS as usize], (100, 1));
        assert_eq!(windows.here, 100);
        // Bounded: a cancelled pass says so.
        let stopped = live.storage(b"/C:", 4096, never, &AtomicBool::new(true));
        assert_eq!(stopped.stopped.as_deref(), Some("Stopped"));
    }

    #[test]
    fn storage_measures_drive_and_subfolders_after_recursive_overlay_replacement() {
        const N: u32 = crate::walk::NONE;
        let d = walk::KIND_DIR;
        let f = walk::KIND_FILE;
        let mut live = Live::new(crate::index::Index::build(
            vec![
                listing(0, &[("C:", d, 0, 1)]),
                listing(1, &[("Users", d, 0, 2)]),
                listing(2, &[("old.bin", f, 999, N)]),
            ],
            0,
            0,
            &[],
        ));
        // Simulate a watcher overflow (e.g. after deleting 195k cache files):
        // `/C:` is killed in `base` and its replacement tree lives in `over`.
        let replacement = vec![
            listing(0, &[("Windows", d, 0, 1), ("Users", d, 0, 2), ("Empty", d, 0, 3)]),
            listing(1, &[("kernel.dll", f, 120, N)]),
            listing(2, &[("photo.jpg", f, 80, N), ("doc.pdf", f, 40, N)]),
            listing(3, &[]),
        ];
        live.apply(Fetched {
            path: b"/C:".to_vec(),
            recursive: true,
            blocked: false,
            attrs: Some(OEnt::new(b"/C:", d, 0, 0)),
            listing: None,
            scans: [(b"/C:".to_vec(), replacement)].into(),
        });
        let never = Instant::now() + std::time::Duration::from_secs(60);
        let s = live.storage(b"/C:", 4096, never, &AtomicBool::new(false));
        assert_eq!(s.stopped, None, "overlay drive root must measure without stalling");
        assert_eq!(s.bytes, 240);
        assert_eq!(s.files, 3);
        assert_eq!(s.categories[WINDOWS as usize], (120, 1));
        assert_eq!(s.categories[PICTURES as usize], (80, 1));
        assert_eq!(s.categories[DOCUMENTS as usize], (40, 1));
        let names: Vec<_> = s
            .children
            .iter()
            .map(|(p, b)| (p.file_name().unwrap().to_string_lossy().into_owned(), *b))
            .collect();
        assert_eq!(
            names,
            [
                ("Users".into(), 120),
                ("Windows".into(), 120),
                ("Empty".into(), 0),
            ]
        );
        // Drilling into an overlay-only subfolder also works (and matches case-insensitively).
        let windows = live.storage(b"/c:/windows", 4096, never, &AtomicBool::new(false));
        assert_eq!(windows.stopped, None);
        assert_eq!(windows.bytes, 120);
        assert_eq!(windows.here, 120);
        assert_eq!(windows.categories[WINDOWS as usize], (120, 1));
    }
}
