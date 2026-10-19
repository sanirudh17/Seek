//! Small bounded LRU for a derived folder ranking value, not index contents.
use std::collections::HashMap;

const ENTRIES: usize = 512;
const BYTES: usize = 512 * 1024;

#[derive(Default)]
pub(super) struct PriorCache {
    entries: HashMap<Vec<u8>, (i8, u64)>,
    bytes: usize,
    clock: u64,
}

impl PriorCache {
    pub fn get(&mut self, path: &[u8]) -> Option<i8> {
        let entry = self.entries.get_mut(path)?;
        self.clock += 1;
        entry.1 = self.clock;
        Some(entry.0)
    }

    pub fn insert(&mut self, path: Vec<u8>, value: i8) {
        if path.len() > BYTES {
            return;
        }
        self.clock += 1;
        if let Some(entry) = self.entries.get_mut(&path) {
            *entry = (value, self.clock);
            return;
        }
        while self.entries.len() >= ENTRIES || self.bytes + path.len() > BYTES {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, (_, tick))| tick)
                .unwrap()
                .0
                .clone();
            self.entries.remove(&oldest);
            self.bytes -= oldest.len();
        }
        self.bytes += path.len();
        self.entries.insert(path, (value, self.clock));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn churn_keeps_recent_values_and_enforces_both_budgets() {
        let mut cache = PriorCache::default();
        cache.insert(b"/frequent".to_vec(), 7);
        for n in 0..10_000 {
            assert_eq!(cache.get(b"/frequent"), Some(7));
            cache.insert(format!("/folder-{n}").into_bytes(), -3);
            assert!(cache.entries.len() <= ENTRIES);
            assert!(cache.bytes <= BYTES);
        }
        assert_eq!(cache.get(b"/folder-0"), None);
        assert_eq!(cache.get(b"/folder-9999"), Some(-3));
        for n in 0..600 {
            let mut path = vec![b'x'; 2048];
            path[..8].copy_from_slice(&(n as u64).to_le_bytes());
            cache.insert(path, 2);
            assert!(cache.entries.len() <= ENTRIES);
            assert!(cache.bytes <= BYTES);
        }
        cache.insert(vec![b'x'; BYTES + 1], 1);
        assert!(cache.bytes <= BYTES);
    }
}
