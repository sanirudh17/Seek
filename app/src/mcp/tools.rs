//! The tools Seek offers agents: names, descriptions, input schemas and
//! hints. This list is the contract (`contract/mcp-v1.json`); a test fails
//! when a tool, field or required input changes in a breaking way.
use serde_json::{Value, json};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    /// About 20 tools for finding, reading and changing files.
    Core,
    /// Core plus storage, duplicates, worktrees and saved items.
    All,
}

pub struct Tool {
    pub name: &'static str,
    pub core: bool,
    /// Changes files; runs only through the running app, and only when
    /// "Let agents change files" is on.
    pub changes: bool,
    pub destructive: bool,
    pub idempotent: bool,
    pub description: &'static str,
    pub input: fn() -> Value,
}

/// A JSON Schema object that rejects unknown fields.
fn object(fields: &[(&str, Value)], required: &[&str]) -> Value {
    let mut properties = serde_json::Map::new();
    for (name, schema) in fields {
        properties.insert((*name).into(), schema.clone());
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}
fn string(description: &str) -> Value {
    json!({"type": "string", "description": description})
}
fn integer(description: &str, min: u64, max: u64) -> Value {
    json!({"type": "integer", "minimum": min, "maximum": max, "description": description})
}
fn boolean(description: &str) -> Value {
    json!({"type": "boolean", "description": description})
}
fn paths(description: &str) -> Value {
    json!({"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 500, "description": description})
}
fn limit() -> Value {
    integer("Most results (default 50).", 1, 500)
}
fn cursor() -> Value {
    string("From a previous result's next_cursor.")
}
fn request_id() -> Value {
    string("Any unique string. Repeating it returns the first outcome instead of changing anything twice.")
}
fn wait() -> Value {
    integer("Seconds to wait for the job to finish (0-60, default 10).", 0, 60)
}

pub fn all() -> Vec<Tool> {
    vec![
        Tool {
            name: "search",
            core: true,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "Find files and folders by name across everything Seek indexes, ranked. Supports Seek's query language (ext:pdf, in:Documents, size:>10mb, mod:7d, kind:folder).",
            input: || object(&[("query", string("What to look for.")), ("folder", string("Only inside this folder.")), ("limit", limit()), ("cursor", cursor())], &["query"]),
        },
        Tool {
            name: "search_content",
            core: true,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "Find documents containing text (text files, PDF, Office), inside a folder.",
            input: || object(&[("text", string("Words to find.")), ("folder", string("Folder to search.")), ("limit", limit())], &["text", "folder"]),
        },
        Tool {
            name: "list_dir",
            core: true,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "List a folder's items (or a ZIP file's, read-only), newest first.",
            input: || object(&[("path", string("Folder or .zip path.")), ("show_hidden", boolean("Include hidden and system items.")), ("limit", limit()), ("cursor", cursor())], &["path"]),
        },
        Tool {
            name: "file_info",
            core: true,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "Size, dates and attributes of a file or folder, including whether it is a cloud placeholder or a link.",
            input: || object(&[("path", string("File or folder."))], &["path"]),
        },
        Tool {
            name: "largest",
            core: true,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "The largest files (or folders) Seek knows, optionally inside a folder.",
            input: || object(&[("folder", string("Only inside this folder.")), ("folders", boolean("Rank folders instead of files.")), ("limit", limit())], &[]),
        },
        Tool {
            name: "folder_size",
            core: true,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "Total size and file count of a folder, from the index.",
            input: || object(&[("path", string("Folder."))], &["path"]),
        },
        Tool {
            name: "read_text",
            core: true,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "Read part of a text file (at most 256 KB per call). Never downloads cloud placeholders.",
            input: || object(&[("path", string("Text file.")), ("offset", integer("Byte to start at.", 0, u64::MAX >> 1)), ("max_bytes", integer("Bytes to read (default 64 KB).", 1, 262_144))], &["path"]),
        },
        Tool {
            name: "recent",
            core: true,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "Files recently opened from Seek, newest first.",
            input: || object(&[("limit", limit())], &[]),
        },
        Tool {
            name: "favorites",
            core: true,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "The person's sidebar places: Desktop, Documents, Downloads and their favorites.",
            input: || object(&[], &[]),
        },
        Tool {
            name: "copy",
            core: true,
            changes: true,
            destructive: false,
            idempotent: false,
            description: "Copy items into a folder. Never overwrites: a name the folder already has is refused (name_taken). Returns a job.",
            input: || object(&[("paths", paths("Items to copy.")), ("folder", string("Destination folder.")), ("wait_seconds", wait()), ("request_id", request_id())], &["paths", "folder"]),
        },
        Tool {
            name: "move",
            core: true,
            changes: true,
            destructive: false,
            idempotent: false,
            description: "Move items into a folder. Never overwrites: a name the folder already has is refused (name_taken). Returns a job; undo puts them back.",
            input: || object(&[("paths", paths("Items to move.")), ("folder", string("Destination folder.")), ("wait_seconds", wait()), ("request_id", request_id())], &["paths", "folder"]),
        },
        Tool {
            name: "rename",
            core: true,
            changes: true,
            destructive: false,
            idempotent: false,
            description: "Rename one item. dry_run returns the plan and a token; confirm with that token runs exactly that plan once, within 10 minutes.",
            input: || object(&[("path", string("Item to rename.")), ("name", string("New name (not a path).")), ("dry_run", boolean("Only check and return a plan and token.")), ("confirm", string("A token from dry_run.")), ("wait_seconds", wait()), ("request_id", request_id())], &[]),
        },
        Tool {
            name: "trash",
            core: true,
            changes: true,
            destructive: true,
            idempotent: false,
            description: "Move items to the Recycle Bin (never deletes permanently). Returns a job; undo puts them back.",
            input: || object(&[("paths", paths("Items to move to the Recycle Bin.")), ("wait_seconds", wait()), ("request_id", request_id())], &["paths"]),
        },
        Tool {
            name: "create_folder",
            core: true,
            changes: true,
            destructive: false,
            idempotent: false,
            description: "Create a folder. Refuses a name that is taken.",
            input: || object(&[("folder", string("Where to create it.")), ("name", string("New folder's name.")), ("wait_seconds", wait()), ("request_id", request_id())], &["folder", "name"]),
        },
        Tool {
            name: "undo",
            core: true,
            changes: true,
            destructive: false,
            idempotent: false,
            description: "Undo Seek's most recent file operation (an agent's or the person's), like Ctrl+Z.",
            input: || object(&[("wait_seconds", wait()), ("request_id", request_id())], &[]),
        },
        Tool {
            name: "job_status",
            core: true,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "A job's state (queued, running, done, failed), optionally waiting for it to finish.",
            input: || object(&[("id", string("Job id.")), ("wait_seconds", wait())], &["id"]),
        },
        Tool {
            name: "storage_overview",
            core: false,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "What takes up space in a folder or drive, by kind (Developer, Apps, Documents, Pictures and so on) and by largest child folder.",
            input: || object(&[("path", string("Folder or drive root."))], &["path"]),
        },
        Tool {
            name: "find_duplicates",
            core: false,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "Groups of byte-identical files in a folder (size, then BLAKE3 samples, then full hashes), and folders whose whole contents match.",
            input: || object(&[("folder", string("Folder to check.")), ("limit", limit())], &["folder"]),
        },
        Tool {
            name: "find_similar_images",
            core: false,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "Groups of pictures in a folder tree that look nearly the same (resized, recompressed or lightly edited), by a perceptual difference hash checked on aspect ratio. Similar, not identical: compare before removing any; trash moves to the Recycle Bin.",
            input: || object(&[
                ("folder", string("Folder to check, with its subfolders.")),
                ("similarity", json!({"type": "string", "enum": ["nearly_identical", "very_similar", "similar"], "description": "How alike pictures must be (default very_similar)."})),
                ("limit", limit()),
            ], &["folder"]),
        },
        Tool {
            name: "largest_files",
            core: false,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "The biggest individual files inside a folder or drive, as Storage lists them (hidden and system files left out), with size and location.",
            input: || object(&[("folder", string("Folder or drive root.")), ("limit", limit())], &["folder"]),
        },
        Tool {
            name: "organize",
            core: false,
            changes: true,
            destructive: false,
            idempotent: false,
            description: "Sort one folder's files into subfolders inside it, by rules: by type (Documents, Images…), by date taken (Year\\Month from EXIF or the recorded date, else modified), or by name (camera sequences such as IMG_, and versions of one name such as v1, v2, final). Always dry_run first: it returns the groups and a token; confirm with that token runs exactly that plan once, within 10 minutes. Never overwrites (Keep Both); files changed since the dry run stay put; undo reverses it.",
            input: || object(&[
                ("folder", string("Folder to organize (required for dry_run).")),
                ("mode", json!({"type": "string", "enum": ["type", "date", "name"], "description": "How to sort (default type)."})),
                ("groups", json!({"type": "array", "items": {"type": "string"}, "maxItems": 200, "description": "Only these destination folders from the dry run (default: all)."})),
                ("dry_run", boolean("Only plan and return groups and a token.")),
                ("confirm", string("A token from dry_run.")),
                ("wait_seconds", wait()),
                ("request_id", request_id()),
            ], &[]),
        },
        Tool {
            name: "list_worktrees",
            core: false,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "Git repositories with linked worktrees on this PC, with each worktree's branch and state.",
            input: || object(&[], &[]),
        },
        Tool {
            name: "worktree_status",
            core: false,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "One worktree's changed, staged, untracked and conflicted files, and ahead/behind counts.",
            input: || object(&[("path", string("Worktree folder."))], &["path"]),
        },
        Tool {
            name: "saved_searches",
            core: false,
            changes: false,
            destructive: false,
            idempotent: true,
            description: "The person's recent and saved searches in Seek.",
            input: || object(&[("limit", limit())], &[]),
        },
    ]
}

pub fn find(name: &str, profile: Profile) -> Option<Tool> {
    all()
        .into_iter()
        .find(|t| t.name == name && (t.core || profile == Profile::All))
}

/// `tools/list` for a profile.
pub fn list(profile: Profile) -> Value {
    Value::Array(
        all()
            .into_iter()
            .filter(|t| t.core || profile == Profile::All)
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": (t.input)(),
                    "annotations": {
                        "readOnlyHint": !t.changes,
                        "destructiveHint": t.destructive,
                        "idempotentHint": t.idempotent,
                        "openWorldHint": false,
                    },
                })
            })
            .collect(),
    )
}

