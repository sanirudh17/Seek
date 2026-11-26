//! Settings > Agents (Phase 10.5), after Glint's Agents page: the coding
//! agents on this PC with their logos and whether each is connected, Connect
//! or setup steps for each, "Set up another agent" for the rest, what agents
//! may change, and their recent activity in plain words.
use super::*;
use crate::appearance::Palette;
use crate::mcp::agents::{AGENTS, Agent, Detected, Setup};
use gpui_component::{menu::DropdownMenu, switch::Switch};

#[derive(Default)]
pub(super) struct AgentsPage {
    pub(super) detected: Option<Vec<Detected>>,
    log: Vec<serde_json::Value>,
    /// The agent whose Connect waits for the person's confirmation.
    confirm: Option<usize>,
    /// The agent whose setup steps are open.
    open: Option<usize>,
    /// The last Connect's outcome, by agent.
    outcome: Option<(usize, Result<(), String>)>,
    loading: bool,
    /// A refresh is scheduled while the page is showing.
    polling: bool,
}

/// Seconds since an RFC 3339 time.
fn age(time: &str) -> Option<i64> {
    let t = chrono::DateTime::parse_from_rfc3339(time).ok()?;
    Some((chrono::Local::now().fixed_offset() - t).num_seconds().max(0))
}

fn ago(time: &str) -> String {
    let Ok(t) = chrono::DateTime::parse_from_rfc3339(time) else {
        return String::new();
    };
    match age(time).unwrap_or(0) {
        0..=59 => "just now".into(),
        s @ 60..=3599 => format!("{} min ago", s / 60),
        s @ 3600..=86_399 => format!("{} h ago", s / 3600),
        _ => t.format("%d %b").to_string(),
    }
}

/// What an agent did, as a sentence and the file it touched.
fn describe(entry: &serde_json::Value) -> (String, Option<String>) {
    let names: Vec<&str> = entry["names"].as_array().into_iter().flatten().filter_map(|n| n.as_str()).collect();
    let first = names.first().map(|n| n.to_string());
    let (text, target): (&str, bool) = match entry["tool"].as_str().unwrap_or_default() {
        "search" => ("Searched for", true),
        "search_content" => ("Searched documents for", true),
        "list_dir" => ("Looked in", true),
        "file_info" => ("Looked at", true),
        "largest" => ("Listed the largest files", false),
        "folder_size" => ("Measured", true),
        "read_text" => ("Read", true),
        "recent" => ("Listed recent files", false),
        "favorites" => ("Listed your places", false),
        "copy" => ("Copied", true),
        "move" => ("Moved", true),
        "rename" => ("Renamed", true),
        "trash" => ("Moved to the Recycle Bin:", true),
        "create_folder" => ("Created a folder in", true),
        "undo" => ("Undid the last change", false),
        "job_status" => ("Checked on a change", false),
        "storage_overview" => ("Looked at storage in", true),
        "find_duplicates" => ("Looked for duplicates in", true),
        "list_worktrees" => ("Listed Git worktrees", false),
        "worktree_status" => ("Checked the worktree", true),
        "saved_searches" => ("Listed saved searches", false),
        other => (other, true),
    };
    let target = if target { first } else { None };
    let more = names.len().saturating_sub(1);
    let target = target.map(|t| if more > 0 && matches!(entry["tool"].as_str(), Some("copy" | "move" | "trash")) { format!("{t} and {more} more") } else { t });
    (text.to_string(), target)
}

