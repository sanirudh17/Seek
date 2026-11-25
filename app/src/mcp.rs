//! Seek as an MCP server for coding agents (Phase 10), after Glint's
//! design (`glint.exe --mcp`, a local control pipe) and Monodrive's
//! contract (docs/mcp.md): `seek.exe --mcp` speaks MCP over stdio.
//!
//! Every call goes to the running app over its pipe first, so one process
//! owns the index, the undo journal, the Recycle Bin and the settings. When
//! Seek isn't running, read-only tools run here against the saved index
//! (mapped for the one call); tools that change files start Seek in the
//! background and wait for its pipe, or answer `app_not_running`.
pub mod activity;
pub mod agents;
pub mod owner;
pub mod pipe;
pub mod read;
pub mod tools;

use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tools::Profile;

/// The contract's version (see docs/MCP.md and contract/mcp-v1.json).
pub const CONTRACT: u32 = 1;

const INSTRUCTIONS: &str = "Seek is a Windows file browser with a fast index of the person's files. \
Use search (Seek's query language: ext:pdf, in:Documents, size:>10mb, mod:7d) to find files, list_dir and \
file_info to look, read_text to read text. Paths are absolute or start with ~. Changes go through Seek: \
copy, move, rename, trash (Recycle Bin only), create_folder and undo; they need the person to turn on \
'Let agents change files' in Seek's Settings > Agents and only work inside the folders listed there. \
Changes return a job; pass wait_seconds or call job_status. Use request_id when retrying a change.";

/// A tool's successful result: terse text and the same as typed data.
pub struct Outcome {
    pub text: String,
    pub data: Value,
}

/// A failed call, with one of the contract's fixed codes.
#[derive(Debug, Clone)]
pub struct ToolError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
    pub path: Option<PathBuf>,
}

impl ToolError {
    pub fn new(code: &'static str, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
            path: None,
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_argument", message, false)
    }
    pub fn failed(message: impl Into<String>) -> Self {
        Self::new("failed", message, false)
    }
    pub fn not_found(message: impl Into<String>, path: &Path) -> Self {
        Self::new("not_found", message, false).at(path)
    }
    pub fn at(mut self, path: &Path) -> Self {
        self.path = Some(path.to_path_buf());
        self
    }
    pub fn to_json(&self) -> Value {
        json!({"code": self.code, "message": self.message, "retryable": self.retryable, "path": self.path})
    }
    pub fn from_json(value: &Value) -> Self {
        let code = match value["code"].as_str().unwrap_or("internal") {
            "invalid_argument" => "invalid_argument",
            "not_found" => "not_found",
            "permission_denied" => "permission_denied",
            "name_taken" => "name_taken",
            "changed_since_preview" => "changed_since_preview",
            "not_downloaded" => "not_downloaded",
            "agents_disabled" => "agents_disabled",
            "app_not_running" => "app_not_running",
            "index_unavailable" => "index_unavailable",
            "busy" => "busy",
            "failed" => "failed",
            _ => "internal",
        };
        Self {
            code,
            message: value["message"].as_str().unwrap_or_default().into(),
            retryable: value["retryable"].as_bool().unwrap_or(false),
            path: value["path"].as_str().map(PathBuf::from),
        }
    }
}

/// The MCP `tools/call` result for an outcome or an error.
fn result(outcome: Result<Outcome, ToolError>) -> Value {
    match outcome {
        Ok(o) => json!({"content": [{"type": "text", "text": o.text}], "structuredContent": o.data, "isError": false}),
        Err(e) => json!({
            "content": [{"type": "text", "text": format!("{} (error: {})", e.message, e.code)}],
            "structuredContent": {"error": e.to_json()},
            "isError": true,
        }),
    }
}

struct Session {
    data: PathBuf,
    pipe: String,
    profile: Profile,
    agent: String,
}

