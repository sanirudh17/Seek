use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub(super) struct Capacity {
    pub free: u64,
    pub total: u64,
}
pub(super) struct State {
    key: Option<(Vec<PathBuf>, u64)>,
    last_refresh: Option<std::time::Instant>,
    capacity_gen: u64,
    pub capacity: BTreeMap<PathBuf, Capacity>,
    pub recent: Vec<PathBuf>,
    task: Task<()>,
    capacity_task: Task<()>,
}
impl State {
    pub fn new() -> Self {
        Self {
            key: None,
            last_refresh: None,
            capacity_gen: 0,
            capacity: BTreeMap::new(),
            recent: vec![],
            task: Task::ready(()),
            capacity_task: Task::ready(()),
        }
    }
}
fn capacity(path: &std::path::Path) -> Option<Capacity> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW};
    let root = path.ancestors().last()?;
    let wide = root
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // Fixed, removable, and network volumes (matching Storage's drive picker).
    if !matches!(unsafe { GetDriveTypeW(wide.as_ptr()) }, 2..=4) {
        return None;
    }
    let mut free = 0;
    let mut total = 0;
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    if unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) }
        == 0
        || total == 0
    {
        return None;
    }
    Some(Capacity {
        free: free.min(total),
        total,
    })
}
fn recent(data: &std::path::Path) -> Vec<PathBuf> {
    use std::io::Read;
    let mut bytes = vec![];
    let Ok(file) = std::fs::File::open(data.join("opened-dates.json")) else {
        return vec![];
    };
    if file.take(1024 * 1024 + 1).read_to_end(&mut bytes).is_err() || bytes.len() > 1024 * 1024 {
        return vec![];
    }
    let Ok(dates) = serde_json::from_slice::<BTreeMap<PathBuf, u64>>(&bytes) else {
        return vec![];
    };
    let mut rows = dates.into_iter().collect::<Vec<_>>();
    rows.sort_by(|(a, x), (b, y)| y.cmp(x).then(a.cmp(b)));
    rows.into_iter()
        .filter_map(|(path, _)| {
            use std::os::windows::fs::MetadataExt;
            if !path.is_absolute() {
                return None;
            }
            let meta = std::fs::symlink_metadata(&path).ok()?;
            (meta.file_attributes() & (0x400 | 0x1000 | 0x40000 | 0x400000) == 0).then_some(path)
        })
        .take(16)
        .collect()
}
impl Seek {
    fn capacity_targets(&self) -> Vec<PathBuf> {
        let mut paths = self.roots.clone();
        if let Some(page) = &self.storage_page
            && !paths
                .iter()
                .any(|p| p.as_os_str().eq_ignore_ascii_case(page.drive.as_os_str()))
        {
            paths.push(page.drive.clone());
        }
        paths
    }

    /// Immediately refreshes OS drive capacity (`GetDiskFreeSpaceExW`) for all
    /// roots and the active Storage drive (e.g. after cleanup or Recycle Bin empty).
    pub(super) fn refresh_capacity(&mut self, cx: &mut Context<Self>) {
        self.places.capacity_gen += 1;
        let generation = self.places.capacity_gen;
        let targets = self.capacity_targets();
        self.places.capacity_task = cx.spawn(async move |this, cx| {
            let result: BTreeMap<PathBuf, Capacity> = cx
                .background_executor()
                .spawn(async move {
                    targets
                        .into_iter()
                        .filter_map(|path| capacity(&path).map(|value| (path, value)))
                        .collect()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.places.capacity_gen == generation {
                    this.places.capacity = result;
                    cx.notify();
                }
            });
        });
    }

    pub(super) fn refresh_places(&mut self, cx: &mut Context<Self>) {
        let targets = self.capacity_targets();
        let key = (targets.clone(), self.cache_epoch);
        let same_targets = self
            .places
            .key
            .as_ref()
            .is_some_and(|(old, _)| *old == targets);
        if self.places.key.as_ref() == Some(&key)
            || (same_targets
                && self
                    .places
                    .last_refresh
                    .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(5)))
        {
            return;
        }
        self.places.key = Some(key.clone());
        self.places.last_refresh = Some(std::time::Instant::now());
        let data = self.store.directory.clone();
        self.places.task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    (
                        targets
                            .into_iter()
                            .filter_map(|path| capacity(&path).map(|value| (path, value)))
                            .collect(),
                        recent(&data),
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.places.key.as_ref() == Some(&key) {
                    (this.places.capacity, this.places.recent) = result;
                    cx.notify();
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{capacity, recent};
    use std::collections::BTreeMap;
    #[test]
    fn recent_evidence_is_sorted_and_missing_paths_are_omitted() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a.txt");
        let b = temp.path().join("b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let dates = BTreeMap::from([
            (a.clone(), 1u64),
            (b.clone(), 2),
            (temp.path().join("missing"), 3),
        ]);
        std::fs::write(
            temp.path().join("opened-dates.json"),
            serde_json::to_vec(&dates).unwrap(),
        )
        .unwrap();
        assert_eq!(recent(temp.path()), vec![b, a]);
        let disk = capacity(temp.path()).unwrap();
        assert!(disk.total > 0 && disk.free <= disk.total);
    }
}
