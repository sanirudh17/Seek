use seek_engine::{Engine, Options, Query};
mod stress;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn option(args: &[String], key: &str) -> Option<String> {
    args.windows(2).find(|a| a[0] == key).map(|a| a[1].clone())
}
fn source_mode(args: &[String]) -> Result<seek_engine::SourceMode, String> {
    match option(args, "--source").as_deref() {
        None | Some("auto") => Ok(seek_engine::SourceMode::Auto),
        Some("walker") => Ok(seek_engine::SourceMode::Walker),
        Some("helper") => Ok(seek_engine::SourceMode::Helper),
        _ => Err("--source must be auto, walker or helper".into()),
    }
}
fn open_source(
    root: &Path,
    data: &Path,
    source: seek_engine::SourceMode,
) -> Result<Engine, String> {
    let root = std::fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    Engine::start_with_source(
        Options {
            dir: data.into(),
            roots: vec![root],
        },
        source,
    )
}
fn ready(engine: &Engine) -> Result<seek_engine::Status, String> {
    if std::env::args().any(|a| a == "--progress") {
        let started = Instant::now();
        loop {
            match engine.wait_ready(Duration::from_secs(1)) {
                Ok(status) => return Ok(status),
                Err(error) => {
                    let status = engine.status();
                    eprintln!(
                        "{}",
                        serde_json::json!({"elapsed_ms":started.elapsed().as_secs_f64()*1000.,"startup_progress":status})
                    );
                    if status.error.is_some() || started.elapsed() >= Duration::from_secs(3600) {
                        return Err(error);
                    }
                }
            }
        }
    }
    engine.wait_ready(Duration::from_secs(3600))
}
fn output(value: &impl serde::Serialize, path: Option<&str>) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    if let Some(path) = path {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, &json).map_err(|e| e.to_string())?;
    }
    println!("{json}");
    Ok(())
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("help");
    let root = PathBuf::from(option(&args, "--root").unwrap_or_else(|| ".".into()));
    let data = PathBuf::from(option(&args, "--data").unwrap_or_else(|| ".bench-data/index".into()));
    let destination = option(&args, "--output");
    let source = source_mode(&args)?;
    match command {
        "startup-profile" => {
            let began = Instant::now();
            let engine = open_source(&root, &data, source)?;
            while !engine.status().ready && engine.status().error.is_none() {
                std::thread::sleep(Duration::from_millis(1));
                if began.elapsed() > Duration::from_secs(300) {
                    return Err("No cached results within five minutes".into());
                }
            }
            let cached_ms = began.elapsed().as_secs_f64() * 1000.;
            let status = ready(&engine)?;
            output(
                &serde_json::json!({"cached_ms":cached_ms,"fully_current_ms":began.elapsed().as_secs_f64()*1000.,"status":status,"method":"Engine readiness and profiled work on a copied saved index; this excludes native window presentation."}),
                destination.as_deref(),
            )
        }
        "largest" => {
            let engine = open_source(&root, &data, source)?;
            let status = ready(&engine)?;
            let mut q = Query::parse("", "")?;
            q.limit = 200;
            q.hide_hidden_system = !args.iter().any(|s| s == "--show-hidden");
            q.scope = option(&args, "--scope").map(|p| seek_engine::paths::internal(Path::new(&p)));
            let folders = args.iter().any(|s| s == "--folders");
            let mut times = Vec::new();
            let mut rows = Vec::new();
            for i in 0..33 {
                let began = Instant::now();
                rows = engine.largest(&q, folders);
                if i >= 3 {
                    times.push(began.elapsed().as_secs_f64() * 1000.);
                }
            }
            let mut sorted = times.clone();
            sorted.sort_by(f64::total_cmp);
            output(
                &serde_json::json!({"folders":folders,"entries":status.entries,"rows":rows.len(),"median_ms":sorted[15],"p95_ms":sorted[28],"samples_ms":times,"top_indexed_sizes":rows.iter().take(10).map(|f|f.size).collect::<Vec<_>>(),"rounding":"Each file at least 2 GB is rounded down by less than 2 MiB; folder totals sum indexed sizes. No disk content or directory scan in the measured Largest query."}),
                destination.as_deref(),
            )
        }
        "profile-index" => {
            let started = Instant::now();
            let index = seek_engine::index::Index::load(&data.join("index.bin"))
                .ok_or("Cannot load diagnostic index")?;
            let load_ms = started.elapsed().as_secs_f64() * 1000.;
            let (event, synced, entries, bytes) =
                (index.event_id, index.synced_at, index.n, index.bytes());
            if args.iter().any(|arg| arg == "--load-only") {
                return output(
                    &serde_json::json!({"entries":entries,"index_bytes":bytes,"load_ms":load_ms,"measurement":"Read-only mmap and structural validation; no index reconstruction or writes"}),
                    destination.as_deref(),
                );
            }
            let live = seek_engine::live::Live::new(index);
            let started = Instant::now();
            let listings = live.to_listings();
            let listings_ms = started.elapsed().as_secs_f64() * 1000.;
            drop(live);
            let started = Instant::now();
            let index = seek_engine::index::Index::build(listings, event, synced, &[]);
            let build_ms = started.elapsed().as_secs_f64() * 1000.;
            if index.n != entries {
                return Err("Index reconstruction changed the entry count".into());
            }
            output(
                &serde_json::json!({"entries":entries,"index_bytes":bytes,"load_ms":load_ms,"listings_ms":listings_ms,"build_ms":build_ms,"measurement":"Read-only offline index reconstruction; no crawl, helper, watcher or index writes"}),
                destination.as_deref(),
            )
        }
        "profile-helper" => {
            // Diagnostic only: consume a trusted baseline without index building,
            // persistence or filesystem watchers to locate scan/transport costs.
            let root = std::fs::canonicalize(&root).map_err(|e| e.to_string())?;
            let root = seek_engine::paths::internal(&root);
            let cancel = seek_engine::kernel::Cancel::new().map_err(|e| e.to_string())?;
            let mut pipe = seek_engine::helper::installed_connection(cancel.clone())
                .map_err(|e| e.to_string())?
                .ok_or("Seek helper is not installed")?;
            let started = Instant::now();
            seek_engine::helper::write(
                &mut pipe,
                &seek_engine::helper::Request::Sync {
                    version: seek_engine::helper::VERSION,
                    roots: vec![root.clone()],
                    cursors: vec![],
                    scan: true,
                },
            )
            .map_err(|e| e.to_string())?;
            let mut pipe = std::io::BufReader::with_capacity(1024 * 1024, pipe);
            let mut first = None;
            let (mut frames, mut entries, mut directories, mut skipped, mut reconciliations) =
                (0usize, 0usize, 0usize, 0usize, 0usize);
            loop {
                use seek_engine::helper::Response;
                match seek_engine::helper::read_response(&mut pipe).map_err(|e| e.to_string())? {
                    Response::Chunk {
                        root: actual,
                        listing,
                        last,
                    } => {
                        if actual != root {
                            return Err("Unexpected profiling root".into());
                        }
                        first.get_or_insert_with(|| started.elapsed().as_secs_f64() * 1000.);
                        frames += 1;
                        entries += listing.ents.len();
                        directories += usize::from(last);
                    }
                    Response::Skipped { paths } => skipped += paths.len(),
                    Response::Changes { .. } => {}
                    Response::Overflow { .. } => reconciliations += 1,
                    Response::Ready {
                        cursors,
                        scan_profiles,
                    } => {
                        let total = started.elapsed().as_secs_f64() * 1000.;
                        cancel.cancel();
                        return output(
                            &serde_json::json!({
                                "root":String::from_utf8_lossy(&root),"first_chunk_ms":first,"stream_after_first_chunk_ms":first.map(|t| total-t),
                                "total_ms":total,"frames":frames,"entries":entries,"directories":directories,"skipped":skipped,"reconciliations":reconciliations,"cursors":cursors,"scan_profiles":scan_profiles,
                                "measurement":"Trusted helper baseline only; excludes index construction, persistence, ordinary watcher and independent verification"
                            }),
                            destination.as_deref(),
                        );
                    }
                    Response::Fallback { reason, .. } | Response::Error { message: reason } => {
                        return Err(reason);
                    }
                    _ => return Err("Unexpected profiling response".into()),
                }
            }
        }
        "invalidate-journal" => {
            // A diagnostic for history-loss recovery on an OWNED test index.
            // It never creates, deletes or alters the Windows USN journal.
            let path = data.join("index.bin");
            let index = seek_engine::index::Index::load(&path)
                .ok_or("Cannot load test index; close Seek before this diagnostic")?;
            let mut state: serde_json::Value = serde_json::from_slice(
                index
                    .saved_state()
                    .ok_or("Index has no helper checkpoint")?,
            )
            .map_err(|e| e.to_string())?;
            let cursors = state["cursors"]
                .as_array_mut()
                .ok_or("Missing volume cursors")?;
            if cursors.is_empty() {
                return Err("Index has no USN checkpoint to invalidate".into());
            }
            for cursor in cursors {
                cursor["journal_id"] = serde_json::json!(
                    cursor["journal_id"].as_u64().ok_or("Invalid journal ID")? ^ 1
                );
            }
            let event_id = index.event_id;
            let live = seek_engine::live::Live::new(index);
            let rebuilt = seek_engine::index::Index::build(
                live.to_listings(),
                event_id,
                seek_engine::query::now_secs(),
                &[],
            );
            drop(live);
            rebuilt
                .save_with_state(
                    &path,
                    Some(&serde_json::to_vec(&state).map_err(|e| e.to_string())?),
                )
                .map_err(|e| e.to_string())?;
            output(
                &serde_json::json!({"test_index":path,"journal_checkpoint_invalidated":true,"windows_journal_changed":false}),
                destination.as_deref(),
            )
        }
        "helper-probe" => {
            let sid = seek_engine::helper::user_sid().map_err(|e| e.to_string())?;
            let cancel = seek_engine::kernel::Cancel::new().map_err(|e| e.to_string())?;
            let console = args.iter().any(|a| a == "--console");
            let pipe = if console {
                Some(
                    seek_engine::helper::connect(&seek_engine::helper::pipe_name(&sid), cancel)
                        .map_err(|e| e.to_string())?,
                )
            } else {
                seek_engine::helper::installed_connection(cancel).map_err(|e| e.to_string())?
            };
            if let Some(mut pipe) = pipe {
                let pid = seek_engine::helper::pipe_server_pid(&pipe).map_err(|e| e.to_string())?;
                seek_engine::helper::write(
                    &mut pipe,
                    &seek_engine::helper::Request::Ping {
                        version: seek_engine::helper::VERSION,
                    },
                )
                .map_err(|e| e.to_string())?;
                let response: seek_engine::helper::Response =
                    seek_engine::helper::read_response(&mut pipe).map_err(|e| e.to_string())?;
                output(
                    &serde_json::json!({"available":true,"server_pid":pid,"trusted_installed_service":seek_engine::helper::service_pid(&sid).map_err(|e|e.to_string())? == Some(pid),"response":response}),
                    destination.as_deref(),
                )
            } else {
                output(
                    &serde_json::json!({"available":false,"reason":"Helper is not installed/running"}),
                    destination.as_deref(),
                )
            }
        }
        "ntfs-probe" => {
            let drive = option(&args, "--drive").unwrap_or_else(|| "C".into());
            let report = match seek_engine::ntfs::NtfsSource::open(
                drive.as_bytes().first().copied().ok_or("empty drive")?,
                seek_engine::kernel::Cancel::new().map_err(|e| e.to_string())?,
            ) {
                Ok(source) => match source.cursor() {
                    Ok((cursor, first)) => {
                        serde_json::json!({"available":true,"cursor":cursor,"first_usn":first})
                    }
                    Err(e) => {
                        serde_json::json!({"available":false,"error":e.to_string(),"windows_error":e.raw_os_error()})
                    }
                },
                Err(e) => {
                    serde_json::json!({"available":false,"error":e.to_string(),"windows_error":e.raw_os_error()})
                }
            };
            output(&report, destination.as_deref())
        }
        "index" => {
            let engine = open_source(&root, &data, source)?;
            let status = ready(&engine)?;
            let checkpoint = Instant::now();
            engine.checkpoint()?;
            let mut report = serde_json::to_value(&status).map_err(|e| e.to_string())?;
            report["checkpoint_ms"] = serde_json::json!(checkpoint.elapsed().as_secs_f64() * 1000.);
            report["checkpoint_status"] =
                serde_json::to_value(engine.status()).map_err(|e| e.to_string())?;
            let shutdown = Instant::now();
            drop(engine);
            report["shutdown_ms"] = serde_json::json!(shutdown.elapsed().as_secs_f64() * 1000.);
            output(&report, destination.as_deref())
        }
        "search" => {
            let engine = open_source(&root, &data, source)?;
            ready(&engine)?;
            let q = option(&args, "--query").ok_or("search requires --query")?;
            output(
                &engine.search(&Query::parse(
                    &q,
                    &std::env::var("USERPROFILE").unwrap_or_default(),
                )?)?,
                destination.as_deref(),
            )
        }
        "verify" => {
            let engine = open_source(&root, &data, source)?;
            let status = ready(&engine)?;
            let started = Instant::now();
            let verification_root = option(&args, "--verify-root")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.clone());
            let report = verify(&engine, &verification_root)?;
            engine.checkpoint()?;
            output(
                &serde_json::json!({"report": report, "catch_up_ms": status.catch_up_ms, "verify_ms": started.elapsed().as_secs_f64() * 1000., "status": status}),
                destination.as_deref(),
            )?;
            if !report.missing.is_empty() || !report.extra.is_empty() || !report.stale.is_empty() {
                return Err("Verification failed".into());
            }
            Ok(())
        }
        "bench" => bench(&args),
        "stress" => stress::run(&args),
        "self-test" => self_test(&args),
        _ => {
            println!(
                "seek-cli index|verify|search --root FOLDER --data INDEX_DIRECTORY [--query QUERY] [--output JSON]\nverify optionally accepts --verify-root SUBFOLDER to check part of a whole-drive index.\nseek-cli bench --files 10000 --runs 30 [--ui-executable SEEK_EXE] [--output JSON]\nseek-cli stress --files 10000 --source walker|helper [--output JSON]\nseek-cli self-test [--output JSON]\nAll indexing commands accept --source auto|walker|helper.\nseek-cli ntfs-probe --drive C [--output JSON]\nseek-cli helper-probe [--console] [--output JSON]\nseek-cli profile-helper --root FOLDER [--output JSON]\nseek-cli profile-index --data INDEX_DIRECTORY [--output JSON]\nDiagnostic (close Seek first): seek-cli invalidate-journal --data OWN_TEST_INDEX_DIRECTORY"
            );
            Ok(())
        }
    }
}

