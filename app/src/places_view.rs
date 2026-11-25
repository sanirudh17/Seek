use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub(super) struct Capacity {
    pub free: u64,
    pub total: u64,
}
pub(super) struct State {
    key: Option<(Vec<PathBuf>, u64)>,
    pub capacity: BTreeMap<PathBuf, Capacity>,
    pub recent: Vec<PathBuf>,
    task: Task<()>,
}
impl State {
    pub fn new() -> Self {
        Self {
            key: None,
            capacity: BTreeMap::new(),
            recent: vec![],
            task: Task::ready(()),
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
    // Fixed/removable local volumes only. Unavailable/remote roots stay unknown.
    if !matches!(unsafe { GetDriveTypeW(wide.as_ptr()) }, 2 | 3) {
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
    pub(super) fn refresh_places(&mut self, cx: &mut Context<Self>) {
        let key = (self.roots.clone(), self.cache_epoch);
        if self.places.key.as_ref() == Some(&key) {
            return;
        }
        self.places.key = Some(key.clone());
        let data = self.store.directory.clone();
        self.places.task = cx.spawn(async move |this, cx| {
            let roots = key.0.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    (
                        roots
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
