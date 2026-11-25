//! Its own test binary: the folder weights are process-wide, so this test
//! must not share a process with tests that build indexes in parallel.
use seek_engine::{Engine, Options, Query};
use std::time::Duration;
fn start_walker(options: Options) -> Result<Engine, String> {
    Engine::start_with_source(options, seek_engine::SourceMode::Walker)
}

#[test]
fn ranking_file_reweights_a_saved_index_without_a_rescan() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    std::fs::create_dir_all(root.join("alpha")).unwrap();
    std::fs::create_dir_all(root.join("beta")).unwrap();
    std::fs::write(root.join("alpha/ledger.txt"), "a").unwrap();
    std::fs::write(root.join("beta/ledger.txt"), "b").unwrap();
    let data = temp.path().join("index");
    let options = || Options {
        dir: data.clone(),
        roots: vec![root.clone()],
    };
    let first = |engine: &Engine| {
        let rows = engine.search(&Query::parse("ledger", "").unwrap()).unwrap();
        rows[0]
            .path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    };
    for (favoured, other) in [("alpha", "beta"), ("beta", "alpha")] {
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(
            data.join("ranking.json"),
            format!(r#"[{{"name":"{favoured}","weight":40}},{{"name":"{other}","weight":-40}}]"#),
        )
        .unwrap();
        let engine = start_walker(options()).unwrap();
        engine.wait_ready(Duration::from_secs(30)).unwrap();
        assert_eq!(first(&engine), favoured);
        drop(engine);
    }
    // An unreadable file means the defaults, under which neither folder
    // name counts: the index is reweighted again and still finds both.
    std::fs::write(data.join("ranking.json"), "not json").unwrap();
    let engine = start_walker(options()).unwrap();
    engine.wait_ready(Duration::from_secs(30)).unwrap();
    assert_eq!(
        seek_engine::index::weights(),
        seek_engine::index::default_weights()
    );
    assert_eq!(
        engine
            .search(&Query::parse("ledger", "").unwrap())
            .unwrap()
            .len(),
        2
    );
}