#[derive(serde::Serialize)]
struct Verification {
    ground_truth: usize,
    indexed: usize,
    missing: Vec<String>,
    extra: Vec<String>,
    stale: Vec<String>,
    denied: Vec<String>,
}
fn verify(engine: &Engine, root: &Path) -> Result<Verification, String> {
    use std::os::windows::fs::MetadataExt;
    let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let mut truth = BTreeMap::new();
    let mut denied = Vec::new();
    let mut dirs = vec![root.clone()];
    let metadata = std::fs::symlink_metadata(&root).map_err(|e| e.to_string())?;
    truth.insert(
        seek_engine::paths::internal(&root),
        (
            1,
            0,
            (metadata.last_write_time() / 10_000_000).saturating_sub(11_644_473_600) as u32,
        ),
    );
    while let Some(dir) = dirs.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                denied.push(format!("{}: {e}", dir.display()));
                continue;
            }
        };
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(e) => {
                    denied.push(format!("{}: {e}", path.display()));
                    continue;
                }
            };
            // std's Windows file type distinguishes name-surrogate links
            // from provider-backed entries without sharing the walker decoder.
            let kind = if metadata.file_type().is_symlink() {
                2
            } else if metadata.is_dir() {
                1
            } else {
                0
            };
            if kind == 1 {
                dirs.push(path.clone());
            }
            let time =
                (metadata.last_write_time() / 10_000_000).saturating_sub(11_644_473_600) as u32;
            truth.insert(
                seek_engine::paths::internal(&path),
                (kind, if kind == 1 { 0 } else { metadata.len() }, time),
            );
        }
    }
    let prefix = seek_engine::paths::internal(&root);
    let mut indexed = BTreeMap::new();
    for entry in engine.snapshot() {
        let path = seek_engine::paths::internal(&entry.path);
        if path.len() >= prefix.len()
            && seek_engine::paths::cmp_ci(&path[..prefix.len()], &prefix).is_eq()
            && (path.len() == prefix.len() || path.get(prefix.len()) == Some(&b'/'))
        {
            indexed.insert(
                path,
                (
                    entry.kind & 3,
                    if entry.kind & 3 == 1 { 0 } else { entry.size },
                    entry.mtime,
                ),
            );
        }
    }
    let show = |p: &[u8]| seek_engine::paths::native(p).display().to_string();
    Ok(Verification {
        ground_truth: truth.len(),
        indexed: indexed.len(),
        missing: truth
            .keys()
            .filter(|p| !indexed.contains_key(*p))
            .map(|p| show(p))
            .collect(),
        extra: indexed
            .keys()
            .filter(|p| !truth.contains_key(*p))
            .map(|p| show(p))
            .collect(),
        stale: truth
            .iter()
            .filter(|(p, data)| indexed.get(*p).is_some_and(|i| i != *data))
            .map(|(p, _)| show(p))
            .collect(),
        denied,
    })
}

