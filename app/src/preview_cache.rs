//! Small volatile cache of already-selected previews. No disk text cache,
//! native document handles, timers, or file watchers are retained here.
use crate::document_preview::{Key, Preview};
use std::{collections::VecDeque, time::SystemTime};
#[derive(Clone, PartialEq, Eq)]
pub struct Stamp {
    size: u64,
    modified: SystemTime,
}
impl Stamp {
    pub fn read(key: &Key) -> Option<Self> {
        let m = std::fs::symlink_metadata(&key.file.path).ok()?;
        if !m.is_file()
            || m.len() > crate::document_preview::MAX_FILE
            || crate::content::unavailable(&key.file.path, &m)
        {
            return None;
        }
        Some(Self {
            size: m.len(),
            modified: m.modified().ok()?,
        })
    }
}
struct Entry {
    key: Key,
    stamp: Stamp,
    value: Preview,
    bytes: usize,
}
pub struct Cache {
    entries: VecDeque<Entry>,
    bytes: usize,
    limit: usize,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            limit: 24 * 1024 * 1024,
        }
    }
}
impl Cache {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
    pub fn get(&mut self, key: &Key) -> Option<Preview> {
        let stamp = Stamp::read(key);
        self.entries
            .retain(|e| e.key.file.path != key.file.path || Some(&e.stamp) == stamp.as_ref());
        self.bytes = self.entries.iter().map(|e| e.bytes).sum();
        let i = self
            .entries
            .iter()
            .position(|e| &e.key == key && Some(&e.stamp) == stamp.as_ref())?;
        let entry = self.entries.remove(i)?;
        let value = entry.value.clone();
        self.entries.push_back(entry);
        Some(value)
    }
    pub fn put(&mut self, key: Key, stamp: Option<Stamp>, value: &Preview) {
        let Some(stamp) = stamp else {
            return;
        };
        if Stamp::read(&key).as_ref() != Some(&stamp) {
            return;
        }
        let bytes = value.image.as_ref().map_or(0, |_| {
            value.info.width as usize * value.info.height as usize * 4
        }) + value.info.text.as_ref().map_or(0, String::len)
            + value.info.sheet.as_ref().map_or(0, |s| {
                s.rows.iter().flatten().map(String::len).sum::<usize>()
            });
        if bytes > self.limit {
            return;
        }
        self.entries.retain(|e| e.key != key);
        self.bytes = self.entries.iter().map(|e| e.bytes).sum();
        while self.bytes + bytes > self.limit || self.entries.len() >= 4 {
            let Some(old) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= old.bytes;
        }
        self.entries.push_back(Entry {
            key,
            stamp,
            value: value.clone(),
            bytes,
        });
        self.bytes += bytes;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{document_preview::Wire, media};
    fn key(p: &std::path::Path, page: u32) -> Key {
        Key {
            file: media::Key {
                path: p.to_path_buf(),
                mtime: 0,
                size: 3,
                edge: 768,
                details: false,
                folder: false,
            },
            page,
        }
    }
    fn text(value: &str) -> Preview {
        Preview {
            info: Wire {
                text: Some(value.into()),
                ..Default::default()
            },
            image: None,
        }
    }
    #[test]
    fn precise_file_change_delete_and_lru_never_return_stale_text() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owned.docx");
        std::fs::write(&path, "old").unwrap();
        let k = key(&path, 0);
        let mut cache = Cache {
            limit: 8,
            ..Default::default()
        };
        cache.put(k.clone(), Stamp::read(&k), &text("old"));
        assert_eq!(cache.get(&k).unwrap().info.text.as_deref(), Some("old"));
        std::thread::sleep(std::time::Duration::from_millis(2));
        std::fs::write(&path, "new").unwrap();
        assert!(
            cache.get(&k).is_none(),
            "The coarse indexed mtime and size did not change; precise stamp must still invalidate"
        );
        for i in 0..3 {
            let k = key(&path, i);
            cache.put(k.clone(), Stamp::read(&k), &text("new"));
        }
        assert!(cache.bytes <= 8);
        assert!(cache.get(&key(&path, 0)).is_none());
        assert!(cache.get(&key(&path, 2)).is_some());
        std::fs::remove_file(path).unwrap();
        assert!(cache.get(&k).is_none());
        assert!(cache.entries.is_empty());
        cache.clear();
        assert_eq!(cache.bytes, 0);
    }
}