/// Colour logos rasterised once per theme text colour and kept, so the
/// page draws them on its first frame instead of loading them afterwards.
fn rendered(agent: &Agent, bytes: &[u8], p: Palette) -> Option<std::sync::Arc<gpui::RenderImage>> {
    use std::{collections::HashMap, sync::{Arc, LazyLock, Mutex}};
    type Rendered = HashMap<(&'static str, u32), Arc<gpui::RenderImage>>;
    static CACHE: LazyLock<Mutex<Rendered>> =
        LazyLock::new(Default::default);
    let text: gpui::Rgba = p.text.into();
    let key = (agent.logo, u32::from(text));
    if let Some(image) = CACHE.lock().ok()?.get(&key) {
        return Some(image.clone());
    }
    let hex = format!("#{:06x}", u32::from(text) >> 8);
    let mut source = String::from_utf8_lossy(bytes).replace("currentColor", &hex);
    if agent.ink {
        source = source.replacen("<svg ", &format!("<svg fill=\"{hex}\" "), 1);
    }
    // Sized for a 32 px tile on a 2x display; smaller tiles scale it down.
    let width = source
        .split("viewBox=\"")
        .nth(1)
        .and_then(|v| v.split('"').next())
        .and_then(|v| v.split_whitespace().nth(2)?.parse::<f32>().ok())
        .unwrap_or(256.);
    let image = gpui::SvgRenderer::new(std::sync::Arc::new(()))
        .render_single_frame(source.as_bytes(), 72. / width.max(1.))
        .ok()?;
    CACHE.lock().ok()?.insert(key, image.clone());
    Some(image)
}

/// An agent's logo in a small rounded tile; a terminal mark for clients
/// Seek doesn't know.
fn logo(agent: Option<&Agent>, size: f32, p: Palette) -> Div {
    let mark = (size * 0.56).round();
    let bytes = agent.and_then(|a| crate::ui_icons::load(a.logo));
    // A one-colour mark with shades (OpenCode's) can't go through the icon
    // mask, which drops opacity, so it is drawn as an image in the text colour.
    let shaded = bytes.is_some_and(|b| b.windows(7).any(|w| w == b"opacity"));
    let inner: AnyElement = match agent {
        Some(a) if a.ink && !shaded => svg().path(a.logo).size(px(mark)).text_color(p.text).into_any_element(),
        Some(a) => match bytes.and_then(|bytes| rendered(a, bytes, p)) {
            Some(image) => img(image).size(px(mark)).into_any_element(),
            None => div().into_any_element(),
        },
        None => Icon::new(gpui_component_assets::IconName::SquareTerminal)
            .size(px(mark))
            .text_color(p.muted)
            .into_any_element(),
    };
    div()
        .size(px(size))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(size * 0.26))
        .border_1()
        .border_color(p.line)
        .bg(p.text.opacity(0.04))
        .child(inner)
}