fn bench(args: &[String]) -> Result<(), String> {
    let files: usize = option(args, "--files")
        .unwrap_or_else(|| "10000".into())
        .parse()
        .map_err(|_| "bad --files")?;
    let runs: usize = option(args, "--runs")
        .unwrap_or_else(|| "30".into())
        .parse()
        .map_err(|_| "bad --runs")?;
    if files < 3 || runs < 30 {
        return Err("bench requires >=3 files and >=30 runs".into());
    }
    let base = PathBuf::from(format!(".bench-data/{files}"));
    let root = base.join("files");
    let data = base.join("index");
    let reused = args.iter().any(|arg| arg == "--reuse-dataset");
    if reused && (!root.is_dir() || !data.join("index.bin").is_file()) {
        return Err("--reuse-dataset requires an existing generated folder and saved index; run bench without this flag first".into());
    }
    let generated = Instant::now();
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let names = ["atlas-notes", "orbit-budget", "pixel-design"];
    for i in 0..if reused { 0 } else { files } {
        let folder = root.join(format!("batch-{:04}", i / 1000));
        if i % 1000 == 0 {
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        }
        let file = folder.join(format!("{}-{i:05}.txt", names[i % 3]));
        if !file.exists() {
            std::fs::write(&file, b"Seek filename benchmark\n").map_err(|e| e.to_string())?;
        }
    }
    let generation_ms = generated.elapsed().as_secs_f64() * 1000.;
    let engine = open_source(&root, &data, source_mode(args)?)?;
    let status = ready(&engine)?;
    let scope =
        seek_engine::paths::internal(&std::fs::canonicalize(&root).map_err(|e| e.to_string())?);
    let targets = [
        "atlas-notes-00000.txt",
        "orbit-budget-00001.txt",
        "pixel-design-00002.txt",
    ];
    let extended = args.iter().any(|arg| arg == "--extended");
    let mut cases: Vec<_> = targets
        .into_iter()
        .map(|target| (format!("'{target} limit:500"), Some(target)))
        .collect();
    if extended {
        cases.extend([
            ("atlas limit:200".into(), None),
            ("altas-notes-00000 limit:200".into(), Some(targets[0])),
            ("^orbit-budget-00001 limit:200".into(), Some(targets[1])),
            (
                "pixel-design-00002 ext:txt limit:200".into(),
                Some(targets[2]),
            ),
        ]);
    }
    let mut rows = Vec::new();
    for (query, target) in cases {
        let mut samples = Vec::new();
        for i in 0..runs + 3 {
            let started = Instant::now();
            let mut q = Query::parse(&query, "")?;
            q.scope = Some(scope.clone());
            let hits = engine.search(&q)?;
            let ms = started.elapsed().as_secs_f64() * 1000.;
            let correct = target.map_or_else(
                || {
                    hits.len() == 200
                        && hits.iter().all(|h| {
                            h.path
                                .file_name()
                                .is_some_and(|n| n.to_string_lossy().starts_with("atlas"))
                        })
                },
                |target| {
                    hits.iter()
                        .take(5)
                        .any(|h| h.path.file_name().is_some_and(|n| n == target))
                },
            );
            if !correct || (query.starts_with('\'') && hits.len() != 1) {
                return Err(format!(
                    "benchmark result mismatch for {query}: {} hits",
                    hits.len()
                ));
            }
            if i >= 3 {
                samples.push(ms);
            }
        }
        let mut sorted = samples.clone();
        sorted.sort_by(f64::total_cmp);
        let median = if runs.is_multiple_of(2) {
            (sorted[runs / 2 - 1] + sorted[runs / 2]) / 2.
        } else {
            sorted[runs / 2]
        };
        rows.push(serde_json::json!({"query": query, "median_ms": median, "p95_ms": sorted[(runs * 95).div_ceil(100) - 1], "samples_ms": samples}));
    }
    let mut report = serde_json::json!({"files": files, "runs": runs, "warmups": 3, "reused_dataset": reused, "generation_ms": generation_ms, "index_or_catch_up_ms": status.catch_up_ms, "status": status, "index_bytes_per_entry": status.index_bytes as f64 / status.entries as f64, "measurement": "backend parse + search + materialization; no IPC or painting", "queries": rows});
    drop(engine);
    if let Some(executable) = option(args, "--ui-executable") {
        let ui_output = base.join("ui-report.json");
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/bench-window.ps1");
        let result = std::process::Command::new("powershell.exe")
            .arg("-NoProfile")
            .arg("-File")
            .arg(script)
            .arg("-Executable")
            .arg(executable)
            .arg("-Files")
            .arg(files.to_string())
            .arg("-Runs")
            .arg(runs.to_string())
            .arg("-Output")
            .arg(&ui_output)
            .args(if extended { vec!["-Extended"] } else { vec![] })
            .args(
                if matches!(source_mode(args)?, seek_engine::SourceMode::Walker) {
                    vec!["-Walker"]
                } else {
                    vec![]
                },
            )
            .output()
            .map_err(|e| e.to_string())?;
        if !result.status.success() {
            return Err(format!(
                "UI benchmark failed: {} {}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            ));
        }
        let bytes = std::fs::read(ui_output).map_err(|e| e.to_string())?;
        report["ui"] =
            serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
                .map_err(|e| e.to_string())?;
    }
    output(&report, option(args, "--output").as_deref())
}

fn wait_verified(engine: &Engine, root: &Path) -> Result<Verification, String> {
    let start = Instant::now();
    loop {
        let report = verify(engine, root)?;
        if report.missing.is_empty() && report.extra.is_empty() && report.stale.is_empty() {
            return Ok(report);
        }
        if start.elapsed() > Duration::from_secs(15) {
            return Err(format!(
                "{}; status={:?}",
                serde_json::to_string(&report).unwrap(),
                engine.status()
            ));
        }
        std::thread::sleep(Duration::from_millis(10)); // Test harness only, never production polling.
    }
}
fn self_test(args: &[String]) -> Result<(), String> {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = PathBuf::from(format!(
        ".bench-data/correctness-{}-{unique}",
        std::process::id()
    ));
    let root = base.join("files");
    let data = base.join("index");
    std::fs::create_dir_all(root.join("Downloads")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(root.join("moved")).map_err(|e| e.to_string())?;
    for name in [
        "Résumé-🚀.txt",
        "annual-report.txt",
        "CaseDifference.TXT",
        "invoice-budget.pdf",
        "desktop-design.txt",
        "obsolete.txt",
    ] {
        std::fs::write(root.join("Downloads").join(name), b"hello").map_err(|e| e.to_string())?;
    }
    for i in 0..3000 {
        std::fs::write(root.join("moved").join(format!("bulk-{i:04}.txt")), b"bulk")
            .map_err(|e| e.to_string())?;
    }
    let long = root
        .join("long-component".repeat(10))
        .join("another-component".repeat(7));
    std::fs::create_dir_all(&long).map_err(|e| e.to_string())?;
    std::fs::write(long.join("long-path-🚀.txt"), b"long").map_err(|e| e.to_string())?;
    std::fs::hard_link(
        root.join("Downloads/invoice-budget.pdf"),
        root.join("hard-link.pdf"),
    )
    .map_err(|e| e.to_string())?;
    use std::os::windows::ffi::OsStringExt;
    let mut surrogate: Vec<_> = "unpaired-".encode_utf16().collect();
    surrogate.push(0xd800);
    surrogate.extend(".txt".encode_utf16());
    std::fs::write(
        root.join(std::ffi::OsString::from_wide(&surrogate)),
        b"WTF-8 on disk",
    )
    .map_err(|e| e.to_string())?;
    let junction = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(std::path::absolute(root.join("junction-loop")).map_err(|e| e.to_string())?)
        .arg(std::fs::canonicalize(&root).map_err(|e| e.to_string())?)
        .output()
        .map_err(|e| e.to_string())?;
    if !junction.status.success() {
        return Err(format!(
            "Junction fixture creation failed: {}",
            String::from_utf8_lossy(&junction.stderr)
        ));
    }
    let engine = open_source(&root, &data, source_mode(args)?)?;
    let first_status = ready(&engine)?;
    let first = wait_verified(&engine, &root)?;
    let live_started = Instant::now();
    std::fs::write(root.join("created-live.txt"), b"new").map_err(|e| e.to_string())?;
    std::fs::rename(
        root.join("Downloads/annual-report.txt"),
        root.join("Downloads/renamed-report.txt"),
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(
        root.join("Downloads/CaseDifference.TXT"),
        root.join("moved/CaseDifference.TXT"),
    )
    .map_err(|e| e.to_string())?;
    std::fs::remove_file(root.join("Downloads/obsolete.txt")).map_err(|e| e.to_string())?;
    std::fs::write(
        root.join("Downloads/invoice-budget.pdf"),
        b"changed file metadata",
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(
        root.join("moved/CaseDifference.TXT"),
        root.join("moved/casedifference.txt"),
    )
    .map_err(|e| e.to_string())?;
    let live = wait_verified(&engine, &root)?;
    let live_ms = live_started.elapsed().as_secs_f64() * 1000.;
    engine.checkpoint()?;
    drop(engine);
    std::fs::write(root.join("created-closed.txt"), b"closed").map_err(|e| e.to_string())?;
    std::fs::rename(
        root.join("Downloads/renamed-report.txt"),
        root.join("Downloads/annual-report-final.txt"),
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(
        root.join("created-live.txt"),
        root.join("moved/moved-closed.txt"),
    )
    .map_err(|e| e.to_string())?;
    std::fs::remove_file(root.join("Downloads/desktop-design.txt")).map_err(|e| e.to_string())?;
    std::fs::write(
        root.join("Downloads/invoice-budget.pdf"),
        b"closed file modification with stable directory metadata",
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(
        root.join("moved/casedifference.txt"),
        root.join("moved/CaseDifference.TXT"),
    )
    .map_err(|e| e.to_string())?;
    let engine = open_source(&root, &data, source_mode(args)?)?;
    let relaunch_status = ready(&engine)?;
    let closed = wait_verified(&engine, &root)?;
    let mut cases: Vec<(String, String)> = [
        ("annual-report-final", "annual-report-final.txt"),
        ("report", "annual-report-final.txt"),
        ("invoce", "invoice-budget.pdf"),
        ("invocie", "invoice-budget.pdf"),
        ("Résumé", "Résumé-🚀.txt"),
        ("casedifference", "CaseDifference.TXT"),
        ("long-path", "long-path-🚀.txt"),
    ]
    .into_iter()
    .map(|(q, target)| (q.into(), target.into()))
    .collect();
    cases.push((
        "'annual-report-final.txt".into(),
        "annual-report-final.txt".into(),
    ));
    cases.push(("^annual".into(), "annual-report-final.txt".into()));
    cases.push((
        format!(
            "report in:\"{}\"",
            std::path::absolute(root.join("Downloads"))
                .unwrap()
                .display()
        ),
        "annual-report-final.txt".into(),
    ));
    let total = cases.len();
    let mut accuracy = Vec::new();
    for (query, target) in cases {
        let mut q = Query::parse(&query, "")?;
        q.limit = 5;
        let hits = engine.search(&q)?;
        accuracy.push(serde_json::json!({"query": query, "target": target, "top5": hits.iter().any(|h| h.path.file_name().is_some_and(|n| n == target.as_str()))}));
    }
    let passed = accuracy.iter().filter(|c| c["top5"] == true).count();
    output(
        &serde_json::json!({"root": root, "fixtures": ["3000-file folder", "emoji", "Unicode", "long path", "unpaired surrogate on disk", "hard link", "junction loop skipped", "live and closed case-only rename"], "first": first, "live": live, "closed": closed, "first_status": first_status, "relaunch_status": relaunch_status, "live_ms": live_ms, "walker_catch_up_ms": if relaunch_status.source == "walker" {Some(relaunch_status.catch_up_ms)} else {None}, "helper_catch_up_ms": if relaunch_status.source.contains("usn") {Some(relaunch_status.catch_up_ms)} else {None}, "accuracy": accuracy, "accuracy_passed": passed, "accuracy_total": total, "accuracy_percent": 100. * passed as f64 / total as f64, "typo_claim_comparison": "Different workload; no comparison with upstream percentage"}),
        option(args, "--output").as_deref(),
    )?;
    if passed != total {
        return Err("Accuracy test failed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn walker_first_live_and_closed_changes_match_independent_ground_truth() {
        super::self_test(&["--source".into(), "walker".into()]).unwrap();
    }
}
