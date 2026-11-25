//! Finite local preview diagnostics. No timer, watcher, network or resident worker.
use std::{
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub const PROTOCOL: u32 = 1;
pub const COLD_TIMEOUT: Duration = Duration::from_secs(15);
pub const NORMAL_TIMEOUT: Duration = Duration::from_secs(4);
pub const UI_WATCHDOG: Duration = Duration::from_secs(35);
static LOCK: Mutex<()> = Mutex::new(());
static MEMORY: Mutex<Vec<String>> = Mutex::new(Vec::new());
static HOST: OnceLock<(Instant, u64)> = OnceLock::new();
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn path() -> PathBuf {
    #[cfg(test)]
    let data = std::env::current_dir()
        .unwrap()
        .join(".bench-data")
        .join(format!("preview-log-tests-{}", std::process::id()));
    #[cfg(not(test))]
    let data = crate::argument("--data")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Seek")
        });
    data.join("logs/preview-host.log")
}
fn append_at(path: &Path, text: &str, limit: u64) -> std::io::Result<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    if fs::metadata(path).is_ok_and(|m| m.len() >= limit) {
        for index in (1..=3).rev() {
            let target = path.with_extension(format!("log.{index}"));
            let source = if index == 1 {
                path.to_path_buf()
            } else {
                path.with_extension(format!("log.{}", index - 1))
            };
            if target.exists() {
                fs::remove_file(&target)?;
            }
            if source.exists() {
                fs::rename(source, target)?;
            }
        }
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(text.as_bytes())
}
pub fn record(stage: &str, detail: serde_json::Value) {
    let line = format!(
        "{}\n",
        serde_json::json!({"at_ms":now_ms(),"stage":stage,"version":env!("CARGO_PKG_VERSION"),"protocol":PROTOCOL,"pid":std::process::id(),"detail":detail})
    );
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut memory = MEMORY.lock().unwrap_or_else(|e| e.into_inner());
    if memory.len() >= 32 {
        memory.remove(0);
    }
    memory.push(line.clone());
    if let Err(error) = append_at(&path(), &line, 512 * 1024) {
        memory.push(format!("Diagnostic log could not be written: {error}\n"));
    }
    let excess = memory.len().saturating_sub(32);
    memory.drain(..excess);
}
pub fn stderr() -> Stdio {
    record("host_launch", serde_json::json!({"memory_limit_mib":256}));
    match OpenOptions::new().create(true).append(true).open(path()) {
        Ok(file) => Stdio::from(file),
        Err(_) => Stdio::null(), // The in-memory copy reports an unwritable log.
    }
}
pub fn snapshot() -> String {
    let mut text = format!(
        "Seek {} / preview protocol {}\nLog: {}\n",
        env!("CARGO_PKG_VERSION"),
        PROTOCOL,
        path().display()
    );
    if let Ok(mut file) = fs::File::open(path()) {
        let len = file.metadata().map_or(0, |m| m.len());
        let _ = file.seek(SeekFrom::Start(len.saturating_sub(24 * 1024)));
        let mut bytes = Vec::new();
        let _ = file.take(24 * 1024).read_to_end(&mut bytes);
        text.push_str(&String::from_utf8_lossy(&bytes));
    } else {
        text.push_str(&MEMORY.lock().unwrap_or_else(|e| e.into_inner()).join(""));
    }
    text
}
pub fn host_error(pid: u32, fallback: &str) -> String {
    snapshot()
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|v| v["pid"].as_u64() == Some(pid as u64) && v["stage"] == "host_error")
        .and_then(|v| v["detail"]["error"].as_str().map(str::to_owned))
        .unwrap_or_else(|| fallback.to_owned())
}
pub fn host_stage(stage: &str, detail: serde_json::Value) {
    let (start, _) = HOST.get_or_init(|| (Instant::now(), now_ms()));
    let line = format!(
        "{}\n",
        serde_json::json!({"stage":stage,"pid":std::process::id(),"version":env!("CARGO_PKG_VERSION"),"protocol":PROTOCOL,"elapsed_ms":start.elapsed().as_secs_f64()*1000.,"detail":detail})
    );
    let _ = std::io::stderr().lock().write_all(line.as_bytes());
}
pub fn host_started_ms() -> u64 {
    HOST.get().map_or(0, |(_, time)| *time)
}
pub fn validate(version: &str, protocol: u32) -> Result<(), String> {
    if version == env!("CARGO_PKG_VERSION") && protocol == PROTOCOL {
        return Ok(());
    }
    Err(format!(
        "Preview host version mismatch: app {} / protocol {}, host {} / protocol {}. Restart Seek after completing the update.",
        env!("CARGO_PKG_VERSION"),
        PROTOCOL,
        if version.is_empty() {
            "unreported"
        } else {
            version
        },
        protocol
    ))
}
#[derive(Default)]
pub struct Budget {
    attempted: bool,
}
impl Budget {
    pub fn next(&mut self) -> Duration {
        let timeout = if self.attempted {
            NORMAL_TIMEOUT
        } else {
            COLD_TIMEOUT
        };
        self.attempted = true;
        timeout
    }
}
pub fn recover<T>(
    mut attempt: impl FnMut(Duration) -> Result<T, String>,
    budget: &mut Budget,
    cancelled: &dyn Fn() -> bool,
) -> Result<T, String> {
    for index in 0..2 {
        if cancelled() {
            return Err("Cancelled".into());
        }
        let timeout = budget.next();
        let start = Instant::now();
        let result = attempt(timeout);
        record(
            "result",
            serde_json::json!({"attempt":index+1,"budget_ms":timeout.as_millis(),"elapsed_ms":start.elapsed().as_secs_f64()*1000.,"success":result.is_ok(),"error":result.as_ref().err()}),
        );
        match result {
            Err(error)
                if index == 0
                    && !cancelled()
                    && !error.contains("version mismatch")
                    && error != "Cancelled" => {}
            value => return value,
        }
    }
    unreachable!()
}
/// Explicit, hidden diagnostic mode; never starts the index or the GPUI app.
pub fn self_test() -> i32 {
    let result = (|| -> Result<(), String> {
        let file = match crate::argument("--document-file") {
            Some(file) => PathBuf::from(file),
            None => {
                let folder = path().parent().unwrap().join(format!(
                    "self-test-{}-{}",
                    std::process::id(),
                    now_ms()
                ));
                fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                let file = folder.join("owned-preview-self-test.pdf");
                fs::write(&file, self_test_pdf()).map_err(|e| e.to_string())?;
                file
            }
        };
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        println!(
            "{}",
            serde_json::json!({"stage":"self_test","version":env!("CARGO_PKG_VERSION"),"protocol":PROTOCOL,"executable_bytes":fs::metadata(executable).map_err(|e|e.to_string())?.len(),"log":path(),"cold_allowance_ms":COLD_TIMEOUT.as_millis(),"normal_allowance_ms":NORMAL_TIMEOUT.as_millis()})
        );
        let mut loader = crate::pdf_session::Loader::default();
        for (index, edge) in [768, 768, 1536].into_iter().enumerate() {
            let key = crate::document_preview::Key {
                file: crate::media::Key {
                    path: file.clone(),
                    mtime: 0,
                    size: fs::metadata(&file).map_err(|e| e.to_string())?.len(),
                    edge,
                    details: false,
                    text_preview: true,
                    folder: false,
                },
                page: 0,
            };
            let request_wall = now_ms();
            let start = Instant::now();
            let preview = loader.load(&key, &|| false)?;
            println!(
                "{}",
                serde_json::json!({"stage":"preview_result","request":index+1,"edge":edge,"success":true,"pages":preview.info.pages,"width":preview.info.width,"height":preview.info.height,"elapsed_ms":start.elapsed().as_secs_f64()*1000.,"process_entry_ms":(preview.info.host_started_ms>=request_wall).then(||preview.info.host_started_ms-request_wall),"scope":"process entry uses wall clock; render worker, excludes UI paint; not a cache-flushed launch"})
            );
        }
        loader.clear();
        println!("{}", serde_json::json!({"stage":"cleanup","success":true}));
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(error) => {
            record("self_test_error", serde_json::json!({"error":error}));
            println!(
                "{}",
                serde_json::json!({"stage":"self_test_error","error":error,"log":path()})
            );
            1
        }
    }
}
fn self_test_pdf() -> Vec<u8> {
    let text = "BT /F1 18 Tf 40 100 Td (Seek owned preview self-test) Tj ET";
    let objects=["<< /Type /Catalog /Pages 2 0 R >>".into(),"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".into(),"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R >>".into(),"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R >>".into(),"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),format!("<< /Length {} >>\nstream\n{}\nendstream",text.len(),text)];
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 7\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    bytes
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handshake_rejects_missing_stale_or_incompatible_hosts() {
        assert!(validate(env!("CARGO_PKG_VERSION"), PROTOCOL).is_ok());
        for (version, protocol) in [
            ("", 0),
            ("0.0.0", PROTOCOL),
            (env!("CARGO_PKG_VERSION"), PROTOCOL + 1),
        ] {
            assert!(
                validate(version, protocol)
                    .unwrap_err()
                    .contains("version mismatch")
            );
        }
        let mut old = serde_json::to_value(crate::document_preview::Wire {
            text: Some("owned".into()),
            ..Default::default()
        })
        .unwrap();
        old.as_object_mut().unwrap().remove("version");
        old.as_object_mut().unwrap().remove("protocol");
        let mut bytes = serde_json::to_vec(&old).unwrap();
        bytes.push(b'\n');
        assert!(
            crate::document_preview::decode_wire(bytes)
                .err()
                .unwrap()
                .contains("version mismatch")
        );
    }
    #[test]
    fn cold_allowance_retry_and_cancel_are_finite() {
        let mut budget = Budget::default();
        let mut calls = vec![];
        let value = recover(
            |timeout| {
                calls.push(timeout);
                if calls.len() == 1 {
                    Err("Preview timed out".into())
                } else {
                    Ok(7)
                }
            },
            &mut budget,
            &|| false,
        )
        .unwrap();
        assert_eq!(value, 7);
        assert_eq!(calls, vec![COLD_TIMEOUT, NORMAL_TIMEOUT]);
        assert_eq!(budget.next(), NORMAL_TIMEOUT);
        let mut calls = 0;
        let result: Result<(), String> = recover(
            |_| {
                calls += 1;
                Err("broken".into())
            },
            &mut budget,
            &|| false,
        );
        assert!(result.is_err());
        assert_eq!(calls, 2);
        let result: Result<(), String> =
            recover(|_| panic!("Cancelled work ran"), &mut budget, &|| true);
        assert_eq!(result.unwrap_err(), "Cancelled");
        let mut calls = 0;
        let _: Result<(), String> = recover(
            |_| {
                calls += 1;
                Err("version mismatch".into())
            },
            &mut budget,
            &|| false,
        );
        assert_eq!(calls, 1);
    }
    #[test]
    fn rotation_is_bounded_and_does_not_touch_unrelated_files() {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("preview-host.log");
        let sentinel = temp.path().join("saved-data");
        fs::write(&sentinel, b"keep").unwrap();
        for _ in 0..20 {
            append_at(&log, "123456789\n", 10).unwrap();
        }
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 5);
        assert_eq!(fs::read(sentinel).unwrap(), b"keep");
        for index in 1..=3 {
            assert_eq!(
                fs::metadata(log.with_extension(format!("log.{index}")))
                    .unwrap()
                    .len(),
                10
            );
        }
    }
}
