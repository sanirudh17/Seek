//! Collapse duplicate filesystem notifications before doing directory I/O.
//! Non-filesystem messages remain ordering barriers, especially checkpoints.
use crate::watch::Change;
use std::{collections::BTreeMap, sync::mpsc};

pub(super) struct Batch {
    pub paths: Vec<(Vec<u8>, bool)>,
    pub pending: Option<Change>,
}

pub(super) fn collect(first: Change, rx: &mpsc::Receiver<Change>) -> Batch {
    let mut paths = BTreeMap::<Vec<u8>, bool>::new();
    let mut next = Some(first);
    let mut pending = None;
    for _ in 0..256 {
        match next.take().expect("filesystem batch") {
            Change::Directories(dirs) => {
                for path in dirs {
                    paths.entry(path).or_insert(false);
                }
            }
            Change::Overflow(root) => {
                paths.insert(root, true);
            }
            barrier => {
                pending = Some(barrier);
                break;
            }
        }
        if paths.len() >= 1024 {
            break;
        }
        match rx.try_recv() {
            Ok(change) => next = Some(change),
            Err(_) => break,
        }
    }
    // The loop can have received a barrier or another event at its budget.
    // Keep it for the next turn; never discard a dequeued notification.
    if pending.is_none() {
        pending = next;
    }
    let recursive: Vec<_> = paths
        .iter()
        .filter(|(_, recursive)| **recursive)
        .map(|(p, _)| p.clone())
        .collect();
    let paths = paths
        .into_iter()
        .filter(|(path, _)| {
            !recursive.iter().any(|root| {
                path.len() > root.len()
                    && crate::paths::cmp_ci(&path[..root.len()], root).is_eq()
                    && path.get(root.len()) == Some(&b'/')
            })
        })
        .collect();
    Batch { paths, pending }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_directories_and_overflow_subtrees_are_listed_once() {
        let (tx, rx) = mpsc::channel();
        tx.send(Change::Directories(vec![
            b"/C:/files/a".to_vec(),
            b"/C:/files-other".to_vec(),
        ]))
        .unwrap();
        tx.send(Change::Overflow(b"/C:/files".to_vec())).unwrap();
        tx.send(Change::Directories(vec![b"/C:/files/a/b".to_vec()]))
            .unwrap();
        let batch = collect(Change::Directories(vec![b"/C:/files/a".to_vec()]), &rx);
        assert_eq!(
            batch.paths,
            vec![
                (b"/C:/files".to_vec(), true),
                (b"/C:/files-other".to_vec(), false)
            ]
        );
        assert!(batch.pending.is_none());
    }

    #[test]
    fn checkpoint_is_an_ordering_barrier_and_later_changes_remain_queued() {
        let (tx, rx) = mpsc::channel();
        let (saved, _) = mpsc::channel();
        tx.send(Change::Checkpoint(saved)).unwrap();
        tx.send(Change::Directories(vec![b"/later".to_vec()]))
            .unwrap();
        let batch = collect(Change::Directories(vec![b"/first".to_vec()]), &rx);
        assert!(matches!(batch.pending, Some(Change::Checkpoint(_))));
        assert!(
            matches!(rx.try_recv().unwrap(), Change::Directories(v) if v == vec![b"/later".to_vec()])
        );
    }
    #[test]
    fn reconciliation_boundary_is_not_swallowed_by_later_drive_activity() {
        let (tx, rx) = mpsc::channel();
        tx.send(Change::Reconciled(7)).unwrap();
        for _ in 0..600 {
            tx.send(Change::Directories(vec![b"/busy".to_vec()]))
                .unwrap();
        }
        let batch = collect(Change::Directories(vec![b"/earlier".to_vec()]), &rx);
        assert!(matches!(batch.pending, Some(Change::Reconciled(7))));
        assert!(matches!(rx.try_recv().unwrap(), Change::Directories(_)));
    }

    #[test]
    fn bounded_batches_keep_every_notification() {
        let (tx, rx) = mpsc::channel();
        for n in 0..600 {
            tx.send(Change::Directories(vec![format!("/{n}").into_bytes()]))
                .unwrap();
        }
        let mut next = rx.recv().unwrap();
        let mut count = 0;
        loop {
            let batch = collect(next, &rx);
            assert!(batch.paths.len() <= 256);
            count += batch.paths.len();
            match batch.pending.or_else(|| rx.try_recv().ok()) {
                Some(change) => next = change,
                None => break,
            }
        }
        assert_eq!(count, 600);
    }
}
