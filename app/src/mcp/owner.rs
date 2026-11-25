//! The running app's side of the agent pipe: the one owner of the index,
//! the undo journal, the Recycle Bin and the settings. Read-only tools run
//! here against the live index; changes are checked against the person's
//! policy ("Let agents change files", the allowed folders) and handed to the
//! window, which runs them through the same journaled file worker as its
//! own, so Ctrl+Z (and `undo`) reverses them.
use super::{Outcome, ToolError, read};
use crate::file_ops::Operation;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Condvar, Mutex, OnceLock, RwLock},
    time::{Duration, Instant, SystemTime},
};

/// What the person allows, kept current by the window.
#[derive(Clone, Default)]
pub struct Policy {
    pub can_change: bool,
    pub folders: Vec<PathBuf>,
}
pub static POLICY: RwLock<Policy> = RwLock::new(Policy {
    can_change: false,
    folders: Vec::new(),
});

/// A change for the window to run.
pub enum Request {
    File { op: Operation, job: String },
    Undo { job: String },
}

static DATA: OnceLock<PathBuf> = OnceLock::new();
static SENDER: OnceLock<async_channel::Sender<Request>> = OnceLock::new();

#[derive(Clone)]
struct Job {
    tool: String,
    state: &'static str,
    error: Option<String>,
    items: Vec<PathBuf>,
}
#[derive(Default)]
struct Jobs {
    next: u64,
    jobs: HashMap<String, Job>,
    order: VecDeque<String>,
}
static JOBS: Mutex<Option<Jobs>> = Mutex::new(None);
static JOB_DONE: Condvar = Condvar::new();

/// Replies to changing calls by request_id, so a retry changes nothing twice.
type Replies = (HashMap<String, Value>, VecDeque<String>);
static REPLIES: Mutex<Option<Replies>> = Mutex::new(None);

/// Rename plans from dry runs: token → (path, name, size, modified, expiry).
type Planned = (PathBuf, String, u64, Option<SystemTime>, Instant);
static PLANS: Mutex<Option<HashMap<String, Planned>>> = Mutex::new(None);

/// Organize plans from dry runs: token → (folder, groups, expiry).
type Organized = (PathBuf, Vec<(String, Vec<crate::organize::Item>)>, Instant);
static ORGANIZE_PLANS: Mutex<Option<HashMap<String, Organized>>> = Mutex::new(None);

/// Starts the pipe for this data folder; changes arrive on the receiver.
pub fn start(data: PathBuf) -> Option<async_channel::Receiver<Request>> {
    let (tx, rx) = async_channel::unbounded();
    SENDER.set(tx).ok()?;
    let _ = DATA.set(data.clone());
    let name = super::pipe::name(&data).ok()?;
    super::pipe::serve(name, std::sync::Arc::new(handle)).ok()?;
    Some(rx)
}

fn new_job(tool: &str, items: Vec<PathBuf>) -> String {
    let mut guard = JOBS.lock().unwrap();
    let jobs = guard.get_or_insert_with(Default::default);
    jobs.next += 1;
    let id = format!("job-{}", jobs.next);
    jobs.jobs.insert(
        id.clone(),
        Job {
            tool: tool.into(),
            state: "queued",
            error: None,
            items,
        },
    );
    jobs.order.push_back(id.clone());
    while jobs.order.len() > 500 {
        if let Some(old) = jobs.order.pop_front() {
            jobs.jobs.remove(&old);
        }
    }
    id
}

/// The window reports a job's start (sent to the worker) or failure to send.
pub fn started(job: &str, error: Option<String>) {
    let mut guard = JOBS.lock().unwrap();
    if let Some(j) = guard.as_mut().and_then(|j| j.jobs.get_mut(job)) {
        match error {
            Some(e) => {
                j.state = "failed";
                j.error = Some(e);
            }
            None => j.state = "running",
        }
    }
    JOB_DONE.notify_all();
}

/// The window reports a job's end.
pub fn finish(job: &str, result: Result<(), String>) {
    let mut guard = JOBS.lock().unwrap();
    if let Some(j) = guard.as_mut().and_then(|j| j.jobs.get_mut(job)) {
        match result {
            Ok(()) => j.state = "done",
            Err(e) => {
                j.state = "failed";
                j.error = Some(e);
            }
        }
    }
    JOB_DONE.notify_all();
}

