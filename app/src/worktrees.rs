//! Git worktrees (Phase 9.4), after Monodrive's docs/worktrees.md. Git stays
//! the authority: Seek keeps no copy of what `git worktree list` says, only
//! the repositories the person added by hand. Git runs from an argument list,
//! never a shell, with optional locks, prompts, the pager and fsmonitor off,
//! a time limit and capped output. Repositories are found by asking the index
//! for `.git` entries, never by walking the disk. Nothing here changes a
//! repository: removal moves folders to the Recycle Bin through the journaled
//! file operations, after a plan the person confirmed.
use std::{
    collections::{BTreeMap, HashSet},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_OUTPUT: usize = 8 << 20;
const MAX_ERRORS: usize = 64 << 10;
/// Link files (`.git`, `gitdir`, `commondir`) larger than this are not read.
const MAX_LINK: u64 = 64 << 10;
/// Oldest Git with `worktree list -z`.
pub const MIN_VERSION: (u32, u32) = (2, 36);
/// Folders inside a worktree that are rebuilt from its sources.
pub const REBUILDABLE: [&str; 6] = ["node_modules", "target", ".venv", "dist", "build", ".next"];

#[derive(Clone, Debug)]
pub struct Git {
    pub exe: PathBuf,
}

impl Git {
    /// `git.exe` on PATH, then the usual Git for Windows folders.
    pub fn find() -> Option<Git> {
        let on_path = std::env::var_os("PATH")
            .into_iter()
            .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .map(|dir| dir.join("git.exe"));
        let program_files = ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"]
            .iter()
            .filter_map(std::env::var_os)
            .flat_map(|base| {
                let base = PathBuf::from(base);
                [
                    base.join(r"Git\cmd\git.exe"),
                    base.join(r"Programs\Git\cmd\git.exe"),
                ]
            });
        on_path
            .chain(program_files)
            .find(|p| p.is_file())
            .map(|exe| Git { exe })
    }

    /// Runs git in `cwd` with `args`; `Err` carries git's own message.
    pub fn run(&self, cwd: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<Vec<u8>, String> {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new(&self.exe);
        command
            .args(["--no-pager", "-c", "core.fsmonitor=false"])
            .args(args)
            .current_dir(cwd)
            // Only what git needs to find the person's own config, so a
            // GIT_DIR or GIT_WORK_TREE inherited by Seek can't redirect it.
            .env_clear()
            .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        for key in [
            "SystemRoot", "SystemDrive", "PATH", "PATHEXT", "USERPROFILE", "HOMEDRIVE",
            "HOMEPATH", "HOME", "APPDATA", "LOCALAPPDATA", "ProgramData", "TEMP", "TMP",
            "XDG_CONFIG_HOME",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .env("LANG", "C")
            .env("LC_ALL", "C");
        let mut child = command.spawn().map_err(|e| format!("Couldn't run Git: {e}"))?;
        if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
            let input = input.to_vec();
            std::thread::spawn(move || {
                use std::io::Write;
                let _ = pipe.write_all(&input);
            });
        }
        let (tx, rx) = mpsc::channel();
        let pipe = |mut reader: Box<dyn Read + Send>, cap: usize, which: u8| {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut out = Vec::new();
                let mut buffer = [0u8; 64 * 1024];
                let mut over = false;
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(n) if out.len() + n > cap => {
                            over = true;
                            break;
                        }
                        Ok(n) => out.extend_from_slice(&buffer[..n]),
                    }
                }
                let _ = tx.send((which, out, over));
            });
        };
        pipe(Box::new(child.stdout.take().expect("piped")), MAX_OUTPUT, 0);
        pipe(Box::new(child.stderr.take().expect("piped")), MAX_ERRORS, 1);
        drop(tx);
        let deadline = Instant::now() + TIMEOUT;
        let (mut out, mut err) = (None, None);
        while out.is_none() || err.is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok((_, _, true)) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("Git printed more than Seek reads; stopped it.".into());
                }
                Ok((0, bytes, _)) => out = Some(bytes),
                Ok((_, bytes, _)) => err = Some(bytes),
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("Git took longer than 10 seconds; stopped it.".into());
                }
            }
        }
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Git took longer than 10 seconds; stopped it.".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        if status.success() {
            Ok(out.unwrap_or_default())
        } else {
            let message = String::from_utf8_lossy(&err.unwrap_or_default()).trim().to_string();
            Err(if message.is_empty() {
                format!("Git failed ({status})")
            } else {
                message
            })
        }
    }

    pub fn version(&self) -> Result<(u32, u32), String> {
        let out = self.run(&std::env::temp_dir(), &["version"], None)?;
        parse_version(&String::from_utf8_lossy(&out)).ok_or_else(|| "Unrecognised Git version".into())
    }
}

