use seek_engine::{Engine, Options, Query};
use std::{path::PathBuf, time::Duration};
fn start_walker(options: Options) -> Result<Engine, String> {
    Engine::start_with_source(options, seek_engine::SourceMode::Walker)
}

#[test]
fn literal_name_keeps_exact_and_prefix_results_ahead_of_recent_hashes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    std::fs::create_dir_all(root.join("t3-parent")).unwrap();
    for name in [
        "t3",
        "t3-notes.md",
        "random-hash-t3-build.o",
        "unrelated.md",
        "a+b.txt",
        "cost$.txt",
    ] {
        std::fs::write(root.join(name), "owned").unwrap();
    }
    std::fs::write(root.join("t3-parent/other.txt"), "parent must not match").unwrap();
    let engine = start_walker(Options {
        dir: temp.path().join("index"),
        roots: vec![root],
    })
    .unwrap();
    engine.wait_ready(Duration::from_secs(30)).unwrap();
    let rows = engine
        .search(&Query::parse("name:\"t3\"", "").unwrap())
        .unwrap();
    assert_eq!(rows[0].path.file_name().unwrap(), "t3");
    assert_eq!(rows[1].path.file_name().unwrap(), "t3-notes.md");
    assert!(
        rows.iter()
            .all(|r| r.path.file_name().unwrap().to_string_lossy().contains("t3"))
    );
    for literal in ["a+b", "cost$"] {
        let rows = engine
            .search(&Query::parse(&format!("name:\"{literal}\""), "").unwrap())
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(
            rows[0]
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains(literal)
        );
    }
}

#[test]
fn chip_ranges_preserve_quoted_paths_and_unicode_source_text() {
    let text = "Résumé in:\"C:\\Test Folder\" ext:txt ";
    let words = seek_engine::query::query_words(text);
    assert_eq!(words.len(), 3);
    assert_eq!(words[0].1, "Résumé");
    assert_eq!(words[1].1, "in:C:\\Test Folder");
    assert_eq!(&text[words[1].0.clone()], "in:\"C:\\Test Folder\"");
    let q = Query::parse(text, "").unwrap();
    assert_eq!(q.scope.as_deref(), Some(b"/C:/Test Folder".as_slice()));
    assert_eq!(q.exts, [b"txt".to_vec()]);
}

#[test]
fn filename_modes_filters_and_content_rejection() {
    let q = Query::parse(
        "'invoice ^bud .pdf$ !obsolete ext:pdf kind:file size:>1k limit:5",
        "",
    )
    .unwrap();
    assert_eq!(q.tokens.len(), 4);
    assert_eq!(q.limit, 5);
    assert_eq!(q.exts, [b"pdf".to_vec()]);
    assert_eq!(q.kind, Some(0));
    assert!(q.size.0 >= 1000);
    for unsupported in [
        "grep:secret",
        "regex:secret",
        "sym:secret",
        "content:secret",
    ] {
        assert!(Query::parse(unsupported, "").is_err());
    }
    assert!(Query::parse("kind:unknown", "").is_err());
    assert!(Query::parse("limit:many", "").is_err());
}

#[test]
fn settings_exclusions_apply_before_top_k_without_hiding_prefix_siblings() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    let denied = root.join("noise");
    let allowed = root.join("noise-allowed");
    std::fs::create_dir_all(&denied).unwrap();
    std::fs::create_dir_all(&allowed).unwrap();
    for n in 0..250 {
        std::fs::write(denied.join(format!("package-{n:03}.json")), "x").unwrap();
    }
    let target = allowed.join("package.json");
    std::fs::write(&target, "x").unwrap();
    let engine = start_walker(Options {
        dir: temp.path().join("index"),
        roots: vec![root],
    })
    .unwrap();
    engine.wait_ready(Duration::from_secs(30)).unwrap();
    let mut q = Query::parse("package limit:1", "").unwrap();
    q.excluded_paths = vec![
        seek_engine::paths::internal(&denied)
            .iter()
            .map(u8::to_ascii_uppercase)
            .collect(),
    ];
    assert_eq!(
        engine
            .search(&q)
            .unwrap()
            .iter()
            .map(|h| &h.path)
            .collect::<Vec<_>>(),
        [&target]
    );
    let live = allowed.join("package-live.json");
    std::fs::write(&live, "xx").unwrap();
    engine.refresh_parent(&live);
    engine.checkpoint().unwrap();
    q.limit = 200;
    let hits = engine.search(&q).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().any(|h| h.path == live));
    let q = Query {
        excluded_paths: vec![seek_engine::paths::internal(&denied)],
        ..Default::default()
    };
    assert!(
        engine
            .search(&q)
            .unwrap()
            .iter()
            .all(|h| !h.path.starts_with(&denied))
    );
}

