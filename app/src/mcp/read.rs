//! Read-only tools. They run in the app over the pipe when Seek is running
//! (with the live index), and in the MCP process itself when it isn't (with
//! the saved index, mapped for the one call). Nothing here changes a file,
//! and nothing reads or downloads a cloud placeholder.
use super::{Outcome, ToolError};
use seek_engine::{Found, Query};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

/// What the read-only tools need from an index: the app's live engine or
/// the saved index.
pub trait IndexView {
    fn search(&self, q: &Query) -> Result<Vec<Found>, String>;
    fn largest(&self, q: &Query, folders: bool) -> Vec<Found>;
    fn storage(&self, scope: &Path, deadline: Instant, cancel: &AtomicBool) -> seek_engine::live::storage::Storage;
}

impl IndexView for seek_engine::Engine {
    fn search(&self, q: &Query) -> Result<Vec<Found>, String> {
        seek_engine::Engine::search(self, q)
    }
    fn largest(&self, q: &Query, folders: bool) -> Vec<Found> {
        seek_engine::Engine::largest(self, q, folders)
    }
    fn storage(&self, scope: &Path, deadline: Instant, cancel: &AtomicBool) -> seek_engine::live::storage::Storage {
        seek_engine::Engine::storage(self, scope, 4096, deadline, cancel)
    }
}

impl IndexView for seek_engine::SavedIndex {
    fn search(&self, q: &Query) -> Result<Vec<Found>, String> {
        Ok(seek_engine::SavedIndex::search(self, q))
    }
    fn largest(&self, q: &Query, folders: bool) -> Vec<Found> {
        seek_engine::SavedIndex::largest(self, q, folders)
    }
    fn storage(&self, scope: &Path, deadline: Instant, cancel: &AtomicBool) -> seek_engine::live::storage::Storage {
        seek_engine::SavedIndex::storage(self, scope, 4096, deadline, cancel)
    }
}

const CLOUD: u32 = 0x400000 | 0x40000 | 0x1000;

