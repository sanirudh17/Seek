//! Opportunistic Windows Search results. The direct scan owns the final answer.
use crate::{backend::Row, content};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{BufRead, Read, Write},
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Serialize, Deserialize)]
struct Query {
    term: String,
    paths: Vec<PathBuf>,
}

/// This host exits before any profile/index initialization. Only exact paths
/// approved by the scanner enter its catalog query; it never reads file bodies.
pub fn host() -> i32 {
    let result = (|| -> Result<(), String> {
        let job = crate::argument("--catalog-job").ok_or("Missing catalog job")?;
        crate::child_job::attach_current(&job).map_err(|e| e.to_string())?;
        let mut input = Vec::new();
        std::io::stdin()
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut input)
            .map_err(|e| e.to_string())?;
        if input.len() > 8 * 1024 * 1024 {
            return Err("Catalog request too large".into());
        }
        let query: Query = serde_json::from_slice(&input).map_err(|e| e.to_string())?;
        catalog(&query, |path| {
            let mut out = std::io::stdout().lock();
            serde_json::to_writer(&mut out, &path).map_err(|e| e.to_string())?;
            out.write_all(b"\n")
                .and_then(|_| out.flush())
                .map_err(|e| e.to_string())
        })
    })();
    if result.is_ok() { 0 } else { 1 }
}