/// `git version 2.55.0.windows.3` → (2, 55).
pub fn parse_version(text: &str) -> Option<(u32, u32)> {
    let number = text.split_whitespace().nth(2)?;
    let mut parts = number.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// Git writes forward slashes; Windows paths read better with backslashes.
fn from_git(path: &str) -> PathBuf {
    PathBuf::from(path.replace('/', "\\"))
}

/// A path without the `\\?\` prefix canonicalize adds, for comparing.
pub fn normal(path: &Path) -> PathBuf {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    match path.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path,
    }
}

fn key(path: &Path) -> String {
    normal(path).to_string_lossy().to_lowercase()
}

/// Reads a small link file: a regular file of at most 64 KB, or nothing.
fn read_link_file(path: &Path) -> Option<String> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_LINK {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// The folder a `.git` file points at (`gitdir: …`), resolved from its own
/// folder when relative.
fn gitdir_of(dot_git: &Path) -> Option<PathBuf> {
    let text = read_link_file(dot_git)?;
    let target = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    let target = from_git(target);
    Some(if target.is_absolute() {
        target
    } else {
        dot_git.parent()?.join(target)
    })
}

/// The common Git folder behind a linked worktree's admin folder
/// (`<common>/worktrees/<name>`), from its `commondir` file.
fn common_of_admin(admin: &Path) -> Option<PathBuf> {
    if !admin
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n.eq_ignore_ascii_case("worktrees"))
    {
        return None;
    }
    let common = read_link_file(&admin.join("commondir"))?;
    let common = from_git(common.trim());
    Some(normal(&if common.is_absolute() {
        common
    } else {
        admin.join(common)
    }))
}

/// Repositories (by common Git folder) behind `.git` entries from the index.
/// A linked worktree's `.git` file leads back to its repository; a `.git`
/// folder counts only when it has linked worktrees; submodules (whose `.git`
/// names a folder under `modules`) are skipped.
pub fn repositories_from(dot_gits: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut out = vec![];
    for dot_git in dot_gits {
        if dot_git
            .components()
            .any(|c| c.as_os_str().eq_ignore_ascii_case("$Recycle.Bin"))
        {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&dot_git) else {
            continue;
        };
        let common = if meta.is_dir() {
            let has_linked = std::fs::read_dir(dot_git.join("worktrees"))
                .is_ok_and(|mut entries| entries.next().is_some());
            has_linked.then(|| normal(&dot_git))
        } else if meta.is_file() {
            gitdir_of(&dot_git).and_then(|admin| common_of_admin(&admin))
        } else {
            None
        };
        if let Some(common) = common
            && seen.insert(key(&common))
        {
            out.push(common);
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Ok,
    /// The folder is gone, perhaps on a drive that isn't connected.
    Missing,
    /// The folder is there but its `.git` is gone or leads nowhere.
    Stale,
    /// Its `.git` leads to another repository's folder.
    Replaced,
    Unreadable,
}

#[derive(Clone, Debug)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    /// Short branch name; `None` when detached or bare.
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub main: bool,
    pub locked: Option<String>,
    pub prunable: Option<String>,
    pub state: State,
}

impl Worktree {
    /// The folder Git keeps for a linked worktree, `<common>/worktrees/<name>`.
    pub fn admin(&self, common: &Path) -> Option<PathBuf> {
        if self.main {
            return None;
        }
        if let Some(admin) = gitdir_of(&self.path.join(".git"))
            && common_of_admin(&admin).is_some_and(|c| key(&c) == key(common))
        {
            return Some(normal(&admin));
        }
        // The folder is gone: find the admin folder whose gitdir names it.
        let wanted = key(&self.path.join(".git"));
        std::fs::read_dir(common.join("worktrees"))
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|admin| {
                read_link_file(&admin.join("gitdir"))
                    .is_some_and(|g| key(&from_git(g.trim())) == wanted)
            })
    }
}