impl Session {
    /// One tool call: the app first, then the saved index for read-only
    /// tools, or a background start of Seek for changes.
    fn call(&self, name: &str, args: Value) -> Result<Outcome, ToolError> {
        let tool = tools::find(name, self.profile)
            .ok_or_else(|| ToolError::new("not_found", format!("No tool \"{name}\"."), false))?;
        tools::check_arguments(&tool, &args).map_err(ToolError::invalid)?;
        let request = json!({"tool": name, "args": args, "agent": self.agent});
        let reply = match pipe::call(&self.pipe, &request) {
            Ok(reply) => reply,
            Err(pipe::CallError::NotRunning) if !tool.changes && name != "job_status" => {
                let saved = seek_engine::SavedIndex::open(&self.data).ok();
                let index = saved.as_ref().map(|s| s as &dyn read::IndexView);
                // Dropped as soon as the call returns (see SavedIndex).
                return read::call(name, &args, index, &self.data);
            }
            Err(pipe::CallError::NotRunning) => {
                // Seek may be running with its pipe momentarily between
                // connections, or still starting: never start a second copy.
                if !pipe::app_running(&self.data) {
                    let exe = std::env::current_exe().map_err(|e| ToolError::failed(e.to_string()))?;
                    pipe::launch(&exe, &self.data).map_err(|e| ToolError::new("app_not_running", e, true))?;
                }
                let deadline = Instant::now() + Duration::from_secs(30);
                loop {
                    match pipe::call(&self.pipe, &request) {
                        Ok(reply) => break reply,
                        Err(pipe::CallError::NotRunning) if Instant::now() < deadline => {
                            std::thread::sleep(Duration::from_millis(200))
                        }
                        Err(_) => {
                            return Err(ToolError::new("app_not_running", "Seek didn't start in time. Open Seek and try again.", true));
                        }
                    }
                }
            }
            Err(pipe::CallError::Failed(e)) => return Err(ToolError::new("busy", e, true)),
        };
        if let Some(error) = reply.get("error") {
            return Err(ToolError::from_json(error));
        }
        Ok(Outcome {
            text: reply["text"].as_str().unwrap_or_default().into(),
            data: reply["data"].clone(),
        })
    }
}

fn respond(out: &mut impl Write, id: &Value, body: Result<Value, (i64, String)>) {
    let message = match body {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    };
    let _ = writeln!(out, "{message}");
    let _ = out.flush();
}

/// `seek.exe --mcp [--data <dir>] [--profile all]`: serves MCP on stdio
/// until stdin closes.
pub fn serve_stdio(data: PathBuf, profile_flag: Option<String>) -> i32 {
    let pipe = match pipe::name(&data) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("seek mcp: {e}");
            return 1;
        }
    };
    let profile = match profile_flag.as_deref() {
        Some("all") => Profile::All,
        Some(_) => Profile::Core,
        None if crate::settings::read(&data).agent_tools_all => Profile::All,
        None => Profile::Core,
    };
    let mut session = Session {
        data: data.clone(),
        pipe,
        profile,
        agent: "Unknown agent".into(),
    };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            respond(&mut out, &Value::Null, Err((-32700, "Parse error".into())));
            continue;
        };
        let method = message["method"].as_str().unwrap_or_default();
        let id = message.get("id").cloned();
        // Notifications get no reply.
        let Some(id) = id else { continue };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let body = match method {
            "initialize" => {
                if let Some(name) = params["clientInfo"]["name"].as_str() {
                    session.agent = activity::agent_name(name);
                }
                Ok(json!({
                    "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                    "capabilities": {"tools": {"listChanged": false}, "experimental": {"seek/contract": {"version": CONTRACT}}},
                    "serverInfo": {"name": "seek", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": INSTRUCTIONS,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools::list(session.profile)})),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default().to_string();
                if tools::find(&name, session.profile).is_none() {
                    Err((-32602, format!("Unknown tool: {name}")))
                } else {
                    let args = params.get("arguments").cloned().unwrap_or(json!({}));
                    let outcome = session.call(&name, args.clone());
                    activity::record(&data, &session.agent, &name, &args, outcome.is_ok());
                    Ok(result(outcome))
                }
            }
            _ => Err((-32601, format!("Method not found: {method}"))),
        };
        respond(&mut out, &id, body);
    }
    0
}