fn job_json(id: &str, wait: u64) -> Result<Outcome, ToolError> {
    let deadline = Instant::now() + Duration::from_secs(wait.min(60));
    let mut guard = JOBS.lock().unwrap();
    loop {
        let job = guard
            .as_ref()
            .and_then(|j| j.jobs.get(id))
            .cloned()
            .ok_or_else(|| ToolError::new("not_found", format!("No job \"{id}\"."), false))?;
        let finished = matches!(job.state, "done" | "failed");
        let now = Instant::now();
        if finished || now >= deadline {
            let data = json!({"id": id, "tool": job.tool, "state": job.state, "error": job.error, "items": job.items});
            let text = match &job.error {
                Some(e) => format!("{id} {}: {e}", job.state),
                None => format!("{id} {}", job.state),
            };
            return Ok(Outcome { text, data });
        }
        guard = JOB_DONE.wait_timeout(guard, deadline - now).unwrap().0;
    }
}

fn reply(outcome: Result<Outcome, ToolError>) -> Value {
    match outcome {
        Ok(o) => json!({"text": o.text, "data": o.data}),
        Err(e) => json!({"error": e.to_json()}),
    }
}

/// One request from `seek.exe --mcp`: `{tool, args, agent}`.
pub fn handle(request: Value) -> Value {
    let tool = request["tool"].as_str().unwrap_or_default().to_string();
    let args = request.get("args").cloned().unwrap_or(json!({}));
    let data = DATA.get().cloned().unwrap_or_default();
    let Some(spec) = super::tools::find(&tool, super::tools::Profile::All) else {
        return reply(Err(ToolError::new("not_found", format!("No tool \"{tool}\"."), false)));
    };
    if tool == "job_status" {
        let id = args["id"].as_str().unwrap_or_default();
        return reply(job_json(id, args["wait_seconds"].as_u64().unwrap_or(0)));
    }
    if !spec.changes {
        let engine = crate::backend::ENGINE.get();
        return reply(read::call(&tool, &args, engine.map(|e| e as &dyn read::IndexView), &data));
    }
    // A retry with the same request_id gets the first outcome.
    let request_id = args["request_id"].as_str().map(str::to_string);
    if let Some(id) = &request_id
        && let Some(previous) = REPLIES.lock().unwrap().as_ref().and_then(|(m, _)| m.get(id).cloned())
    {
        return previous;
    }
    let outcome = change(&tool, &args);
    let value = reply(outcome);
    if let Some(id) = request_id {
        let mut guard = REPLIES.lock().unwrap();
        let (map, order) = guard.get_or_insert_with(Default::default);
        map.insert(id.clone(), value.clone());
        order.push_back(id);
        while order.len() > 1000 {
            if let Some(old) = order.pop_front() {
                map.remove(&old);
            }
        }
    }
    value
}

/// The folder a path is in after resolving links and junctions, checked
/// against the allowed folders (resolved the same way).
fn allowed(path: &Path, policy: &Policy) -> Result<PathBuf, ToolError> {
    let resolved = crate::worktrees::normal(path);
    let lower = resolved.to_string_lossy().to_lowercase();
    let inside = policy.folders.iter().any(|f| {
        let f = crate::worktrees::normal(f).to_string_lossy().to_lowercase();
        lower == f || lower.starts_with(&format!("{}\\", f.trim_end_matches('\\')))
    });
    if inside {
        Ok(resolved)
    } else {
        Err(ToolError::new(
            "permission_denied",
            "That path is outside the folders agents may change. The person can add folders in Seek's Settings > Agents.",
            false,
        )
        .at(path))
    }
}

fn existing(args: &Value, key: &str, policy: &Policy) -> Result<PathBuf, ToolError> {
    let path = read::path_arg(args, key)?;
    if std::fs::symlink_metadata(&path).is_err() {
        return Err(ToolError::not_found("No such file or folder.", &path));
    }
    allowed(&path, policy)
}