/// Parses `git worktree list --porcelain -z`.
pub fn parse_list(out: &[u8]) -> Vec<Worktree> {
    let text = String::from_utf8_lossy(out);
    let mut list = vec![];
    let mut current: Option<Worktree> = None;
    for field in text.split('\0') {
        if field.is_empty() {
            list.extend(current.take());
            continue;
        }
        let (name, value) = field.split_once(' ').unwrap_or((field, ""));
        if name == "worktree" {
            list.extend(current.take());
            current = Some(Worktree {
                path: from_git(value),
                head: None,
                branch: None,
                detached: false,
                bare: false,
                main: list.is_empty(),
                locked: None,
                prunable: None,
                state: State::Ok,
            });
            continue;
        }
        let Some(w) = current.as_mut() else { continue };
        match name {
            "HEAD" => w.head = Some(value.to_string()),
            "branch" => {
                w.branch = Some(value.strip_prefix("refs/heads/").unwrap_or(value).to_string())
            }
            "detached" => w.detached = true,
            "bare" => w.bare = true,
            "locked" => w.locked = Some(value.to_string()),
            "prunable" => w.prunable = Some(value.to_string()),
            _ => {}
        }
    }
    list.extend(current);
    list
}

/// What is actually on disk for a listed worktree.
pub fn check_state(worktree: &Worktree, common: &Path) -> State {
    if worktree.bare {
        return State::Ok;
    }
    match std::fs::symlink_metadata(&worktree.path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return State::Missing,
        Err(_) => return State::Unreadable,
        Ok(_) => {}
    }
    let dot_git = worktree.path.join(".git");
    let Ok(meta) = std::fs::symlink_metadata(&dot_git) else {
        return State::Stale;
    };
    if worktree.main {
        return if meta.is_dir() && key(&dot_git) == key(common) {
            State::Ok
        } else {
            State::Replaced
        };
    }
    if !meta.is_file() {
        return State::Replaced;
    }
    let Some(admin) = gitdir_of(&dot_git) else {
        return State::Unreadable;
    };
    if !admin.is_dir() {
        return State::Stale;
    }
    match common_of_admin(&admin) {
        Some(c) if key(&c) == key(common) => {}
        Some(_) => return State::Replaced,
        None => return State::Stale,
    }
    // The admin folder's gitdir has to name this `.git` back.
    match read_link_file(&admin.join("gitdir")) {
        Some(back) if key(&from_git(back.trim())) == key(&dot_git) => State::Ok,
        Some(_) => State::Replaced,
        None => State::Unreadable,
    }
}

#[derive(Clone, Debug)]
pub struct Repository {
    pub common: PathBuf,
    pub worktrees: Vec<Worktree>,
    pub error: Option<String>,
}

impl Repository {
    /// The name people know it by: the main worktree's folder.
    pub fn name(&self) -> String {
        let shown = self
            .worktrees
            .first()
            .map(|w| w.path.clone())
            .unwrap_or_else(|| self.common.clone());
        let shown = if shown.file_name().is_some_and(|n| n == ".git") {
            shown.parent().map(Path::to_path_buf).unwrap_or(shown)
        } else {
            shown
        };
        shown
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| shown.to_string_lossy().into_owned())
    }
}

pub fn list(git: &Git, common: &Path) -> Repository {
    let common = normal(common);
    let git_dir = format!("--git-dir={}", common.display());
    match git.run(&common, &[&git_dir, "worktree", "list", "--porcelain", "-z"], None) {
        Ok(out) => {
            let mut worktrees = parse_list(&out);
            for w in &mut worktrees {
                w.state = check_state(w, &common);
            }
            Repository { common, worktrees, error: None }
        }
        Err(error) => Repository { common, worktrees: vec![], error: Some(error) },
    }
}

/// The common Git folder of a folder the person picked, through Git.
pub fn common_dir(git: &Git, folder: &Path) -> Result<PathBuf, String> {
    let out = git.run(folder, &["rev-parse", "--path-format=absolute", "--git-common-dir"], None)?;
    Ok(normal(&from_git(String::from_utf8_lossy(&out).trim())))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub staged: usize,
    pub unstaged: usize,
    pub conflicted: usize,
    pub untracked: usize,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    /// Each changed path with its letter, for the removal plan.
    pub changes: Vec<String>,
}

impl Status {
    pub fn clean(&self) -> bool {
        self.changes.is_empty()
    }
}

