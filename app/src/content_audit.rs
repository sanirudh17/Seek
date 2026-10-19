//! Opt-in measurement driver for the actual scope-checked content scanner.
use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64},
};
#[derive(Deserialize)]
struct Case {
    query: String,
    locations: Vec<PathBuf>,
    expected: Option<Vec<String>>,
    #[serde(default)]
    include_paths: bool,
}
pub fn run(manifest: &str) -> i32 {
    let result = (|| -> Result<(), String> {
        let cases: Vec<Case> =
            serde_json::from_slice(&std::fs::read(manifest).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if std::env::args().any(|a| a == "--content-evaluate") {
            println!(
                "{}",
                serde_json::to_string_pretty(&evaluate(&cases)?).unwrap()
            );
            return Ok(());
        }
        let mut output = Vec::new();
        for case in cases {
            if std::env::args().any(|a| a == "--content-plan") {
                let mut report = crate::content::Report::default();
                let plan = crate::content::plan(
                    &case.locations,
                    None,
                    &[],
                    true,
                    &|| false,
                    &mut report,
                    &mut |_| {},
                );
                output.push(serde_json::json!({"query":case.query,"locations":case.locations,"eligible":plan.files.iter().map(|(p,_)|p).collect::<Vec<_>>(),"skipped":report.skipped,"unreadable":report.errors}));
                continue;
            }
            let mut partials = Vec::new();
            let search = if std::env::args().any(|a| a == "--content-quick") {
                crate::content_catalog::search
            } else {
                crate::content::search_progress
            };
            let cache_policy = crate::argument("--text-cache-data")
                .map(PathBuf::from)
                .map(|path| crate::text_cache::Config::new(&path, true));
            let report = crate::text_cache::policy(cache_policy, || {
                search(
                    &case.query,
                    &case.locations,
                    None,
                    &[],
                    true,
                    Arc::new(AtomicU64::new(1)),
                    1,
                    &mut |p| {
                        partials.push(
                        serde_json::json!({"matches":p.rows.len(),"first_ms":p.first_result_ms,"checked":p.checked,"total":p.total,"planning":p.planning,"quick":p.quick,"message":p.message()}),
                    );
                    },
                )
            });
            let mut names: Vec<_> = report.rows.iter().map(|r| r.name.clone()).collect();
            names.sort();
            let accuracy = case.expected.map(|mut expected| {
                expected.sort();
                serde_json::json!({"passed":names==expected,"expected":expected,"actual":names})
            });
            output.push(serde_json::json!({"query":case.query,"locations":case.locations,"accuracy":accuracy,"names":names,"first_result_ms":report.first_result_ms,"first_quick_ms":report.first_quick_ms,"quick_matches":report.quick_matches,"message":report.message(),"elapsed_ms":report.elapsed_ms,"checked":report.checked,"skipped":report.skipped,"unreadable":report.errors,"total":report.total,"finished":report.finished,"cancelled":report.cancelled,"paths":if case.include_paths{Some(report.rows.iter().map(|r|r.file.path.clone()).collect::<Vec<_>>())}else{None},"formats":report.formats,"cache":report.cache,"first_failures":report.first_failures,"failures":report.failures,"retried":report.retried,"recovered":report.recovered,"partials":partials}));
        }
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("{error}");
        1
    } else {
        0
    }
}

/// Diagnostic only: retain extracted strings in RAM, report byte counts and
/// warm-search cost, then discard them. No text cache is created on disk.
fn evaluate(cases: &[Case]) -> Result<serde_json::Value, String> {
    use std::{
        collections::{BTreeMap, HashSet},
        io::Write,
        time::{Duration, Instant},
    };
    let case = cases.first().ok_or("No evaluation scope")?;
    let mut report = crate::content::Report::default();
    let plan = crate::content::plan(
        &case.locations,
        None,
        &[],
        true,
        &|| false,
        &mut report,
        &mut |_| {},
    );
    let started = Instant::now();
    let mut corpus = Vec::new();
    let mut formats = BTreeMap::<String, serde_json::Value>::new();
    let mut raw_bytes = 0usize;
    let mut zip_bytes = 0usize;
    let mut key_bytes = 0usize;
    let mut errors = 0;
    let mut cap = false;
    for (path, metadata) in &plan.files {
        let timer = Instant::now();
        let text = crate::content::extract(path, &plan.roots, &plan.excludes, &|| {
            timer.elapsed() >= Duration::from_millis(1400)
        });
        let elapsed = timer.elapsed().as_secs_f64() * 1000.;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let profile=formats.entry(ext.clone()).or_insert_with(||serde_json::json!({"attempted":0,"readable":0,"input_bytes":0,"text_bytes":0,"extract_ms":0.}));
        profile["attempted"] = serde_json::json!(profile["attempted"].as_u64().unwrap() + 1);
        profile["extract_ms"] =
            serde_json::json!(profile["extract_ms"].as_f64().unwrap() + elapsed);
        match text {
            Ok(text) => {
                profile["readable"] = serde_json::json!(profile["readable"].as_u64().unwrap() + 1);
                profile["input_bytes"] =
                    serde_json::json!(profile["input_bytes"].as_u64().unwrap() + metadata.len());
                profile["text_bytes"] =
                    serde_json::json!(profile["text_bytes"].as_u64().unwrap() + text.len() as u64);
                if matches!(ext.as_str(), "docx" | "pdf") {
                    raw_bytes += text.len();
                    let mut compressed = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
                    compressed
                        .start_file(
                            "text",
                            zip::write::SimpleFileOptions::default()
                                .compression_method(zip::CompressionMethod::Deflated),
                        )
                        .map_err(|e| e.to_string())?;
                    compressed
                        .write_all(text.as_bytes())
                        .map_err(|e| e.to_string())?;
                    zip_bytes += compressed
                        .finish()
                        .map_err(|e| e.to_string())?
                        .into_inner()
                        .len();
                    // UTF-8 path + size + modified timestamp + record lengths.
                    key_bytes += path.to_string_lossy().len() + 32;
                }
                if corpus
                    .iter()
                    .map(|(_, t): &(PathBuf, String)| t.len())
                    .sum::<usize>()
                    + text.len()
                    > 256 * 1024 * 1024
                {
                    cap = true;
                    break;
                }
                corpus.push((path.clone(), text));
            }
            Err(_) => errors += 1,
        }
    }
    let catalog = crate::argument("--catalog-evidence")
        .map(|p| {
            std::fs::read(p)
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|e| e.to_string())
                })
        })
        .transpose()?;
    let normalize = |p: &str| p.trim_start_matches(r"\\?\").to_lowercase();
    let mut queries = Vec::new();
    for case in cases {
        let needle = case.query.to_lowercase();
        let mut samples = Vec::new();
        let mut matches = 0;
        for _ in 0..10 {
            let timer = Instant::now();
            matches = corpus
                .iter()
                .filter(|(_, t)| t.to_lowercase().contains(&needle))
                .count();
            samples.push(timer.elapsed().as_secs_f64() * 1000.);
        }
        samples.sort_by(f64::total_cmp);
        let indexed: HashSet<String> = catalog
            .as_ref()
            .and_then(|c| c["cases"].as_array())
            .and_then(|list| {
                list.iter()
                    .find(|q| q["query"].as_str() == Some(&case.query))
            })
            .and_then(|q| q["samples"].as_array())
            .and_then(|list| list.iter().find(|s| s["label"].as_str() == Some("content")))
            .and_then(|s| s["paths"].as_array())
            .map(|paths| {
                paths
                    .iter()
                    .filter_map(|p| p.as_str())
                    .map(normalize)
                    .collect()
            })
            .unwrap_or_default();
        let mut missing = BTreeMap::<String, (usize, usize)>::new();
        for (path, text) in &corpus {
            let lower = text.to_lowercase();
            if lower.contains(&needle) && !indexed.contains(&normalize(&path.to_string_lossy())) {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                let counts = missing.entry(ext).or_default();
                if lower
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|word| word.starts_with(&needle))
                {
                    counts.0 += 1;
                } else {
                    counts.1 += 1;
                }
            }
        }
        queries.push(serde_json::json!({"query":case.query,"matches":matches,"ram_repeat_median_ms":samples[5],"ram_repeat_p95_ms":samples[9],"catalog_matches":indexed.len(),"direct_only_by_extension_prefix_compatible_then_embedded_only":missing}));
    }
    Ok(
        serde_json::json!({"diagnostic":"No disk cache built. RAM repeat is a lower bound, not measured disk-cache latency.","eligible":plan.files.len(),"readable":corpus.len(),"unreadable":errors,"skipped":report.skipped,"memory_cap_reached":cap,"elapsed_ms":started.elapsed().as_secs_f64()*1000.,"pdf_docx_raw_text_bytes":raw_bytes,"estimated_key_record_bytes":key_bytes,"per_file_deflated_zip_bytes_in_ram":zip_bytes,"formats":formats,"queries":queries}),
    )
}