fn existing_list(args: &Value, policy: &Policy) -> Result<Vec<PathBuf>, ToolError> {
    args["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| existing(&json!({"p": p}), "p", policy))
        .collect()
}

fn placeholder(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_attributes() & (0x400000 | 0x40000 | 0x1000) != 0)
}

fn token(path: &Path, name: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(path.to_string_lossy().as_bytes());
    h.update(name.as_bytes());
    h.update(&SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_nanos().to_le_bytes());
    h.finalize().to_hex()[..24].to_string()
}

fn submit(tool: &str, request: impl FnOnce(String) -> Request, items: Vec<PathBuf>, wait: u64) -> Result<Outcome, ToolError> {
    let sender = SENDER.get().ok_or_else(|| ToolError::new("internal", "Seek's window isn't ready.", true))?;
    let job = new_job(tool, items);
    sender
        .send_blocking(request(job.clone()))
        .map_err(|_| ToolError::new("app_not_running", "Seek is closing.", true))?;
    job_json(&job, wait)
}

fn change(tool: &str, args: &Value) -> Result<Outcome, ToolError> {
    let policy = POLICY.read().unwrap().clone();
    if !policy.can_change {
        return Err(ToolError::new(
            "agents_disabled",
            "Let agents change files is off. The person can turn it on in Seek's Settings > Agents.",
            false,
        ));
    }
    let wait = args["wait_seconds"].as_u64().unwrap_or(10);
    match tool {
        "copy" | "move" => {
            let paths = existing_list(args, &policy)?;
            let folder = existing(args, "folder", &policy)?;
            if !folder.is_dir() {
                return Err(ToolError::invalid("\"folder\" must be a folder.").at(&folder));
            }
            // Windows would ask about a taken name in a dialog nobody sees
            // (the window may be minimized): refuse instead, never overwrite.
            if let Some(taken) = paths.iter().find_map(|p| {
                let target = folder.join(p.file_name()?);
                (target.exists() && !(tool == "move" && target == *p)).then_some(target)
            }) {
                return Err(ToolError::new("name_taken", "The folder already has an item with that name. Rename it first, or choose another folder.", false).at(&taken));
            }
            if tool == "copy"
                && let Some(p) = paths.iter().find(|p| placeholder(p))
            {
                return Err(ToolError::new("not_downloaded", "This is a cloud placeholder; copying it would download it.", false).at(p));
            }
            let items = paths.clone();
            submit(
                tool,
                |job| Request::File {
                    op: if tool == "copy" {
                        Operation::Copy { paths, folder }
                    } else {
                        Operation::Move { paths, folder }
                    },
                    job,
                },
                items,
                wait,
            )
        }
        "trash" => {
            let paths = existing_list(args, &policy)?;
            let items = paths.clone();
            submit(tool, |job| Request::File { op: Operation::Recycle { paths }, job }, items, wait)
        }
        "create_folder" => {
            let folder = existing(args, "folder", &policy)?;
            let name = args["name"].as_str().unwrap_or_default().to_string();
            if !crate::file_ops::valid_name(&name) {
                return Err(ToolError::invalid("Not a valid Windows name."));
            }
            if folder.join(&name).exists() {
                return Err(ToolError::new("name_taken", "Something already has that name.", false).at(&folder.join(&name)));
            }
            let target = folder.join(&name);
            submit(tool, |job| Request::File { op: Operation::NewFolder { folder, name }, job }, vec![target], wait)
        }
        "rename" => {
            let mut plans = PLANS.lock().unwrap();
            let plans = plans.get_or_insert_with(HashMap::new);
            plans.retain(|_, p| p.4 > Instant::now());
            if let Some(confirm) = args["confirm"].as_str() {
                // Exactly the planned rename, once, if nothing changed.
                let (path, name, size, modified, _) = plans.remove(confirm).ok_or_else(|| {
                    ToolError::new("changed_since_preview", "That token is spent, expired or unknown. Dry run again.", false)
                })?;
                let meta = std::fs::symlink_metadata(&path)
                    .map_err(|_| ToolError::new("changed_since_preview", "The item is gone. Dry run again.", false).at(&path))?;
                if meta.len() != size || meta.modified().ok() != modified || path.with_file_name(&name).exists() {
                    return Err(ToolError::new("changed_since_preview", "The item or its folder changed since the dry run. Dry run again.", false).at(&path));
                }
                allowed(&path, &policy)?;
                let target = path.with_file_name(&name);
                return submit(tool, |job| Request::File { op: Operation::Rename { path, name }, job }, vec![target], wait);
            }
            let path = existing(args, "path", &policy)?;
            let name = args["name"]
                .as_str()
                .ok_or_else(|| ToolError::invalid("\"name\" is required unless confirming a token."))?
                .to_string();
            if !crate::file_ops::valid_name(&name) {
                return Err(ToolError::invalid("Not a valid Windows name (a name, not a path)."));
            }
            let target = path.with_file_name(&name);
            if target.exists() && !target.to_string_lossy().eq_ignore_ascii_case(&path.to_string_lossy()) {
                return Err(ToolError::new("name_taken", "Something already has that name.", false).at(&target));
            }
            if args["dry_run"].as_bool() == Some(true) {
                let meta = std::fs::symlink_metadata(&path).map_err(|e| ToolError::failed(e.to_string()))?;
                let token = token(&path, &name);
                plans.insert(
                    token.clone(),
                    (path.clone(), name.clone(), meta.len(), meta.modified().ok(), Instant::now() + Duration::from_secs(600)),
                );
                return Ok(Outcome {
                    text: format!("{} → {name}\nConfirm with token {token} within 10 minutes.", path.display()),
                    data: json!({"plan": [{"from": path, "to": target}], "token": token, "expires_in_seconds": 600}),
                });
            }
            submit(tool, |job| Request::File { op: Operation::Rename { path, name }, job }, vec![target], wait)
        }
        "organize" => {
            let mut plans = ORGANIZE_PLANS.lock().unwrap();
            let plans = plans.get_or_insert_with(HashMap::new);
            plans.retain(|_, p| p.2 > Instant::now());
            if let Some(confirm) = args["confirm"].as_str() {
                // Exactly the planned groups, once. Each file is checked again
                // as it moves; one changed since the dry run stays put.
                let (folder, groups, _) = plans.remove(confirm).ok_or_else(|| {
                    ToolError::new("changed_since_preview", "That token is spent, expired or unknown. Dry run again.", false)
                })?;
                allowed(&folder, &policy)?;
                if !folder.is_dir() {
                    return Err(ToolError::new("changed_since_preview", "The folder is gone. Dry run again.", false).at(&folder));
                }
                let items: Vec<PathBuf> = groups.iter().flat_map(|(_, items)| items.iter().map(|i| i.path.clone())).collect();
                return submit(tool, |job| Request::File { op: Operation::Organize { folder, groups }, job }, items, wait);
            }
            if args["dry_run"].as_bool() != Some(true) {
                return Err(ToolError::invalid("Run organize with dry_run first, then confirm its token."));
            }
            let folder = existing(args, "folder", &policy)?;
            if !folder.is_dir() {
                return Err(ToolError::invalid("\"folder\" must be a folder.").at(&folder));
            }
            let mode = match args["mode"].as_str().unwrap_or("type") {
                "date" => crate::organize::Mode::Date,
                "name" => crate::organize::Mode::Names,
                _ => crate::organize::Mode::Kind,
            };
            let only: Option<Vec<String>> = args["groups"].as_array().map(|names| {
                names.iter().filter_map(|n| n.as_str().map(str::to_lowercase)).collect()
            });
            let proposal = crate::organize::propose_by(&folder, mode).map_err(ToolError::failed)?;
            let groups: Vec<crate::organize::Group> = proposal
                .groups
                .into_iter()
                .filter(|g| only.as_ref().is_none_or(|only| only.contains(&g.name.to_lowercase())))
                .collect();
            if groups.is_empty() {
                return Ok(Outcome {
                    text: "Nothing to organize this way.".into(),
                    data: json!({"groups": [], "unplaced": proposal.unplaced}),
                });
            }
            let token = token(&folder, &format!("organize {mode:?}"));
            let described: Vec<Value> = groups
                .iter()
                .map(|g| {
                    json!({
                        "folder": g.name,
                        "detail": g.detail,
                        "files": g.items.iter().filter_map(|i| i.path.file_name().map(|n| n.to_string_lossy().into_owned())).collect::<Vec<_>>(),
                    })
                })
                .collect();
            let count: usize = groups.iter().map(|g| g.items.len()).sum();
            plans.insert(
                token.clone(),
                (
                    folder.clone(),
                    groups.into_iter().map(|g| (g.name, g.items)).collect(),
                    Instant::now() + Duration::from_secs(600),
                ),
            );
            Ok(Outcome {
                text: format!(
                    "{count} files into {} folders in {}\nConfirm with token {token} within 10 minutes.",
                    described.len(),
                    folder.display()
                ),
                data: json!({"folder": folder, "groups": described, "unplaced": proposal.unplaced, "token": token, "expires_in_seconds": 600}),
            })
        }
        "undo" => submit(tool, |job| Request::Undo { job }, vec![], wait),
        _ => Err(ToolError::new("not_found", format!("No tool \"{tool}\"."), false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_need_the_switch_and_an_allowed_folder() {
        let dir = tempfile::tempdir().unwrap();
        let allowed_dir = dir.path().join("allowed");
        let other = dir.path().join("other");
        std::fs::create_dir_all(&allowed_dir).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("a.txt"), "x").unwrap();
        std::fs::write(allowed_dir.join("b.txt"), "x").unwrap();
        *POLICY.write().unwrap() = Policy::default();
        let off = change("trash", &json!({"paths": [allowed_dir.join("b.txt")]})).err().unwrap();
        assert_eq!(off.code, "agents_disabled");
        *POLICY.write().unwrap() = Policy {
            can_change: true,
            folders: vec![allowed_dir.clone()],
        };
        let outside = change("trash", &json!({"paths": [other.join("a.txt")]})).err().unwrap();
        assert_eq!(outside.code, "permission_denied");
        // A rename dry run gives a token; changing the file spends it.
        let plan = change("rename", &json!({"path": allowed_dir.join("b.txt"), "name": "c.txt", "dry_run": true})).ok().unwrap();
        let token = plan.data["token"].as_str().unwrap().to_string();
        std::fs::write(allowed_dir.join("b.txt"), "changed").unwrap();
        let stale = change("rename", &json!({"confirm": token})).err().unwrap();
        assert_eq!(stale.code, "changed_since_preview");
        let again = change("rename", &json!({"confirm": token})).err().unwrap();
        assert_eq!(again.code, "changed_since_preview", "a token runs once at most");
        let taken = change("create_folder", &json!({"folder": allowed_dir, "name": "b.txt"})).err().unwrap();
        assert_eq!(taken.code, "name_taken");
        // A copy onto a taken name is refused before any dialog could appear.
        std::fs::create_dir_all(allowed_dir.join("into")).unwrap();
        std::fs::write(allowed_dir.join("into").join("b.txt"), "y").unwrap();
        let copy = change("copy", &json!({"paths": [allowed_dir.join("b.txt")], "folder": allowed_dir.join("into")})).err().unwrap();
        assert_eq!(copy.code, "name_taken");
        // Organize: a dry run first; its plan names the groups and a token
        // that runs once; folders outside the allowed ones are refused.
        for name in ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"] {
            std::fs::write(allowed_dir.join(name), "x").unwrap();
        }
        let direct = change("organize", &json!({"folder": allowed_dir})).err().unwrap();
        assert_eq!(direct.code, "invalid_argument");
        let outside = change("organize", &json!({"folder": other, "dry_run": true})).err().unwrap();
        assert_eq!(outside.code, "permission_denied");
        let plan = change("organize", &json!({"folder": allowed_dir, "mode": "name", "dry_run": true})).ok().unwrap();
        assert_eq!(plan.data["groups"][0]["folder"], "Camera IMG");
        assert_eq!(plan.data["groups"][0]["files"].as_array().unwrap().len(), 3);
        let token = plan.data["token"].as_str().unwrap().to_string();
        // No window in a test: the confirm is taken, then refused to run.
        assert!(change("organize", &json!({"confirm": token})).is_err());
        let spent = change("organize", &json!({"confirm": token})).err().unwrap();
        assert_eq!(spent.code, "changed_since_preview", "a token runs once at most");
        *POLICY.write().unwrap() = Policy::default();
    }
}
