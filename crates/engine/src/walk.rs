//! Windows replacement for upstream walk.rs. Never follows name-surrogate links.
//! Cloud/provider entries remain ordinary files and directories in the namespace.
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicU32, AtomicUsize, Ordering},
    },
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_FILES, ERROR_PATH_NOT_FOUND, GetLastError,
        INVALID_HANDLE_VALUE,
    },
    Storage::FileSystem::*,
};

pub const NONE: u32 = u32::MAX;
pub const KIND_FILE: u8 = 0;
pub const KIND_DIR: u8 = 1;
pub const KIND_LINK: u8 = 2;
pub const KIND_OTHER: u8 = 3;
pub const FLAG_HIDDEN: u8 = 1 << 2;
pub const FLAG_MOUNT: u8 = 1 << 3;
pub const FLAG_SYNTH: u8 = 1 << 4;
pub const FLAG_SYSTEM: u8 = 1 << 5;
pub static DENIED: Mutex<BTreeMap<Vec<u8>, u32>> = Mutex::new(BTreeMap::new());
pub static LISTED: AtomicUsize = AtomicUsize::new(0);
/// A vanished directory is a normal change, not a current access failure.
/// Also used for older helper versions that report path-not-found as skipped.
pub fn record_failure(path: Vec<u8>, error: u32) {
    let mut denied = DENIED.lock().unwrap();
    if matches!(error, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) {
        denied.remove(&path);
    } else {
        denied.insert(path, error);
    }
}

