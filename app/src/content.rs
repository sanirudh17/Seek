//! On-demand, bounded content reads, with optional account-private document text reuse.
use crate::backend::Row;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::windows::{
        ffi::OsStringExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub const MAX_FILE: u64 = 4 * 1024 * 1024;
const MAX_TEXT: u64 = 8 * 1024 * 1024;

#[derive(Clone, serde::Serialize)]
pub struct Failure {
    pub path: PathBuf,
    pub error: String,
    pub attempts: usize,
}

#[derive(Default, Clone)]
pub struct Report {
    pub cache: crate::text_cache::Stats,
    pub rows: Vec<Row>,
    pub checked: usize,
    pub skipped: usize,
    pub errors: usize,
    pub total: usize,
    pub planning: bool,
    pub finished: bool,
    pub cancelled: bool,
    pub elapsed_ms: f64,
    pub note: Option<String>,
    pub first_result_ms: Option<f64>,
    pub quick: bool,
    pub first_quick_ms: Option<f64>,
    pub quick_matches: usize,
    pub formats: std::collections::BTreeMap<String, (usize, f64)>,
    pub first_failures: Vec<Failure>,
    pub failures: Vec<Failure>,
    pub retried: usize,
    pub recovered: usize,
    pub retry_pending: usize,
}
impl Report {
    pub fn message(&self) -> String {
        if let Some(note) = &self.note {
            return note.clone();
        }
        let mut parts = vec![
            format!("{} matches", self.rows.len()),
            if self.planning {
                format!("Finding files: {} found", self.total)
            } else {
                format!("Searched {} of {} files", self.checked, self.total)
            },
        ];
        if self.skipped > 0 {
            parts.push(format!("{} skipped", self.skipped));
        }
        if self.errors > 0 {
            parts.push(format!("{} unreadable", self.errors));
        }
        if self.retry_pending > 0 {
            parts.push(format!("{} awaiting retry", self.retry_pending));
        }
        if self.retried > 0 {
            parts.push(format!(
                "{} retried, {} recovered",
                self.retried, self.recovered
            ));
        }
        if self.cache.error.is_some() {
            parts.push("Document text cache unavailable; originals searched".into());
        }
        if self.quick && !self.cancelled && !self.finished {
            parts.insert(0, "Quick results (may be incomplete)".into());
        }
        if self.cancelled {
            parts.push("Cancelled".into());
        } else if self.finished {
            parts.push("Complete".into());
        }
        parts.join(" \u{00b7} ")
    }
}
pub fn default_locations() -> Vec<PathBuf> {
    crate::backend::favorites()
        .into_iter()
        .filter(|(name, _)| matches!(name.as_str(), "Downloads" | "Documents"))
        .map(|(_, p)| p)
        .collect()
}
fn folded(path: &Path) -> Vec<u8> {
    let mut value = seek_engine::paths::internal(path);
    value.make_ascii_lowercase();
    value
}
fn within(path: &Path, roots: &[PathBuf]) -> bool {
    let path = folded(path);
    roots.iter().any(|r| {
        let root = folded(r);
        path == root
            || path
                .strip_prefix(root.as_slice())
                .is_some_and(|rest| rest.starts_with(b"/"))
    })
}
pub fn secret(path: &Path) -> bool {
    path.components().any(|part| {
        let name = part.as_os_str().to_string_lossy().to_lowercase();
        let compact: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
        matches!(
            name.as_str(),
            ".ssh"
                | ".gnupg"
                | ".aws"
                | ".azure"
                | ".git"
                | ".password-store"
                | ".npmrc"
                | ".pypirc"
                | "credentials"
                | "secrets"
                | "keychains"
        ) || name.starts_with(".env")
            || name.starts_with("id_rsa")
            || name.starts_with("id_ed25519")
            || name.contains("password")
            || name.contains("secret")
            || name.contains("credential")
            || compact.contains("apikey")
            || compact.contains("privatekey")
            || compact.contains("accesskey")
            || compact.contains("authtoken")
            || [
                "pem", "key", "p12", "pfx", "kdbx", "kdb", "jks", "keystore", "gpg", "pgp",
            ]
            .contains(&name.rsplit('.').next().unwrap_or(""))
    })
}
fn supported(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "txt"
            | "md"
            | "markdown"
            | "rst"
            | "csv"
            | "tsv"
            | "log"
            | "rs"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "cs"
            | "py"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "html"
            | "css"
            | "scss"
            | "json"
            | "toml"
            | "yaml"
            | "yml"
            | "xml"
            | "sql"
            | "sh"
            | "ps1"
            | "bat"
            | "go"
            | "java"
            | "kt"
            | "swift"
            | "rb"
            | "php"
            | "vue"
            | "svelte"
            | "ini"
            | "cfg"
            | "docx"
            | "pdf"
    ) || matches!(
        path.file_name().and_then(|s| s.to_str()),
        Some("README" | "LICENSE" | "Makefile" | "Dockerfile")
    )
}
/// Hydrated OneDrive files use cloud reparse tags too. Reject links and recall
/// placeholders without excluding locally available redirected documents.
pub(crate) fn unavailable(path: &Path, metadata: &std::fs::Metadata) -> bool {
    use windows_sys::Win32::Storage::FileSystem::*;
    let attrs = metadata.file_attributes();
    if attrs & (0x1000 | 0x40000 | 0x400000) != 0 {
        return true;
    }
    if attrs & 0x400 == 0 {
        return false;
    }
    let Ok(file) = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
    else {
        return true;
    };
    let mut info: FILE_ATTRIBUTE_TAG_INFO = unsafe { std::mem::zeroed() };
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            std::mem::size_of_val(&info) as u32,
        )
    } == 0
    {
        return true;
    }
    info.ReparseTag & !0x0000f000 != 0x9000001a // IO_REPARSE_TAG_CLOUD[_1..F]
}
/// Check the actual handle destination and placeholder/link policy before any
/// bytes are read. A renamed/replaced parent cannot widen the configured scope.
fn open_scoped(path: &Path, roots: &[PathBuf], exclusions: &[PathBuf]) -> std::io::Result<File> {
    use windows_sys::Win32::Storage::FileSystem::*;
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > MAX_FILE || unavailable(path, &meta) {
        return Err(std::io::Error::other(
            "Reparse, cloud-only or oversized file",
        ));
    }
    let mut wide = vec![0u16; 32768];
    let count = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            wide.as_mut_ptr(),
            wide.len() as u32,
            FILE_NAME_NORMALIZED,
        )
    };
    if count == 0 || count as usize >= wide.len() {
        return Err(std::io::Error::last_os_error());
    }
    let actual = PathBuf::from(std::ffi::OsString::from_wide(&wide[..count as usize]));
    if !within(&actual, roots) || within(&actual, exclusions) || secret(&actual) {
        return Err(std::io::Error::other(
            "Outside content scope or protected path",
        ));
    }
    Ok(file)
}
fn decode(bytes: &[u8]) -> Result<String, String> {
    crate::text_decode::decode(bytes, false)
}
pub(crate) fn docx(bytes: &[u8], cancelled: &impl Fn() -> bool) -> Result<String, String> {
    use quick_xml::{Reader, events::Event};
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    // Include body, headers, footers, notes and comments; never expand package
    // files onto disk, follow relationships, or load embedded objects.
    let mut names: Vec<String> = archive
        .file_names()
        .filter(|name| {
            *name == "word/document.xml"
                || *name == "word/footnotes.xml"
                || *name == "word/endnotes.xml"
                || *name == "word/comments.xml"
                || (name.starts_with("word/header") || name.starts_with("word/footer"))
                    && name.ends_with(".xml")
        })
        .map(str::to_owned)
        .collect();
    if !names.iter().any(|n| n == "word/document.xml") {
        return Err("Missing DOCX body".into());
    }
    names.sort_by_key(|n| (n != "word/document.xml", n.clone()));
    let mut text = String::new();
    let mut expanded = 0u64;
    for name in names {
        if cancelled() {
            return Err("Cancelled".into());
        }
        let entry = archive.by_name(&name).map_err(|e| e.to_string())?;
        expanded = expanded.saturating_add(entry.size());
        if expanded > MAX_TEXT {
            return Err("Expanded document exceeds 8 MiB".into());
        }
        let mut xml = Vec::new();
        entry
            .take(MAX_TEXT + 1)
            .read_to_end(&mut xml)
            .map_err(|e| e.to_string())?;
        if xml.len() as u64 > MAX_TEXT {
            return Err("Expanded document exceeds 8 MiB".into());
        }
        let mut reader = Reader::from_reader(xml.as_slice());
        let mut in_text = false;
        loop {
            if cancelled() {
                return Err("Cancelled".into());
            }
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    in_text = e.local_name().as_ref() == b"t";
                    if e.local_name().as_ref() == b"tab" {
                        text.push('\t');
                    }
                }
                Ok(Event::End(e)) => {
                    if e.local_name().as_ref() == b"t" {
                        in_text = false;
                    }
                    if e.local_name().as_ref() == b"p" {
                        text.push('\n');
                    }
                }
                Ok(Event::Text(e)) if in_text => {
                    let value = e.decode().map_err(|e| e.to_string())?;
                    text.push_str(&quick_xml::escape::unescape(&value).map_err(|e| e.to_string())?);
                }
                Ok(Event::CData(e)) if in_text => {
                    text.push_str(&e.decode().map_err(|e| e.to_string())?)
                }
                Ok(Event::GeneralRef(e)) if in_text => {
                    let reference = format!("&{};", e.decode().map_err(|e| e.to_string())?);
                    text.push_str(
                        &quick_xml::escape::unescape(&reference).map_err(|e| e.to_string())?,
                    );
                }
                Ok(Event::Empty(e)) if e.local_name().as_ref() == b"br" => text.push('\n'),
                Ok(Event::Empty(e)) if e.local_name().as_ref() == b"tab" => text.push('\t'),
                Ok(Event::Eof) => break,
                Err(e) => return Err(e.to_string()),
                _ => {}
            }
            if text.len() as u64 > MAX_TEXT {
                return Err("Text exceeds 8 MiB".into());
            }
        }
    }
    Ok(text)
}
fn pdf(bytes: Vec<u8>, cancelled: &impl Fn() -> bool) -> Result<String, String> {
    let job = crate::child_job::Job::new().map_err(|e| e.to_string())?;
    let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg("--content-extract-pdf")
        .arg("--content-pdf-job")
        .arg(&job.name)
        .creation_flags(0x08000000)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    let writer = std::thread::spawn(move || input.write_all(&bytes));
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        output.take(MAX_TEXT + 1).read_to_end(&mut out).map(|_| out)
    });
    let deadline = Instant::now() + Duration::from_millis(1000);
    let result = loop {
        if cancelled() || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break Err("PDF timeout or cancelled".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err("PDF text filter unavailable or unsupported PDF".into())
                };
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(e.to_string());
            }
        }
    };
    let _ = writer.join();
    let bytes = reader
        .join()
        .map_err(|_| "PDF output worker failed")?
        .map_err(|e| e.to_string())?;
    result?;
    if bytes.len() as u64 > MAX_TEXT {
        return Err("PDF output exceeds 8 MiB".into());
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}
pub(crate) fn extract(
    path: &Path,
    roots: &[PathBuf],
    exclusions: &[PathBuf],
    cancelled: &impl Fn() -> bool,
) -> Result<String, String> {
    let mut file = open_scoped(path, roots, exclusions).map_err(|e| e.to_string())?;
    let before = file.metadata().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 65536];
    loop {
        if cancelled() {
            return Err("File timeout or cancelled".into());
        }
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..n]);
        if bytes.len() as u64 > MAX_FILE {
            return Err("File exceeds 4 MiB".into());
        }
    }
    let after = file.metadata().map_err(|e| e.to_string())?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err("File changed while reading; rescan".into());
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let document = matches!(ext.as_str(), "docx" | "pdf");
    if document && let Some(text) = crate::text_cache::lookup(path, &before, &bytes) {
        return Ok(text);
    }
    let result = match ext.as_str() {
        "docx" => docx(&bytes, &cancelled),
        "pdf" => pdf(bytes.clone(), &cancelled),
        _ => decode(&bytes),
    };
    if document
        && !cancelled()
        && let Ok(text) = &result
    {
        crate::text_cache::store(path, &before, &bytes, text);
    }
    result
}