#[test]
fn unicode_case_typos_exclusions_and_size_filters() {
    let q = Query::parse("invoce ext:pdf size:>10", "").unwrap();
    assert!(
        q.match_path(b"/C:/Downloads/invoice-budget.pdf", 0, 100, 0)
            .is_some()
    );
    assert!(
        q.match_path(b"/C:/Downloads/invoice-budget.txt", 0, 100, 0)
            .is_none()
    );
    assert!(
        q.match_path(b"/C:/Downloads/invoice-budget.pdf", 0, 1, 0)
            .is_none()
    );
    let q = Query::parse("'RÉSUMÉ", "").unwrap();
    assert!(
        q.match_path("/C:/Downloads/Résumé-🚀.txt".as_bytes(), 0, 100, 0)
            .is_some()
    );
    let q = Query::parse("invoice !obsolete", "").unwrap();
    assert!(
        q.match_path(b"/C:/Downloads/obsolete-invoice.pdf", 0, 100, 0)
            .is_none()
    );
}

#[test]
fn typos_recover_embedded_filename_words_in_base_and_live_overlay() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    std::fs::create_dir(&root).unwrap();
    let names = [
        "CS23532_CN_Labmanual_2026-27.pdf",
        "course-labmanual-final.pdf",
        "course labmanual final.pdf",
    ];
    for name in names {
        std::fs::write(root.join(name), b"x").unwrap();
    }
    let options = Options {
        dir: temp.path().join("index"),
        roots: vec![root.clone()],
    };
    let engine = start_walker(options.clone()).unwrap();
    engine.wait_ready(Duration::from_secs(30)).unwrap();
    for query in ["lambanual", "labmanal", "labmanuxal"] {
        let hits = engine.search(&Query::parse(query, "").unwrap()).unwrap();
        for name in names {
            assert!(
                hits.iter().take(5).any(|h| h.path == root.join(name)),
                "{query}: {name}"
            );
        }
    }
    let live = root.join("new-labmanual-live.pdf");
    std::fs::write(&live, b"x").unwrap();
    // Explicit reconciliation synchronizes the test without a polling loop.
    engine.refresh_parent(&live);
    engine.checkpoint().unwrap();
    assert!(
        engine
            .search(&Query::parse("lambanual", "").unwrap())
            .unwrap()
            .iter()
            .take(5)
            .any(|h| h.path == live)
    );
    drop(engine);
    let engine = start_walker(options).unwrap();
    engine.wait_ready(Duration::from_secs(30)).unwrap();
    assert!(
        engine
            .search(&Query::parse("lambanual", "").unwrap())
            .unwrap()
            .iter()
            .take(5)
            .any(|h| h.path == live)
    );
    // Wrong digits must not become typo matches for another numbered file.
    assert!(
        Query::parse("hat_98", "")
            .unwrap()
            .match_path(b"/C:/notes_hat_18.txt", 0, 1, 0)
            .is_none()
    );
}

#[test]
fn recent_top200_uses_newest_timestamp_before_truncating() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    std::fs::create_dir(&root).unwrap();
    for n in 0..300 {
        std::fs::write(root.join(format!("old-{n:03}.txt")), b"x").unwrap();
    }
    let newest = root.join("z-newest.txt");
    std::fs::write(&newest, b"new").unwrap();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&newest)
        .unwrap();
    file.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_900_000_000))
        .unwrap();
    let engine = start_walker(Options {
        dir: temp.path().join("index"),
        roots: vec![root],
    })
    .unwrap();
    engine.wait_ready(Duration::from_secs(30)).unwrap();
    let mut q = Query::parse("", "").unwrap();
    q.limit = 200;
    let hits = engine.search(&q).unwrap();
    assert_eq!(hits.len(), 200);
    assert_eq!(hits[0].path.file_name().unwrap(), "z-newest.txt");
    assert!(hits.windows(2).all(|pair| pair[0].mtime >= pair[1].mtime));
}

