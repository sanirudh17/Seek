# Seek for coding agents (MCP)

Seek is an MCP server, so coding agents (Claude Code, Codex, Antigravity CLI and others) can find, read and tidy files through it. The server is Seek's own executable: `seek.exe --mcp`. Code: `app/src/mcp.rs` and `app/src/mcp/`.

## How it runs

- `seek.exe --mcp [--data <dir>] [--profile all]` speaks MCP over stdio.
- Every call goes first to the running Seek over a named pipe, `\\.\pipe\Seek-<user SID>-<data folder hash>-agents`. The pipe refuses remote clients and admits only the signed-in user and SYSTEM. The running app is the only owner of the index, the undo journal, the Recycle Bin and the settings.
- With Seek closed, read-only tools run in the MCP process against the saved index, mapped for one call and released (Windows won't let the app replace a mapped file). Tools that change files start Seek minimized without focus (`--background`) and wait up to 30 s for its pipe; if Seek's single-instance lock is held, it never starts a second copy.
- The agent is named from the client name in `initialize`.

## The contract

`app/contract/mcp-v1.json` holds it, and a test compares it with the live tools: removing or renaming a tool or field, changing a field's type, adding a required input or moving a tool out of core fails; additions must refresh the file in the same commit (`SEEK_UPDATE_CONTRACT=1 cargo test -p seek contract`). `initialize` reports `capabilities.experimental["seek/contract"] = {"version": 1}`.

**Profiles.** `core` (16 tools, about 10 KB of names, descriptions and schemas): search, search_content, list_dir, file_info, largest, folder_size, read_text, recent, favorites, copy, move, rename, trash, create_folder, undo, job_status. `all` adds storage_overview, find_duplicates, find_similar_images, largest_files, organize (by type, date taken or name; dry_run then confirm token only), list_worktrees, worktree_status and saved_searches. Settings > Agents > Offer every tool, or `--profile all`, picks `all`.

**Shapes.** Every tool sets readOnlyHint, destructiveHint, idempotentHint and openWorldHint; input schemas reject unknown fields. Results carry terse text and the same as `structuredContent`. Lists take `limit` and return `truncated` and `next_cursor`. Paths are absolute or start with `~`.

**Errors** set `isError` and `structuredContent.error = {code, message, retryable, path?}` with one of: invalid_argument, not_found, permission_denied, name_taken, changed_since_preview, not_downloaded, agents_disabled, app_not_running, index_unavailable, busy, failed, internal. An unknown tool is a protocol error.

**Jobs.** copy, move, rename, trash, create_folder and undo return a job `{id, state, error, items}`; `wait_seconds` (up to 60) holds the reply until it ends, and job_status reads it later. **request_id** on a change returns the first outcome when repeated. **Dry runs:** rename with `dry_run` returns the plan and a token; `confirm` runs exactly that plan once within 10 minutes, and is refused if the item changed.

## Safety

- **Let agents change files** (Settings > Agents) is off by default and enforced by the app, not the client. Without it agents can only search and read.
- Changes happen only inside the folders listed on that page, checked after resolving links and junctions.
- Deletion is Recycle Bin only; there is no permanent delete. Copy and move never overwrite: a taken name is refused (Windows' Replace dialog would otherwise wait unseen).
- Cloud placeholders are never read or copied (`not_downloaded`).
- Changes run through the same journaled worker as the person's, so Ctrl+Z, or the `undo` tool, reverses them.
- The activity log (`agents-activity.jsonl` in Seek's data folder) keeps only time, agent, tool and file names, never folders or contents, capped at 256 KB; Settings > Agents shows it and can clear it.

## Connecting an agent

Settings > Agents lists the ten agents Glint supports, with their logos (lobe-icons, MIT). Installed ones (found by their command on PATH or their config folder) come first, each showing Active now, when it last used Seek, or Connect / Set up (read from the agent's own MCP config); "Set up another agent" covers the rest. Clients that aren't in the list but have used Seek get their own rows.

| Agent | Setup |
| --- | --- |
| Claude Code | Connect runs `claude mcp add --scope user seek -- <seek.exe> --mcp` |
| Codex | Connect runs `codex mcp add seek -- <seek.exe> --mcp` |
| OpenCode | Connect runs `opencode mcp add --global seek -- <seek.exe> --mcp`; detected from `~/.config/opencode/opencode.json(c)` (`mcp.servers` or the older `mcp`) |
| Antigravity | Connect runs `agy mcp add seek -- <seek.exe> --mcp` |
| GitHub Copilot CLI | `/mcp add`, or its `%USERPROFILE%\.copilot\mcp-config.json` snippet |
| Cursor | Its `%USERPROFILE%\.cursor\mcp.json` snippet |
| Claude Desktop | Its `%APPDATA%\Claude\claude_desktop_config.json` snippet |
| Oh My Pi, Grok Build | The command and argument to paste into their MCP settings |
| Pi | Install an MCP adapter extension into `%USERPROFILE%\.pigent\extensions`, then the same command |

The page always uses the running seek.exe's real path, and adds `--data` for a non-default profile. Connect shows the exact command and runs it only after the person confirms.

## Verified (24 November 2026, test profile)

The four tasks Monodrive used, on a fixture folder with Let agents change files on and only that folder allowed:

| Client | Result |
| --- | --- |
| Scripted client over stdio | All four: ZIP listed; duplicate found, trashed, request_id replayed, undone; rename dry run, confirm, spent token refused; copy job done. Also: a path outside the allowed folder refused, undo immediately after a trash in a fresh window, three queued copies, a copy onto a taken name refused |
| Claude Code 2.1.331 (`claude -p`, per-run `--mcp-config`) | All four from the tools alone: 11 turns, 24 s, $0.12 (16 input + 5,302 cache-creation + 238,458 cache-read + 1,596 output tokens) |
| Codex 0.172.0 | Not run: its configured model (and gpt-5.1-codex) isn't available on the signed-in ChatGPT account |
| Antigravity CLI | Not run: it has no per-run MCP config, and connecting it would change the person's settings |

The first Claude Code run found three gaps that are fixed: undo refused while the worker was busy or the history unloaded, a transient pipe failure starting a second Seek, and copies onto a taken name waiting on an unseen dialog.

Each Connect command was run against a throwaway home folder (24 November 2026: Claude Code, Codex, OpenCode, Antigravity), and Seek then read all four as connected. Antigravity needs `--` before an argument starting with `-`; OpenCode wants flags before the name and writes `mcp.servers`.

## Not built

From the brief's `all` profile: cleanup_duplicates, suggest_names, remove_worktree and run_command (each with dry run and token), and the saved commands list. The optional `ui` profile (navigate, reveal, select, set view, split) is not built.
