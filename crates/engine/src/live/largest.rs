use super::*;
use crate::walk::FLAG_SYNTH;
use crate::{engine::Found, query::Query};
use std::{cmp::Reverse, collections::BinaryHeap};

impl Live {
    /// One pass over indexed file sizes, then one reverse pass over directory
    /// totals. Keeps only top-k candidates; never lists a directory or opens files.
    pub(crate) fn largest(&self, q: &Query, folders: bool) -> Vec<Found> {
        let mut allowed = vec![true; self.base.d];
        let mut totals = vec![0u64; self.base.d];
        let mut path = Vec::new();
        for d in 1..self.base.d {
            let i = self.base.dir_entry()[d] as usize;
            let parent = self.base.dir_parent()[d] as usize;
            let kind = self.attributes(i).0;
            allowed[d] = allowed[parent]
                && !self.is_dead(i as u32)
                && (!q.hide_hidden_system || kind & (walk::FLAG_HIDDEN | walk::FLAG_SYSTEM) == 0);
            if allowed[d] && !q.excluded_paths.is_empty() {
                self.base.path(i, &mut path);
                allowed[d] = !q.excluded_paths.iter().any(|p| below(p, &path));
            }
        }
        let mut best = BinaryHeap::<Reverse<(u64, u8, usize)>>::new();
        let mut extras = Vec::new();
        let mut overlay_totals = BTreeMap::<Vec<u8>, u64>::new();
        let limit = q.limit.min(200);
        let offer = |best: &mut BinaryHeap<Reverse<(u64, u8, usize)>>, size, source, index| {
            if limit == 0 {
                return;
            }
            best.push(Reverse((size, source, index)));
            if best.len() > limit {
                best.pop();
            }
        };
        let scope_dir = q
            .scope
            .as_ref()
            .and_then(|p| self.base.lookup(p))
            .and_then(|e| self.base.dir_of(e));
        let file_range = match (q.scope.as_ref(), scope_dir) {
            (Some(_), Some(d)) => {
                self.base.dir_start()[d as usize] as usize..self.base.dir_end()[d as usize] as usize
            }
            (Some(_), None) => 0..0,
            (None, _) => 1..self.base.n,
        };
        for i in file_range {
            let (kind, size, _) = self.attributes(i);
            if kind & 3 != walk::KIND_FILE
                || self.is_dead(i as u32)
                || !allowed[self.base.parent()[i] as usize]
                || !q.kind_ok(kind)
            {
                continue;
            }
            if !q.excluded_paths.is_empty() {
                self.base.path(i, &mut path);
                if q.excluded_paths.iter().any(|p| below(p, &path)) {
                    continue;
                }
                if q.scope.as_ref().is_some_and(|s| !below(s, &path)) {
                    continue;
                }
            }
            totals[self.base.parent()[i] as usize] =
                totals[self.base.parent()[i] as usize].saturating_add(size);
            if !folders {
                offer(&mut best, size, 0, i);
            }
        }
        for (p, e) in &self.over {
            if folders
                && e.kind & 3 == walk::KIND_DIR
                && !q.excluded_paths.iter().any(|r| below(r, p))
                && q.scope
                    .as_ref()
                    .is_none_or(|r| below(r, p) && crate::paths::cmp_ci(r, p).is_ne())
                && self.largest_lineage(p, &allowed, q).is_some()
            {
                overlay_totals.entry(p.clone()).or_default();
            }
            if e.kind & 3 != walk::KIND_FILE
                || !q.kind_ok(e.kind)
                || q.excluded_paths.iter().any(|r| below(r, p))
                || q.scope.as_ref().is_some_and(|r| !below(r, p))
            {
                continue;
            }
            let Some((base, lineage)) = self.largest_lineage(parent_path(p), &allowed, q) else {
                continue;
            };
            if let Some(d) = base {
                totals[d] = totals[d].saturating_add(e.size);
            }
            for d in lineage {
                let v = overlay_totals.entry(d).or_default();
                *v = v.saturating_add(e.size);
            }
            if !folders {
                let i = extras.len();
                extras.push((p, e));
                offer(&mut best, e.size, 1, i);
            }
        }
        if folders {
            for d in (1..self.base.d).rev() {
                let p = self.base.dir_parent()[d] as usize;
                totals[p] = totals[p].saturating_add(totals[d]);
            }
            for d in 1..self.base.d {
                let i = self.base.dir_entry()[d] as usize;
                if !allowed[d] || self.base.kind()[i] & FLAG_SYNTH != 0 {
                    continue;
                }
                self.base.path(i, &mut path);
                if q.scope
                    .as_ref()
                    .is_none_or(|s| below(s, &path) && crate::paths::cmp_ci(s, &path).is_ne())
                {
                    offer(&mut best, totals[d], 2, i);
                }
            }
        }
        let mut out = Vec::with_capacity(limit);
        for Reverse((size, source, i)) in best {
            let (p, kind, mtime) = if source == 1 {
                let (p, e) = extras[i];
                (p.clone(), e.kind, e.mtime)
            } else {
                self.base.path(i, &mut path);
                (path.clone(), self.attributes(i).0, self.attributes(i).2)
            };
            out.push(Found {
                path: crate::paths::native(&p),
                kind,
                size,
                mtime,
                score: 0,
            });
        }
        if folders {
            for (p, size) in overlay_totals {
                let e = self.over[&p];
                out.push(Found {
                    path: crate::paths::native(&p),
                    kind: e.kind,
                    size,
                    mtime: e.mtime,
                    score: 0,
                });
            }
        }
        out.sort_unstable_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
        out.truncate(limit);
        out
    }
    fn largest_lineage(
        &self,
        path: &[u8],
        allowed: &[bool],
        q: &Query,
    ) -> Option<(Option<usize>, Vec<Vec<u8>>)> {
        let mut path = path.to_vec();
        let mut lineage = Vec::new();
        loop {
            if let Some(d) = self.base_alive(&path).and_then(|i| self.base.dir_of(i)) {
                return allowed[d as usize].then_some((Some(d as usize), lineage));
            }
            if let Some(o) = self.over.get(&path) {
                if q.hide_hidden_system && o.kind & (walk::FLAG_HIDDEN | walk::FLAG_SYSTEM) != 0 {
                    return None;
                }
                lineage.push(path.clone());
            }
            if path.is_empty() || path == b"/" {
                return Some((None, lineage));
            }
            path = parent_path(&path).to_vec();
        }
    }
}
fn below(root: &[u8], p: &[u8]) -> bool {
    p.len() >= root.len()
        && crate::paths::cmp_ci(&p[..root.len()], root).is_eq()
        && (p.len() == root.len() || p.get(root.len()) == Some(&b'/'))
}
fn parent_path(p: &[u8]) -> &[u8] {
    &p[..p.iter().rposition(|b| *b == b'/').unwrap_or(0)]
}