fn catalog(
    query: &Query,
    mut publish: impl FnMut(PathBuf) -> Result<(), String>,
) -> Result<(), String> {
    use windows::{
        Win32::System::{Com::*, Variant::VARIANT},
        core::{BSTR, GUID, PCWSTR, w},
    };
    fn call(
        object: &IDispatch,
        name: &str,
        flags: DISPATCH_FLAGS,
        mut args: Vec<VARIANT>,
    ) -> windows::core::Result<VARIANT> {
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let name = PCWSTR(wide.as_ptr());
        let mut id = 0;
        unsafe {
            object.GetIDsOfNames(&GUID::zeroed(), &name, 1, 0, &mut id)?;
        }
        args.reverse();
        let mut put = -3;
        let params = DISPPARAMS {
            rgvarg: args.as_mut_ptr(),
            cArgs: args.len() as u32,
            rgdispidNamedArgs: if flags == DISPATCH_PROPERTYPUT {
                &mut put
            } else {
                std::ptr::null_mut()
            },
            cNamedArgs: u32::from(flags == DISPATCH_PROPERTYPUT),
        };
        let mut value = VARIANT::default();
        unsafe {
            object.Invoke(
                id,
                &GUID::zeroed(),
                0,
                flags,
                &params,
                Some(&mut value),
                None,
                None,
            )?;
        }
        Ok(value)
    }
    let run = || -> windows::core::Result<()> {
        // Reject full-text grammar/punctuation instead of interpreting it as SQL.
        // Unsupported quick terms still receive the complete literal direct scan.
        if query.term.is_empty()
            || !query
                .term
                .chars()
                .all(|c| c.is_alphanumeric() || c.is_whitespace())
        {
            return Ok(());
        }
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        struct Com;
        impl Drop for Com {
            fn drop(&mut self) {
                unsafe {
                    CoUninitialize();
                }
            }
        }
        let _com = Com;
        let connection: IDispatch = unsafe {
            CoCreateInstance(
                &CLSIDFromProgID(w!("ADODB.Connection"))?,
                None,
                CLSCTX_INPROC_SERVER,
            )?
        };
        call(
            &connection,
            "ConnectionTimeout",
            DISPATCH_PROPERTYPUT,
            vec![1i32.into()],
        )?;
        call(
            &connection,
            "CommandTimeout",
            DISPATCH_PROPERTYPUT,
            vec![1i32.into()],
        )?;
        call(
            &connection,
            "Open",
            DISPATCH_METHOD,
            vec!["Provider=Search.CollatorDSO;Extended Properties='Application=Windows';".into()],
        )?;
        let term = format!("\"{}*\"", query.term.trim());
        for batch in query.paths.chunks(32) {
            let allowed = batch
                .iter()
                .map(|p| {
                    format!(
                        "System.ItemPathDisplay='{}'",
                        p.to_string_lossy()
                            .trim_start_matches(r"\\?\")
                            .replace('\'', "''")
                    )
                })
                .collect::<Vec<_>>()
                .join(" OR ");
            if allowed.is_empty() {
                continue;
            }
            let sql = format!(
                "SELECT System.ItemPathDisplay FROM SystemIndex WHERE ({allowed}) AND CONTAINS(System.Search.Contents, '{}')",
                term.replace('\'', "''")
            );
            let rows = IDispatch::try_from(&call(
                &connection,
                "Execute",
                DISPATCH_METHOD,
                vec![sql.as_str().into()],
            )?)?;
            while !bool::try_from(&call(&rows, "EOF", DISPATCH_PROPERTYGET, vec![])?)? {
                let fields =
                    IDispatch::try_from(&call(&rows, "Fields", DISPATCH_PROPERTYGET, vec![])?)?;
                let field = IDispatch::try_from(&call(
                    &fields,
                    "Item",
                    DISPATCH_PROPERTYGET,
                    vec![0i32.into()],
                )?)?;
                let path = BSTR::try_from(&call(&field, "Value", DISPATCH_PROPERTYGET, vec![])?)?
                    .to_string();
                // Never trust a provider to honor the SQL's path constraint.
                let normalized = path.to_lowercase();
                if let Some(approved) = batch.iter().find(|p| {
                    p.to_string_lossy()
                        .trim_start_matches(r"\\?\")
                        .to_lowercase()
                        == normalized
                }) {
                    publish(approved.clone()).map_err(|_| {
                        windows::core::Error::from_hresult(windows::core::HRESULT(
                            0x80004005u32 as i32,
                        ))
                    })?;
                }
                call(&rows, "MoveNext", DISPATCH_METHOD, vec![])?;
            }
            call(&rows, "Close", DISPATCH_METHOD, vec![])?;
        }
        call(&connection, "Close", DISPATCH_METHOD, vec![])?;
        Ok(())
    };
    let mut run = run;
    run().map_err(|e| e.to_string())
}

fn quick(query: Query, cancelled: impl Fn() -> bool, publish: impl Fn(PathBuf) + Send + 'static) {
    let run = || -> Result<(), String> {
        let job = crate::child_job::Job::new().map_err(|e| e.to_string())?;
        let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
            .args(["--content-catalog-host", "--catalog-job", &job.name])
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let mut input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let writer = std::thread::spawn(move || serde_json::to_writer(&mut input, &query));
        let reader = std::thread::spawn(move || {
            for line in std::io::BufReader::new(output.take(8 * 1024 * 1024))
                .lines()
                .map_while(Result::ok)
            {
                if let Ok(path) = serde_json::from_str::<PathBuf>(&line) {
                    publish(path);
                }
            }
        });
        // Poll output separately so a slow provider cannot hold up direct results.
        // The channel below receives each newline immediately, not at host exit.
        let deadline = Instant::now() + Duration::from_secs(2);
        while !cancelled() && Instant::now() < deadline {
            if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = child.kill();
        let _ = child.wait();
        let _ = writer.join();
        let _ = reader.join();
        Ok(())
    };
    let _ = run();
}

#[allow(clippy::too_many_arguments)]
pub fn search(
    query: &str,
    locations: &[PathBuf],
    scope: Option<&std::path::Path>,
    exclusions: &[PathBuf],
    show_hidden: bool,
    generation: Arc<AtomicU64>,
    expected: u64,
    progress: &mut impl FnMut(&content::Report),
) -> content::Report {
    if query.trim().is_empty() || locations.is_empty() {
        return content::search_progress(
            query,
            locations,
            scope,
            exclusions,
            show_hidden,
            generation,
            expected,
            progress,
        );
    }
    let started = Instant::now();
    let cancelled = || generation.load(Ordering::Relaxed) != expected;
    let mut planning = content::Report::default();
    let plan = content::plan(
        locations,
        scope,
        exclusions,
        show_hidden,
        &cancelled,
        &mut planning,
        progress,
    );
    let approved: HashMap<_, _> = plan
        .files
        .iter()
        .map(|(p, m)| (key(p), (p.clone(), m.clone())))
        .collect();
    let paths = plan
        .files
        .iter()
        .take(8192)
        .map(|(p, _)| p.clone())
        .collect();
    enum Event {
        Direct(content::Report),
        Quick(PathBuf),
        Done(content::Report),
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    let cache_policy = crate::text_cache::current();
    std::thread::scope(|threads| {
        let direct_tx = tx.clone();
        let direct_generation = generation.clone();
        threads.spawn(move || {
            let done = crate::text_cache::policy(cache_policy, || {
                content::search_progress(
                    query,
                    locations,
                    scope,
                    exclusions,
                    show_hidden,
                    direct_generation,
                    expected,
                    &mut |report| {
                        let _ = direct_tx.send(Event::Direct(report.clone()));
                    },
                )
            });
            let _ = direct_tx.send(Event::Done(done));
        });
        let quick_tx = tx.clone();
        let quick_generation = generation.clone();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let quick_stop = stop.clone();
        threads.spawn(move || {
            quick(
                Query {
                    term: query.into(),
                    paths,
                },
                || {
                    quick_stop.load(Ordering::Relaxed)
                        || quick_generation.load(Ordering::Relaxed) != expected
                },
                move |path| {
                    let _ = quick_tx.send(Event::Quick(path));
                },
            )
        });
        drop(tx);
        let mut latest = content::Report {
            total: approved.len(),
            ..Default::default()
        };
        let mut provisional = HashMap::new();
        let mut first = None;
        let mut first_quick = None;
        while let Ok(event) = rx.recv() {
            match event {
                Event::Done(mut report) => {
                    stop.store(true, Ordering::Relaxed);
                    report.first_result_ms = first.or(report.first_result_ms);
                    report.first_quick_ms = first_quick;
                    report.quick_matches = provisional.len();
                    report.elapsed_ms = started.elapsed().as_secs_f64() * 1000.;
                    progress(&report);
                    // Drain before scoped join: a sender must never block shutdown.
                    drop(rx);
                    return report;
                }
                Event::Direct(report) => latest = report,
                Event::Quick(path) => {
                    if let Some((path, metadata)) = approved.get(&key(&path)) {
                        first_quick.get_or_insert_with(|| started.elapsed().as_secs_f64() * 1000.);
                        let mtime = metadata
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map_or(0, |t| t.as_secs().min(u32::MAX as u64) as u32);
                        let mut row = Row::new(seek_engine::Found {
                            path: path.clone(),
                            kind: seek_engine::walk::KIND_FILE,
                            size: metadata.len(),
                            mtime,
                            score: 0,
                        });
                        row.snippet = Some("Quick catalog match; awaiting complete scan".into());
                        row.folder = row.folder.trim_start_matches(r"\\?\").to_owned();
                        provisional.insert(key(path), row);
                    }
                }
            }
            let mut view = latest.clone();
            merge(&mut view.rows, &provisional);
            view.quick = !provisional.is_empty();
            view.rows.sort_by(|a, b| {
                b.file
                    .mtime
                    .cmp(&a.file.mtime)
                    .then_with(|| a.file.path.cmp(&b.file.path))
            });
            if !view.rows.is_empty() {
                first.get_or_insert_with(|| started.elapsed().as_secs_f64() * 1000.);
            }
            view.first_result_ms = first;
            if !cancelled() {
                progress(&view);
            }
        }
        latest
    })
}
fn key(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .to_lowercase()
}

fn merge(rows: &mut Vec<Row>, provisional: &HashMap<String, Row>) {
    let present: std::collections::HashSet<_> = rows.iter().map(|r| key(&r.file.path)).collect();
    rows.extend(
        provisional
            .values()
            .filter(|r| !present.contains(&key(&r.file.path)))
            .cloned(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(path: &str, snippet: &str) -> Row {
        let mut row = Row::new(seek_engine::Found {
            path: path.into(),
            kind: seek_engine::walk::KIND_FILE,
            size: 1,
            mtime: 1,
            score: 0,
        });
        row.snippet = Some(snippet.into());
        row
    }
    #[test]
    fn quick_aliases_deduplicate_and_verified_line_wins() {
        let verified = row(r"\\?\C:\Scope\File.docx", "12: literal needle");
        let quick = row(r"c:/scope/file.docx", "Quick catalog match");
        let mut rows = vec![verified];
        merge(&mut rows, &HashMap::from([(key(&quick.file.path), quick)]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].snippet.as_deref(), Some("12: literal needle"));
    }
    #[test]
    fn provisional_only_matches_cannot_enter_the_complete_answer() {
        let quick = row(r"C:\Scope\Stale.pdf", "Quick catalog match");
        let provisional = HashMap::from([(key(&quick.file.path), quick)]);
        let mut view = content::Report::default();
        merge(&mut view.rows, &provisional);
        view.quick = true;
        assert!(view.message().contains("Quick results (may be incomplete)"));
        let final_direct = content::Report {
            finished: true,
            ..Default::default()
        };
        assert!(final_direct.rows.is_empty());
        assert!(final_direct.message().contains("Complete"));
        assert!(!final_direct.message().contains("Quick results"));
    }
}
