use seek_engine::{Engine, Found, Options, Query, Status};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Instant,
};
use windows_sys::Win32::{
    Storage::FileSystem::*,
    UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
};

#[derive(Clone)]
pub struct Row {
    pub file: Found,
    pub name: String,
    pub folder: String,
    pub modified: String,
    pub size: String,
    pub kind: String,
    pub highlights: Vec<std::ops::Range<usize>>,
    pub sort_name: Vec<u8>,
    pub snippet: Option<String>,
}
#[derive(Clone, Default)]
pub struct Request {
    pub text: String,
    pub scope: Option<PathBuf>,
    pub show_hidden: bool,
    pub exclusions: Vec<PathBuf>,
    pub expanded: Vec<PathBuf>,
    pub generation: u64,
    pub started: Option<Instant>,
    pub native_input: bool,
    pub largest: Option<bool>,
    pub content: bool,
    pub content_locations: Vec<PathBuf>,
    pub remember_document_text: bool,
}
impl Request {
    fn same_view(&self, other: &Self) -> bool {
        self.text == other.text
            && self.scope == other.scope
            && self.show_hidden == other.show_hidden
            && self.exclusions == other.exclusions
            && self.largest == other.largest
            && self.content == other.content
            && self.content_locations == other.content_locations
            && self.remember_document_text == other.remember_document_text
    }
}
pub enum Command {
    Search(Request),
    Changed,
    CancelContent(u64),
    ClearDocumentCache(u64),
    BrowseFinished(PathBuf, u64, Result<Vec<Found>, String>),
    Open(PathBuf, bool),
    Stop,
    HelperStatus,
    HelperAction(bool),
    HelperFinished(Result<(), String>),
    Rebuild,
    RefreshParents(Vec<PathBuf>),
    FilesRemoved(Vec<PathBuf>),
}
#[derive(Clone, Default)]
pub struct HelperState {
    pub installed: bool,
    pub busy: bool,
    pub message: Option<String>,
}
pub struct Update {
    pub content_failures: Vec<crate::content::Failure>,
    pub rows: Vec<Row>,
    pub status: Status,
    pub generation: u64,
    pub message: Option<String>,
    pub started: Option<Instant>,
    pub native_input: bool,
    pub favorites: Vec<(String, PathBuf)>,
    pub roots: Vec<PathBuf>,
    pub helper: HelperState,
    pub children: std::collections::HashMap<PathBuf, Vec<Row>>,
    pub cache_epoch: u64,
    pub progress_only: bool,
}
#[derive(Clone)]
pub struct Backend {
    pub tx: mpsc::Sender<Command>,
    pub generation: Arc<AtomicU64>,
}

