//! Reproducible burst/restart/corrupt-snapshot checks on owned fixtures only.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) fn run(args: &[String]) -> Result<(), String> {
    let count: usize = option(args, "--files")
        .unwrap_or_else(|| "10000".into())
        .parse()
        .map_err(|_| "bad --files")?;
    if !(1000..=200_000).contains(&count) || !count.is_multiple_of(4) {
        return Err("stress requires 1000..200000 files, divisible by four".into());
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::path::absolute(format!(".bench-data/stress-{}-{nonce}", std::process::id()))
        .map_err(|e| e.to_string())?;
    let root = base.join("files");
    let data = base.join("index");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    std::fs::write(root.join("anchor.txt"), b"stable query target").map_err(|e| e.to_string())?;
    for lane in 0..4 {
        let dir = root.join(format!("lane-{lane}"));
        std::fs::create_dir_all(dir.join("from")).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(dir.join("to")).map_err(|e| e.to_string())?;
        for i in 0..count / 4 {
            std::fs::write(dir.join(format!("base-{i:05}.txt")), b"baseline")
                .map_err(|e| e.to_string())?;
        }
    }
    let mode = source_mode(args)?;
    let engine = open_source(&root, &data, mode)?;
    let first_status = ready(&engine)?;
    let first = wait_verified(&engine, &root)?;
    let stop = Arc::new(AtomicBool::new(false));
    let query_stop = stop.clone();
    let query_engine = engine.clone();
    let reader = std::thread::spawn(move || -> Result<Vec<f64>, String> {
        let query = Query::parse("'anchor.txt limit:5", "")?;
        let mut samples = Vec::new();
        while !query_stop.load(Ordering::Acquire) {
            let started = Instant::now();
            let hits = query_engine.search(&query)?;
            if hits.len() != 1 || hits[0].path.file_name().is_none_or(|v| v != "anchor.txt") {
                return Err("Stable file disappeared during burst queries".into());
            }
            if samples.len() < 30_000 {
                samples.push(started.elapsed().as_secs_f64() * 1000.);
            }
            std::thread::sleep(Duration::from_millis(5)); // Test workload, not app polling.
        }
        Ok(samples)
    });
    let started = Instant::now();
    let mut writers = Vec::new();
    for lane in 0..4 {
        let dir = root.join(format!("lane-{lane}"));
        writers.push(std::thread::spawn(move || -> Result<usize, String> {
            let mut operations = 0;
            for i in 0..count / 4 {
                let created = dir.join("from").join(format!("created-{i:05}-🚀.txt"));
                let renamed = dir.join("from").join(format!("renamed-{i:05}.txt"));
                let moved = dir.join("to").join(format!("moved-{i:05}.txt"));
                std::fs::write(&created, b"new").map_err(|e| e.to_string())?;
                std::fs::rename(&created, &renamed).map_err(|e| e.to_string())?;
                std::fs::rename(&renamed, &moved).map_err(|e| e.to_string())?;
                std::fs::write(&moved, b"modified metadata and content")
                    .map_err(|e| e.to_string())?;
                operations += 4;
                if i % 2 == 0 {
                    std::fs::remove_file(&moved).map_err(|e| e.to_string())?;
                    operations += 1;
                }
                let baseline = dir.join(format!("base-{i:05}.txt"));
                if i % 7 == 0 {
                    std::fs::remove_file(&baseline).map_err(|e| e.to_string())?;
                    operations += 1;
                } else if i % 3 == 0 {
                    std::fs::write(&baseline, b"updated baseline with new length")
                        .map_err(|e| e.to_string())?;
                    operations += 1;
                }
            }
            Ok(operations)
        }));
    }
    let mut operations = 0;
    let mut write_error = None;
    for writer in writers {
        match writer
            .join()
            .map_err(|_| "Writer panicked".to_string())
            .and_then(|v| v)
        {
            Ok(n) => operations += n,
            Err(error) => write_error = Some(error),
        }
    }
    let writing_ms = started.elapsed().as_secs_f64() * 1000.;
    stop.store(true, Ordering::Release);
    let mut samples = reader.join().map_err(|_| "Query reader panicked")??;
    if let Some(error) = write_error {
        return Err(error);
    }
    let convergence = Instant::now();
    let live = wait_verified(&engine, &root)?;
    let convergence_ms = convergence.elapsed().as_secs_f64() * 1000.;
    let checkpoint = Instant::now();
    engine.checkpoint()?;
    let checkpoint_ms = checkpoint.elapsed().as_secs_f64() * 1000.;
    drop(engine);
    for lane in 0..4 {
        let dir = root.join(format!("lane-{lane}"));
        std::fs::write(dir.join("closed-created-🧪.txt"), b"closed create")
            .map_err(|e| e.to_string())?;
        std::fs::rename(
            dir.join("to/moved-00001.txt"),
            dir.join("closed-renamed.txt"),
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(
            dir.join("to/moved-00003.txt"),
            root.join(format!("closed-moved-{lane}.txt")),
        )
        .map_err(|e| e.to_string())?;
        std::fs::remove_file(dir.join("to/moved-00005.txt")).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("base-00002.txt"), b"closed metadata update")
            .map_err(|e| e.to_string())?;
    }
    let engine = open_source(&root, &data, mode)?;
    let restart_status = ready(&engine)?;
    let closed = wait_verified(&engine, &root)?;
    engine.checkpoint()?;
    drop(engine);
    let path = data.join("index.bin");
    let mut bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    bytes[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let engine = open_source(&root, &data, mode)?;
    let recovery_status = ready(&engine)?;
    let recovered = wait_verified(&engine, &root)?;
    let query_count = samples.len();
    samples.sort_by(f64::total_cmp);
    let percentile = |p: usize| {
        samples
            .get((query_count * p).div_ceil(100).saturating_sub(1))
            .copied()
    };
    output(
        &serde_json::json!({
            "root": root, "generated_files": count+1, "writers": 4,
            "live_operations": operations, "closed_operations": 20,
            "writing_ms": writing_ms, "convergence_after_writers_ms": convergence_ms,
            "checkpoint_ms": checkpoint_ms, "first": first, "live": live,
            "closed": closed, "corrupt_snapshot_recovery": recovered,
            "first_status": first_status, "restart_status": restart_status, "recovery_status": recovery_status,
            "concurrent_query_samples": query_count, "concurrent_backend_median_ms": percentile(50), "concurrent_backend_p95_ms": percentile(95),
            "query_scope": "Stable exact filename while four writers create, rename, move, edit and delete files; includes lock contention. This is not UI paint timing."
        }),
        option(args, "--output").as_deref(),
    )
}
