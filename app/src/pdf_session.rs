//! Reuse one loaded PDF while paging. The host is selected-document-only,
//! has the existing job/time/memory bounds, and exits after two idle seconds.
use crate::{
    document_preview::{Key, MAX_FILE, Preview, Wire, decode_wire},
    preview_cache::Stamp,
};
use std::{
    io::{Read, Write},
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};
#[derive(Default)]
pub struct Loader {
    session: Option<Session>,
}
impl Loader {
    pub fn clear(&mut self) {
        self.session = None;
    }
    pub fn load(&mut self, key: &Key, cancelled: &dyn Fn() -> bool) -> Result<Preview, String> {
        if key
            .file
            .path
            .extension()
            .is_none_or(|s| !s.eq_ignore_ascii_case("pdf"))
        {
            self.clear();
            return crate::document_preview::load(key, cancelled);
        }
        if !self.session.as_mut().is_some_and(|s| s.valid(key)) {
            self.clear();
            self.session = Some(Session::start(key)?);
        }
        let result = self.session.as_mut().unwrap().page(key, cancelled);
        match result {
            Err(ref e) if e == "Preview renderer closed" && !cancelled() => {
                self.clear();
                self.session = Some(Session::start(key)?);
                let result = self.session.as_mut().unwrap().page(key, cancelled);
                if result.is_err() {
                    self.clear();
                }
                result
            }
            Err(e) => {
                self.clear();
                Err(e)
            }
            Ok(value) => Ok(value),
        }
    }
}
struct Session {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
    _job: crate::child_job::Job,
    key: Key,
    stamp: Stamp,
}
impl Session {
    fn start(key: &Key) -> Result<Self, String> {
        if key.file.size > MAX_FILE {
            return Err("File exceeds the 32 MiB preview limit".into());
        }
        let stamp = Stamp::read(key).ok_or("Document unavailable")?;
        let job = crate::child_job::Job::new().map_err(|e| e.to_string())?;
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        #[cfg(test)]
        let executable = std::env::var_os("SEEK_TEST_PREVIEW_HOST")
            .map(std::path::PathBuf::from)
            .unwrap_or(executable);
        let mut child = Command::new(executable)
            .args([
                "--document-preview-host",
                "--document-session",
                "--document-job",
            ])
            .arg(&job.name)
            .arg("--document-file")
            .arg(&key.file.path)
            .args(["--document-page", "0"])
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let input = child.stdin.take().ok_or("Missing preview input")?;
        let output = child.stdout.take().ok_or("Missing preview output")?;
        Ok(Self {
            child,
            input,
            output,
            _job: job,
            key: key.clone(),
            stamp,
        })
    }
    fn valid(&mut self, key: &Key) -> bool {
        self.key.file.path == key.file.path
            && Stamp::read(key).as_ref() == Some(&self.stamp)
            && self.child.try_wait().is_ok_and(|s| s.is_none())
    }
    fn page(&mut self, key: &Key, cancelled: &dyn Fn() -> bool) -> Result<Preview, String> {
        writeln!(self.input, "{} {}", key.page, key.file.edge)
            .and_then(|_| self.input.flush())
            .map_err(|_| "Preview renderer closed")?;
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 16384];
        let mut expected = None;
        loop {
            if cancelled() {
                return Err("Cancelled".into());
            }
            if Instant::now() >= deadline {
                return Err("Preview timed out".into());
            }
            let mut available = 0;
            let ready = unsafe {
                PeekNamedPipe(
                    self.output.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                )
            };
            if ready != 0 && available > 0 {
                let n = (available as usize).min(buffer.len());
                let read = self
                    .output
                    .read(&mut buffer[..n])
                    .map_err(|_| "Preview renderer closed")?;
                bytes.extend_from_slice(&buffer[..read]);
                if bytes.len() > 16 * 1024 * 1024 {
                    return Err("Preview output exceeds limit".into());
                }
                if expected.is_none()
                    && let Some(split) = bytes.iter().position(|b| *b == b'\n')
                {
                    if split > 4096 {
                        return Err("Invalid preview header".into());
                    }
                    let info: Wire =
                        serde_json::from_slice(&bytes[..split]).map_err(|e| e.to_string())?;
                    if let Some(error) = info.error {
                        return Err(error);
                    }
                    if info.width > 1536 || info.height > 1536 {
                        return Err("Invalid preview size".into());
                    }
                    expected = Some(split + 1 + info.width as usize * info.height as usize * 4);
                }
                if expected.is_some_and(|n| bytes.len() >= n) {
                    return decode_wire(bytes);
                }
            } else {
                if self.child.try_wait().map_err(|e| e.to_string())?.is_some() {
                    return Err("Preview renderer closed".into());
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn PeekNamedPipe(
        pipe: *mut std::ffi::c_void,
        buffer: *mut std::ffi::c_void,
        size: u32,
        read: *mut u32,
        available: *mut u32,
        left: *mut u32,
    ) -> i32;
}
pub fn serve(bytes: &[u8]) -> anyhow::Result<()> {
    use std::{
        io::BufRead,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
    };
    use windows::{Data::Pdf::PdfDocument, Storage::Streams::*};
    let input = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&input.GetOutputStreamAt(0)?)?;
    writer.WriteBytes(bytes)?;
    writer.StoreAsync()?.join()?;
    writer.DetachStream()?;
    let document = PdfDocument::LoadFromStreamAsync(&input)?.join()?;
    let start = Instant::now();
    let active = Arc::new(AtomicBool::new(false));
    let last = Arc::new(AtomicU64::new(0));
    let busy = active.clone();
    let touched = last.clone();
    std::thread::spawn(move || {
        loop {
            let idle = (start.elapsed().as_millis() as u64)
                .saturating_sub(touched.load(Ordering::Acquire));
            if !busy.load(Ordering::Acquire) && idle >= 2000 {
                std::process::exit(0);
            }
            std::thread::sleep(Duration::from_millis(if idle < 2000 {
                2000 - idle
            } else {
                50
            }));
        }
    });
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        anyhow::ensure!(line.len() <= 64, "Invalid page request");
        active.store(true, Ordering::Release);
        let values: Vec<_> = line.split_whitespace().collect();
        anyhow::ensure!(values.len() == 2, "Invalid page request");
        let page = values[0].parse::<u32>()?;
        let edge = values[1].parse::<u16>()?.clamp(256, 1536);
        let (info, pixels) = match crate::document_preview::render_loaded_pdf(&document, page, edge)
        {
            Ok(value) => value,
            Err(error) => (
                Wire {
                    source: "PDF".into(),
                    page,
                    error: Some(error.to_string()),
                    ..Default::default()
                },
                vec![],
            ),
        };
        {
            let mut out = std::io::stdout().lock();
            serde_json::to_writer(&mut out, &info)?;
            out.write_all(b"\n")?;
            out.write_all(&pixels)?;
            out.flush()?;
        }
        last.store(start.elapsed().as_millis() as u64, Ordering::Release);
        active.store(false, Ordering::Release);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Explicit read-only selected PDF and built host required"]
    fn selected_pdf_reuses_host_then_frees_it_when_idle() {
        let path = std::path::PathBuf::from(std::env::var_os("SEEK_TEST_PREVIEW_PDF").unwrap());
        let mut key = Key {
            file: crate::media::Key {
                size: std::fs::metadata(&path).unwrap().len(),
                path,
                mtime: 0,
                edge: 1536,
                details: false,
                folder: false,
            },
            page: 0,
        };
        let mut loader = Loader::default();
        let start = Instant::now();
        let first = loader.load(&key, &|| false).unwrap();
        assert!(first.info.pages > 2);
        let first_ms = start.elapsed().as_secs_f64() * 1000.;
        let pid = loader.session.as_ref().unwrap().child.id();
        let mut times = vec![];
        for page in [1, 2, 1, 0] {
            key.page = page;
            let started = Instant::now();
            let value = loader.load(&key, &|| false).unwrap();
            times.push(started.elapsed().as_secs_f64() * 1000.);
            assert_eq!(value.info.page, page);
            assert_eq!(loader.session.as_ref().unwrap().child.id(), pid);
        }
        eprintln!(
            "Enlarged selected PDF session: first {first_ms:.3} ms; uncached page turns {times:?} ms, same host; excludes UI paint"
        );
        std::thread::sleep(Duration::from_millis(2400));
        assert!(
            loader
                .session
                .as_mut()
                .unwrap()
                .child
                .try_wait()
                .unwrap()
                .is_some(),
            "Idle native renderer must exit automatically"
        );
        let _ = loader.load(&key, &|| false).unwrap();
        assert_ne!(loader.session.as_ref().unwrap().child.id(), pid);
        loader.clear();
        assert!(loader.session.is_none());
    }
}