/// Parses `git status --porcelain=v2 -z --branch`.
pub fn parse_status(out: &[u8]) -> Status {
    let text = String::from_utf8_lossy(out);
    let mut status = Status::default();
    let mut fields = text.split('\0');
    while let Some(field) = fields.next() {
        let mut parts = field.splitn(2, ' ');
        let kind = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("");
        match kind {
            "#" => {
                if let Some(ab) = rest.strip_prefix("branch.ab ") {
                    let mut it = ab.split_whitespace();
                    status.ahead = it.next().and_then(|a| a.trim_start_matches('+').parse().ok());
                    status.behind = it.next().and_then(|b| b.trim_start_matches('-').parse().ok());
                }
            }
            "1" | "2" => {
                let xy = rest.get(..2).unwrap_or("..");
                // The path follows the 7th field (8th for a rename).
                let path = rest
                    .match_indices(' ')
                    .nth(if kind == "1" { 6 } else { 7 })
                    .map(|(i, _)| &rest[i + 1..])
                    .unwrap_or("");
                if xy.as_bytes()[0] != b'.' {
                    status.staged += 1;
                }
                if xy.as_bytes()[1] != b'.' {
                    status.unstaged += 1;
                }
                status.changes.push(format!("{} {path}", xy.trim_matches('.')));
                if kind == "2" {
                    fields.next(); // the original path of a rename
                }
            }
            "u" => {
                status.conflicted += 1;
                let path = rest.match_indices(' ').nth(8).map(|(i, _)| &rest[i + 1..]).unwrap_or("");
                status.changes.push(format!("U {path}"));
            }
            "?" => {
                status.untracked += 1;
                status.changes.push(format!("? {rest}"));
            }
            _ => {}
        }
    }
    status
}

/// Filters named in the repository's own config that would run a command
/// during `git status`, from `git config --list --show-scope -z`.
pub fn local_filters(out: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(out);
    let mut found = vec![];
    let mut scope = "";
    for token in text.split('\0') {
        if matches!(token, "local" | "worktree" | "global" | "system" | "command") {
            scope = token;
            continue;
        }
        if !matches!(scope, "local" | "worktree") {
            continue;
        }
        let key = token.split(['\n', '=']).next().unwrap_or("").to_lowercase();
        if let Some(rest) = key.strip_prefix("filter.")
            && let Some((name, what)) = rest.rsplit_once('.')
            && (what == "clean" || what == "process")
        {
            found.push(name.to_string());
        }
    }
    found.sort();
    found.dedup();
    found
}

pub fn status(git: &Git, worktree: &Path) -> Result<Status, String> {
    let config = git.run(worktree, &["config", "--list", "--show-scope", "-z"], None)?;
    let filters = local_filters(&config);
    if !filters.is_empty() {
        return Err(format!(
            "This repository's own config sets filters that run commands ({}), so Seek skipped status.",
            filters.join(", ")
        ));
    }
    let out = git.run(
        worktree,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=normal",
            "--ignore-submodules=dirty",
        ],
        None,
    )?;
    Ok(parse_status(&out))
}

/// Bytes in a worktree's rebuildable folders (node_modules, target and so
/// on), measured by `size`.
pub fn reclaimable(worktree: &Path, size: impl Fn(&Path) -> u64) -> u64 {
    REBUILDABLE
        .iter()
        .map(|name| worktree.join(name))
        .filter(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()))
        .map(|p| size(&p))
        .sum()
}

/// What removing a worktree would take, worked out again just before it
/// happens; any difference refuses the removal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub worktree: PathBuf,
    /// `<common>/worktrees/<name>`, moved with the folder so Git forgets
    /// the worktree, and Ctrl+Z brings both back.
    pub admin: PathBuf,
    pub folder_exists: bool,
    pub changes: Vec<String>,
    /// Commits only this worktree's HEAD reaches: hash and subject.
    pub commits: Vec<(String, String)>,
    pub detached: bool,
    pub fingerprint: [u8; 32],
}

impl Plan {
    pub fn at_risk(&self) -> bool {
        !self.changes.is_empty() || !self.commits.is_empty()
    }
    /// What goes to the Recycle Bin, in one undoable operation.
    pub fn paths(&self) -> Vec<PathBuf> {
        let mut paths = vec![];
        if self.folder_exists {
            paths.push(self.worktree.clone());
        }
        paths.push(self.admin.clone());
        paths
    }
}