#[test]
fn retained_recent_hits_refresh_metadata_and_remove_deleted_entries() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    std::fs::create_dir(&root).unwrap();
    for name in ["a.txt", "b.txt"] {
        std::fs::write(root.join(name), b"x").unwrap();
    }
    let engine = start_walker(Options {
        dir: temp.path().join("index"),
        roots: vec![root.clone()],
    })
    .unwrap();
    engine.wait_ready(Duration::from_secs(30)).unwrap();
    let q = Query::parse("ext:txt", "").unwrap();
    let mut previous = engine.search(&q).unwrap();
    previous.sort_by(|a, b| b.path.cmp(&a.path));
    std::fs::write(root.join("b.txt"), b"changed size").unwrap();
    std::fs::remove_file(root.join("a.txt")).unwrap();
    std::fs::write(root.join("new.txt"), b"new").unwrap();
    let until = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let current = engine.refresh_results(&previous, &q);
        if current.len() == 1
            && current[0].path.file_name().unwrap() == "b.txt"
            && current[0].size == 12
            && engine
                .search(&q)
                .unwrap()
                .iter()
                .any(|h| h.path.file_name().unwrap() == "new.txt")
        {
            break;
        }
        assert!(
            std::time::Instant::now() < until,
            "retained hits kept stale/deleted metadata"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        engine
            .search(&q)
            .unwrap()
            .iter()
            .any(|h| h.path.file_name().unwrap() == "new.txt")
    );
}

#[test]
fn exact_and_prefix_matches_survive_location_and_open_boosts() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    std::fs::create_dir_all(root.join("Downloads")).unwrap();
    for name in ["invoice", "invoice-budget.pdf", "x-invoice.pdf"] {
        std::fs::write(
            root.join(if name == "x-invoice.pdf" {
                PathBuf::from("Downloads").join(name)
            } else {
                PathBuf::from(name)
            }),
            b"x",
        )
        .unwrap();
    }
    let engine = start_walker(Options {
        dir: temp.path().join("index"),
        roots: vec![root.clone()],
    })
    .unwrap();
    engine.wait_ready(Duration::from_secs(30)).unwrap();
    for _ in 0..20 {
        engine.record_open(&root.join("Downloads/x-invoice.pdf"));
    }
    engine.checkpoint().unwrap();
    let hits = engine
        .search(&Query::parse("invoice", "").unwrap())
        .unwrap();
    assert_eq!(hits[0].path.file_name().unwrap(), "invoice");
    assert_eq!(hits[1].path.file_name().unwrap(), "invoice-budget.pdf");
}

#[test]
fn view_filter_does_not_remove_hidden_entries_and_cancellation_returns_no_hits() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("hidden-report.txt");
    std::fs::write(&path, b"x").unwrap();
    let encoded = seek_engine::paths::internal(&path);
    assert_ne!(
        unsafe {
            windows_sys::Win32::Storage::FileSystem::SetFileAttributesW(
                seek_engine::paths::extended(&encoded).as_ptr(),
                2,
            )
        },
        0
    );
    let engine = start_walker(Options {
        dir: temp.path().join("index"),
        roots: vec![root],
    })
    .unwrap();
    engine.wait_ready(Duration::from_secs(30)).unwrap();
    let mut q = Query::parse("hidden-report", "").unwrap();
    assert_eq!(engine.search(&q).unwrap().len(), 1);
    q.hide_hidden_system = true;
    assert!(engine.search(&q).unwrap().is_empty());
    assert!(engine.snapshot().iter().any(|file| file.path == path));
    q.hide_hidden_system = false;
    q.cancellation = Some((std::sync::Arc::new(std::sync::atomic::AtomicU64::new(2)), 1));
    assert!(engine.search(&q).unwrap().is_empty());
}
