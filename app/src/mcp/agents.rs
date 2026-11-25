//! Settings > Agents: which coding agents are on this PC, whether each
//! already launches Seek's MCP server, and how to connect it. The same ten
//! agents Glint supports (adapted from its agents.rs); each
//! agent's own config is only read, never written (Connect runs the agent's
//! own `mcp add` command, after the person confirms).
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Setup {
    /// A Connect button runs the agent's own `mcp add` command.
    Command,
    /// Cursor: a snippet for its mcp.json.
    CursorJson,
    /// GitHub Copilot CLI: `/mcp add`, or its mcp-config.json.
    CopilotJson,
    /// Claude Desktop: its claude_desktop_config.json.
    DesktopJson,
    /// Oh My Pi and Grok Build: the command and argument to paste.
    Paste,
    /// Pi has no MCP of its own: an adapter extension, then the command.
    PiAdapter,
}

pub struct Agent {
    pub id: &'static str,
    pub name: &'static str,
    pub command: &'static str,
    pub setup: Setup,
    /// The logo (from Glint, drawn from lobe-icons), and whether it is a
    /// single-colour mark drawn in the theme's text colour.
    pub logo: &'static str,
    pub ink: bool,
}

const fn agent(id: &'static str, name: &'static str, command: &'static str, setup: Setup, logo: &'static str, ink: bool) -> Agent {
    Agent { id, name, command, setup, logo, ink }
}

pub const AGENTS: [Agent; 10] = [
    agent("claude-code", "Claude Code", "claude", Setup::Command, "agents/claude-code.svg", false),
    agent("codex", "Codex", "codex", Setup::Command, "agents/codex.svg", true),
    agent("opencode", "OpenCode", "opencode", Setup::Command, "agents/opencode.svg", true),
    agent("antigravity", "Antigravity", "agy", Setup::Command, "agents/antigravity.svg", false),
    agent("copilot", "GitHub Copilot CLI", "copilot", Setup::CopilotJson, "agents/copilot.svg", true),
    agent("cursor", "Cursor", "cursor-agent", Setup::CursorJson, "agents/cursor.svg", true),
    agent("claude-desktop", "Claude Desktop", "", Setup::DesktopJson, "agents/claude-desktop.svg", false),
    agent("grok", "Grok Build", "grok", Setup::Paste, "agents/grok.svg", true),
    agent("oh-my-pi", "Oh My Pi", "omp", Setup::Paste, "agents/oh-my-pi.svg", true),
    agent("pi", "Pi", "pi", Setup::PiAdapter, "agents/pi.svg", false),
];

/// The agent an MCP client name belongs to, if Seek knows it.
pub fn agent_of(client: &str) -> Option<&'static Agent> {
    let c = client.to_lowercase();
    let id = if c == "claude-ai" || c.contains("claude desktop") || c.contains("claude-desktop") {
        "claude-desktop"
    } else if c.contains("claude") {
        "claude-code"
    } else if c.contains("codex") {
        "codex"
    } else if c.contains("opencode") {
        "opencode"
    } else if c.contains("antigravity") || c == "agy" {
        "antigravity"
    } else if c.contains("copilot") {
        "copilot"
    } else if c.contains("oh-my-pi") || c.contains("oh my pi") || c == "omp" {
        "oh-my-pi"
    } else if c == "pi" || c.starts_with("pi-") || c.contains("pi coding") {
        "pi"
    } else if c.contains("grok") {
        "grok"
    } else if c.contains("cursor") {
        "cursor"
    } else {
        return None;
    };
    AGENTS.iter().find(|a| a.id == id)
}

#[derive(Clone, Debug)]
pub struct Detected {
    pub index: usize,
    pub installed: bool,
    /// The agent's command on PATH, when it is there.
    pub program: Option<PathBuf>,
    pub connected: bool,
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default()
}

/// The full path of a command on PATH (.exe, .cmd, .bat or .ps1).
pub fn on_path(command: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        ["exe", "cmd", "bat", "ps1"]
            .iter()
            .map(|ext| dir.join(format!("{command}.{ext}")))
            .find(|p| p.is_file())
    })
}