pub fn plan(git: &Git, repo: &Repository, worktree: &Worktree) -> Result<Plan, String> {
    if worktree.main {
        return Err("This is the repository's own checkout; Seek only removes linked worktrees.".into());
    }
    if worktree.bare {
        return Err("A bare repository isn't removed here.".into());
    }
    if let Some(reason) = &worktree.locked {
        return Err(if reason.is_empty() {
            "Git has this worktree locked. Unlock it with git worktree unlock first.".into()
        } else {
            format!("Git has this worktree locked: {reason}")
        });
    }
    let state = check_state(worktree, &repo.common);
    match state {
        State::Replaced => return Err("This folder now holds another checkout; Seek won't move it.".into()),
        State::Unreadable => return Err("Seek can't look inside this worktree.".into()),
        State::Stale if worktree.prunable.is_none() => {
            return Err("This worktree's link is broken. git worktree repair can fix it.".into());
        }
        _ => {}
    }
    if worktree.prunable.is_some() && state == State::Ok {
        return Err("Git would prune this worktree though its folder still links to it. Run git worktree repair first.".into());
    }
    let folder_exists = state != State::Missing;
    if folder_exists {
        let meta = std::fs::symlink_metadata(&worktree.path).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() || {
            use std::os::windows::fs::MetadataExt;
            meta.file_attributes() & 0x400 != 0
        } {
            return Err("The worktree's path is a link; the Recycle Bin would take the link, not the checkout.".into());
        }
    }
    let inside = key(&worktree.path);
    if repo.worktrees.iter().any(|other| {
        let k = key(&other.path);
        k != inside && k.starts_with(&format!("{inside}\\"))
    }) {
        return Err("Another worktree sits inside this one.".into());
    }
    let admin = worktree
        .admin(&repo.common)
        .ok_or("Seek couldn't find the folder Git keeps for this worktree.")?;
    if std::fs::read_dir(admin.join("modules")).is_ok_and(|mut e| e.next().is_some()) {
        return Err("This worktree has submodules checked out; Git refuses to remove those too.".into());
    }
    let changes = if folder_exists {
        status(git, &worktree.path)?.changes
    } else {
        vec![]
    };
    let commits = match &worktree.head {
        Some(head) => unique_commits(git, &repo.common, head, worktree.branch.as_deref())?,
        None => vec![],
    };
    let mut hasher = blake3::Hasher::new();
    for change in &changes {
        hasher.update(change.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(b"\x01");
    for (hash, _) in &commits {
        hasher.update(hash.as_bytes());
        hasher.update(b"\0");
    }
    Ok(Plan {
        worktree: worktree.path.clone(),
        admin,
        folder_exists,
        changes,
        commits,
        detached: worktree.detached,
        fingerprint: *hasher.finalize().as_bytes(),
    })
}

/// Commits `head` reaches that no other branch, tag or remote-tracking ref
/// does (remotes as last fetched; Seek never fetches).
fn unique_commits(
    git: &Git,
    common: &Path,
    head: &str,
    own_branch: Option<&str>,
) -> Result<Vec<(String, String)>, String> {
    let git_dir = format!("--git-dir={}", common.display());
    let refs = git.run(
        common,
        &[&git_dir, "for-each-ref", "--format=%(refname)", "refs/heads", "refs/tags", "refs/remotes"],
        None,
    )?;
    let own = own_branch.map(|b| format!("refs/heads/{b}"));
    let mut stdin = String::new();
    for name in String::from_utf8_lossy(&refs).lines() {
        if Some(name) != own.as_deref() && !name.ends_with("/HEAD") {
            stdin.push('^');
            stdin.push_str(name);
            stdin.push('\n');
        }
    }
    let out = git.run(
        common,
        &[&git_dir, "rev-list", "--max-count=1000", "--format=%s", "--stdin", head],
        Some(stdin.as_bytes()),
    )?;
    let text = String::from_utf8_lossy(&out);
    let mut commits = vec![];
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if let Some(hash) = line.strip_prefix("commit ") {
            commits.push((hash.to_string(), lines.next().unwrap_or("").to_string()));
        }
    }
    Ok(commits)
}

/// The Recycle Bin's size limit on the drive holding `path`, from the
/// person's own Recycle Bin settings; `None` when Windows doesn't say.
pub fn recycle_bin_capacity(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetVolumeNameForVolumeMountPointW, GetVolumePathNameW};
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut mount = [0u16; 260];
    if unsafe { GetVolumePathNameW(wide.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) } == 0 {
        return None;
    }
    let mut volume = [0u16; 64];
    if unsafe { GetVolumeNameForVolumeMountPointW(mount.as_ptr(), volume.as_mut_ptr(), volume.len() as u32) } == 0 {
        return None;
    }
    let volume = String::from_utf16_lossy(&volume);
    let guid = volume.split(['{', '}']).nth(1)?;
    let subkey: Vec<u16> = format!(
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket\Volume\{{{guid}}}"
    )
    .encode_utf16()
    .chain([0])
    .collect();
    let value: Vec<u16> = "MaxCapacity".encode_utf16().chain([0]).collect();
    let mut megabytes = 0u32;
    let mut size = 4u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut megabytes as *mut u32).cast(),
            &mut size,
        )
    } == 0;
    ok.then_some(megabytes as u64 * 1024 * 1024)
}

