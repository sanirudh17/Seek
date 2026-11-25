//! `seek-cli search-bench`: latency, accuracy and junk-rate measurements on a
//! saved index, without starting the engine's catch-up.
//!
//!   seek-cli search-bench --data <dir with index.bin> [--rebuild] [--runs 30]
//!       [--samples 300] [--output report.json] [--label before]
//!
//! `--rebuild` reads the saved index back into listings and builds it again
//! with the current ranking weights (`Index::relisted`); without it the saved
//! index keeps the weights it was built with. Both runs use the same entries,
//! queries and sampled targets, so they form an A/B pair.
//!
//! Method (docs/REFERENCES.md): 3 warmups, `--runs` samples, median and
//! nearest-rank p95, every sample's results checked. The name cache is
//! cleared before each sample, so a sample is a fresh search, not a repeat
//! answered from the previous keystroke; typing is measured separately.

use seek_engine::{
    Query, index::Index, live::Live, query::Searcher, search_live, walk::FLAG_SYNTH,
};
use std::{collections::HashMap, time::Instant};

const WARMUPS: usize = 3;
/// The first page the app asks for (app/src/backend.rs).
const PAGE: usize = 200;
/// "Every result": enough for the largest result sets on a 3.5M index.
const FULL: usize = 100_000;

fn option(args: &[String], key: &str) -> Option<String> {
    args.windows(2).find(|a| a[0] == key).map(|a| a[1].clone())
}

fn stats(mut v: Vec<f64>) -> (f64, f64) {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    let median = if n.is_multiple_of(2) {
        (v[n / 2 - 1] + v[n / 2]) / 2.
    } else {
        v[n / 2]
    };
    (median, v[((n * 95).div_ceil(100)).max(1) - 1])
}

fn peak_memory() -> u64 {
    use windows_sys::Win32::System::{ProcessStatus::*, Threading::GetCurrentProcess};
    let mut c: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    c.PeakWorkingSetSize as u64
}

/// Small deterministic generator, so both runs of an A/B pair sample the
/// same targets.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Folders whose results count as junk when they crowd out user files.
fn junk(path: &str) -> bool {
    let p = path.to_ascii_lowercase().replace('\\', "/");
    let seg = |s: &str| p.contains(&format!("/{s}/"));
    let top = |s: &str| {
        p.get(2..)
            .is_some_and(|rest| rest.starts_with(&format!("/{s}/")))
    };
    seg("appdata")
        || top("windows")
        || top("program files")
        || top("program files (x86)")
        || top("programdata")
        || top("$recycle.bin")
        || top("system volume information")
        || seg("node_modules")
        || seg(".git")
        || seg("__pycache__")
        || seg("site-packages")
        || seg("temp")
        || seg("tmp")
        || seg("cache")
        || seg("caches")
        || seg(".cargo")
        || seg(".rustup")
        || seg("target")
        || seg("winsxs")
}

/// A file in the user's own folders: under the profile, outside AppData and
/// dot folders.
fn user_file(path: &str, home: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.starts_with(&home.to_ascii_lowercase())
        && !junk(path)
        && !p[home.len().min(p.len())..].contains("\\.")
}

/// Words of a file stem: split at spaces, punctuation, digits-to-letters and
/// lower-to-upper case changes.
fn words(stem: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev: Option<char> = None;
    for c in stem.chars() {
        let boundary = !c.is_alphanumeric()
            || prev.is_some_and(|p| {
                (p.is_lowercase() && c.is_uppercase()) || (p.is_alphabetic() != c.is_alphabetic())
            });
        if boundary && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if c.is_alphanumeric() {
            cur.push(c);
        }
        prev = Some(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Queries, top-1, top-5 and top-20 hits, and the sum of reciprocal ranks.
type Tally = (usize, usize, usize, usize, f64);

struct Target {
    path: String,
    name: String,
    user: bool,
}

/// Derived queries for one target: (kind, query).
fn derive(t: &Target, rng: &mut Rng) -> Vec<(&'static str, String)> {
    let name = t.name.to_lowercase();
    let stem = match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => &name[..],
    };
    let chars: Vec<char> = stem.chars().collect();
    let mut out = vec![("exact", name.clone())];
    if chars.len() >= 6 {
        out.push((
            "prefix",
            chars[..chars.len().div_ceil(2).max(3)].iter().collect(),
        ));
        let len = 4 + rng.below(3).min(chars.len() - 5);
        let start = 1 + rng.below(chars.len() - len);
        let fragment: String = chars[start..start + len].iter().collect();
        if fragment.chars().all(|c| c.is_alphanumeric()) {
            out.push(("middle", fragment));
        }
    }
    let ws = words(&t.name);
    if ws.len() >= 3 {
        out.push((
            "acronym",
            ws.iter()
                .filter_map(|w| w.chars().next())
                .collect::<String>()
                .to_lowercase(),
        ));
    }
    if ws.len() >= 2 && ws[0].len() >= 3 && ws[1].len() >= 3 {
        out.push(("two words", format!("{} {}", ws[0], ws[1]).to_lowercase()));
    }
    if chars.len() >= 6
        && chars
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || "-_ ".contains(*c))
    {
        // One substitution in the middle, keeping the first letter.
        let mut typo = chars.clone();
        let i = 1 + rng.below(chars.len() - 2);
        if typo[i].is_ascii_lowercase() {
            typo[i] = if typo[i] == 'z' {
                'a'
            } else {
                (typo[i] as u8 + 1) as char
            };
            out.push(("typo", typo.into_iter().collect()));
        }
    }
    let parent = std::path::Path::new(&t.path)
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_lowercase());
    if let Some(parent) = parent.filter(|p| p.len() >= 3 && !p.contains(' ') && chars.len() >= 6) {
        out.push((
            "path hint",
            format!(
                "{parent} {}",
                chars[..chars.len().div_ceil(2).max(3)]
                    .iter()
                    .collect::<String>()
            ),
        ));
    }
    // Queries the parser would read as operators are not name queries.
    out.retain(|(_, q)| !q.contains(':') && !q.starts_with(['-', '\'', '^', '!', '"']));
    out
}