fn matching_line(text: &str, needle: &str) -> Option<String> {
    let needle = needle.to_lowercase();
    text.lines().enumerate().find_map(|(i, line)| {
        let lower = line.to_lowercase();
        let offset = lower.find(&needle)?;
        // Locate by character rather than reusing byte offsets from Unicode folding.
        let before = lower[..offset].chars().count();
        let start = before.saturating_sub(50);
        let sample: String = line
            .chars()
            .skip(start)
            .take(180)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        Some(format!(
            "{}: {}{}",
            i + 1,
            if start > 0 { "…" } else { "" },
            sample
        ))
    })
}
#[cfg(test)]
pub fn search(
    query: &str,
    locations: &[PathBuf],
    scope: Option<&Path>,
    exclusions: &[PathBuf],
    show_hidden: bool,
    generation: Arc<AtomicU64>,
    expected: u64,
) -> Report {
    search_progress(
        query,
        locations,
        scope,
        exclusions,
        show_hidden,
        generation,
        expected,
        &mut |_| {},
    )
}
#[allow(clippy::too_many_arguments)]
pub fn search_progress(
    query: &str,
    locations: &[PathBuf],
    scope: Option<&Path>,
    exclusions: &[PathBuf],
    show_hidden: bool,
    generation: Arc<AtomicU64>,
    expected: u64,
    progress: &mut impl FnMut(&Report),
) -> Report {
    let started = Instant::now();
    let _cache = crate::text_cache::begin(locations, exclusions);
    let mut report = Report::default();
    let cancelled = || generation.load(Ordering::Relaxed) != expected;
    if query.trim().is_empty() {
        report.note = Some("Type a word or phrase to search contents.".into());
        return report;
    }
    if locations.is_empty() {
        report.note = Some(
            "Content search is disabled. Choose locations in Settings > Files and search.".into(),
        );
        return report;
    }
    let plan = plan(
        locations,
        scope,
        exclusions,
        show_hidden,
        &cancelled,
        &mut report,
        progress,
    );
    scan_files(
        plan,
        query,
        started,
        &cancelled,
        &mut report,
        progress,
        &mut |p, roots, excludes, cancelled| extract(p, roots, excludes, &|| cancelled()),
    );
    report.cancelled = cancelled();
    report.finished = !report.cancelled;
    report.cache = crate::text_cache::finish();
    report.elapsed_ms = started.elapsed().as_secs_f64() * 1000.;
    report
}