/// Folders a search for repositories never enters: app data, package and
/// build caches, and the Recycle Bin.
const SKIP_DIRS: [&str; 18] = [
    "appdata", "node_modules", "target", ".cargo", ".rustup", ".gradle", ".m2", ".npm",
    ".nuget", "$recycle.bin", ".venv", "__pycache__", ".cache", "scoop", "go", ".vscode",
    ".android", "site-packages",
];

/// `.git` entries under `home`, by walking it in parallel: for
/// repositories the index doesn't cover. A folder holding `.git` is a
/// repository and isn't walked into (its `.git` lists its worktrees, and its
/// build folders are large). Depth, entry count and time are bounded; links,
/// junctions and cloud placeholders are not entered.
pub fn find_on_disk(home: &Path, cancel: &std::sync::atomic::AtomicBool) -> Vec<PathBuf> {
    use std::sync::{Mutex, atomic::{AtomicUsize, Ordering}};
    const DEPTH: usize = 8;
    const ENTRIES: usize = 1_500_000;
    let deadline = Instant::now() + Duration::from_secs(8);
    let found = Mutex::new(Vec::new());
    let seen = AtomicUsize::new(0);
    fn visit<'s>(
        scope: &rayon::Scope<'s>,
        dir: PathBuf,
        depth: usize,
        ctx: &'s (
            &'s Mutex<Vec<PathBuf>>,
            &'s AtomicUsize,
            &'s std::sync::atomic::AtomicBool,
            Instant,
        ),
    ) {
        use std::os::windows::fs::MetadataExt;
        let (found, seen, cancel, deadline) = ctx;
        if cancel.load(Ordering::Relaxed)
            || seen.load(Ordering::Relaxed) >= ENTRIES
            || Instant::now() >= *deadline
        {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        let mut children = vec![];
        let mut repository = false;
        for entry in entries.flatten() {
            seen.fetch_add(1, Ordering::Relaxed);
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.eq_ignore_ascii_case(".git") {
                if let Ok(mut f) = found.lock() {
                    f.push(entry.path());
                }
                repository = true;
                continue;
            }
            // `file_type` comes with the directory listing; no extra call.
            let Ok(kind) = entry.file_type() else { continue };
            if !kind.is_dir() || kind.is_symlink() {
                continue;
            }
            let lower = name.to_ascii_lowercase();
            if depth + 1 < DEPTH && !SKIP_DIRS.contains(&lower.as_str()) {
                children.push(entry);
            }
        }
        if repository {
            return;
        }
        for entry in children {
            // Junctions and cloud placeholders stay shut.
            let Ok(meta) = entry.metadata() else { continue };
            if meta.file_attributes() & (0x400 | 0x400000 | 0x40000 | 0x1000) != 0 {
                continue;
            }
            let path = entry.path();
            scope.spawn(move |scope| visit(scope, path, depth + 1, ctx));
        }
    }
    let ctx = (&found, &seen, cancel, deadline);
    rayon::scope(|scope| visit(scope, home.to_path_buf(), 0, &ctx));
    found.into_inner().unwrap_or_default()
}