impl Seek {
    /// Detection and the activity log, read once without starting the
    /// page's refresh loop.
    pub(super) fn prefetch_agents(&mut self, cx: &mut Context<Self>) {
        let data = self.store.directory.as_ref().clone();
        let job = cx.background_executor().spawn(async move {
            (crate::mcp::agents::detect(), crate::mcp::activity::read(&data, 40))
        });
        cx.spawn(async move |this, cx| {
            let (detected, log) = job.await;
            let _ = this.update(cx, |this, cx| {
                if this.agents.detected.is_none() {
                    this.agents.detected = Some(detected);
                    this.agents.log = log;
                    this.agents.loading = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn refresh_agents(&mut self, cx: &mut Context<Self>) {
        self.agents.loading = self.agents.detected.is_none();
        let data = self.store.directory.as_ref().clone();
        let job = cx.background_executor().spawn(async move {
            (crate::mcp::agents::detect(), crate::mcp::activity::read(&data, 40))
        });
        cx.spawn(async move |this, cx| {
            let (detected, log) = job.await;
            let _ = this.update(cx, |this, cx| {
                this.agents.detected = Some(detected);
                this.agents.log = log;
                this.agents.loading = false;
                cx.notify();
            });
        })
        .detach();
        // While the page shows, refresh every 1.5 s (Glint: 2.5 s), so a
        // connection, a removal or an agent's action appears without pressing
        // anything.
        if !self.agents.polling {
            self.agents.polling = true;
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
                    let showing = this
                        .update(cx, |this, _| this.settings && this.settings_tab == 11)
                        .unwrap_or(false);
                    if !showing {
                        let _ = this.update(cx, |this, _| this.agents.polling = false);
                        return;
                    }
                    let data = this.update(cx, |this, _| this.store.directory.as_ref().clone()).ok();
                    let Some(data) = data else { return };
                    let fresh = cx
                        .background_executor()
                        .spawn(async move { (crate::mcp::agents::detect(), crate::mcp::activity::read(&data, 40)) })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.agents.detected = Some(fresh.0);
                        this.agents.log = fresh.1;
                        cx.notify();
                    });
                }
            })
            .detach();
        }
        cx.notify();
    }

    fn connect_agent(&mut self, index: usize, cx: &mut Context<Self>) {
        self.agents.confirm = None;
        let Some(program) = self
            .agents
            .detected
            .as_ref()
            .and_then(|d| d.iter().find(|a| a.index == index))
            .and_then(|a| a.program.clone())
        else {
            self.agents.outcome = Some((index, Err("Its command isn't on PATH, so Seek can't run it. Use the setup steps instead.".into())));
            self.agents.open = Some(index);
            cx.notify();
            return;
        };
        let data = self.store.directory.as_ref().clone();
        let job = cx.background_executor().spawn(async move {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            crate::mcp::agents::connect(&AGENTS[index], &program, &exe, &data)
        });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                this.agents.outcome = Some((index, result));
                this.refresh_agents(cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn add_agent_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Let agents change files in".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                let _ = this.update_in(cx, |this, window, cx| {
                    for path in paths {
                        if !this.preferences.agent_folders.contains(&path) {
                            this.preferences.agent_folders.push(path);
                        }
                    }
                    this.save_preferences(window, cx);
                });
            }
        })
        .detach();
    }

    fn agent_link(&self, id: impl Into<ElementId>, label: &'static str, p: Palette) -> Stateful<Div> {
        use crate::tokens::*;
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .h(px(26.))
            .rounded(RADIUS_CONTROL)
            .cursor_pointer()
            .text_size(TEXT_SMALL)
            .font_weight(FontWeight::MEDIUM)
            .text_color(p.accent)
            .hover(move |s| s.bg(p.accent.opacity(0.1)))
            .child(label)
            .child(Icon::new(gpui_component_assets::IconName::ArrowUpRight).size(px(12.)))
    }

    pub(super) fn agents_settings(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        use super::settings_view::{control, section};
        use crate::tokens::*;
        let state = &self.agents;
        let exe = std::env::current_exe().unwrap_or_default();
        let data = self.store.directory.as_ref().clone();
        let subtitle = |text: &'static str| {
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.muted)
                .child(text)
        };
        let card = || {
            div()
                .w_full()
                .flex()
                .flex_col()
                .rounded(RADIUS_CARD)
                .border_1()
                .border_color(p.text.opacity(0.1))
                .bg(p.settings_card)
        };
        let detected = state.detected.clone().unwrap_or_default();
        let last_used = |name: &str| {
            state
                .log
                .iter()
                .find(|e| e["agent"].as_str() == Some(name))
                .and_then(|e| e["time"].as_str())
                .map(str::to_string)
        };
        let installed: Vec<&Detected> = detected
            .iter()
            .filter(|d| d.installed || last_used(AGENTS[d.index].name).is_some())
            .collect();
        let others: Vec<usize> = detected
            .iter()
            .filter(|d| !d.installed && last_used(AGENTS[d.index].name).is_none())
            .map(|d| d.index)
            .collect();
        // "Set up another agent": the ones not on this PC.
        let owner = cx.entity().downgrade();
        let menu_items = others.clone();
        let setup_menu = (!others.is_empty()).then(move || {
            Button::new("agents-setup-another")
                .ghost()
                .h(px(26.))
                .px_2()
                .text_size(TEXT_SMALL)
                .text_color(p.muted)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child("Set up another agent")
                        .child(Icon::new(gpui_component_assets::IconName::ChevronDown).size(px(12.))),
                )
                .dropdown_menu(move |menu, window, _| {
                    let mut menu = super::daily::menu_style(menu, window);
                    for index in menu_items.clone() {
                        let owner = owner.clone();
                        menu = menu.item(gpui_component::menu::PopupMenuItem::new(AGENTS[index].name).on_click(
                            move |_, _, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.agents.open = Some(index);
                                    this.agents.confirm = None;
                                    cx.notify();
                                });
                            },
                        ));
                    }
                    menu
                })
        });
        let mut rows = card();
        let row = |id: ElementId| {
            div()
                .id(id)
                .min_h(px(60.))
                .px_4()
                .flex()
                .items_center()
                .gap_3()
        };
        // An agent not on this PC whose setup was picked from the menu.
        let picked = state.open.filter(|i| others.contains(i));
        let shown: Vec<usize> = installed.iter().map(|d| d.index).chain(picked).collect();
        if state.detected.is_some() && shown.is_empty() {
            rows = rows.child(
                div()
                    .py_8()
                    .px_6()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .children(["claude-code", "codex", "opencode"].iter().map(|id| logo(AGENTS.iter().find(|a| a.id == *id), 36., p))),
                    )
                    .child(div().text_size(TEXT_BODY).font_weight(FontWeight::MEDIUM).child("No coding agents found"))
                    .child(
                        div()
                            .max_w(px(420.))
                            .text_center()
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child("Install an agent such as Claude Code, Codex or OpenCode, then connect it to let it find, read and tidy files with Seek."),
                    ),
            );
        } else if state.detected.is_none() {
            rows = rows.child(div().px_4().py_5().text_size(TEXT_SMALL).text_color(p.muted).child("Looking for agents…"));
        }
        for (position, index) in shown.iter().copied().enumerate() {
            let agent = &AGENTS[index];
            let found = detected.iter().find(|d| d.index == index);
            let connected = found.is_some_and(|d| d.connected);
            let used = last_used(agent.name);
            let live = used.as_deref().and_then(age).is_some_and(|s| s < 120);
            let right: AnyElement = if live {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(TEXT_SMALL)
                    .child(div().size(px(7.)).rounded_full().bg(p.tints.green))
                    .child("Active now")
                    .into_any_element()
            } else if connected || used.is_some() {
                div()
                    .text_size(TEXT_SMALL)
                    .text_color(p.muted)
                    .child(match &used {
                        Some(t) => format!("Last used {}", ago(t)),
                        None => "Connected".into(),
                    })
                    .into_any_element()
            } else {
                let label = if agent.setup == Setup::Command { "Connect" } else { "Set up" };
                self.agent_link(("agent-connect", index), label, p)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if AGENTS[index].setup == Setup::Command {
                            this.agents.confirm = if this.agents.confirm == Some(index) { None } else { Some(index) };
                            this.agents.open = None;
                        } else {
                            this.agents.open = if this.agents.open == Some(index) { None } else { Some(index) };
                            this.agents.confirm = None;
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            };
            let mut block = div()
                .flex()
                .flex_col()
                .when(position > 0, |d| d.border_t_1().border_color(p.line))
                .child(
                    row(("agent-row", index).into())
                        .child(logo(Some(agent), 32., p))
                        .child(div().flex_1().text_size(TEXT_BODY).font_weight(FontWeight::MEDIUM).child(agent.name))
                        .child(right),
                );
            let panel = || {
                div()
                    .mx_4()
                    .mb_4()
                    .p_3()
                    .rounded(RADIUS_CONTROL)
                    .bg(p.text.opacity(0.04))
                    .flex()
                    .flex_col()
                    .gap_2()
            };
            if state.confirm == Some(index) {
                let command = crate::mcp::agents::instructions(agent, &exe, &data);
                block = block.child(
                    panel()
                        .child(div().text_size(TEXT_SMALL).child(format!("Seek will run this to add itself to {}:", agent.name)))
                        .child(div().font_family(FONT_MONO).text_size(TEXT_SMALL).text_color(p.muted).child(command))
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap_2()
                                .child(
                                    control(("agent-connect-cancel", index), "Cancel", p, cx)
                                        .h(BUTTON_HEIGHT)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.agents.confirm = None;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new(("agent-connect-run", index))
                                        .h(BUTTON_HEIGHT)
                                        .px_3()
                                        .rounded(RADIUS_CONTROL)
                                        .bg(p.text)
                                        .text_color(crate::appearance::modal_surface(p))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(super::interface::button_text("Connect"))
                                        .on_click(cx.listener(move |this, _, _, cx| this.connect_agent(index, cx))),
                                ),
                        ),
                );
            }
            if state.open == Some(index) {
                let text = crate::mcp::agents::instructions(agent, &exe, &data);
                let copy = text.clone();
                block = block.child(
                    panel()
                        .child(div().font_family(FONT_MONO).text_size(TEXT_SMALL).child(text))
                        .child(
                            div().flex().justify_end().child(
                                control(("agent-setup-copy", index), "Copy", p, cx)
                                    .h(BUTTON_HEIGHT)
                                    .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))),
                            ),
                        ),
                );
            }
            if let Some((i, result)) = &state.outcome
                && *i == index
            {
                block = block.child(
                    div()
                        .px_4()
                        .pb_3()
                        .text_size(TEXT_SMALL)
                        .text_color(p.muted)
                        .child(match result {
                            Ok(()) => format!("Connected. Restart {} to use Seek.", agent.name),
                            Err(e) => format!("Couldn't connect: {e}"),
                        }),
                );
            }
            rows = rows.child(block);
        }
        // What agents may do.
        let can_change = Switch::new("agents-can-change")
            .color(p.accent)
            .checked(self.preferences.agents_can_change)
            .accessibility_label("Let agents change files")
            .on_change(cx.listener(|this, checked, window, cx| {
                this.preferences.agents_can_change = *checked;
                this.save_preferences(window, cx);
            }));
        let all_tools = Switch::new("agents-all-tools")
            .color(p.accent)
            .checked(self.preferences.agent_tools_all)
            .accessibility_label("Offer every tool")
            .on_change(cx.listener(|this, checked, window, cx| {
                this.preferences.agent_tools_all = *checked;
                this.save_preferences(window, cx);
            }));
        let read_secrets = Switch::new("agents-read-secrets")
            .color(p.accent)
            .checked(self.preferences.agent_read_secrets)
            .accessibility_label("Let agents read secret values")
            .on_change(cx.listener(|this, checked, window, cx| {
                this.preferences.agent_read_secrets = *checked;
                this.save_preferences(window, cx);
            }));
        let setting = |id: &'static str, title: &'static str, detail: &'static str, control: AnyElement| {
            div()
                .id(id)
                .w_full()
                .px_4()
                .py_4()
                .flex()
                .items_center()
                .gap_6()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_size(TEXT_BODY).font_weight(FontWeight::MEDIUM).child(title))
                        .child(div().w_full().text_size(TEXT_SMALL).line_height(px(18.)).text_color(p.muted).child(detail)),
                )
                .child(div().flex_shrink_0().child(control))
        };
        let mut folders = div().px_4().pb_4().flex().flex_col().gap_1();
        for (index, folder) in self.preferences.agent_folders.iter().enumerate() {
            folders = folders.child(
                div()
                    .w_full()
                    .h(px(34.))
                    .pl_3()
                    .pr_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(RADIUS_CONTROL)
                    .bg(p.text.opacity(0.04))
                    .child(Icon::new(gpui_component_assets::IconName::Folder).size(px(14.)).flex_shrink_0().text_color(p.muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(TEXT_SMALL)
                            .child(folder.to_string_lossy().into_owned()),
                    )
                    .child(
                        div()
                            .id(("agent-folder-remove", index))
                            .role(Role::Button)
                            .aria_label("Remove")
                            .size(px(26.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_CONTROL)
                            .cursor_pointer()
                            .text_color(p.muted)
                            .hover(move |s| s.bg(p.hover).text_color(p.text))
                            .child(Icon::new(gpui_component_assets::IconName::Close).size(px(12.)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if index < this.preferences.agent_folders.len() {
                                    this.preferences.agent_folders.remove(index);
                                    this.save_preferences(window, cx);
                                }
                            })),
                    ),
            );
        }
        let permissions = card()
            .child(setting(
                "agents-change-row",
                "Let agents change files",
                "Agents can always search and read. With this on, they can also copy, move, rename and recycle, and you can undo every change.",
                can_change.into_any_element(),
            ))
            .child(
                div()
                    .border_t_1()
                    .border_color(p.line)
                    .child(setting(
                        "agents-folders-row",
                        "Folders agents may change",
                        if self.preferences.agent_folders.is_empty() { "None yet, so agents change nothing." } else { "Nothing outside these folders, even through links." },
                        control("agent-folder-add", "Add folder…", p, cx)
                            .h(BUTTON_HEIGHT)
                            .on_click(cx.listener(|this, _, window, cx| this.add_agent_folder(window, cx)))
                            .into_any_element(),
                    ))
                    .child(folders),
            )
            .child(
                div().border_t_1().border_color(p.line).child(setting(
                    "agents-tools-row",
                    "Offer every tool",
                    "Adds storage, duplicates, worktrees and saved searches. Off keeps agents' context small.",
                    all_tools.into_any_element(),
                )),
            )
            .child(
                div().border_t_1().border_color(p.line).child(setting(
                    "agents-secrets-row",
                    "Let agents read secret values",
                    "Values in .env files are masked when agents call read_text. Turn this on only when you want agents to see the real values.",
                    read_secrets.into_any_element(),
                )),
            );
        // Recent activity, in plain words.
        let mut activity = card();
        let entries: Vec<&serde_json::Value> = state
            .log
            .iter()
            .filter(|e| e["tool"].as_str() != Some("job_status"))
            .take(12)
            .collect();
        if entries.is_empty() {
            activity = activity.child(
                div()
                    .px_4()
                    .py_5()
                    .text_size(TEXT_SMALL)
                    .text_color(p.muted)
                    .child("Nothing yet. What agents do with Seek appears here: the agent, the action and file names, never file contents."),
            );
        }
        for (position, entry) in entries.iter().enumerate() {
            let (text, target) = describe(entry);
            let agent = crate::mcp::agents::agent_of(entry["agent"].as_str().unwrap_or_default())
                .or_else(|| AGENTS.iter().find(|a| Some(a.name) == entry["agent"].as_str()));
            activity = activity.child(
                div()
                    .h(px(44.))
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .when(position > 0, |d| d.border_t_1().border_color(p.line))
                    .child(logo(agent, 22., p))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(TEXT_SMALL)
                            .child(div().flex_shrink_0().text_color(p.muted).child(text))
                            .when_some(target, |d, t| {
                                d.child(div().min_w_0().overflow_hidden().text_ellipsis().font_weight(FontWeight::MEDIUM).child(t))
                            })
                            .when(entry["ok"] == false, |d| d.child(div().flex_shrink_0().text_color(p.tints.orange).child("· failed"))),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child(ago(entry["time"].as_str().unwrap_or_default())),
                    ),
            );
        }
        let clear = (!entries.is_empty()).then(|| {
            div()
                .id("agents-clear-log")
                .role(Role::Button)
                .px_2()
                .h(px(26.))
                .flex()
                .items_center()
                .rounded(RADIUS_CONTROL)
                .cursor_pointer()
                .text_size(TEXT_SMALL)
                .text_color(p.muted)
                .hover(move |s| s.bg(p.hover).text_color(p.text))
                .child("Clear")
                .on_click(cx.listener(|this, _, _, cx| {
                    crate::mcp::activity::clear(this.store.directory.as_ref());
                    this.agents.log.clear();
                    cx.notify();
                }))
        });
        let head = |left: Div, right: Option<AnyElement>| {
            div()
                .mb_2()
                .flex()
                .items_center()
                .justify_between()
                .child(left)
                .children(right)
        };
        section("Agents")
            .child(head(subtitle("Your agents"), setup_menu.map(|b| b.into_any_element())))
            .child(rows.mb_6())
            .child(head(subtitle("What agents may do"), None))
            .child(permissions.mb_6())
            .child(head(subtitle("Recent activity"), clear.map(|c| c.into_any_element())))
            .child(activity)
            .into_any_element()
    }
}