#[allow(clippy::too_many_arguments)]
fn scan_files(
    plan: Plan,
    query: &str,
    started: Instant,
    cancelled: &impl Fn() -> bool,
    report: &mut Report,
    progress: &mut impl FnMut(&Report),
    read: &mut impl FnMut(&Path, &[PathBuf], &[PathBuf], &dyn Fn() -> bool) -> Result<String, String>,
) {
    report.planning = false;
    report.total = plan.files.len();
    let mut last_progress = Instant::now();
    let mut sent_first = false;
    let mut pending: std::collections::VecDeque<_> =
        plan.files.into_iter().map(|(p, m)| (p, m, false)).collect();
    while let Some((path, metadata, retry)) = pending.pop_front() {
        if cancelled() {
            break;
        }
        let file_started = Instant::now();
        let file_cancelled =
            || cancelled() || file_started.elapsed() >= Duration::from_millis(1400);
        // Count every candidate attempted, including unreadable/changed files.
        if retry {
            report.retried += 1;
            report.retry_pending -= 1;
        } else {
            report.checked += 1;
        }
        let text = read(&path, &plan.roots, &plan.excludes, &file_cancelled);
        if cancelled() {
            break;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let profile = report.formats.entry(ext.clone()).or_default();
        profile.0 += 1;
        profile.1 += file_started.elapsed().as_secs_f64() * 1000.;
        match text {
            Ok(text) => {
                if retry {
                    report.recovered += 1;
                }
                if let Some(line) = matching_line(&text, query) {
                    let file = seek_engine::Found {
                        path,
                        kind: seek_engine::walk::KIND_FILE,
                        size: metadata.len(),
                        mtime: metadata
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map_or(0, |t| t.as_secs().min(u32::MAX as u64) as u32),
                        score: 0,
                    };
                    let mut row = Row::new(file);
                    row.snippet = Some(line);
                    row.folder = row.folder.trim_start_matches(r"\\?\").to_owned();
                    report.rows.push(row);
                    report
                        .first_result_ms
                        .get_or_insert_with(|| started.elapsed().as_secs_f64() * 1000.);
                }
            }
            Err(error) if !retry && matches!(ext.as_str(), "pdf" | "docx") => {
                report.first_failures.push(Failure {
                    path: path.clone(),
                    error,
                    attempts: 1,
                });
                report.retry_pending += 1;
                pending.push_back((path, metadata, true));
            }
            Err(error) => {
                report.errors += 1;
                report.failures.push(Failure {
                    path,
                    error,
                    attempts: if retry { 2 } else { 1 },
                });
            }
        }
        // Nonmatching files also advance the status. No global time, entry or
        // match cutoff: only a new generation/cancel ends the full scan early.
        if (!sent_first && !report.rows.is_empty())
            || last_progress.elapsed() >= Duration::from_millis(80)
        {
            if !cancelled() {
                progress(report);
            }
            sent_first |= !report.rows.is_empty();
            last_progress = Instant::now();
        }
    }
}

pub(crate) struct Plan {
    pub files: Vec<(PathBuf, std::fs::Metadata)>,
    pub roots: Vec<PathBuf>,
    pub excludes: Vec<PathBuf>,
}
/// Enumerate metadata before reading contents, so the denominator is real and
/// the read order is globally newest-first. Never follow provider/reparse links.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan(
    locations: &[PathBuf],
    scope: Option<&Path>,
    exclusions: &[PathBuf],
    show_hidden: bool,
    cancelled: &impl Fn() -> bool,
    report: &mut Report,
    progress: &mut impl FnMut(&Report),
) -> Plan {
    let local = |p: &Path| matches!(p.components().next(), Some(std::path::Component::Prefix(p)) if matches!(p.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)));
    let roots: Vec<_> = locations
        .iter()
        .filter(|r| !secret(r) && local(r))
        .filter_map(|r| r.canonicalize().ok())
        .collect();
    report.errors += locations.len().saturating_sub(roots.len());
    for location in locations {
        if !roots
            .iter()
            .any(|r| location.canonicalize().ok().as_ref() == Some(r))
        {
            report.failures.push(Failure {
                path: location.clone(),
                error: "Content location is unavailable or unsupported".into(),
                attempts: 1,
            });
        }
    }
    let excludes: Vec<_> = exclusions
        .iter()
        .map(|r| r.canonicalize().unwrap_or_else(|_| r.clone()))
        .collect();
    let scope = scope.map(|s| s.canonicalize());
    if let Some(Err(error)) = &scope {
        report.errors += 1;
        report.failures.push(Failure {
            path: locations.first().cloned().unwrap_or_default(),
            error: format!("Search scope: {error}"),
            attempts: 1,
        });
    }
    let mut pending: Vec<_> = roots
        .iter()
        .filter_map(|r| match &scope {
            Some(Ok(s)) if within(s, std::slice::from_ref(r)) => Some(s.clone()),
            Some(Ok(s)) if !within(r, std::slice::from_ref(s)) => None,
            Some(Err(_)) => None,
            _ => Some(r.clone()),
        })
        .collect();
    let mut visited = std::collections::HashSet::new();
    let mut files = Vec::new();
    let mut last_progress = Instant::now();
    report.planning = true;
    while let Some(dir) = pending.pop() {
        if cancelled() {
            break;
        }
        if secret(&dir) || within(&dir, &excludes) || !visited.insert(folded(&dir)) {
            report.skipped += 1;
            continue;
        }
        // Validate even root/scope attributes: do not hydrate cloud directories.
        match std::fs::symlink_metadata(&dir) {
            Ok(m) if !unavailable(&dir, &m) => (),
            Ok(_) => {
                report.skipped += 1;
                continue;
            }
            Err(error) => {
                report.errors += 1;
                report.failures.push(Failure {
                    path: dir.clone(),
                    error: error.to_string(),
                    attempts: 1,
                });
                continue;
            }
        }
        let listing = match std::fs::read_dir(&dir) {
            Ok(l) => l,
            Err(error) => {
                report.errors += 1;
                report.failures.push(Failure {
                    path: dir.clone(),
                    error: error.to_string(),
                    attempts: 1,
                });
                continue;
            }
        };
        for entry in listing {
            if cancelled() {
                break;
            }
            if last_progress.elapsed() >= Duration::from_millis(80) {
                report.total = files.len();
                progress(report);
                last_progress = Instant::now();
            }
            let entry = match entry {
                Ok(e) => e,
                Err(error) => {
                    report.errors += 1;
                    report.failures.push(Failure {
                        path: dir.clone(),
                        error: error.to_string(),
                        attempts: 1,
                    });
                    continue;
                }
            };
            let path = entry.path();
            if secret(&path) || within(&path, &excludes) {
                report.skipped += 1;
                continue;
            }
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(error) => {
                    report.errors += 1;
                    report.failures.push(Failure {
                        path: path.clone(),
                        error: error.to_string(),
                        attempts: 1,
                    });
                    continue;
                }
            };
            if unavailable(&path, &metadata)
                || (!show_hidden && metadata.file_attributes() & 6 != 0)
            {
                report.skipped += 1;
                continue;
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() && supported(&path) && metadata.len() <= MAX_FILE {
                files.push((path, metadata));
            } else {
                report.skipped += 1;
            }
        }
    }
    files.sort_unstable_by(|(a, am), (b, bm)| {
        bm.last_write_time()
            .cmp(&am.last_write_time())
            .then_with(|| a.cmp(b))
    });
    Plan {
        files,
        roots,
        excludes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn document_failures_retry_once_after_every_first_attempt() {
        let root = tempfile::tempdir().unwrap();
        let names = ["Transient.pdf", "Broken.docx", "Last.txt"];
        let files = names
            .iter()
            .map(|n| {
                let p = root.path().join(n);
                std::fs::write(&p, "fixture").unwrap();
                let m = std::fs::metadata(&p).unwrap();
                (p, m)
            })
            .collect();
        let plan = Plan {
            files,
            roots: vec![root.path().into()],
            excludes: vec![],
        };
        let mut calls = Vec::new();
        let mut report = Report::default();
        scan_files(
            plan,
            "quartz",
            Instant::now(),
            &|| false,
            &mut report,
            &mut |_| {},
            &mut |p, _, _, _| {
                let name = p.file_name().unwrap().to_str().unwrap().to_owned();
                let count = calls.iter().filter(|n| **n == name).count();
                calls.push(name.clone());
                if name == "Broken.docx" || (name == "Transient.pdf" && count == 0) {
                    Err("Owned extraction failure".into())
                } else {
                    Ok("quartz".into())
                }
            },
        );
        assert_eq!(
            calls,
            [
                "Transient.pdf",
                "Broken.docx",
                "Last.txt",
                "Transient.pdf",
                "Broken.docx"
            ]
        );
        assert_eq!(
            (
                report.checked,
                report.total,
                report.retried,
                report.recovered,
                report.errors,
                report.retry_pending
            ),
            (3, 3, 2, 1, 1, 0)
        );
        assert_eq!(report.first_failures.len(), 2);
        assert_eq!(report.rows.len(), 2);
        assert_eq!(report.failures[0].path.file_name().unwrap(), "Broken.docx");
        assert_eq!(report.failures[0].attempts, 2);
    }
    #[test]
    fn cancellation_does_not_turn_pending_retries_into_unreadable_files() {
        let root = tempfile::tempdir().unwrap();
        let p = root.path().join("Transient.pdf");
        std::fs::write(&p, "owned").unwrap();
        let m = std::fs::metadata(&p).unwrap();
        let plan = Plan {
            files: vec![(p, m)],
            roots: vec![root.path().into()],
            excludes: vec![],
        };
        let cancelled = std::cell::Cell::new(false);
        let mut report = Report::default();
        scan_files(
            plan,
            "quartz",
            Instant::now(),
            &|| cancelled.get(),
            &mut report,
            &mut |_| {},
            &mut |_, _, _, _| {
                cancelled.set(true);
                Err("cancelled".into())
            },
        );
        assert_eq!(report.errors, 0);
        assert_eq!(report.retried, 0);
        assert!(report.failures.is_empty());
    }
    #[test]
    fn complete_scan_passes_old_time_and_match_limits_in_newest_order() {
        let root = tempfile::tempdir().unwrap();
        for n in 0..251 {
            let path = root.path().join(format!("{n:03}.txt"));
            std::fs::write(&path, "quartz").unwrap();
            File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1000 + n))
                .unwrap();
        }
        let mut first = true;
        let result = search_progress(
            "quartz",
            &[root.path().canonicalize().unwrap()],
            None,
            &[],
            true,
            Arc::new(AtomicU64::new(1)),
            1,
            &mut |p| {
                if first && !p.rows.is_empty() {
                    assert_eq!(p.rows[0].name, "250.txt");
                    assert_eq!(p.total, 251);
                    std::thread::sleep(Duration::from_millis(3050));
                    first = false;
                }
            },
        );
        assert_eq!(
            (result.checked, result.total, result.rows.len()),
            (251, 251, 251)
        );
        assert!(result.finished && !result.cancelled && result.elapsed_ms > 3000.);
        assert!(result.message().contains("Searched 251 of 251 files"));
    }
    #[test]
    fn early_matches_are_published_and_a_new_generation_stops_the_scan() {
        let root = tempfile::tempdir().unwrap();
        for n in 0..20 {
            std::fs::write(root.path().join(format!("{n}.txt")), "quartz match").unwrap();
        }
        let generation = Arc::new(AtomicU64::new(1));
        let mut published = 0;
        let run = search_progress(
            "quartz",
            &[root.path().into()],
            None,
            &[],
            true,
            generation.clone(),
            1,
            &mut |partial| {
                assert_eq!(partial.rows.len(), 1);
                assert!(partial.first_result_ms.is_some());
                published += 1;
                generation.store(2, Ordering::Relaxed);
            },
        );
        assert_eq!(published, 1);
        assert!(run.cancelled);
        assert_eq!(run.checked, 1);
    }
    #[test]
    fn secrets_and_keys_are_protected() {
        for name in [
            r"C:\Users\you\.ssh\config",
            r"C:\x\server.pem",
            r"C:\x\id_rsa.pub",
            r"C:\x\.env.production",
            r"C:\x\passwords.txt",
            r"C:\x\.aws\config",
            r"C:\x\Service API key.txt",
            r"C:\x\private_key.json",
        ] {
            assert!(secret(Path::new(name)), "{name}");
        }
        assert!(!secret(Path::new(r"C:\Downloads\report.txt")));
    }
    #[test]
    fn scope_limits_cancellation_unicode_and_actual_lines() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("note.md"),
            "first\nA café QUARTZ marker\nlast",
        )
        .unwrap();
        std::fs::write(root.path().join("secret.txt"), "quartz").unwrap();
        std::fs::write(root.path().join("binary.txt"), b"quartz\0data").unwrap();
        std::fs::write(outside.path().join("outside.txt"), "quartz").unwrap();
        let generation = Arc::new(AtomicU64::new(1));
        let run = search(
            "quartz",
            &[root.path().into()],
            None,
            &[],
            true,
            generation.clone(),
            1,
        );
        assert_eq!(run.rows.len(), 1);
        assert_eq!(
            run.rows[0].snippet.as_deref(),
            Some("2: A café QUARTZ marker")
        );
        assert_eq!(run.errors, 1);
        let run = search(
            "quartz",
            &[root.path().into()],
            Some(outside.path()),
            &[],
            true,
            generation.clone(),
            1,
        );
        assert!(run.rows.is_empty());
        generation.store(2, Ordering::Relaxed);
        let run = search(
            "quartz",
            &[root.path().into()],
            None,
            &[],
            true,
            generation,
            1,
        );
        assert!(run.cancelled);
        assert!(run.rows.is_empty());
    }
    #[test]
    fn docx_runs_entities_and_paragraphs() {
        let mut output = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut output);
            zip.start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(br#"<w:document xmlns:w="word"><w:p><w:r><w:t>quar</w:t></w:r><w:r><w:t>tz &amp; stone</w:t></w:r></w:p><w:p><w:r><w:t>second</w:t></w:r></w:p></w:document>"#).unwrap();
            zip.finish().unwrap();
        }
        let text = docx(output.get_ref(), &|| false).unwrap();
        assert_eq!(text, "quartz & stone\nsecond\n");
    }
    #[test]
    fn junctions_and_exclusions_cannot_widen_the_read_scope() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let external = outside.path().join("external.txt");
        std::fs::write(&external, "quartz outside").unwrap();
        let junction = root.path().join("junction");
        let status = Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(outside.path())
            .creation_flags(0x08000000)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(
            status.success(),
            "Owned junction fixture could not be created"
        );
        let roots = vec![root.path().canonicalize().unwrap()];
        assert!(open_scoped(&junction.join("external.txt"), &roots, &[]).is_err());
        let excluded = root.path().join("private");
        std::fs::create_dir(&excluded).unwrap();
        std::fs::write(excluded.join("excluded.txt"), "quartz").unwrap();
        let run = search(
            "quartz",
            &[root.path().into()],
            None,
            std::slice::from_ref(&excluded),
            true,
            Arc::new(AtomicU64::new(1)),
            1,
        );
        assert!(run.rows.is_empty());
        assert!(run.skipped >= 2);
        assert!(!within(&outside.path().canonicalize().unwrap(), &roots));
        assert!(!within(&root.path().with_extension("sibling"), &roots));
        // Remove only the junction itself; the external target stays intact.
        std::fs::remove_dir(&junction).unwrap();
        assert!(external.exists());
    }
    #[test]
    fn oversized_files_and_expanded_docx_are_reported() {
        let root = tempfile::tempdir().unwrap();
        File::create(root.path().join("huge.txt"))
            .unwrap()
            .set_len(MAX_FILE + 1)
            .unwrap();
        let run = search(
            "quartz",
            &[root.path().into()],
            None,
            &[],
            true,
            Arc::new(AtomicU64::new(1)),
            1,
        );
        assert_eq!(run.skipped, 1);
        assert!(run.rows.is_empty());
        let mut output = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut output);
            zip.start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(&vec![b'a'; MAX_TEXT as usize + 1]).unwrap();
            zip.finish().unwrap();
        }
        assert!(
            docx(output.get_ref(), &|| false)
                .unwrap_err()
                .contains("8 MiB")
        );
    }
}