/// Repositories grouped for the page, by common folder.
pub fn scan(git: &Git, commons: impl IntoIterator<Item = PathBuf>) -> Vec<Repository> {
    let mut by_key: BTreeMap<String, PathBuf> = BTreeMap::new();
    for common in commons {
        by_key.entry(key(&common)).or_insert(common);
    }
    let commons: Vec<PathBuf> = by_key.into_values().collect();
    let mut repos: Vec<Repository> = {
        use rayon::prelude::*;
        commons.par_iter().map(|c| list(git, c)).collect()
    };
    repos.sort_by_key(|r| r.name().to_lowercase());
    repos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git() -> Option<Git> {
        Git::find()
    }

    fn sh(git: &Git, cwd: &Path, args: &[&str]) {
        let mut full = vec!["-c", "user.name=Seek", "-c", "user.email=seek@example.invalid"];
        full.extend_from_slice(args);
        git.run(cwd, &full, None).unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    }

    /// A repository with one commit and a linked worktree beside it.
    fn fixture(git: &Git) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        sh(git, &repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        sh(git, &repo, &["add", "."]);
        sh(git, &repo, &["commit", "-q", "-m", "first"]);
        let linked = dir.path().join("linked");
        sh(git, &repo, &["worktree", "add", "-q", "-b", "feature", linked.to_str().unwrap()]);
        (dir, repo, linked)
    }

    #[test]
    fn versions_parse() {
        assert_eq!(parse_version("git version 2.55.0.windows.3"), Some((2, 55)));
        assert_eq!(parse_version("git version 2.36.1"), Some((2, 36)));
        assert!((2, 35) < MIN_VERSION);
    }

    #[test]
    fn list_parses_branch_detached_locked_and_prunable() {
        let out = b"worktree C:/r\0HEAD abc\0branch refs/heads/main\0\0worktree C:/w\0HEAD def\0detached\0locked on a drive\0prunable gitdir file points to non-existent location\0\0";
        let list = parse_list(out);
        assert_eq!(list.len(), 2);
        assert!(list[0].main && !list[1].main);
        assert_eq!(list[0].branch.as_deref(), Some("main"));
        assert_eq!(list[0].path, PathBuf::from(r"C:\r"));
        assert!(list[1].detached);
        assert_eq!(list[1].locked.as_deref(), Some("on a drive"));
        assert!(list[1].prunable.is_some());
    }

    #[test]
    fn status_counts_and_lists_changes() {
        let out = b"# branch.oid abc\0# branch.head main\0# branch.ab +2 -1\x001 M. N... 100644 100644 100644 a b src/x y.rs\x001 .M N... 100644 100644 100644 a b z.rs\x002 R. N... 100644 100644 100644 a b R100 new.rs\0old.rs\0? notes.txt\0";
        let s = parse_status(out);
        assert_eq!((s.staged, s.unstaged, s.untracked), (2, 1, 1));
        assert_eq!((s.ahead, s.behind), (Some(2), Some(1)));
        assert_eq!(s.changes, vec!["M src/x y.rs", "M z.rs", "R new.rs", "? notes.txt"]);
    }

    #[test]
    fn only_local_filters_that_run_commands_count() {
        let out = b"global\0filter.lfs.clean\ngit-lfs clean -- %f\0local\0core.bare\nfalse\0local\0filter.evil.process\nrun-me\0worktree\0filter.x.smudge\ny\0";
        assert_eq!(local_filters(out), vec!["evil".to_string()]);
    }

    #[test]
    fn finds_lists_and_plans_a_real_worktree() {
        let Some(git) = git() else {
            eprintln!("git not installed; skipped");
            return;
        };
        let (_dir, repo, linked) = fixture(&git);
        // Found from the linked worktree's .git file, and from the main .git
        // folder (it has linked worktrees), as one repository.
        let found = repositories_from([linked.join(".git"), repo.join(".git")]);
        assert_eq!(found.len(), 1, "{found:?}");
        let listed = list(&git, &found[0]);
        assert_eq!(listed.error, None);
        assert_eq!(listed.worktrees.len(), 2);
        assert_eq!(listed.name(), "repo");
        let w = &listed.worktrees[1];
        assert_eq!(w.state, State::Ok);
        assert_eq!(w.branch.as_deref(), Some("feature"));
        assert!(plan(&git, &listed, &listed.worktrees[0]).is_err(), "main is refused");
        // Clean, and its commit is on main too: nothing at risk.
        let clean = plan(&git, &listed, w).unwrap();
        assert!(!clean.at_risk(), "{clean:?}");
        assert!(clean.admin.ends_with(r"worktrees\linked"));
        assert_eq!(clean.paths().len(), 2);
        // An untracked file and a commit only this branch has are at risk,
        // and change the fingerprint.
        std::fs::write(linked.join("new.txt"), "n").unwrap();
        let dirty = plan(&git, &listed, w).unwrap();
        assert_eq!(dirty.changes, vec!["? new.txt"]);
        sh(&git, &linked, &["add", "."]);
        sh(&git, &linked, &["commit", "-q", "-m", "only here"]);
        let listed = list(&git, &found[0]);
        let committed = plan(&git, &listed, &listed.worktrees[1]).unwrap();
        assert_eq!(committed.commits.len(), 1);
        assert_eq!(committed.commits[0].1, "only here");
        assert_ne!(committed.fingerprint, clean.fingerprint);
        assert_ne!(committed.fingerprint, dirty.fingerprint);
    }

    #[test]
    fn missing_locked_and_status_states() {
        let Some(git) = git() else { return };
        let (_dir, repo, linked) = fixture(&git);
        let common = repo.join(".git");
        sh(&git, &repo, &["worktree", "lock", "--reason", "usb", linked.to_str().unwrap()]);
        let listed = list(&git, &common);
        let err = plan(&git, &listed, &listed.worktrees[1]).unwrap_err();
        assert!(err.contains("usb"), "{err}");
        sh(&git, &repo, &["worktree", "unlock", linked.to_str().unwrap()]);
        assert!(status(&git, &linked).unwrap().clean());
        // A filter in the repository's own config would run a command during
        // status, so status is refused (and so is a removal plan).
        sh(&git, &repo, &["config", "filter.evil.clean", "echo hi"]);
        let refused = status(&git, &linked).unwrap_err();
        assert!(refused.contains("evil"), "{refused}");
        sh(&git, &repo, &["config", "--unset", "filter.evil.clean"]);
        // A folder that went away is missing, and its plan moves only the
        // folder Git keeps for it.
        std::fs::rename(&linked, linked.with_file_name("moved")).unwrap();
        let listed = list(&git, &common);
        assert_eq!(listed.worktrees[1].state, State::Missing);
        let gone = plan(&git, &listed, &listed.worktrees[1]).unwrap();
        assert!(!gone.folder_exists);
        assert_eq!(gone.paths(), vec![gone.admin.clone()]);
    }

    #[test]
    fn repositories_are_found_on_disk_but_not_inside_caches() {
        let dir = tempfile::tempdir().unwrap();
        for rel in [
            r"code\app\.git",
            r"code\deep\a\b\c\.git",
            r"AppData\Local\x\.git",
            r"code\node_modules\pkg\.git",
        ] {
            std::fs::create_dir_all(dir.path().join(rel)).unwrap();
        }
        let mut found: Vec<_> =
            find_on_disk(dir.path(), &std::sync::atomic::AtomicBool::new(false))
                .into_iter()
                .map(|p| p.strip_prefix(dir.path()).unwrap().to_path_buf())
                .collect();
        found.sort();
        assert_eq!(
            found,
            vec![PathBuf::from(r"code\app\.git"), PathBuf::from(r"code\deep\a\b\c\.git")]
        );
    }

    /// Read-only: where the time goes on this PC.
    #[test]
    #[ignore]
    fn timing_on_this_pc() {
        let home = PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
        let t = Instant::now();
        let gits = find_on_disk(&home, &std::sync::atomic::AtomicBool::new(false));
        println!("walk: {} .git in {:?}", gits.len(), t.elapsed());
        let t = Instant::now();
        let commons = repositories_from(gits);
        println!("resolve: {} repos in {:?}", commons.len(), t.elapsed());
        let git = Git::find().unwrap();
        let t = Instant::now();
        git.version().unwrap();
        println!("version: {:?}", t.elapsed());
        let t = Instant::now();
        let repos = scan(&git, commons);
        let n: usize = repos.iter().map(|r| r.worktrees.len()).sum();
        println!("list: {n} worktrees in {:?}", t.elapsed());
        let t = Instant::now();
        for w in repos.iter().flat_map(|r| &r.worktrees).filter(|w| w.state == State::Ok && !w.bare) {
            let _ = status(&git, &w.path);
        }
        println!("status all: {:?}", t.elapsed());
    }

    #[test]
    fn submodule_git_files_are_not_repositories() {
        let dir = tempfile::tempdir().unwrap();
        let modules = dir.path().join(r"super\.git\modules\sub");
        std::fs::create_dir_all(&modules).unwrap();
        std::fs::write(modules.join("commondir"), "../..").unwrap();
        let sub = dir.path().join(r"super\sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join(".git"), "gitdir: ../.git/modules/sub\n").unwrap();
        assert!(repositories_from([sub.join(".git")]).is_empty());
    }
}