/// Unknown fields and wrong top-level types, as a model-readable message.
pub fn check_arguments(tool: &Tool, args: &Value) -> Result<(), String> {
    let schema = (tool.input)();
    let Some(args) = args.as_object() else {
        return Err("Arguments must be an object.".into());
    };
    let properties = schema["properties"].as_object().cloned().unwrap_or_default();
    for (key, value) in args {
        let Some(field) = properties.get(key) else {
            return Err(format!("Unknown argument \"{key}\"."));
        };
        let ok = match field["type"].as_str() {
            Some("string") => value.is_string(),
            Some("integer") => value.as_u64().is_some_and(|n| {
                n >= field["minimum"].as_u64().unwrap_or(0)
                    && n <= field["maximum"].as_u64().unwrap_or(u64::MAX)
            }),
            Some("boolean") => value.is_boolean(),
            Some("array") => value.as_array().is_some_and(|a| {
                !a.is_empty() && a.len() <= 500 && a.iter().all(Value::is_string)
            }),
            _ => true,
        };
        if !ok {
            return Err(format!("\"{key}\" has the wrong type or is out of range."));
        }
    }
    for required in schema["required"].as_array().into_iter().flatten() {
        let key = required.as_str().unwrap_or_default();
        if !args.contains_key(key) {
            return Err(format!("\"{key}\" is required."));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_is_strict_and_hinted_and_core_stays_small() {
        let core = list(Profile::Core);
        let all_tools = list(Profile::All);
        assert!(core.as_array().unwrap().len() <= 20);
        assert!(all_tools.as_array().unwrap().len() > core.as_array().unwrap().len());
        for tool in all_tools.as_array().unwrap() {
            assert_eq!(tool["inputSchema"]["additionalProperties"], false, "{}", tool["name"]);
            for hint in ["readOnlyHint", "destructiveHint", "idempotentHint", "openWorldHint"] {
                assert!(tool["annotations"][hint].is_boolean(), "{} {hint}", tool["name"]);
            }
        }
        // What core costs an agent's context stays under 16 KB.
        let size = serde_json::to_string(&core).unwrap().len();
        assert!(size < 16 * 1024, "core is {size} bytes");
    }

    #[test]
    fn arguments_are_checked() {
        let search = find("search", Profile::Core).unwrap();
        assert!(check_arguments(&search, &json!({"query": "x"})).is_ok());
        assert!(check_arguments(&search, &json!({"query": "x", "colour": 1})).is_err());
        assert!(check_arguments(&search, &json!({})).is_err());
        assert!(check_arguments(&search, &json!({"query": "x", "limit": 0})).is_err());
        assert!(find("storage_overview", Profile::Core).is_none());
        assert!(find("storage_overview", Profile::All).is_some());
    }
}

#[cfg_attr(not(test), allow(dead_code))]
/// The contract as stored in `contract/mcp-v1.json`: per tool, its profile,
/// hints, input fields with types, and required inputs.
pub fn contract() -> Value {
    let tools: Vec<Value> = all()
        .iter()
        .map(|t| {
            let schema = (t.input)();
            let fields: serde_json::Map<String, Value> = schema["properties"]
                .as_object()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|(k, v)| (k, v["type"].clone()))
                .collect();
            json!({
                "name": t.name,
                "core": t.core,
                "readOnly": !t.changes,
                "destructive": t.destructive,
                "idempotent": t.idempotent,
                "inputs": fields,
                "required": schema["required"],
            })
        })
        .collect();
    json!({"version": super::CONTRACT, "tools": tools})
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    /// Breaking changes fail; additions ask for the file to be refreshed
    /// (`SEEK_UPDATE_CONTRACT=1 cargo test -p seek contract`).
    #[test]
    fn the_live_tools_keep_the_contract() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("contract/mcp-v1.json");
        let live = contract();
        if std::env::var_os("SEEK_UPDATE_CONTRACT").is_some() || !path.exists() {
            std::fs::write(&path, serde_json::to_string_pretty(&live).unwrap() + "\n").unwrap();
        }
        let stored: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(stored["version"], live["version"], "the contract version changed");
        let find = |set: &Value, name: &str| set["tools"].as_array().unwrap().iter().find(|t| t["name"] == name).cloned();
        let mut additions = vec![];
        for old in stored["tools"].as_array().unwrap() {
            let name = old["name"].as_str().unwrap();
            let new = find(&live, name).unwrap_or_else(|| panic!("breaking: tool {name} was removed or renamed"));
            assert!(!(old["core"] == true && new["core"] == false), "breaking: {name} left core");
            for (field, kind) in old["inputs"].as_object().unwrap() {
                let now = new["inputs"].get(field).unwrap_or_else(|| panic!("breaking: {name}.{field} was removed"));
                assert_eq!(now, kind, "breaking: {name}.{field} changed type");
            }
            for required in new["required"].as_array().unwrap() {
                assert!(
                    old["required"].as_array().unwrap().contains(required),
                    "breaking: {name} has a new required input {required}"
                );
            }
            if new != *old {
                additions.push(name.to_string());
            }
        }
        for new in live["tools"].as_array().unwrap() {
            if find(&stored, new["name"].as_str().unwrap()).is_none() {
                additions.push(new["name"].as_str().unwrap().to_string());
            }
        }
        assert!(
            additions.is_empty(),
            "the contract grew ({additions:?}); refresh contract/mcp-v1.json with SEEK_UPDATE_CONTRACT=1 in the same commit"
        );
    }
}