/// OpenCode's opencode.json(c): `"mcp": {"servers": {"seek": {"command":
/// ["seek.exe", "--mcp"]}}}` (what `opencode mcp add` writes now), or the
/// older `"mcp": {"seek": {...}}`.
fn opencode_uses_seek(config: &Value) -> bool {
    let mcp = &config["mcp"];
    let servers = if mcp["servers"].is_object() { &mcp["servers"] } else { mcp };
    servers.as_object().is_some_and(|servers| {
        servers.values().any(|s| {
            let parts: Vec<String> = s["command"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            parts.first().is_some_and(|c| launches_seek(c, &parts[1..]))
        })
    })
}

/// Whether a server entry launches Seek's MCP server.
fn launches_seek(command: &str, args: &[String]) -> bool {
    let stem = Path::new(command.trim_matches('"'))
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    stem == "seek" && args.iter().any(|a| a == "--mcp")
}

fn servers_use_seek(servers: Option<&Value>) -> bool {
    servers.and_then(Value::as_object).is_some_and(|servers| {
        servers.values().any(|s| {
            let args: Vec<String> = s["args"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            s["command"].as_str().is_some_and(|c| launches_seek(c, &args))
        })
    })
}

/// `//` and `/* */` comments removed outside strings (JSONC configs).
fn strip_jsonc(text: &str) -> String {
    let (mut out, mut chars, mut in_str) = (String::new(), text.chars().peekable(), false);
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == '/' && chars.peek() == Some(&'/') {
            for n in chars.by_ref() {
                if n == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut last = ' ';
            for n in chars.by_ref() {
                if last == '*' && n == '/' {
                    break;
                }
                last = n;
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&strip_jsonc(&std::fs::read_to_string(path).ok()?)).ok()
}

fn mcp_file(path: &Path) -> bool {
    read_json(path).is_some_and(|c| servers_use_seek(c.get("mcpServers")))
}

/// Codex's config.toml: an `[mcp_servers.*]` table whose command is Seek's.
pub fn codex_uses_seek(text: &str) -> bool {
    let quoted = |s: &str| -> Vec<String> {
        s.split(['"', '\''])
            .enumerate()
            .filter(|(i, _)| i % 2 == 1)
            .map(|(_, v)| v.replace("\\\\", "\\"))
            .collect()
    };
    let (mut in_server, mut found) = (false, false);
    let (mut command, mut args) = (String::new(), Vec::new());
    for line in text.lines().map(str::trim).chain(std::iter::once("[end]")) {
        if line.starts_with('[') {
            found |= !command.is_empty() && launches_seek(&command, &args);
            command.clear();
            args.clear();
            in_server = line.starts_with("[mcp_servers.") && !line.contains(".env");
            continue;
        }
        if !in_server {
            continue;
        }
        if let Some(v) = line.strip_prefix("command").map(str::trim_start).and_then(|r| r.strip_prefix('=')) {
            command = quoted(v).into_iter().next().unwrap_or_default();
        } else if let Some(v) = line.strip_prefix("args").map(str::trim_start).and_then(|r| r.strip_prefix('=')) {
            args = quoted(v);
        }
    }
    found
}

/// Installed (command on PATH or its home folder) and connected (its own
/// MCP config launches Seek), for each of the seven.
fn detect_one(agent: &Agent) -> (bool, bool) {
    let home = home();
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default();
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
    let command = !agent.command.is_empty() && on_path(agent.command).is_some();
    match agent.id {
        "claude-code" => {
            let config = read_json(&home.join(".claude.json"));
            let connected = config.as_ref().is_some_and(|c| {
                servers_use_seek(c.get("mcpServers"))
                    || c["projects"]
                        .as_object()
                        .is_some_and(|p| p.values().any(|p| servers_use_seek(p.get("mcpServers"))))
            });
            (command || home.join(".claude").is_dir(), connected)
        }
        "codex" => {
            let dir = home.join(".codex");
            let connected = std::fs::read_to_string(dir.join("config.toml")).is_ok_and(|t| codex_uses_seek(&t));
            (command || dir.is_dir(), connected)
        }
        "antigravity" => {
            let gemini = home.join(".gemini");
            let connected = mcp_file(&gemini.join("config").join("mcp_config.json"))
                || mcp_file(&gemini.join("antigravity").join("mcp_config.json"));
            (command || gemini.join("antigravity").is_dir() || local.join(r"Programs\Antigravity").is_dir(), connected)
        }
        "opencode" => {
            let dir = home.join(r".config\opencode");
            let connected = ["opencode.json", "opencode.jsonc"]
                .iter()
                .filter_map(|f| read_json(&dir.join(f)))
                .any(|c| opencode_uses_seek(&c));
            (command || dir.is_dir(), connected)
        }
        "copilot" => {
            let dir = home.join(".copilot");
            (command || dir.is_dir(), mcp_file(&dir.join("mcp-config.json")))
        }
        "claude-desktop" => {
            let config = appdata.join(r"Claude\claude_desktop_config.json");
            (config.is_file() || local.join("AnthropicClaude").is_dir(), mcp_file(&config))
        }
        "pi" => {
            let dir = home.join(".pi");
            (command || dir.is_dir(), mcp_file(&dir.join(r"agent\mcp.json")))
        }
        "oh-my-pi" => {
            let dir = home.join(".omp");
            (command || dir.is_dir(), mcp_file(&dir.join(r"agent\mcp.json")) || mcp_file(&dir.join("mcp.json")))
        }
        "grok" => (command || home.join(".grok").is_dir(), mcp_file(&home.join(r".grok\mcp.json"))),
        "cursor" => {
            let dir = home.join(".cursor");
            (command || dir.is_dir(), mcp_file(&dir.join("mcp.json")))
        }
        _ => (false, false),
    }
}

/// Every supported agent, with whether it is installed on this PC.
pub fn detect() -> Vec<Detected> {
    AGENTS
        .iter()
        .enumerate()
        .map(|(index, agent)| {
            let (installed, connected) = detect_one(agent);
            Detected {
                index,
                installed,
                program: (!agent.command.is_empty()).then(|| on_path(agent.command)).flatten(),
                connected,
            }
        })
        .collect()
}

/// The arguments after Seek's own path: `--mcp`, and `--data` when this
/// Seek uses a data folder other than the default.
pub fn server_args(data: &Path) -> Vec<String> {
    let default = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Seek");
    let mut args = vec!["--mcp".to_string()];
    if crate::worktrees::normal(data) != crate::worktrees::normal(&default) {
        args.push("--data".into());
        args.push(data.display().to_string());
    }
    args
}

/// The agent's own `mcp add` arguments for Seek (Claude Code, Codex,
/// Antigravity), pointing at the running seek.exe.
pub fn connect_args(agent: &Agent, exe: &Path, data: &Path) -> Vec<String> {
    let exe = exe.display().to_string();
    let mut args: Vec<String> = match agent.id {
        "claude-code" => vec!["mcp", "add", "--scope", "user", "seek", "--", &exe],
        "codex" => vec!["mcp", "add", "seek", "--", &exe],
        "opencode" => vec!["mcp", "add", "--global", "seek", "--", &exe],
        "antigravity" => vec!["mcp", "add", "seek", "--", &exe],
        _ => vec![],
    }
    .into_iter()
    .map(str::to_string)
    .collect();
    args.extend(server_args(data));
    args
}

/// What the page shows for agents set up by hand.
pub fn instructions(agent: &Agent, exe: &Path, data: &Path) -> String {
    let args = server_args(data);
    match agent.setup {
        Setup::CursorJson => serde_json::to_string_pretty(&serde_json::json!({
            "mcpServers": {"seek": {"command": exe, "args": args}}
        }))
        .unwrap_or_default(),
        Setup::CopilotJson => format!(
            "Run /mcp add inside Copilot CLI, or add this to %USERPROFILE%\\.copilot\\mcp-config.json:\n{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {"seek": {"type": "local", "command": exe, "args": args, "tools": ["*"]}}
            }))
            .unwrap_or_default()
        ),
        Setup::DesktopJson => format!(
            "Add this to %APPDATA%\\Claude\\claude_desktop_config.json (Settings > Developer > Edit Config), then restart Claude Desktop:\n{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {"seek": {"command": exe, "args": args}}
            }))
            .unwrap_or_default()
        ),
        Setup::Paste => format!(
            "Add Seek as a local (stdio) MCP server in {}'s MCP settings:\nCommand: {}\nArguments: {}",
            agent.name,
            exe.display(),
            args.join(" ")
        ),
        Setup::PiAdapter => format!(
            "Pi has no built-in MCP. Install an MCP adapter extension into %USERPROFILE%\\.pi\\agent\\extensions, then use:\nCommand: {}\nArguments: {}",
            exe.display(),
            args.join(" ")
        ),
        Setup::Command => {
            let program = agent.command;
            format!("{program} {}", connect_args(agent, exe, data).join(" "))
        }
    }
}

