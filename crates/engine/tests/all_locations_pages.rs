use seek_engine::{Engine, Options, Query, SourceMode};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
fn truth(path: &Path, out: &mut BTreeSet<PathBuf>) {
    out.insert(path.to_path_buf());
    if path.is_dir() {
        for entry in std::fs::read_dir(path).unwrap() {
            truth(&entry.unwrap().path(), out);
        }
    }
}
fn pages(engine: &Engine, q: &Query) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    let mut offset = 0;
    let mut last = None;
    loop {
        let (total, rows) = engine.all_locations_page(q, offset, 127).unwrap();
        assert!(rows.len() <= 127);
        for row in &rows {
            if let Some(mtime) = last {
                assert!(mtime >= row.mtime);
            }
            last = Some(row.mtime);
            assert!(
                found.insert(row.path.clone()),
                "A row must not repeat across pages"
            );
        }
        offset += rows.len();
        if offset >= total {
            assert_eq!(found.len(), total);
            break;
        }
        assert!(!rows.is_empty());
    }
    found
}
#[test]
fn every_page_matches_directory_truth_including_folders_and_live_changes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    std::fs::create_dir_all(root.join("nested")).unwrap();
    for i in 0..637 {
        std::fs::write(root.join(format!("item-{i:04}.txt")), "owned fixture").unwrap();
    }
    std::fs::write(root.join("nested/Ω.txt"), "owned unicode").unwrap();
    let engine = Engine::start_with_source(
        Options {
            dir: temp.path().join("index"),
            roots: vec![root.clone()],
        },
        SourceMode::Walker,
    )
    .unwrap();
    engine.wait_ready(Duration::from_secs(30)).unwrap();
    let query = Query::parse("", "").unwrap();
    let mut expected = BTreeSet::new();
    truth(&root, &mut expected);
    let started = Instant::now();
    assert_eq!(pages(&engine, &query), expected);
    eprintln!(
        "All locations: {} entries, full paged comparison {:.3} ms (test harness, owned fixture)",
        expected.len(),
        started.elapsed().as_secs_f64() * 1000.
    );
    let mut excluded = query.clone();
    excluded
        .excluded_paths
        .push(seek_engine::paths::internal(&root.join("nested")));
    let filtered = pages(&engine, &excluded);
    assert_eq!(
        filtered,
        expected
            .iter()
            .filter(|p| !p.starts_with(root.join("nested")))
            .cloned()
            .collect()
    );
    assert!(
        engine
            .all_locations_page(&query, usize::MAX, 127)
            .unwrap()
            .1
            .is_empty()
    );
    std::fs::remove_file(root.join("item-0001.txt")).unwrap();
    std::fs::rename(root.join("item-0002.txt"), root.join("moved.txt")).unwrap();
    std::fs::write(root.join("added.txt"), "new overlay").unwrap();
    engine.refresh_parent(&root.join("added.txt"));
    expected.clear();
    truth(&root, &mut expected);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if pages(&engine, &query) == expected {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Live pages did not reach directory truth"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
#[test]
fn visibility_flags_and_cancellation_are_applied_before_paging() {
    use std::os::windows::ffi::OsStrExt;
    use std::sync::{Arc, atomic::AtomicU64};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    std::fs::create_dir(&root).unwrap();
    for (name, flag) in [("visible.txt", 0x80), ("hidden.txt", 2), ("system.txt", 4)] {
        let path = root.join(name);
        std::fs::write(&path, "owned").unwrap();
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        assert_ne!(
            unsafe {
                windows_sys::Win32::Storage::FileSystem::SetFileAttributesW(wide.as_ptr(), flag)
            },
            0
        );
    }
    let engine = Engine::start_with_source(
        Options {
            dir: temp.path().join("index"),
            roots: vec![root.clone()],
        },
        SourceMode::Walker,
    )
    .unwrap();
    engine.wait_ready(Duration::from_secs(30)).unwrap();
    let mut query = Query::parse("", "").unwrap();
    assert_eq!(pages(&engine, &query).len(), 4);
    query.hide_hidden_system = true;
    assert_eq!(
        pages(&engine, &query),
        BTreeSet::from([root.clone(), root.join("visible.txt")])
    );
    query.cancellation = Some((Arc::new(AtomicU64::new(2)), 1));
    assert!(engine.all_locations_page(&query, 0, 500).is_err());
}