fn listed(path: &[u8], listing: &Listing) {
    let mut denied = DENIED.lock().unwrap();
    denied.remove(path);
    // A complete parent listing proves absent child subtrees no longer exist.
    // Only inspect recorded failures under this prefix, not the whole map.
    let mut prefix = path.to_vec();
    prefix.push(b'/');
    let obsolete = denied
        .range(prefix.clone()..)
        .take_while(|(p, _)| p.starts_with(&prefix))
        .filter(|(p, _)| {
            let child = p[prefix.len()..].split(|b| *b == b'/').next().unwrap();
            !listing.ents.iter().any(|entry| {
                let name = &listing.names[entry.name_off as usize..][..entry.name_len as usize];
                entry.kind & 3 == KIND_DIR && crate::paths::cmp_ci(name, child).is_eq()
            })
        })
        .map(|(p, _)| p.clone())
        .collect::<Vec<_>>();
    for path in obsolete {
        denied.remove(&path);
    }
}
pub fn blocked(_: &[u8]) -> bool {
    false
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct RawEnt {
    pub name_off: u32,
    pub name_len: u16,
    pub kind: u8,
    pub size: u64,
    pub mtime: u32,
    pub child: u32,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Listing {
    pub id: u32,
    pub names: Vec<u8>,
    pub ents: Vec<RawEnt>,
}

pub trait IndexSource: Send + Sync {
    fn scan(&self, root: &[u8], threads: usize) -> std::io::Result<Vec<Listing>>;
    fn list(&self, path: &[u8]) -> Option<Listing>;
}
pub struct WalkerSource;
impl IndexSource for WalkerSource {
    fn scan(&self, root: &[u8], threads: usize) -> std::io::Result<Vec<Listing>> {
        Ok(scan(root, threads))
    }
    fn list(&self, path: &[u8]) -> Option<Listing> {
        list_one(path)
    }
}

pub fn timestamp(time: windows_sys::Win32::Foundation::FILETIME) -> u32 {
    let ticks = ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64;
    (ticks / 10_000_000)
        .saturating_sub(11_644_473_600)
        .min(u32::MAX as u64) as u32
}

pub fn flags(attrs: u32) -> u8 {
    flags_with_tag(attrs, 0)
}
pub fn flags_with_tag(attrs: u32, reparse_tag: u32) -> u8 {
    (if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0
        && (reparse_tag == 0 || reparse_tag & 0x2000_0000 != 0)
    {
        KIND_LINK
    } else if attrs & FILE_ATTRIBUTE_DIRECTORY != 0 {
        KIND_DIR
    } else {
        KIND_FILE
    }) | (if attrs & FILE_ATTRIBUTE_HIDDEN != 0 {
        FLAG_HIDDEN
    } else {
        0
    }) | (if attrs & FILE_ATTRIBUTE_SYSTEM != 0 {
        FLAG_SYSTEM
    } else {
        0
    })
}

// Read the tag only; never open a content stream or resolve the target.
pub fn reparse_tag(path: &[u8]) -> Option<u32> {
    let mut data: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
    let handle = unsafe {
        FindFirstFileExW(
            crate::paths::extended(path).as_ptr(),
            FindExInfoBasic,
            &mut data as *mut _ as _,
            FindExSearchNameMatch,
            std::ptr::null(),
            0,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    unsafe {
        FindClose(handle);
    }
    Some(data.dwReserved0)
}

pub fn list_one(path: &[u8]) -> Option<Listing> {
    let mut wide = crate::paths::extended(path);
    wide.pop();
    if wide.last() != Some(&92) {
        wide.push(92);
    }
    wide.extend([42, 0]);
    let mut data: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
    let mut handle = unsafe {
        FindFirstFileExW(
            wide.as_ptr(),
            FindExInfoBasic,
            &mut data as *mut _ as _,
            FindExSearchNameMatch,
            std::ptr::null(),
            FIND_FIRST_EX_LARGE_FETCH,
        )
    };
    if handle == INVALID_HANDLE_VALUE && unsafe { GetLastError() } == 87 {
        handle = unsafe {
            FindFirstFileExW(
                wide.as_ptr(),
                FindExInfoBasic,
                &mut data as *mut _ as _,
                FindExSearchNameMatch,
                std::ptr::null(),
                0,
            )
        };
    }
    let mut listing = Listing {
        id: 0,
        names: Vec::new(),
        ents: Vec::new(),
    };
    if handle == INVALID_HANDLE_VALUE {
        let error = unsafe { GetLastError() };
        if error == ERROR_FILE_NOT_FOUND {
            listed(path, &listing);
            return Some(listing);
        }
        record_failure(path.to_vec(), error);
        return None;
    }
    loop {
        let len = data
            .cFileName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(data.cFileName.len());
        let name = &data.cFileName[..len];
        if name != [46] && name != [46, 46] {
            let bytes = crate::paths::encode(name);
            listing.ents.push(RawEnt {
                name_off: listing.names.len() as u32,
                name_len: bytes.len() as u16,
                kind: flags_with_tag(data.dwFileAttributes, data.dwReserved0),
                size: ((data.nFileSizeHigh as u64) << 32) | data.nFileSizeLow as u64,
                mtime: timestamp(data.ftLastWriteTime),
                child: NONE,
            });
            listing.names.extend(bytes);
            LISTED.fetch_add(1, Ordering::Relaxed);
        }
        if unsafe { FindNextFileW(handle, &mut data) } == 0 {
            let error = unsafe { GetLastError() };
            unsafe {
                FindClose(handle);
            }
            if error != ERROR_NO_MORE_FILES {
                record_failure(path.to_vec(), error);
                return None;
            }
            listed(path, &listing);
            return Some(listing);
        }
    }
}

pub fn scan(root: &[u8], threads: usize) -> Vec<Listing> {
    scan_observed(root, threads, &|_| {})
}
pub fn scan_observed(
    root: &[u8],
    threads: usize,
    progress: &(dyn Fn(usize) + Sync),
) -> Vec<Listing> {
    let next = AtomicU32::new(1);
    let out = Mutex::new(Vec::new());
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .start_handler(|_| crate::qos::utility())
        .build()
        .expect("walker pool");
    fn visit<'a>(
        scope: &rayon::Scope<'a>,
        path: Vec<u8>,
        id: u32,
        next: &'a AtomicU32,
        out: &'a Mutex<Vec<Listing>>,
        progress: &'a (dyn Fn(usize) + Sync),
    ) {
        let mut listing = list_one(&path).unwrap_or(Listing {
            id,
            names: Vec::new(),
            ents: Vec::new(),
        });
        listing.id = id;
        for entry in &mut listing.ents {
            if entry.kind & 3 == KIND_DIR {
                entry.child = next.fetch_add(1, Ordering::Relaxed);
                let child = crate::live::join(
                    &path,
                    &listing.names[entry.name_off as usize..][..entry.name_len as usize],
                );
                let child_id = entry.child;
                scope.spawn(move |scope| visit(scope, child, child_id, next, out, progress));
            }
        }
        let count = {
            let mut out = out.lock().unwrap();
            out.push(listing);
            out.len()
        };
        if count.is_multiple_of(128) {
            progress(count);
        }
    }
    pool.scope(|scope| visit(scope, root.to_vec(), 0, &next, &out, progress));
    let result = out.into_inner().unwrap();
    progress(result.len());
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vanished_failures_and_empty_recovered_folders_are_not_inaccessible() {
        let temp = tempfile::tempdir().unwrap();
        let missing = crate::paths::internal(&temp.path().join("vanished"));
        DENIED.lock().unwrap().insert(missing.clone(), 5);
        assert!(list_one(&missing).is_none());
        assert!(!DENIED.lock().unwrap().contains_key(&missing));
        // Old helper frames must not reintroduce a missing-path failure.
        record_failure(missing.clone(), ERROR_PATH_NOT_FOUND);
        assert!(!DENIED.lock().unwrap().contains_key(&missing));
        let empty = crate::paths::internal(temp.path());
        DENIED.lock().unwrap().insert(empty.clone(), 5);
        DENIED.lock().unwrap().insert(missing.clone(), 5);
        assert!(list_one(&empty).unwrap().ents.is_empty());
        let denied = DENIED.lock().unwrap();
        assert!(!denied.contains_key(&empty));
        assert!(!denied.contains_key(&missing));
    }
    #[test]
    fn parent_listing_clears_only_removed_child_subtrees() {
        let temp = tempfile::tempdir().unwrap();
        let kept = temp.path().join("kept");
        let removed = temp.path().join("removed");
        std::fs::create_dir_all(removed.join("deep")).unwrap();
        std::fs::create_dir(&kept).unwrap();
        let kept_key = crate::paths::internal(&kept);
        let removed_key = crate::paths::internal(&removed);
        let deep_key = crate::paths::internal(&removed.join("deep"));
        for key in [&kept_key, &removed_key, &deep_key] {
            DENIED.lock().unwrap().insert(key.clone(), 5);
        }
        list_one(&crate::paths::internal(temp.path())).unwrap();
        assert!(DENIED.lock().unwrap().contains_key(&removed_key));
        std::fs::remove_dir_all(removed).unwrap();
        list_one(&crate::paths::internal(temp.path())).unwrap();
        let mut denied = DENIED.lock().unwrap();
        assert!(denied.contains_key(&kept_key));
        assert!(!denied.contains_key(&removed_key));
        assert!(!denied.contains_key(&deep_key));
        denied.remove(&kept_key);
    }
    #[test]
    fn provider_entries_are_searchable_but_redirecting_links_are_not_followed() {
        let attributes = FILE_ATTRIBUTE_REPARSE_POINT
            | FILE_ATTRIBUTE_DIRECTORY
            | FILE_ATTRIBUTE_HIDDEN
            | FILE_ATTRIBUTE_SYSTEM;
        for tag in [0x9000_001a, 0x9000_101a, 0x9000_f01a, 0x9000_001c] {
            assert_eq!(
                flags_with_tag(attributes, tag),
                KIND_DIR | FLAG_HIDDEN | FLAG_SYSTEM
            );
            assert_eq!(
                flags_with_tag(attributes & !FILE_ATTRIBUTE_DIRECTORY, tag),
                FLAG_HIDDEN | FLAG_SYSTEM
            );
        }
        for tag in [0, 0xa000_0003, 0xa000_000c] {
            assert_eq!(
                flags_with_tag(attributes, tag),
                KIND_LINK | FLAG_HIDDEN | FLAG_SYSTEM
            );
        }
    }
}