pub fn run(args: &[String]) -> Result<(), String> {
    let data = std::path::PathBuf::from(option(args, "--data").ok_or("--data <dir> is required")?);
    let runs: usize = option(args, "--runs")
        .map_or(Ok(30), |v| v.parse())
        .map_err(|_| "bad --runs")?;
    let samples: usize = option(args, "--samples")
        .map_or(Ok(300), |v| v.parse())
        .map_err(|_| "bad --samples")?;
    let label = option(args, "--label").unwrap_or_else(|| "run".into());
    let home_internal = seek_engine::places::home();
    let home = seek_engine::paths::native(&home_internal)
        .to_string_lossy()
        .into_owned();
    if let Some(file) = option(args, "--weights") {
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}"))?;
        seek_engine::index::set_weights(Some(
            serde_json::from_str(&text).map_err(|e| e.to_string())?,
        ));
    }

    // Restart to first result: load the saved index, map it, first search.
    let loaded = Instant::now();
    let index = Index::load(&data.join("index.bin")).ok_or("no readable index.bin in --data")?;
    let load_ms = loaded.elapsed().as_secs_f64() * 1000.;
    let (index, rebuild_ms) = if args.iter().any(|a| a == "--rebuild") {
        let started = Instant::now();
        let listings = index.relisted();
        let (event, synced) = (index.event_id, index.synced_at);
        drop(index);
        let rebuilt = Index::build(listings, event, synced, &home_internal);
        let ms = started.elapsed().as_secs_f64() * 1000.;
        // Like the app: save the reweighted index and search the file map.
        let rebuilt = match option(args, "--save-to") {
            Some(dir) => {
                let path = std::path::Path::new(&dir).join("index.bin");
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                rebuilt.save(&path).map_err(|e| e.to_string())?;
                drop(rebuilt);
                Index::load(&path).ok_or("saved index failed to load")?
            }
            None => rebuilt,
        };
        (rebuilt, Some(ms))
    } else {
        (index, None)
    };
    let live = Live::new(index);
    let opened = HashMap::new();
    let first = Query::parse("report", "")?;
    let first_hits = search_live(&live, &first, &opened).len();
    let restart_ms = loaded.elapsed().as_secs_f64() * 1000.;
    let base = &live.base;
    let entries = base.n;
    if let Some(shown) = option(args, "--show") {
        for q in shown.split(',') {
            let mut query = Query::parse(q, "")?;
            query.limit = 20;
            println!("== {q}");
            for h in search_live(&live, &query, &opened) {
                println!(
                    "{:>5} {}{}",
                    h.score,
                    if junk(&h.path.to_string_lossy()) {
                        "J "
                    } else {
                        "  "
                    },
                    h.path.display()
                );
            }
        }
        return Ok(());
    }

    let fresh = |q: &str, limit: usize| -> Result<(f64, usize), String> {
        live.names_cache.trim_if_idle(std::time::Duration::ZERO);
        let mut query = Query::parse(q, "")?;
        query.limit = limit;
        let started = Instant::now();
        let hits = search_live(&live, &query, &opened);
        Ok((started.elapsed().as_secs_f64() * 1000., hits.len()))
    };

    // Latency by query kind.
    let cases: &[(&str, &str)] = &[
        ("1 char", "a"),
        ("2 chars", "re"),
        ("3 chars", "con"),
        ("whole word", "report"),
        ("whole word", "invoice"),
        ("prefix", "^read"),
        ("substring", "port"),
        ("camelCase", "MainWindow"),
        ("underscore", "test_utils"),
        ("typo", "documnet"),
        ("several words", "windows update log"),
        ("path-qualified", "users downloads pdf"),
        ("extension", "ext:pdf invoice"),
        ("extension only", "ext:png"),
        ("exact file", "readme.md"),
        ("no match", "zzqxv"),
    ];
    let mut latency = Vec::new();
    for (kind, q) in cases {
        let (mut page, mut full) = (Vec::new(), Vec::new());
        let (mut page_hits, mut full_hits) = (0, 0);
        for i in 0..WARMUPS + runs {
            let (ms, n) = fresh(q, PAGE)?;
            let (full_ms, full_n) = fresh(q, FULL)?;
            if i > 0 && (n != page_hits || full_n != full_hits) {
                return Err(format!(
                    "{q}: result count changed between samples ({page_hits}/{n}, {full_hits}/{full_n})"
                ));
            }
            (page_hits, full_hits) = (n, full_n);
            if i >= WARMUPS {
                page.push(ms);
                full.push(full_ms);
            }
        }
        let (pm, pp) = stats(page);
        let (fm, fp) = stats(full);
        latency.push(serde_json::json!({"kind": kind, "query": q, "first_page_median_ms": pm, "first_page_p95_ms": pp,
            "page_hits": page_hits, "all_results_median_ms": fm, "all_results_p95_ms": fp, "all_hits": full_hits}));
    }

    // Typing one key at a time, reusing the previous keystroke's names.
    let typed = "readme.md";
    let mut keys = Vec::new();
    for i in 0..WARMUPS + runs {
        live.names_cache.trim_if_idle(std::time::Duration::ZERO);
        for n in 1..=typed.len() {
            let mut q = Query::parse(&typed[..n], "")?;
            q.limit = PAGE;
            let started = Instant::now();
            search_live(&live, &q, &opened);
            if i >= WARMUPS {
                keys.push(started.elapsed().as_secs_f64() * 1000.);
            }
        }
    }
    let (key_median, key_p95) = stats(keys);

    // Heimdall's query set, limit 50, searched fresh each time like its
    // bench (a new Searcher per iteration).
    let mut heimdall = Vec::new();
    for q in [
        "a",
        "re",
        "report",
        "main.rs",
        "readme",
        "heimdall",
        "config json",
        "ext:pdf invoice",
        "node_modules",
        "zzqxv",
    ] {
        let mut t = Vec::new();
        let mut hits = 0;
        for i in 0..WARMUPS + runs {
            live.names_cache.trim_if_idle(std::time::Duration::ZERO);
            let mut query = Query::parse(q, "")?;
            query.limit = 50;
            let started = Instant::now();
            hits = Searcher { live: &live }.search(&query).len();
            if i >= WARMUPS {
                t.push(started.elapsed().as_secs_f64() * 1000.);
            }
        }
        let (m, p) = stats(t);
        heimdall.push(serde_json::json!({"query": q, "hits": hits, "median_ms": m, "p95_ms": p}));
    }

    // Accuracy: targets are files whose name is unique in the index, half
    // from the user's own folders and half from anywhere else.
    let ents_off = base.name_ents_off();
    let mut rng = Rng(0x5eed_5eed);
    let mut targets: Vec<Target> = Vec::new();
    let (mut user_n, mut other_n) = (0, 0);
    let mut path = Vec::new();
    let mut tries = 0;
    while (user_n < samples / 2 || other_n < samples - samples / 2) && tries < 50_000_000 {
        tries += 1;
        let e = 1 + rng.below(entries - 1);
        let kind = base.kind()[e];
        if kind & 3 != 0 || kind & FLAG_SYNTH != 0 {
            continue;
        }
        let k = base.ent_name()[e] as usize;
        if ents_off[k + 1] - ents_off[k] != 1 {
            continue;
        }
        let name = String::from_utf8_lossy(base.name(e)).into_owned();
        if name.chars().count() < 6 || !name.chars().any(|c| c.is_alphabetic()) {
            continue;
        }
        base.path(e, &mut path);
        let native = seek_engine::paths::native(&path)
            .to_string_lossy()
            .into_owned();
        let user = user_file(&native, &home);
        // Mostly user files need a wider net: they are a small share.
        if user && user_n < samples / 2 {
            user_n += 1;
        } else if !user && other_n < samples - samples / 2 {
            other_n += 1;
        } else {
            continue;
        }
        targets.push(Target {
            path: native,
            name,
            user,
        });
    }
    let mut by_kind: HashMap<(&str, bool), Tally> = HashMap::new();
    let mut misses = Vec::new();
    for t in &targets {
        for (kind, q) in derive(t, &mut rng) {
            let mut query = Query::parse(&q, "")?;
            query.limit = 100;
            let hits = search_live(&live, &query, &opened);
            let rank = hits
                .iter()
                .position(|h| h.path.to_string_lossy() == t.path)
                .map(|r| r + 1);
            let row = by_kind.entry((kind, t.user)).or_default();
            row.0 += 1;
            if let Some(r) = rank {
                row.1 += (r == 1) as usize;
                row.2 += (r <= 5) as usize;
                row.3 += (r <= 20) as usize;
                row.4 += 1. / r as f64;
            } else if misses.len() < 40 {
                misses.push(serde_json::json!({"kind": kind, "query": q, "user": t.user}));
            }
        }
    }
    let summarize = |filter: &dyn Fn(&str, bool) -> bool| {
        let (mut n, mut t1, mut t5, mut t20, mut rr) = (0, 0, 0, 0, 0.);
        for ((k, u), v) in &by_kind {
            if filter(k, *u) {
                n += v.0;
                t1 += v.1;
                t5 += v.2;
                t20 += v.3;
                rr += v.4;
            }
        }
        let f = |x: usize| x as f64 / n.max(1) as f64;
        serde_json::json!({"queries": n, "top1": f(t1), "top5": f(t5), "top20": f(t20), "mrr": rr / n.max(1) as f64})
    };
    let mut kinds: Vec<_> = by_kind.keys().map(|(k, _)| *k).collect();
    kinds.sort();
    kinds.dedup();
    let accuracy = serde_json::json!({
        "targets": targets.len(),
        "all": summarize(&|_, _| true),
        "user_targets": summarize(&|_, u| u),
        "other_targets": summarize(&|_, u| !u),
        "by_kind": kinds.iter().map(|k| (k.to_string(), summarize(&|kk, _| kk == *k))).collect::<serde_json::Map<_, _>>(),
        "sample_misses": misses,
    });

    // Junk rate: of queries with a match in the user's folders, how often
    // junk folders fill more than half of the top 20.
    let mut words_seen = std::collections::BTreeSet::new();
    for t in targets.iter().filter(|t| t.user) {
        for w in words(&t.name) {
            let w = w.to_lowercase();
            if w.len() >= 4 && w.chars().all(|c| c.is_ascii_alphabetic()) {
                words_seen.insert(w);
            }
        }
    }
    for w in [
        "report",
        "config",
        "image",
        "test",
        "index",
        "notes",
        "setup",
        "readme",
        "logo",
        "photo",
        "invoice",
        "resume",
        "project",
        "backup",
        "settings",
        "screenshot",
        "data",
        "main",
        "icon",
        "license",
    ] {
        words_seen.insert(w.into());
    }
    let (mut eligible, mut dominated, mut junk_top20) = (0, 0, 0usize);
    let mut dominated_queries = Vec::new();
    for w in &words_seen {
        let mut query = Query::parse(w, "")?;
        query.limit = 1000;
        let hits = search_live(&live, &query, &opened);
        if !hits
            .iter()
            .any(|h| user_file(&h.path.to_string_lossy(), &home))
        {
            continue;
        }
        eligible += 1;
        let top: Vec<_> = hits.iter().take(20).collect();
        let j = top
            .iter()
            .filter(|h| junk(&h.path.to_string_lossy()))
            .count();
        junk_top20 += j;
        if j * 2 > top.len() {
            dominated += 1;
            if dominated_queries.len() < 30 {
                dominated_queries.push(w.clone());
            }
        }
    }
    let junk_report = serde_json::json!({
        "queries_with_user_match": eligible,
        "dominated": dominated,
        "junk_rate": dominated as f64 / eligible.max(1) as f64,
        "mean_junk_share_top20": junk_top20 as f64 / (eligible.max(1) * 20) as f64,
        "dominated_queries": dominated_queries,
    });

    let report = serde_json::json!({
        "label": label,
        "measurement": "in-process engine search on a loaded index; no IPC, no painting",
        "entries": entries,
        "runs": runs, "warmups": WARMUPS,
        "load_index_ms": load_ms,
        "rebuild_from_index_ms": rebuild_ms,
        "restart_to_first_result_ms": restart_ms,
        "first_result_hits": first_hits,
        "peak_working_set_bytes": peak_memory(),
        "latency": latency,
        "typing": {"text": typed, "per_key_median_ms": key_median, "per_key_p95_ms": key_p95},
        "heimdall_query_set_limit50": heimdall,
        "accuracy": accuracy,
        "junk": junk_report,
    });
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    if let Some(out) = option(args, "--output") {
        std::fs::write(&out, &json).map_err(|e| e.to_string())?;
    }
    println!("{json}");
    Ok(())
}