pub fn start(
    root: Option<PathBuf>,
    data: PathBuf,
    source: seek_engine::SourceMode,
    locations: Vec<PathBuf>,
) -> (
    Backend,
    async_channel::Receiver<Update>,
    thread::JoinHandle<()>,
) {
    let (tx, rx) = mpsc::channel();
    let (result_tx, result_rx) = async_channel::bounded(1);
    let generation = Arc::new(AtomicU64::new(0));
    let backend = Backend {
        tx: tx.clone(),
        generation: generation.clone(),
    };
    let worker = thread::Builder::new()
        .name("seek-search".into())
        .spawn(move || {
            crate::metrics::phase("search_worker_started");
            let roots = root.map_or_else(|| if locations.is_empty(){drives()}else{locations}, |r| vec![std::fs::canonicalize(&r).unwrap_or(r)]);
            let favorites = favorites();
            crate::metrics::phase("known_folders_resolved");
            let engine = match Engine::start_with_source(Options {
                dir: data.clone(),
                roots: roots.clone(),
            }, source) {
                Ok(engine) => engine,
                Err(error) => {
                    let _ = result_tx.force_send(Update {
                        content_failures:vec![],
                        rows: vec![],
                        status: Status {
                            error: Some(error),
                            ..Default::default()
                        },
                        generation: 0,
                        message: None,
                        started: None,
                        native_input: false,
                        favorites,
                        roots,
                        helper:HelperState::default(),
                        children: Default::default(),
                        cache_epoch:0,progress_only:false,
                    });
                    return;
                }
            };
            let notify=tx.clone();
            let changed = Arc::new(AtomicBool::new(false));
            let notify_changed = changed.clone();
            engine.set_notifier(move || {
                if !notify_changed.swap(true, Ordering::Relaxed) {
                    let _ = notify.send(Command::Changed);
                }
            });
            let mut helper=HelperState::default();
            let mut request = Request::default();
            let mut first_cached_search = true;
            let mut recent_files = Vec::<Found>::new();
            let mut cache_epoch=0u64;
            let mut content_cache: Option<(Vec<Row>, String)> = None;
            let mut content_failures = Vec::new();
            let mut removed_files=Vec::<Found>::new();
            let mut browse = crate::browse::Cache::new(tx.clone());
            let mut startup_locations=favorites.iter().filter(|(_,folder)|roots.iter().any(|root|crate::file_changes::under_any(folder,std::slice::from_ref(root)))).cloned().collect::<Vec<_>>();
            startup_locations.extend(roots.iter().map(|root|(root.display().to_string(),root.clone())));
            browse.prime(&startup_locations);

            while let Ok(first) = rx.recv() {
                let mut command = Some(first);
                let mut message = None;
                loop {
                    match command.take().unwrap() {
                        Command::Stop => {
                            return;
                        }
                        Command::CancelContent(cancel_generation) => { if generation.load(Ordering::Relaxed)==cancel_generation {request.generation=cancel_generation;} },
                        Command::ClearDocumentCache(clear_generation) => {
                            request.generation=clear_generation;
                            let summary=match crate::text_cache::clear(&crate::text_cache::Config::new(&data,false)){Ok(())=>"Saved document text cleared.".to_owned(),Err(error)=>format!("Cannot clear saved document text: {error}")};
                            if let Some((_,old))=&mut content_cache{*old=summary.clone();}else if request.content{content_cache=Some((vec![],summary.clone()));}
                            message=Some(summary);
                        },
                        Command::Search(next) => {
                            content_failures.clear();
                            if (request.content_locations!=next.content_locations || request.remember_document_text!=next.remember_document_text || request.exclusions!=next.exclusions)
                                && let Err(error)=crate::text_cache::maintain(&crate::text_cache::Config::new(&data,next.remember_document_text),&next.content_locations,&next.exclusions){eprintln!("Cannot maintain document cache: {error}");}
                            content_cache = None;
                            if !next.content && next.text.trim().is_empty() && next.largest.is_none() && let Some(folder)=next.scope.as_ref(){browse.refresh(folder);}
                            if !request.same_view(&next) { recent_files.clear(); }
                            request = next;
                        },
                        Command::BrowseFinished(folder, revision, result) => { browse.finish(folder, revision, result); },
                        Command::Changed => {changed.store(false, Ordering::Relaxed); cache_epoch=cache_epoch.wrapping_add(1);if let Some(folder)=&request.scope{browse.refresh(folder);}},
                        Command::Rebuild => { engine.rebuild(); },
                        Command::FilesRemoved(paths)=>{
                            content_cache=None;
                            for path in paths {
                                if !removed_files.iter().any(|f|f.path==path){removed_files.push(Found{path:path.clone(),kind:0,size:0,mtime:0,score:0});}
                                if let Some(parent)=path.parent(){browse.invalidate(parent);browse.refresh(parent);}
                                engine.refresh_parent(&path);
                            }
                        },
                        Command::RefreshParents(paths)=>{if !paths.is_empty(){content_cache=None;}for path in paths{browse.invalidate(&path);engine.refresh_parent(&path.join(".seek-refresh"));}},
                        Command::HelperStatus=>{
                            match seek_engine::helper::user_sid().and_then(|sid|seek_engine::helper::service_present(&sid)) {Ok(installed)=>helper.installed=installed,Err(e)=>helper.message=Some(e.to_string())}
                        },
                        Command::HelperAction(install)=>{
                            if !helper.busy{
                                helper.busy=true;helper.message=Some("Waiting for administrator approval…".into());let reply=tx.clone();
                                thread::spawn(move ||{let _=reply.send(Command::HelperFinished(helper_action(install)));});
                            }
                        },
                        Command::HelperFinished(result)=>{
                            helper.busy=false;
                            helper.installed=seek_engine::helper::user_sid().and_then(|sid|seek_engine::helper::service_present(&sid)).unwrap_or(false);
                            helper.message=Some(match result{Ok(())=>if helper.installed{"Helper installed. Close and reopen Seek to enable fast indexing.".into()}else{"Helper removed. Standard indexing remains available.".into()},Err(e)=>e});
                        },
                        Command::Open(path, reveal) => match std::fs::metadata(&path) {
                            Ok(_) => {
                                let result = shell_open(&path, reveal);
                                #[cfg(feature = "benchmarks")]
                                crate::metrics::interaction("shell_dispatch", serde_json::json!({"path": path, "reveal": reveal, "accepted": result.is_ok()}));
                                if let Err(error) = result {
                                    message = Some(error);
                                } else if !reveal {
                                    engine.record_open(&path);
                                    crate::media::record_open(&data, &path);
                                    cache_epoch=cache_epoch.wrapping_add(1);
                                }
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                                #[cfg(feature = "benchmarks")]
                                crate::metrics::interaction("stale_open", serde_json::json!({"path": path}));
                                engine.refresh_parent(&path);
                                content_cache = None;
                                message =
                                    Some("That file no longer exists. Updating results…".into());
                            }
                            Err(error) => message = Some(format!("Cannot open this file: {error}")),
                        },
                    }
                    match rx.try_recv() {
                        Ok(next) => command = Some(next),
                        Err(_) => break,
                    }
                }
                let status = engine.status();
                if status.ready && first_cached_search {
                    crate::metrics::phase("cached_search_started");
                }
                let work_started = Instant::now();
                let mut dates = std::collections::HashMap::new();
                let mut rows = if !request.content && request.text.trim().is_empty() && request.largest.is_none() && let Some(folder)=request.scope.as_ref() {
                    // A direct folder listing is independent of a long catch-up index write lock.
                    // Keep its cached rows while a finite refresh validates this visit.
                    let files=browse.get(folder,request.show_hidden,&request.exclusions);
                    match files {
                        Some(Ok(files))=>{
                            message=Some(if files.is_empty(){"This folder has no visible items.".into()}else{"Folder contents".into()});
                            let mut files=files;files.sort_unstable_by(|a,b|b.mtime.cmp(&a.mtime).then(a.path.cmp(&b.path)));
                            let stable=retain_browse_order(&recent_files,files);recent_files=stable.clone();stable.into_iter().map(|file| Row::with_dates(file, &mut dates)).collect()
                        },
                        Some(Err(error))=>{message=Some(format!("Folder unavailable: {error}"));vec![]},
                        None=>{message=Some("Indexing this folder...".into());vec![]},
                    }
                } else if request.content {
                    if content_cache.is_none() {
                        let mut published_rows=0;
                        let report = crate::text_cache::policy(Some(crate::text_cache::Config::new(&data,request.remember_document_text)), || crate::content_catalog::search(&request.text, &request.content_locations, request.scope.as_deref(), &request.exclusions, request.show_hidden, generation.clone(), request.generation, &mut |partial| {
                            #[cfg(feature="benchmarks")]
                            crate::metrics::interaction("content_partial", serde_json::json!({"matches":partial.rows.len(),"checked":partial.checked,"elapsed_ms":request.started.map(|t|t.elapsed().as_secs_f64()*1000.)}));
                            use std::hash::{Hash, Hasher};
                            let mut signature=std::collections::hash_map::DefaultHasher::new();
                            for row in &partial.rows {row.file.path.hash(&mut signature);row.snippet.hash(&mut signature);}
                            let signature=signature.finish();
                            let progress_only=signature==published_rows;
                            let update=Update {
                                content_failures: partial.failures.clone(),
                                rows: if progress_only {vec![]}else{partial.rows.clone()}, status: status.clone(), generation:request.generation,
                                message:Some(partial.message()),
                                started:request.started, native_input:request.native_input,
                                favorites:favorites.clone(), roots:roots.clone(), helper:helper.clone(),
                                children:Default::default(),cache_epoch,progress_only,
                            };
                            if progress_only {let _=result_tx.try_send(update);}else{let _=result_tx.force_send(update);published_rows=signature;}
                        }));
                        #[cfg(feature="benchmarks")]
                        crate::metrics::interaction("content_search", serde_json::json!({"checked":report.checked,"skipped":report.skipped,"errors":report.errors,"total":report.total,"finished":report.finished,"cancelled":report.cancelled,"elapsed_ms":report.elapsed_ms,"matches":report.rows.len(),"first_result_ms":report.first_result_ms,"formats":report.formats,"cache":report.cache,"first_failures":report.first_failures,"failures":report.failures,"retried":report.retried,"recovered":report.recovered}));
                        let summary = report.message();
                        content_failures = report.failures.clone();
                        content_cache = Some((report.rows, summary));
                    }
                    let (rows, summary) = content_cache.as_ref().unwrap();
                    message = Some(summary.clone());
                    rows.clone()
                } else if status.ready {
                    let parsed = Query::parse(
                        &request.text,
                        &std::env::var("USERPROFILE").unwrap_or_default(),
                    );
                    match parsed {
                        Ok(mut query) => {
                            if request.text.trim().is_empty()
                                && request.scope.is_none()
                                && query.scope.is_none()
                                && query.kind.is_none()
                            {
                                query.kind = Some(seek_engine::walk::KIND_FILE);
                            }
                            query.limit = query.limit.min(200);
                            if !request.text.contains("limit:") {
                                query.limit = 200;
                            }
                            if query.scope.is_none() {
                                query.scope = request
                                    .scope
                                    .as_ref()
                                    .map(|s| seek_engine::paths::internal(s));
                            }
                            query.hide_hidden_system = !request.show_hidden;
                            query.excluded_paths = request.exclusions.iter().map(|p| {let mut root=seek_engine::paths::internal(p);while root.last()==Some(&b'/'){root.pop();}root}).collect();
                            query.cancellation = Some((generation.clone(), request.generation));
                            let search = if let Some(folders)=request.largest {
                                Some(engine.largest(&query,folders))
                            }else if request.text.trim().is_empty() {
                                request.scope.as_ref().map(|folder| {
                                    #[cfg(feature = "benchmarks")]
                                    let mut files = {
                                        let queued_ms = request.started.map(|t| t.elapsed().as_secs_f64()*1000.);
                                        let (files, stages) = engine.profiled_children(folder, request.show_hidden, &request.exclusions);
                                        crate::metrics::interaction("browse_profile", serde_json::json!({"folder":folder.file_name().map(|n| n.to_string_lossy()),"queued_ms":queued_ms,"stages_ms":stages,"rows":files.len()}));
                                        files
                                    };
                                    #[cfg(not(feature = "benchmarks"))]
                                    let mut files = engine.children(folder, request.show_hidden, &request.exclusions);
                                    files.sort_unstable_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| a.path.cmp(&b.path)));
                                    files
                                })
                            } else { None };
                            match search.map_or_else(|| engine.search(&query), Ok) {
                                Ok(files) => {
                                    let files = if request.text.trim().is_empty() && request.largest.is_none() {
                                        if request.scope.is_some() {
                                            let stable = retain_browse_order(&recent_files, files);
                                            recent_files = stable.clone();
                                            stable
                                        } else {
                                        let mut stable = engine.refresh_results(&recent_files, &query);
                                        let mut seen: std::collections::HashSet<_> = stable.iter().map(|f| f.path.clone()).collect();
                                        for file in files { if seen.insert(file.path.clone()) { stable.push(file); } }
                                        if request.scope.is_none() { stable.truncate(query.limit); }
                                        recent_files = stable.clone();
                                        stable
                                        }
                                    } else { recent_files.clear(); files };
                                    files.into_iter().map(|file| {
                                        let mut row = Row::with_dates(file, &mut dates);
                                        if request.largest.is_some(){row.size=format!("≈ {}",if row.file.size>=1<<30{format!("{:.2} GB",row.file.size as f64/(1u64<<30)as f64)}else{format!("{:.1} KB",row.file.size as f64/1024.)});}
                                        row.highlights = crate::highlights::ranges(&row.name, &query);
                                        row
                                    }).collect()
                                },
                                Err(error) => {
                                    message = Some(error);
                                    vec![]
                                }
                            }
                        }
                        Err(error) => {
                            message = Some(error);
                            vec![]
                        }
                    }
                } else if !request.content && request.text.trim().is_empty() && request.scope.is_none() && request.largest.is_none() {
                    message=Some("Recent files · indexing all locations...".into());
                    browse.recent(&startup_locations,request.show_hidden,&request.exclusions).into_iter().map(|file|Row::with_dates(file,&mut dates)).collect()
                } else {
                    message=Some("Indexing files...".into());
                    vec![]
                };
                #[cfg(feature="benchmarks")]
                if !request.content && request.text.trim().is_empty() && request.scope.is_some(){crate::metrics::interaction("folder_view",serde_json::json!({"scope":request.scope,"generation":request.generation,"rows":rows.len(),"message":message,"index_ready":status.ready}));}
                #[cfg(feature="benchmarks")]
                crate::metrics::interaction("search_work", serde_json::json!({"generation":request.generation,"elapsed_ms":work_started.elapsed().as_secs_f64()*1000.}));
                let _ = work_started;
                if generation.load(Ordering::Relaxed) != request.generation {
                    continue;
                }
                if status.ready && first_cached_search {
                    crate::metrics::phase("cached_rows_materialized");
                    first_cached_search = false;
                }
                let mut children:std::collections::HashMap<PathBuf,Vec<Row>>=if request.content {Default::default()} else {request.expanded.iter().map(|folder|(folder.clone(),engine.children(folder,request.show_hidden,&request.exclusions).into_iter().map(Row::new).collect())).collect()};
                // Only retain a suppression while the index still has the deleted entry.
                // A same-path file recreated on disk is a new visible item, never hidden.
                removed_files.retain(|f|std::fs::symlink_metadata(&f.path).is_err_and(|e|e.kind()==std::io::ErrorKind::NotFound));
                let removed:Vec<_>=removed_files.iter().map(|f|f.path.clone()).collect();
                rows.retain(|r|!crate::file_changes::under_any(&r.file.path,&removed));
                for rows in children.values_mut(){rows.retain(|r|!crate::file_changes::under_any(&r.file.path,&removed));}
                removed_files=engine.refresh_results(&removed_files,&Query::default());
                let progress_only=message.as_deref()==Some("Indexing this folder...");
                if result_tx
                    .force_send(Update {
                        content_failures: if request.content {content_failures.clone()} else {vec![]},
                        rows,
                        status,
                        generation: request.generation,
                        message,
                        started: request.started.take(),
                        native_input: request.native_input,
                        favorites: favorites.clone(),
                        roots: roots.clone(),
                        helper:helper.clone(),
                        children,
                        cache_epoch,progress_only,
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .expect("start search worker");
    (backend, result_rx, worker)
}

/// A complete indexed folder listing already contains current metadata. Retain
/// surviving row positions by joining it once, rather than re-querying every
/// previous path and moving newly modified files to the top during scrolling.
fn retain_browse_order(previous: &[Found], files: Vec<Found>) -> Vec<Found> {
    let mut current: std::collections::HashMap<_, _> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (f.path.clone(), i))
        .collect();
    let mut order = Vec::with_capacity(files.len());
    for old in previous {
        if let Some(index) = current.remove(&old.path) {
            order.push(index);
        }
    }
    for (index, file) in files.iter().enumerate() {
        if current.remove(&file.path).is_some() {
            order.push(index);
        }
    }
    let mut files: Vec<_> = files.into_iter().map(Some).collect();
    order
        .into_iter()
        .map(|i| files[i].take().unwrap())
        .collect()
}
fn helper_action(install: bool) -> Result<(), String> {
    use windows_sys::Win32::{System::Threading::*, UI::Shell::*};
    let sid = seek_engine::helper::user_sid().map_err(|e| e.to_string())?;
    let executable = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name("seek-helper.exe");
    if !executable.is_file() {
        return Err("seek-helper.exe is missing beside Seek. Build the workspace first.".into());
    }
    let file = seek_engine::paths::extended(&seek_engine::paths::internal(&executable));
    let verb = seek_engine::helper::wide("runas");
    let args = seek_engine::helper::wide(&format!(
        "{} --sid {sid}",
        if install { "install" } else { "uninstall" }
    ));
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of_val(&info) as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = args.as_ptr();
    info.nShow = 0;
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = std::io::Error::last_os_error();
        return Err(if error.raw_os_error() == Some(1223) {
            "Administrator approval was cancelled. Standard indexing continues.".into()
        } else {
            format!("Cannot start helper installer: {error}")
        });
    }
    let process =
        unsafe { seek_engine::kernel::Handle::owned(info.hProcess) }.map_err(|e| e.to_string())?;
    unsafe {
        WaitForSingleObject(process.raw(), INFINITE);
    }
    let mut code = 0;
    if unsafe { GetExitCodeProcess(process.raw(), &mut code) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    if code == 0 {
        Ok(())
    } else {
        Err(format!(
            "Helper setup failed (exit {code}). Run seek-helper {} in an administrator terminal for details.",
            if install { "install" } else { "uninstall" }
        ))
    }
}

impl Row {
    pub(crate) fn new(file: Found) -> Self {
        Self::with_date(file, None)
    }
    fn date(mtime: u32) -> String {
        chrono::DateTime::from_timestamp(mtime as i64, 0)
            .map(|d| {
                d.with_timezone(&chrono::Local)
                    .format("%d %b %Y, %H:%M")
                    .to_string()
            })
            .unwrap_or_default()
    }
    fn with_dates(file: Found, dates: &mut std::collections::HashMap<u32, String>) -> Self {
        if dates.len() >= 512 && !dates.contains_key(&file.mtime) {
            return Self::new(file);
        }
        let modified = dates
            .entry(file.mtime)
            .or_insert_with(|| Self::date(file.mtime))
            .clone();
        Self::with_date(file, Some(modified))
    }
    fn with_date(file: Found, modified: Option<String>) -> Self {
        use std::os::windows::ffi::OsStrExt;
        let sort_name = seek_engine::paths::encode(
            &file
                .path
                .file_name()
                .unwrap_or(file.path.as_os_str())
                .encode_wide()
                .collect::<Vec<_>>(),
        );
        let name = file
            .path
            .file_name()
            .unwrap_or(file.path.as_os_str())
            .to_string_lossy()
            .into_owned();
        let folder = file
            .path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let modified = modified.unwrap_or_else(|| Self::date(file.mtime));
        let kind = if file.kind & 3 == seek_engine::walk::KIND_DIR {
            "Folder".into()
        } else if file.kind & 3 == seek_engine::walk::KIND_LINK {
            "Link / reparse point".into()
        } else {
            match file
                .path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str()
            {
                "txt" => "Text document".into(),
                "md" => "Markdown".into(),
                "pdf" => "PDF document".into(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "heic" | "svg" => "Image".into(),
                "rs" | "py" | "js" | "ts" | "cpp" | "h" | "cs" => "Source code".into(),
                "zip" | "7z" | "rar" | "tar" | "gz" => "Archive".into(),
                "exe" | "msi" => "Application".into(),
                "doc" | "docx" => "Word document".into(),
                "xls" | "xlsx" => "Excel workbook".into(),
                "ppt" | "pptx" => "PowerPoint".into(),
                "" => "File".into(),
                ext => format!("{} file", ext.to_uppercase()),
            }
        };
        let size = if file.kind & 3 == seek_engine::walk::KIND_DIR {
            "—".into()
        } else if file.size < 1024 {
            format!("{} B", file.size)
        } else if file.size < 1024 * 1024 {
            format!("{:.1} KB", file.size as f64 / 1024.)
        } else {
            format!("{:.1} MB", file.size as f64 / (1024. * 1024.))
        };
        Self {
            file,
            name,
            folder,
            modified,
            size,
            kind,
            highlights: vec![],
            sort_name,
            snippet: None,
        }
    }
}

fn drives() -> Vec<PathBuf> {
    let mask = unsafe { GetLogicalDrives() };
    (0..26)
        .filter(|letter| mask & (1 << letter) != 0)
        .map(|letter| PathBuf::from(format!("{}:\\", char::from(b'A' + letter))))
        .collect()
}
pub(crate) fn favorites() -> Vec<(String, PathBuf)> {
    seek_engine::places::favorites()
}
fn shell_open(path: &std::path::Path, reveal: bool) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let target = if reveal {
        wide(std::ffi::OsStr::new("explorer.exe"))
    } else {
        wide(path.as_os_str())
    };
    let args = if reveal {
        wide(std::ffi::OsStr::new(&format!(
            "/select,\"{}\"",
            path.display()
        )))
    } else {
        vec![0]
    };
    let open: Vec<_> = "open".encode_utf16().chain(Some(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            open.as_ptr(),
            target.as_ptr(),
            args.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result <= 32 {
        Err(format!(
            "Windows could not open the file (shell error {result})"
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn batch_dates_match_uncached_rows_and_bound_temporary_memory() {
        let mut dates = std::collections::HashMap::new();
        for mtime in (0..600).chain([1, 2, 1]) {
            let file = seek_engine::Found {
                path: r"D:\owned.txt".into(),
                kind: 0,
                size: 42,
                mtime,
                score: 3,
            };
            let cached = super::Row::with_dates(file.clone(), &mut dates);
            let direct = super::Row::new(file);
            assert_eq!(cached.modified, direct.modified);
            assert_eq!(cached.name, direct.name);
            assert_eq!(cached.size, direct.size);
        }
        assert_eq!(dates.len(), 512);
    }

    #[test]
    fn folder_rows_arrive_before_the_first_global_index_is_ready() {
        use super::*;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("files");
        let bulk = root.join("Bulk");
        std::fs::create_dir_all(&bulk).unwrap();
        for n in 0..10_000 {
            std::fs::create_dir(bulk.join(format!("d{n}"))).unwrap();
        }
        std::fs::write(root.join("marker.txt"), "owned").unwrap();
        let (backend, updates, worker) = start(
            Some(root.clone()),
            tmp.path().join("fresh-data"),
            seek_engine::SourceMode::Walker,
            vec![],
        );
        backend.generation.store(1, Ordering::Relaxed);
        let began = Instant::now();
        backend
            .tx
            .send(Command::Search(Request {
                scope: Some(root),
                show_hidden: true,
                generation: 1,
                ..Default::default()
            }))
            .unwrap();
        let mut loaded = None;
        while began.elapsed() < std::time::Duration::from_secs(10) {
            match updates.try_recv() {
                Ok(update) if update.generation == 1 && update.rows.len() == 2 => {
                    loaded = Some(update);
                    break;
                }
                _ => std::thread::sleep(std::time::Duration::from_millis(1)),
            }
        }
        let elapsed = began.elapsed();
        backend.generation.store(2, Ordering::Relaxed);
        let all_began = Instant::now();
        backend
            .tx
            .send(Command::Search(Request {
                generation: 2,
                show_hidden: true,
                ..Default::default()
            }))
            .unwrap();
        let mut all = None;
        while all_began.elapsed().as_secs() < 5 {
            if let Ok(update) = updates.try_recv() {
                if update.generation == 2 && update.rows.iter().any(|r| r.name == "marker.txt") {
                    all = Some(update);
                    break;
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        let all = all.expect("All locations must populate before the full scan finishes");
        assert!(!all.status.ready);
        assert!(
            all.message
                .as_deref()
                .unwrap()
                .contains("indexing all locations")
        );
        assert!(
            all.rows
                .iter()
                .all(|r| r.file.kind & 3 == seek_engine::walk::KIND_FILE)
        );
        println!(
            "All locations provisional rows: {:.3} ms, index_ready={}",
            all_began.elapsed().as_secs_f64() * 1000.,
            all.status.ready
        );
        backend.tx.send(Command::Stop).unwrap();
        worker.join().unwrap();
        let update = loaded.expect("folder metadata did not load");
        assert!(
            !update.status.ready,
            "test must cover a genuinely unfinished first index"
        );
        assert!(update.rows.iter().any(|r| r.name == "marker.txt"));
        eprintln!(
            "Task 1 fresh folder: {} rows, {:.3} ms, global_index_ready={}",
            update.rows.len(),
            elapsed.as_secs_f64() * 1000.,
            update.status.ready
        );
    }
    use super::*;
    #[test]
    fn content_results_refresh_after_a_file_operation_without_a_new_query() {
        let temp = tempfile::tempdir().unwrap();
        let files = temp.path().join("files");
        std::fs::create_dir(&files).unwrap();
        std::fs::write(files.join("before.md"), "quartz body").unwrap();
        let (backend, updates, worker) = start(
            Some(files.clone()),
            temp.path().join("index"),
            seek_engine::SourceMode::Walker,
            vec![],
        );
        backend.generation.store(1, Ordering::Relaxed);
        backend
            .tx
            .send(Command::Search(Request {
                text: "quartz".into(),
                content: true,
                content_locations: vec![files.clone()],
                generation: 1,
                ..Default::default()
            }))
            .unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        let wait_for = |name: &str| {
            loop {
                assert!(Instant::now() < deadline, "Expected content row {name}");
                let Ok(update) = updates.try_recv() else {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                };
                if update.rows.len() == 1 && update.rows[0].name == name {
                    break;
                }
            }
        };
        wait_for("before.md");
        std::fs::rename(files.join("before.md"), files.join("after.md")).unwrap();
        backend
            .tx
            .send(Command::RefreshParents(vec![files]))
            .unwrap();
        wait_for("after.md");
        backend.tx.send(Command::Stop).unwrap();
        worker.join().unwrap();
    }
    #[test]
    fn browsing_keeps_survivors_in_place_with_current_metadata_and_new_files_at_end() {
        let file = |name: &str, size| Found {
            path: name.into(),
            kind: 0,
            size,
            mtime: size as u32,
            score: 0,
        };
        let old = vec![file("a", 1), file("b", 1), file("c", 1)];
        let current = vec![file("new", 3), file("c", 2), file("a", 1)];
        let rows = retain_browse_order(&old, current);
        assert_eq!(
            rows.iter()
                .map(|r| (r.path.to_str().unwrap(), r.size))
                .collect::<Vec<_>>(),
            [("a", 1), ("c", 2), ("new", 3)]
        );
        let request = Request {
            scope: Some("folder".into()),
            ..Default::default()
        };
        let mut expanded = request.clone();
        expanded.generation = 15;
        expanded.expanded.push("folder/child".into());
        assert!(
            request.same_view(&expanded),
            "Expansion must not reset browsing order or the viewport"
        );
        expanded.scope = Some("other".into());
        assert!(!request.same_view(&expanded));
    }
    #[test]
    fn confirmed_deletion_cannot_reappear_and_same_path_recreation_is_visible() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("files");
        std::fs::create_dir(&folder).unwrap();
        let deleted = folder.join("Owned.txt");
        let keep = folder.join("Keep.txt");
        std::fs::write(&deleted, "old").unwrap();
        std::fs::write(&keep, "untouched").unwrap();
        let (backend, updates, worker) = start(
            Some(folder.clone()),
            temp.path().join("index"),
            seek_engine::SourceMode::Walker,
            vec![],
        );
        let ask = |generation| {
            backend.generation.store(generation, Ordering::Relaxed);
            backend
                .tx
                .send(Command::Search(Request {
                    scope: Some(folder.clone()),
                    generation,
                    ..Default::default()
                }))
                .unwrap();
        };
        let wait = |generation, count| {
            let began = Instant::now();
            loop {
                match updates.try_recv() {
                    Ok(update) if update.generation == generation && !update.progress_only => {
                        if generation == 2 {
                            assert!(!update.rows.iter().any(|r| r.file.path == deleted));
                        }
                        if update.rows.len() == count {
                            break update.rows;
                        }
                    }
                    Ok(_) => {}
                    Err(async_channel::TryRecvError::Empty) => {
                        assert!(began.elapsed().as_secs() < 10, "folder update timed out");
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        };
        ask(1);
        wait(1, 2);
        std::fs::remove_file(&deleted).unwrap();
        backend
            .tx
            .send(Command::FilesRemoved(vec![deleted.clone()]))
            .unwrap();
        ask(2);
        assert_eq!(wait(2, 1)[0].file.path, keep);
        std::fs::write(&deleted, "new replacement").unwrap();
        ask(3);
        let rows = wait(3, 2);
        let actual: std::collections::BTreeSet<_> = rows
            .iter()
            .map(|r| r.file.path.file_name().unwrap().to_owned())
            .collect();
        let expected = std::fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(std::fs::read_to_string(&keep).unwrap(), "untouched");
        backend.tx.send(Command::Stop).unwrap();
        worker.join().unwrap();
    }
    #[test]
    fn indexed_children_are_direct_complete_and_obey_visibility_and_exclusions() {
        use std::os::windows::ffi::OsStrExt;
        let temp = tempfile::tempdir().unwrap();
        let files = temp.path().join("files");
        let folder = files.join("Album 🦀");
        std::fs::create_dir_all(folder.join("nested/deeper")).unwrap();
        for i in 0..2048 {
            std::fs::write(folder.join(format!("item-{i:04}.txt")), b"x").unwrap();
        }
        std::fs::write(folder.join("nested/deeper/package.json"), b"{}").unwrap();
        let hidden = folder.join("hidden.txt");
        std::fs::write(&hidden, b"x").unwrap();
        let wide = hidden
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        assert_ne!(
            unsafe { SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN) },
            0
        );
        let engine = Engine::start_with_source(
            Options {
                dir: temp.path().join("index"),
                roots: vec![files],
            },
            seek_engine::SourceMode::Walker,
        )
        .unwrap();
        engine
            .wait_ready(std::time::Duration::from_secs(10))
            .unwrap();
        let children = engine.children(&folder, false, &[]);
        assert_eq!(children.len(), 2049);
        assert!(
            children
                .iter()
                .all(|r| r.path.parent() == Some(folder.as_path()))
        );
        assert_eq!(engine.children(&folder, true, &[]).len(), 2050);
        assert_eq!(
            engine
                .children(&folder, false, &[folder.join("nested")])
                .len(),
            2048
        );
        let case = PathBuf::from(folder.to_string_lossy().to_uppercase());
        assert_eq!(engine.children(&case, false, &[]).len(), 2049);
        let mut query = Query::parse("ext:txt", "").unwrap();
        query.excluded_paths = vec![seek_engine::paths::internal(&folder)];
        assert!(engine.search(&query).unwrap().is_empty());
        std::fs::rename(folder.join("item-0000.txt"), folder.join("renamed.txt")).unwrap();
        engine.refresh_parent(&folder.join("renamed.txt"));
        let timeout = Instant::now();
        loop {
            let children = engine.children(&folder, false, &[]);
            if children
                .iter()
                .any(|r| r.path.file_name().unwrap() == "renamed.txt")
            {
                assert_eq!(children.len(), 2049);
                assert!(
                    !children
                        .iter()
                        .any(|r| r.path.file_name().unwrap() == "item-0000.txt")
                );
                break;
            }
            assert!(timeout.elapsed().as_secs() < 10);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    #[test]
    fn sidebar_shows_folders_and_package_search_respects_the_selected_subtree() {
        let temp = tempfile::tempdir().unwrap();
        let files = temp.path().join("files");
        let pictures = files.join("Pictures");
        std::fs::create_dir_all(pictures.join("album/deeper")).unwrap();
        std::fs::create_dir_all(files.join("Documents/project")).unwrap();
        let inside = pictures.join("album/deeper/package.json");
        let outside = files.join("Documents/project/package.json");
        for path in [&inside, &outside] {
            std::fs::write(path, b"{}").unwrap();
        }
        for i in 0..240 {
            std::fs::write(pictures.join(format!("photo-{i:04}.txt")), b"photo").unwrap();
        }
        let (backend, updates, worker) = start(
            Some(files),
            temp.path().join("index"),
            seek_engine::SourceMode::Walker,
            vec![],
        );
        let ask = |text: &str, scope: Option<PathBuf>, id: u64| {
            backend.generation.store(id, Ordering::Relaxed);
            backend
                .tx
                .send(Command::Search(Request {
                    text: text.into(),
                    scope,
                    generation: id,
                    ..Default::default()
                }))
                .unwrap();
            loop {
                let update = updates.recv_blocking().unwrap();
                assert!(update.status.error.is_none(), "{:?}", update.status.error);
                if update.status.ready && update.generation == id {
                    break update.rows;
                }
            }
        };
        let browse = ask("", Some(pictures.clone()), 1);
        assert!(
            browse
                .iter()
                .any(|r| r.name == "album" && r.file.kind & 3 == seek_engine::walk::KIND_DIR)
        );
        assert_eq!(
            browse.len(),
            241,
            "Folder browsing must not use the 200-hit search cap"
        );
        assert!(
            browse
                .iter()
                .all(|r| r.file.path.parent() == Some(pictures.as_path()))
        );
        assert!(!browse.iter().any(|r| r.file.path == inside));
        assert!(!browse.iter().any(|r| r.file.path == outside));
        let all = ask("package.json", None, 2);
        assert_eq!(all.len(), 2);
        let scoped = ask("package.json", Some(pictures), 3);
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].file.path, inside);
        backend.tx.send(Command::Stop).unwrap();
        worker.join().unwrap();
    }
}