fn arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}
fn number(args: &Value, key: &str, default: u64) -> u64 {
    args.get(key).and_then(Value::as_u64).unwrap_or(default)
}
pub(super) fn path_arg(args: &Value, key: &str) -> Result<PathBuf, ToolError> {
    let raw = arg(args, key).ok_or_else(|| ToolError::invalid(format!("\"{key}\" is required.")))?;
    let path = expand(raw);
    if !path.is_absolute() {
        return Err(ToolError::invalid(format!("\"{key}\" must be an absolute path or start with ~.")));
    }
    Ok(path)
}
/// `~` is the person's profile folder.
pub(super) fn expand(raw: &str) -> PathBuf {
    match raw.strip_prefix('~') {
        Some(rest) => PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default())
            .join(rest.trim_start_matches(['\\', '/'])),
        None => PathBuf::from(raw),
    }
}
fn kind(kind: u8) -> &'static str {
    match kind & 3 {
        1 => "folder",
        2 => "link",
        _ => "file",
    }
}
fn time(secs: u32) -> String {
    chrono::DateTime::from_timestamp(secs as i64, 0)
        .map(|t| t.with_timezone(&chrono::Local).to_rfc3339())
        .unwrap_or_default()
}
fn system_time(t: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(t).to_rfc3339()
}
fn found(f: &Found) -> Value {
    json!({"path": f.path, "kind": kind(f.kind), "size": f.size, "modified": time(f.mtime)})
}
fn lines(items: &[Value]) -> String {
    items
        .iter()
        .filter_map(|i| i["path"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}
/// A page of `items` from `cursor`, with `truncated` and `next_cursor`.
fn page(items: Vec<Value>, args: &Value, key: &str) -> Outcome {
    let offset: usize = arg(args, "cursor").and_then(|c| c.parse().ok()).unwrap_or(0);
    let limit = number(args, "limit", 50) as usize;
    let total = items.len();
    let shown: Vec<Value> = items.into_iter().skip(offset).take(limit).collect();
    let next = offset + shown.len();
    let truncated = next < total;
    let mut text = lines(&shown);
    if shown.is_empty() {
        text = "Nothing found.".into();
    }
    if truncated {
        text.push_str(&format!("\n(more: cursor {next})"));
    }
    let mut data = json!({ key: shown, "truncated": truncated });
    if truncated {
        data["next_cursor"] = json!(next.to_string());
    }
    Outcome { text, data }
}

/// Runs a read-only tool. `index` is `None` when no index is available.
pub fn call(name: &str, args: &Value, index: Option<&dyn IndexView>, data: &Path) -> Result<Outcome, ToolError> {
    let need_index = || index.ok_or_else(|| ToolError::new("index_unavailable", "Seek's index isn't available yet.", true));
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    match name {
        "search" => {
            let index = need_index()?;
            let mut query = Query::parse(arg(args, "query").unwrap_or_default(), &home).map_err(ToolError::invalid)?;
            let offset: usize = arg(args, "cursor").and_then(|c| c.parse().ok()).unwrap_or(0);
            query.limit = offset + number(args, "limit", 50) as usize + 1;
            if let Some(folder) = arg(args, "folder") {
                query.scope = Some(seek_engine::paths::internal(&expand(folder)));
            }
            let results = index.search(&query).map_err(|e| ToolError::new("index_unavailable", e, true))?;
            Ok(page(results.iter().map(found).collect(), args, "results"))
        }
        "search_content" => {
            let folder = path_arg(args, "folder")?;
            let text = arg(args, "text").unwrap_or_default();
            let generation = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1));
            let report = crate::content_catalog::search(text, std::slice::from_ref(&folder), Some(&folder), &[], false, generation, 1, &mut |_| {});
            let items: Vec<Value> = report.rows.iter().map(|r| found(&r.file)).collect();
            Ok(page(items, args, "results"))
        }
        "list_dir" => {
            let folder = path_arg(args, "path")?;
            let show_hidden = args.get("show_hidden").and_then(Value::as_bool).unwrap_or(false);
            let items = crate::browse::listing(&folder, show_hidden, &[]).map_err(|e| ToolError::not_found(e, &folder))?;
            let items: Vec<Value> = items.iter().map(found).collect();
            Ok(page(items, args, "items"))
        }
        "file_info" => {
            use std::os::windows::fs::MetadataExt;
            let path = path_arg(args, "path")?;
            let meta = std::fs::symlink_metadata(&path).map_err(|e| ToolError::not_found(e.to_string(), &path))?;
            let attributes = meta.file_attributes();
            let info = json!({
                "path": path,
                "kind": if meta.is_dir() { "folder" } else if meta.file_type().is_symlink() { "link" } else { "file" },
                "size": meta.len(),
                "created": meta.created().ok().map(system_time),
                "modified": meta.modified().ok().map(system_time),
                "hidden": attributes & 0x2 != 0,
                "read_only": attributes & 0x1 != 0,
                "link_or_junction": attributes & 0x400 != 0,
                "cloud_placeholder": attributes & CLOUD != 0,
            });
            Ok(Outcome {
                text: format!("{} · {} bytes", path.display(), meta.len()),
                data: info,
            })
        }
        "largest" => {
            let index = need_index()?;
            let mut query = Query::parse("", "").map_err(ToolError::invalid)?;
            query.limit = number(args, "limit", 50) as usize;
            if let Some(folder) = arg(args, "folder") {
                query.scope = Some(seek_engine::paths::internal(&expand(folder)));
            }
            let folders = args.get("folders").and_then(Value::as_bool).unwrap_or(false);
            let items: Vec<Value> = index.largest(&query, folders).iter().map(found).collect();
            Ok(page(items, &json!({"limit": number(args, "limit", 50)}), "results"))
        }
        "folder_size" => {
            let index = need_index()?;
            let path = path_arg(args, "path")?;
            let s = index.storage(&path, Instant::now() + Duration::from_secs(10), &AtomicBool::new(false));
            Ok(Outcome {
                text: format!("{} · {} bytes in {} files", path.display(), s.bytes, s.files),
                data: json!({"path": path, "bytes": s.bytes, "on_disk": s.on_disk, "files": s.files, "stopped": s.stopped}),
            })
        }
        "storage_overview" => {
            let index = need_index()?;
            let path = path_arg(args, "path")?;
            let s = index.storage(&path, Instant::now() + Duration::from_secs(10), &AtomicBool::new(false));
            let categories: Vec<Value> = seek_engine::live::storage::CATEGORIES
                .iter()
                .zip(&s.categories)
                .filter(|(_, (bytes, _))| *bytes > 0)
                .map(|(name, (bytes, files))| json!({"kind": name, "bytes": bytes, "files": files}))
                .collect();
            let folders: Vec<Value> = s
                .children
                .iter()
                .take(20)
                .map(|(p, b)| json!({"path": p, "bytes": b}))
                .collect();
            Ok(Outcome {
                text: categories
                    .iter()
                    .map(|c| format!("{}: {} bytes", c["kind"].as_str().unwrap_or(""), c["bytes"]))
                    .collect::<Vec<_>>()
                    .join("\n"),
                data: json!({"path": path, "bytes": s.bytes, "files": s.files, "kinds": categories, "folders": folders, "stopped": s.stopped}),
            })
        }
        "read_text" => {
            use std::io::{Read, Seek, SeekFrom};
            use std::os::windows::fs::MetadataExt;
            let path = path_arg(args, "path")?;
            let meta = std::fs::symlink_metadata(&path).map_err(|e| ToolError::not_found(e.to_string(), &path))?;
            if meta.file_attributes() & CLOUD != 0 {
                return Err(ToolError::new("not_downloaded", "This file is a cloud placeholder; Seek won't download it.", false).at(&path));
            }
            if !meta.is_file() {
                return Err(ToolError::invalid("read_text reads files."));
            }
            let offset = number(args, "offset", 0);
            let max = number(args, "max_bytes", 64 * 1024).min(256 * 1024);
            let mut file = std::fs::File::open(&path).map_err(|e| ToolError::failed(e.to_string()))?;
            file.seek(SeekFrom::Start(offset)).map_err(|e| ToolError::failed(e.to_string()))?;
            let mut bytes = vec![];
            file.take(max).read_to_end(&mut bytes).map_err(|e| ToolError::failed(e.to_string()))?;
            if bytes.iter().take(8192).any(|b| *b == 0) && !bytes.starts_with(&[0xff, 0xfe]) && !bytes.starts_with(&[0xfe, 0xff]) {
                return Err(ToolError::invalid("This looks like a binary file, not text."));
            }
            let end = offset + bytes.len() as u64;
            let raw_text = crate::text_decode::decode(&bytes, end < meta.len()).map_err(ToolError::failed)?;
            let settings = crate::settings::read(data);
            let secrets_masked = crate::env_secrets::is_env_file(&path) && !settings.agent_read_secrets;
            let text = if secrets_masked {
                crate::env_secrets::mask_all(&raw_text)
            } else {
                raw_text
            };
            let mut payload = json!({"path": path, "text": text, "offset": offset, "next_offset": end, "size": meta.len(), "truncated": end < meta.len()});
            if secrets_masked {
                payload["secrets_masked"] = json!(true);
            }
            Ok(Outcome {
                data: payload,
                text,
            })
        }
        "recent" => {
            let bytes = std::fs::read(data.join("opened.json")).unwrap_or_default();
            let mut opened: Vec<(Vec<u8>, u32)> = serde_json::from_slice(&bytes).unwrap_or_default();
            opened.sort_by_key(|o| std::cmp::Reverse(o.1));
            let items: Vec<Value> = opened
                .iter()
                .map(|(p, t)| json!({"path": seek_engine::paths::native(p), "opened": time(*t)}))
                .collect();
            Ok(page(items, args, "files"))
        }
        "favorites" => {
            let mut items: Vec<Value> = crate::backend::favorites()
                .into_iter()
                .map(|(name, path)| json!({"name": name, "path": path}))
                .collect();
            let settings = crate::settings::read(data);
            let layout = &settings.sidebar;
            let folders = layout
                .favorites
                .iter()
                .chain(layout.groups.iter().flat_map(|g| g.items.iter()));
            for item in folders {
                if let crate::sidebar_groups::Item::Folder { path, label } = item
                    && !items.iter().any(|i| i["path"] == json!(path))
                {
                    let name = label.clone().or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()));
                    items.push(json!({"name": name, "path": path}));
                }
            }
            Ok(Outcome {
                text: lines(&items),
                data: json!({"places": items}),
            })
        }
        "saved_searches" => {
            let settings = crate::settings::read(data);
            let row = |s: &crate::settings::Search, saved: bool| json!({"query": s.query, "folder": s.scope, "content": s.content, "saved": saved});
            let mut items: Vec<Value> = settings.saved_searches.iter().map(|s| row(s, true)).collect();
            items.extend(settings.recent_searches.iter().map(|s| row(s, false)));
            items.truncate(number(args, "limit", 50) as usize);
            Ok(Outcome {
                text: items.iter().filter_map(|i| i["query"].as_str()).collect::<Vec<_>>().join("\n"),
                data: json!({"searches": items}),
            })
        }
        "find_duplicates" => {
            let folder = path_arg(args, "folder")?;
            let cancel = std::sync::Arc::new(AtomicBool::new(false));
            let stop = cancel.clone();
            // Bounded: an agent call never runs for more than about a minute.
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(55));
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            });
            let report = crate::duplicates::find(&folder, &cancel, |_, _, _| {});
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            let limit = number(args, "limit", 50) as usize;
            let groups: Vec<Value> = report
                .groups
                .iter()
                .take(limit)
                .map(|g| json!({"size": g.size, "copies": g.copies.iter().map(|c| &c.path).collect::<Vec<_>>()}))
                .collect();
            let folders: Vec<Value> = report
                .folders
                .iter()
                .map(|f| json!({"a": f.a, "b": f.b, "files": f.files, "bytes": f.bytes}))
                .collect();
            Ok(Outcome {
                text: format!(
                    "{} of identical files, {} identical folder {}",
                    match report.groups.len() { 1 => "1 group".to_string(), n => format!("{n} groups") },
                    report.folders.len(),
                    if report.folders.len() == 1 { "pair" } else { "pairs" }
                ),
                data: json!({"groups": groups, "folders": folders, "files_checked": report.files, "truncated": report.groups.len() > limit, "stopped": report.stopped}),
            })
        }
        "find_similar_images" => {
            let folder = path_arg(args, "folder")?;
            let level = match args.get("similarity").and_then(Value::as_str).unwrap_or("very_similar") {
                "nearly_identical" => 0,
                "similar" => 2,
                _ => 1,
            };
            let cancel = std::sync::Arc::new(AtomicBool::new(false));
            let stop = cancel.clone();
            // Bounded: an agent call never runs for more than about a minute.
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(55));
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            });
            let scan = crate::similar::scan(std::slice::from_ref(&folder), &cancel, false, |_, _, _| {});
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            let found = crate::similar::groups(&scan.pictures, crate::similar::LEVELS[level].0);
            let limit = number(args, "limit", 50) as usize;
            let groups: Vec<Value> = found
                .iter()
                .take(limit)
                .map(|g| {
                    json!({"pictures": g.iter().map(|&i| {
                        let p = &scan.pictures[i];
                        json!({"path": p.path, "width": p.width, "height": p.height, "size": p.size})
                    }).collect::<Vec<_>>()})
                })
                .collect();
            // Say when the time ran out, and how many were really compared:
            // "0 groups among 4,026" after comparing a third of them misleads.
            let compared = scan.pictures.len();
            let partial = match &scan.stopped {
                Some(why) if why == "Stopped" => format!(
                    "Stopped after about a minute, having compared {compared} of {} pictures; \
                     check a smaller folder for the rest. ",
                    scan.seen
                ),
                Some(why) => format!("{why}. "),
                None => String::new(),
            };
            Ok(Outcome {
                text: format!(
                    "{partial}{} of similar (not identical) pictures among {compared} compared ({})",
                    match found.len() { 1 => "1 group".to_string(), n => format!("{n} groups") },
                    crate::similar::LEVELS[level].1.to_lowercase()
                ),
                data: json!({"groups": groups, "pictures_listed": scan.seen, "pictures_compared": compared, "truncated": found.len() > limit, "stopped": scan.stopped}),
            })
        }
        "largest_files" => {
            let index = need_index()?;
            let folder = path_arg(args, "folder")?;
            let mut query = Query::parse("", "").map_err(ToolError::invalid)?;
            let mut root = seek_engine::paths::internal(&folder);
            while root.last() == Some(&b'/') {
                root.pop();
            }
            query.scope = Some(root);
            query.limit = number(args, "limit", 50) as usize;
            query.hide_hidden_system = true;
            let items: Vec<Value> = index
                .largest(&query, false)
                .iter()
                .filter(|f| f.kind & 3 != seek_engine::walk::KIND_DIR)
                .map(found)
                .collect();
            Ok(page(items, &json!({"limit": number(args, "limit", 50)}), "results"))
        }
        "list_worktrees" => {
            let git = crate::worktrees::Git::find().ok_or_else(|| ToolError::failed("Git isn't installed."))?;
            let mut dot_gits = vec![];
            if let Some(index) = index
                && let Ok(mut q) = Query::parse("'.git", "")
            {
                q.limit = 20_000;
                dot_gits.extend(index.search(&q).unwrap_or_default().into_iter().map(|f| f.path).filter(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(".git"))));
            }
            dot_gits.extend(crate::worktrees::find_on_disk(Path::new(&home), &AtomicBool::new(false)));
            let repos = crate::worktrees::scan(&git, crate::worktrees::repositories_from(dot_gits));
            let items: Vec<Value> = repos
                .iter()
                .map(|r| {
                    json!({
                        "repository": r.common,
                        "error": r.error,
                        "worktrees": r.worktrees.iter().map(|w| json!({
                            "path": w.path, "branch": w.branch, "detached": w.detached, "main": w.main,
                            "locked": w.locked, "prunable": w.prunable, "state": format!("{:?}", w.state).to_lowercase(),
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect();
            Ok(Outcome {
                text: repos.iter().flat_map(|r| r.worktrees.iter().map(|w| w.path.display().to_string())).collect::<Vec<_>>().join("\n"),
                data: json!({"repositories": items}),
            })
        }
        "worktree_status" => {
            let path = path_arg(args, "path")?;
            let git = crate::worktrees::Git::find().ok_or_else(|| ToolError::failed("Git isn't installed."))?;
            let s = crate::worktrees::status(&git, &path).map_err(ToolError::failed)?;
            Ok(Outcome {
                text: if s.clean() { "Clean".into() } else { s.changes.join("\n") },
                data: json!({"path": path, "clean": s.clean(), "staged": s.staged, "unstaged": s.unstaged, "untracked": s.untracked, "conflicted": s.conflicted, "ahead": s.ahead, "behind": s.behind, "changes": s.changes}),
            })
        }
        _ => Err(ToolError::new("not_found", format!("No read-only tool \"{name}\"."), false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_text_masks_env_secrets_by_default_and_respects_setting() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let env_file = dir.path().join(".env.local");
        std::fs::write(&env_file, "# comment\nAPI_TOKEN=super_secret_token_42\n").unwrap();

        let out = call(
            "read_text",
            &json!({"path": env_file.to_string_lossy()}),
            None,
            &data,
        )
        .unwrap();
        assert!(out.text.contains("API_TOKEN="));
        assert!(out.text.contains("# comment"));
        assert!(!out.text.contains("super_secret_token_42"));
        assert_eq!(out.data["secrets_masked"], json!(true));
        assert!(!out.data["text"].as_str().unwrap().contains("super_secret_token_42"));

        // Turning on "Let agents read secret values" reveals real values.
        let mut settings = crate::settings::Settings::default();
        settings.agent_read_secrets = true;
        crate::settings::write(&data, &settings).unwrap();
        let revealed = call(
            "read_text",
            &json!({"path": env_file.to_string_lossy()}),
            None,
            &data,
        )
        .unwrap();
        assert!(revealed.text.contains("super_secret_token_42"));
        assert!(revealed.data.get("secrets_masked").is_none());
    }
}
