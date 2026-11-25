//! The agents' activity log: which agent did what to which file name.
//! Only the tool, the agent and file names are kept, never file contents
//! or folders, and the log is capped in size.
use serde_json::{Value, json};
use std::{io::Write, path::{Path, PathBuf}};

/// Largest the log may grow before its older half is dropped.
const CAP: u64 = 256 * 1024;

pub fn file(data: &Path) -> PathBuf {
    data.join("agents-activity.jsonl")
}

/// A readable name for an MCP client: a known agent's name ("claude-code"
/// → "Claude Code"), or the client's own name tidied ("acme-cli" → "Acme CLI").
pub fn agent_name(client: &str) -> String {
    if let Some(agent) = super::agents::agent_of(client) {
        return agent.name.into();
    }
    client
        .split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .take(6)
        .map(|w| match w.to_lowercase().as_str() {
            "cli" | "mcp" | "ai" | "ide" | "sdk" | "api" => w.to_uppercase(),
            _ => w[..1].to_uppercase() + &w[1..],
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The file names (not folders, never contents) a call touched.
fn names(args: &Value) -> Vec<String> {
    let mut out = vec![];
    for key in ["path", "folder", "query"] {
        if let Some(p) = args[key].as_str() {
            out.push(
                Path::new(p)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.chars().take(64).collect()),
            );
        }
    }
    for p in args["paths"].as_array().into_iter().flatten().take(20) {
        if let Some(p) = p.as_str() {
            out.push(Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        }
    }
    if let Some(n) = args["name"].as_str() {
        out.push(n.chars().take(64).collect());
    }
    out
}

pub fn record(data: &Path, agent: &str, tool: &str, args: &Value, ok: bool) {
    let line = json!({
        "time": chrono::Local::now().to_rfc3339(),
        "agent": agent,
        "tool": tool,
        "names": names(args),
        "ok": ok,
    });
    let path = file(data);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > CAP)
        && let Ok(text) = std::fs::read_to_string(&path)
    {
        let lines: Vec<&str> = text.lines().collect();
        let keep = lines[lines.len() / 2..].join("\n");
        let _ = std::fs::write(&path, keep + "\n");
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
}

/// The most recent entries, newest first.
pub fn read(data: &Path, limit: usize) -> Vec<Value> {
    let text = std::fs::read_to_string(file(data)).unwrap_or_default();
    text.lines()
        .rev()
        .filter_map(|l| serde_json::from_str(l).ok())
        .take(limit)
        .collect()
}

pub fn clear(data: &Path) {
    let _ = std::fs::remove_file(file(data));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_keeps_names_not_folders_and_stays_small() {
        let dir = tempfile::tempdir().unwrap();
        let args = json!({"paths": [r"C:\Users\me\Secret\plan.pdf"], "folder": r"C:\Users\me\Archive"});
        record(dir.path(), "Codex", "move", &args, true);
        let entry = &read(dir.path(), 10)[0];
        let text = entry.to_string();
        assert!(text.contains("plan.pdf"));
        assert!(!text.contains("Secret"), "{text}");
        for _ in 0..3000 {
            record(dir.path(), "Codex", "search", &json!({"query": "x".repeat(60)}), true);
        }
        assert!(std::fs::metadata(file(dir.path())).unwrap().len() < CAP + 64 * 1024);
        assert_eq!(agent_name("claude-code"), "Claude Code");
        clear(dir.path());
        assert!(read(dir.path(), 10).is_empty());
    }
}
