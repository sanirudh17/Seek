#![cfg(windows)]
use seek_engine::{Engine, Options, Query, SourceMode};
use std::{fs, path::Path, time::Duration};
fn open(root: &Path, data: &Path) -> Engine {
    let e = Engine::start_with_source(
        Options {
            dir: data.into(),
            roots: vec![root.into()],
        },
        SourceMode::Walker,
    )
    .unwrap();
    e.wait_ready(Duration::from_secs(30)).unwrap();
    e
}
#[test]
fn largest_totals_follow_closed_changes_hidden_exclusions_and_new_subtrees() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("files");
    let data = t.path().join("index");
    fs::create_dir_all(root.join("A/Nested")).unwrap();
    fs::create_dir_all(root.join("B")).unwrap();
    fs::write(root.join("A/a.txt"), vec![0; 70]).unwrap();
    fs::write(root.join("A/Nested/deep.md"), vec![0; 40]).unwrap();
    fs::write(root.join("B/image.jpeg"), vec![0; 100]).unwrap();
    let e = open(&root, &data);
    let mut q = Query::parse("", "").unwrap();
    q.limit = 200;
    let dirs = e.largest(&q, true);
    assert_eq!(
        dirs.iter().find(|r| r.path == root.join("A")).unwrap().size,
        110
    );
    assert_eq!(e.largest(&q, false)[0].size, 100);
    e.checkpoint().unwrap();
    drop(e);
    fs::rename(root.join("A/a.txt"), root.join("B/renamed.txt")).unwrap();
    fs::write(root.join("A/Nested/deep.md"), vec![0; 150]).unwrap();
    fs::create_dir(root.join("New")).unwrap();
    fs::create_dir(root.join("Empty")).unwrap();
    fs::write(root.join("New/new.rs"), vec![0; 200]).unwrap();
    let e = open(&root, &data);
    let files = e.largest(&q, false);
    assert_eq!(files.len(), 4);
    assert_eq!(files[0].size, 200);
    assert!(!e.snapshot().iter().any(|r| r.path == root.join("A/a.txt")));
    let dirs = e.largest(&q, true);
    assert_eq!(
        dirs.iter()
            .find(|r| r.path == root.join("Empty"))
            .unwrap()
            .size,
        0
    );
    assert_eq!(
        dirs.iter().find(|r| r.path == root.join("B")).unwrap().size,
        170
    );
    q.excluded_paths = vec![seek_engine::paths::internal(&root.join("New"))];
    assert_eq!(e.largest(&q, false)[0].size, 150);
    q.scope = Some(seek_engine::paths::internal(&root.join("B")));
    assert_eq!(e.largest(&q, false).len(), 2);
    for (filter, expected) in [
        ("ext:jpeg", 1),
        ("type:code", 1),
        ("kind:folder", 6),
        ("type:doc", 2),
    ] {
        let mut q = Query::parse(filter, "").unwrap();
        q.limit = 200;
        assert_eq!(e.search(&q).unwrap().len(), expected, "{filter}");
    }
    let hidden =
        seek_engine::paths::extended(&seek_engine::paths::internal(&root.join("B/image.jpeg")));
    unsafe {
        assert_ne!(
            windows_sys::Win32::Storage::FileSystem::SetFileAttributesW(
                hidden.as_ptr(),
                windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN
            ),
            0
        );
    }
    e.refresh_parent(&root.join("B/image.jpeg"));
    e.checkpoint().unwrap();
    let mut q = Query::parse("", "").unwrap();
    q.hide_hidden_system = true;
    assert_eq!(e.largest(&q, false).len(), 3);
}
#[test]
fn largest_retains_exact_top_200_and_reports_existing_large_file_rounding() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("files");
    fs::create_dir(&root).unwrap();
    for i in 0..241 {
        fs::write(root.join(format!("{i}.bin")), vec![0; i + 1]).unwrap();
    }
    let e = open(&root, &t.path().join("index"));
    let mut q = Query::parse("", "").unwrap();
    q.limit = 200;
    let rows = e.largest(&q, false);
    assert_eq!(rows.len(), 200);
    assert_eq!(rows.first().unwrap().size, 241);
    assert_eq!(rows.last().unwrap().size, 42);
    let size = (1u64 << 32) + 1234567;
    let rounded = seek_engine::index::dec_size(seek_engine::index::enc_size(size));
    assert!(size >= rounded && size - rounded < (1 << 21));
}