/// Runs the agent's `mcp add` (an argument list, never a shell).
pub fn connect(agent: &Agent, program: &Path, exe: &Path, data: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new(program)
        .args(connect_args(agent, exe, data))
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|e| format!("Couldn't run {}: {e}", agent.command))?;
    if output.status.success() {
        Ok(())
    } else {
        let mut message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if message.is_empty() {
            message = String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
        Err(if message.is_empty() { format!("{} mcp add failed", agent.command) } else { message })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configs_that_launch_seek_count_as_connected() {
        let toml = "[mcp_servers.seek]\ncommand = 'C:\\\\Apps\\\\Seek\\\\seek.exe'\nargs = [\"--mcp\"]\n[other]\n";
        assert!(codex_uses_seek(toml));
        assert!(!codex_uses_seek("[mcp_servers.glint]\ncommand = \"glint.exe\"\nargs = [\"--mcp\"]\n"));
        // OpenCode: what `opencode mcp add --global` writes, and the older layout.
        let new = serde_json::json!({"mcp": {"servers": {"seek": {"type": "local", "command": [r"C:\Apps\Seek\seek.exe", "--mcp"]}}}});
        let old = serde_json::json!({"mcp": {"seek": {"type": "local", "command": [r"C:\Apps\Seek\seek.exe", "--mcp"]}}});
        assert!(opencode_uses_seek(&new) && opencode_uses_seek(&old));
        assert!(!opencode_uses_seek(&serde_json::json!({"mcp": {"servers": {"glint": {"command": ["glint.exe", "--mcp"]}}}})));
        let json: Value = serde_json::from_str(&strip_jsonc(
            "{ // comment\n \"mcpServers\": {\"seek\": {\"command\": \"C:/x/seek.exe\", \"args\": [\"--mcp\"]}} }",
        ))
        .unwrap();
        assert!(servers_use_seek(json.get("mcpServers")));
    }

    #[test]
    fn connect_commands_point_at_this_seek() {
        let exe = Path::new(r"C:\Apps\Seek\seek.exe");
        let default = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Seek");
        let claude = connect_args(&AGENTS[0], exe, &default);
        assert_eq!(claude, ["mcp", "add", "--scope", "user", "seek", "--", r"C:\Apps\Seek\seek.exe", "--mcp"]);
        let other = connect_args(&AGENTS[1], exe, Path::new(r"C:\Test\profile"));
        assert!(other.ends_with(&["--mcp".to_string(), "--data".to_string(), r"C:\Test\profile".to_string()]));
        assert!(instructions(&AGENTS[5], exe, &default).contains("mcpServers"));
        let opencode = connect_args(&AGENTS[2], exe, &default);
        assert_eq!(opencode, ["mcp", "add", "--global", "seek", "--", r"C:\Apps\Seek\seek.exe", "--mcp"]);
        assert_eq!(agent_of("claude-code").unwrap().id, "claude-code");
        assert_eq!(agent_of("claude-ai").unwrap().id, "claude-desktop");
        assert_eq!(agent_of("opencode").unwrap().id, "opencode");
        assert!(agent_of("seek-check").is_none());
    }
}

#[cfg(test)]
mod on_this_pc {
    /// Read-only: which supported agents this PC has.
    #[test]
    #[ignore]
    fn detected_here() {
        for d in super::detect() {
            println!("{} installed={} connected={} program={:?}", super::AGENTS[d.index].name, d.installed, d.connected, d.program);
        }
    }
}
