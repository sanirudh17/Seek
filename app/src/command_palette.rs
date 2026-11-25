use super::*;
use gpui_component::ActiveTheme;
use gpui_component::command::{Command as Palette, CommandGroup, CommandItem, CommandState};

#[derive(Clone)]
enum Destination {
    Location(Option<PathBuf>),
    Search(crate::settings::Search),
    RemoveSearch(crate::settings::Search),
    ClearRecent,
    RecentFile(PathBuf),
    SavedCommand(crate::saved_commands::SavedCommand),
    Settings(usize),
    View(View),
    NewTab,
    NewGroup,
    Related,
    Pane(PaneCommand),
    History,
    Info,
    /// Storage (false) or Find duplicates: identical files (Some(false)) or
    /// similar pictures (Some(true)).
    Tool(Option<bool>),
    File(super::daily::Action),
}
#[derive(Clone, Copy)]
pub(super) enum PaneCommand {
    SplitRight,
    SplitDown,
    Close,
    Next,
    CopyToOther,
    MoveToOther,
    Zoom,
}
#[derive(Clone)]
struct Entry {
    label: String,
    keywords: Vec<String>,
    destination: Destination,
    enabled: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    Locations,
    Searches,
    RecentFiles,
    Programs,
    Views,
    Files,
    Operations,
    Settings,
}
impl Group {
    const ALL: [Self; 8] = [
        Self::Locations,
        Self::Searches,
        Self::RecentFiles,
        Self::Programs,
        Self::Views,
        Self::Files,
        Self::Operations,
        Self::Settings,
    ];
    /// One Hugeicons glyph per group, so rows read at a glance.
    fn icon(self) -> &'static str {
        match self {
            Self::Locations => "icons/folder.svg",
            Self::Searches => "icons/search.svg",
            Self::RecentFiles => "icons/clock.svg",
            Self::Programs => "icons/terminal.svg",
            Self::Views => "icons/layout-grid.svg",
            Self::Files => "icons/file-text.svg",
            Self::Operations => "icons/refresh-cw.svg",
            Self::Settings => "icons/settings.svg",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Locations => "Locations",
            Self::Searches => "Searches",
            Self::RecentFiles => "Recent files",
            Self::Programs => "Saved commands",
            Self::Views => "Views and tabs",
            Self::Files => "Files",
            Self::Operations => "Operations",
            Self::Settings => "Settings",
        }
    }
    fn aliases(self) -> &'static str {
        match self {
            Self::Locations => "navigation favorites drives folders",
            Self::Searches => "queries bookmarks filters",
            Self::RecentFiles => "recent opened documents",
            Self::Programs => "programs run executables",
            Self::Views => "layout display tabs",
            Self::Files => "file actions clipboard selection",
            Self::Operations => "history recovery undo redo interrupted",
            Self::Settings => "preferences configuration",
        }
    }
}
impl Entry {
    fn group(&self) -> Group {
        match self.destination {
            Destination::Location(_) => Group::Locations,
            Destination::Search(_)
            | Destination::RemoveSearch(_)
            | Destination::ClearRecent
            | Destination::File(daily::Action::SaveSearch) => Group::Searches,
            Destination::RecentFile(_) => Group::RecentFiles,
            Destination::SavedCommand(_) => Group::Programs,
            Destination::Settings(_) => Group::Settings,
            Destination::View(_)
            | Destination::NewTab
            | Destination::NewGroup
            | Destination::Related
            | Destination::Pane(_)
            | Destination::Info
            | Destination::Tool(_) => Group::Views,
            Destination::History
            | Destination::File(
                daily::Action::CancelOperations
                | daily::Action::UndoRename
                | daily::Action::RedoRename
                | daily::Action::PutBack,
            ) => Group::Operations,
            Destination::File(_) => Group::Files,
        }
    }
    fn search_keywords(&self) -> Vec<String> {
        let mut words = self.keywords.clone();
        words.push(self.group().label().into());
        words.push(self.group().aliases().into());
        words
    }
}
/// The label a row shows: "Go to Downloads" reads as "Downloads" under
/// Locations, and a long path keeps its last folders.
fn display_label(label: &str) -> String {
    let label = label.strip_prefix("Go to ").unwrap_or(label);
    if label.chars().count() > 64 && label.contains('\\') {
        let parts: Vec<&str> = label.split('\\').collect();
        let tail = parts[parts.len().saturating_sub(2)..].join("\\");
        format!("…\\{tail}")
    } else {
        label.to_string()
    }
}
fn grouped(entries: &[Entry]) -> Vec<(Group, Vec<Entry>)> {
    Group::ALL
        .into_iter()
        .filter_map(|group| {
            let rows: Vec<_> = entries
                .iter()
                .filter(|entry| entry.group() == group)
                .cloned()
                .collect();
            (!rows.is_empty()).then_some((group, rows))
        })
        .collect()
}
fn confirmed_entry(groups: &[(Group, Vec<Entry>)], section: usize, row: usize) -> Option<Entry> {
    groups
        .get(section)?
        .1
        .get(row)
        .filter(|entry| entry.enabled)
        .cloned()
}
pub(super) struct State {
    pub(super) state: Entity<CommandState>,
    previous: Option<FocusHandle>,
    entries: Vec<Entry>,
    selection: std::collections::HashSet<PathBuf>,
    selected_path: Option<PathBuf>,
    scope: Option<PathBuf>,
}
impl Seek {
    /// Everything Jump offers right now. Rebuilt when it opens and again
    /// while it is open, as the index and places change.
    fn palette_entries(&self, cx: &mut Context<Self>) -> Vec<Entry> {
        let mut entries = vec![Entry {
            label: "Go to All locations".into(),
            keywords: vec!["jump search everywhere".into()],
            destination: Destination::Location(None),
            enabled: true,
        }];
        for (name, path) in &self.favorites {
            entries.push(Entry {
                label: format!("Go to {name}"),
                keywords: vec![
                    path.to_string_lossy().into_owned(),
                    "jump folder favorite".into(),
                ],
                destination: Destination::Location(Some(path.clone())),
                enabled: true,
            });
        }
        for path in &self.roots {
            if self.favorites.iter().any(|(_, favorite)| favorite == path) {
                continue;
            }
            entries.push(Entry {
                label: format!(
                    "Go to {}",
                    path.to_string_lossy().trim_start_matches(r"\\?\")
                ),
                keywords: vec!["jump location drive folder".into()],
                destination: Destination::Location(Some(path.clone())),
                enabled: true,
            });
        }
        for (saved, searches) in [
            (true, &self.preferences.saved_searches),
            (false, &self.preferences.recent_searches),
        ] {
            for search in searches {
                entries.push(Entry {
                    label: format!(
                        "{} search: {}{}",
                        if saved { "Saved" } else { "Recent" },
                        if search.content { "Contents: " } else { "" },
                        search.query
                    ),
                    keywords: vec!["query recall find filter".into()],
                    destination: Destination::Search(search.clone()),
                    enabled: true,
                });
            }
        }
        entries.push(Entry {
            label: "Save current search".into(),
            keywords: vec!["query bookmark keep filter".into()],
            destination: Destination::File(daily::Action::SaveSearch),
            enabled: !self.input.read(cx).value().trim().is_empty() && !self.settings,
        });
        for search in &self.preferences.saved_searches {
            entries.push(Entry {
                label: format!("Remove saved search: {}", search.query),
                keywords: vec!["manage bookmark delete query".into()],
                destination: Destination::RemoveSearch(search.clone()),
                enabled: true,
            });
        }
        if !self.preferences.recent_searches.is_empty() {
            entries.push(Entry {
                label: "Clear recent searches".into(),
                keywords: vec!["history remove queries".into()],
                destination: Destination::ClearRecent,
                enabled: true,
            });
        }
        for path in &self.places.recent {
            entries.push(Entry {
                label: format!(
                    "Locate recent: {}",
                    path.file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy()
                ),
                keywords: vec![
                    "opened file history reveal".into(),
                    path.to_string_lossy().into_owned(),
                ],
                destination: Destination::RecentFile(path.clone()),
                enabled: true,
            });
        }
        let command_paths = self.command_selection(None, cx);
        for command in &self.preferences.saved_commands {
            entries.push(Entry {
                label: format!("Run: {}", command.name),
                keywords: vec!["saved command program launch".into()],
                destination: Destination::SavedCommand(command.clone()),
                enabled: !self.settings
                    && command
                        .expand(&command_paths, self.scope.as_deref())
                        .is_ok(),
            });
        }
        for (page, name, _, keywords) in super::settings_view::SETTINGS_PAGES {
            entries.push(Entry {
                label: format!("Settings: {name}"),
                keywords: vec![keywords.into(), "preferences".into()],
                destination: Destination::Settings(page),
                enabled: true,
            });
        }
        for (name, view) in [
            ("List", View::List),
            ("Icons", View::Icons),
            ("Gallery", View::Gallery),
            ("Columns", View::Columns),
        ] {
            entries.push(Entry {
                label: format!("View: {name}"),
                keywords: vec!["layout display".into()],
                destination: Destination::View(view),
                enabled: true,
            });
        }
        for (label, keywords, tool) in [
            ("Storage", "disk space usage largest folders cleanup", None),
            ("Find duplicates", "identical copies same files storage", Some(false)),
            ("Find similar images", "duplicate photos pictures alike near", Some(true)),
        ] {
            entries.push(Entry {
                label: label.into(),
                keywords: vec![keywords.into()],
                destination: Destination::Tool(tool),
                enabled: !self.settings,
            });
        }
        entries.push(Entry {
            label: "Operation history".into(),
            keywords: vec!["files recovery interrupted".into()],
            destination: Destination::History,
            enabled: !self.settings,
        });
        entries.push(Entry {
            label: "New tab".into(),
            keywords: vec!["session Ctrl T".into()],
            destination: Destination::NewTab,
            enabled: true,
        });
        entries.push(Entry {
            label: if self.related_visible {
                "Hide related folders".into()
            } else {
                "Show related folders".into()
            },
            keywords: vec!["tab chips links linked".into()],
            destination: Destination::Related,
            enabled: !self.settings,
        });
        entries.push(Entry {
            label: "New sidebar group".into(),
            keywords: vec!["favorites organise organize sidebar".into()],
            destination: Destination::NewGroup,
            enabled: !self.settings,
        });
        let split = self.split_mode();
        for (label, keywords, command, enabled) in [
            (
                "Split right",
                "pane side by side Ctrl+\\",
                PaneCommand::SplitRight,
                true,
            ),
            (
                "Split down",
                "pane stacked Ctrl+Shift+\\",
                PaneCommand::SplitDown,
                true,
            ),
            (
                "Close pane",
                "split Ctrl+Shift+W",
                PaneCommand::Close,
                split,
            ),
            ("Focus next pane", "split F6", PaneCommand::Next, split),
            (
                if self.zoomed {
                    "Show all panes"
                } else {
                    "Zoom pane"
                },
                "split maximize fill focus Ctrl+Shift+Enter",
                PaneCommand::Zoom,
                split,
            ),
            (
                "Copy to other pane",
                "split duplicate Alt+F5",
                PaneCommand::CopyToOther,
                split,
            ),
            (
                "Move to other pane",
                "split transfer Alt+F6",
                PaneCommand::MoveToOther,
                split,
            ),
        ] {
            entries.push(Entry {
                label: label.into(),
                keywords: vec![keywords.into()],
                destination: Destination::Pane(command),
                enabled: enabled && !self.settings,
            });
        }
        // A keyboard caret alone is not a selection (for example after Ctrl+click deselect).
        let selection = &self.table.read(cx).delegate().selected_many;
        let selected = !selection.is_empty();
        let single = selection.len() == 1;
        let selected_row = self
            .selected(cx)
            .filter(|row| selection.contains(&row.file.path));
        let previewable = selected_row
            .as_ref()
            .is_some_and(|row| row.file.kind & 3 == seek_engine::walk::KIND_FILE);
        let destination = self.scope.is_some()
            || selected_row
                .as_ref()
                .is_some_and(|row| row.file.kind & 3 == seek_engine::walk::KIND_DIR);
        entries.push(Entry {
            label: "Toggle information panel".into(),
            keywords: vec!["inspector details Ctrl I".into()],
            destination: Destination::Info,
            enabled: selected && !self.settings,
        });
        for (name, action, enabled) in [
            ("Open selected item", daily::Action::Open, selected),
            (
                "Cancel file operations",
                daily::Action::CancelOperations,
                self.files_pending != 0,
            ),
            ("Quick Look", daily::Action::QuickLook, previewable),
            (
                "Open in document pane",
                daily::Action::DocumentPane,
                previewable,
            ),
            ("Reveal in Explorer", daily::Action::Reveal, selected),
            ("Copy path", daily::Action::Path, selected),
            ("Copy name", daily::Action::Name, selected),
            ("Copy path in quotes", daily::Action::QuotedPath, selected),
            ("Open with...", daily::Action::OpenWith, selected),
            ("Share...", daily::Action::Share, selected),
            ("Move to Recycle Bin", daily::Action::Recycle, selected),
            (
                "Delete permanently...",
                daily::Action::DeletePermanently,
                selected,
            ),
            ("Copy files", daily::Action::Copy, selected),
            ("Cut files", daily::Action::Cut, selected),
            ("Paste files", daily::Action::Paste, destination),
            ("Rename", daily::Action::Rename, single),
            ("Batch rename", daily::Action::BatchRename, selected),
            ("Suggest names", daily::Action::SuggestNames, selected),
            ("New folder", daily::Action::NewFolder, destination),
            (
                "New folder with selection",
                daily::Action::GroupSelection,
                selected,
            ),
            (
                "Put Back last recycled items",
                daily::Action::PutBack,
                self.can_put_back(),
            ),
            (
                "Undo last operation",
                daily::Action::UndoRename,
                self.can_reverse_rename(false),
            ),
            (
                "Redo last operation",
                daily::Action::RedoRename,
                self.can_reverse_rename(true),
            ),
            ("Properties", daily::Action::Properties, selected),
            (
                "Organize folder (by type, date taken or name)",
                daily::Action::Organize,
                self.scope.is_some(),
            ),
        ] {
            entries.push(Entry {
                label: name.into(),
                keywords: vec!["file action".into()],
                destination: Destination::File(action),
                enabled: enabled && !self.settings,
            });
        }
        entries
    }
    /// Jump is open and the index changed: refresh its entries in place,
    /// keeping the query and the focused row.
    pub(super) fn refresh_command_palette(&mut self, cx: &mut Context<Self>) {
        if self.command_palette.is_none() {
            return;
        }
        let entries = self.palette_entries(cx);
        if let Some(palette) = &mut self.command_palette
            && palette.entries.iter().map(|e| (&e.label, e.enabled)).ne(entries.iter().map(|e| (&e.label, e.enabled)))
        {
            palette.entries = entries;
            cx.notify();
        }
    }
    pub(super) fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::WindowExt;
        if self.history_open
            || self.preview_enlarged
            || self.edit.is_some()
            || self.permanent_delete.is_some()
            || window.has_active_dialog(cx)
        {
            return;
        }
        if self.command_palette.is_some() {
            self.close_command_palette(window, cx);
            return;
        }
        let entries = self.palette_entries(cx);
        let previous = window.focused(cx);
        let state = cx.new(|cx| CommandState::new(window, cx));
        state.update(cx, |state, cx| state.focus(window, cx));
        self.suggestions = None;
        self.command_palette = Some(State {
            state,
            previous,
            entries,
            selection: self.table.read(cx).delegate().selected_many.clone(),
            selected_path: self.selected(cx).map(|row| row.file.path),
            scope: self.scope.clone(),
        });
        cx.notify();
    }
    pub(super) fn close_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(palette) = self.command_palette.take() {
            if let Some(focus) = palette.previous {
                focus.focus(window, cx);
            } else {
                self.table.read(cx).focus_handle(cx).focus(window, cx);
            }
            cx.notify();
        }
    }
    pub(super) fn command_palette_view(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(palette) = &self.command_palette else {
            return div().into_any_element();
        };
        let owner = cx.entity().downgrade();
        let cancel_owner = owner.clone();
        let groups = grouped(&palette.entries);
        let selection = palette.selection.clone();
        let selected_path = palette.selected_path.clone();
        let scope = palette.scope.clone();
        let mut commands = Palette::new(&palette.state)
            .text_size(px(13.))
            .placeholder("Jump to a folder or run a command");
        for (group, entries) in &groups {
            commands = commands.group(CommandGroup::new().label(group.label()).items(
                entries.iter().map(|entry| {
                    let mut keywords = entry.search_keywords();
                    keywords.push(entry.label.clone());
                    CommandItem::new()
                        .icon(Icon::empty().path(group.icon()))
                        .label(display_label(&entry.label))
                        .keywords(keywords)
                        .disabled(!entry.enabled)
                }),
            ));
        }
        let commands = commands
            .max_h(px(380.))
            .bordered(false)
            // GPUI clips children to a rectangle, so the surface itself is
            // rounded to stay inside the frame's corners.
            .rounded(crate::tokens::RADIUS_CARD)
            .empty(|_, _, _| {
                div()
                    .py_6()
                    .w_full()
                    .text_center()
                    .text_size(crate::tokens::TEXT_SMALL)
                    .child("No matches")
            })
            .on_cancel(move |window, cx| {
                let _ = cancel_owner.update(cx, |this, cx| this.close_command_palette(window, cx));
            })
            .on_confirm(move |index, window, cx| {
                let Some(entry) = confirmed_entry(&groups, index.section, index.row) else {
                    return;
                };
                let _ = owner.update(cx, |this, cx| {
                    let changed = matches!(
                        entry.destination,
                        Destination::File(_) | Destination::Info | Destination::SavedCommand(_)
                    ) && (selection != this.table.read(cx).delegate().selected_many
                        || selected_path != this.selected(cx).map(|row| row.file.path)
                        || scope != this.scope);
                    this.close_command_palette(window, cx);
                    if changed {
                        this.message =
                            Some("Selection or location changed. Choose the command again.".into());
                        cx.notify();
                        return;
                    }
                    match entry.destination {
                        Destination::RemoveSearch(search) => {
                            this.preferences.saved_searches.retain(|row| row != &search);
                            this.store.save(&this.preferences);
                        }
                        Destination::ClearRecent => {
                            this.preferences.recent_searches.clear();
                            this.store.save(&this.preferences);
                        }
                        Destination::RecentFile(path) => {
                            if let Some(parent) = path.parent() {
                                this.settings = false;
                                this.scope_to(Some(parent.into()), window, cx);
                            }
                        }
                        Destination::SavedCommand(command) => {
                            let paths = this.command_selection(None, cx);
                            this.invoke_saved_command(command, paths, window, cx);
                        }
                        Destination::Search(search) => this.recall_search(search, window, cx),
                        Destination::Location(path) => {
                            this.settings = false;
                            this.scope_to(path, window, cx);
                            this.table.read(cx).focus_handle(cx).focus(window, cx);
                        }
                        Destination::Settings(page) => {
                            this.settings = true;
                            this.settings_tab = page;
                            this.settings_search
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            this.settings_open_motion = this.settings_open_motion.wrapping_add(1);
                            this.settings_page_motion = this.settings_page_motion.wrapping_add(1);
                            this.settings_focus.focus(window, cx);
                            let _ = this.backend.tx.send(Command::HelperStatus);
                        }
                        Destination::View(view) => {
                            this.settings = false;
                            this.set_view(view, cx);
                        }
                        Destination::NewTab => {
                            this.settings = false;
                            this.new_tab(None, window, cx);
                        }
                        Destination::NewGroup => this.new_sidebar_group(window, cx),
                        Destination::Related => this.toggle_related(cx),
                        Destination::Pane(command) => {
                            use crate::pane_layout::Axis;
                            match command {
                                PaneCommand::SplitRight => {
                                    this.split_pane(Axis::Horizontal, None, window, cx)
                                }
                                PaneCommand::SplitDown => {
                                    this.split_pane(Axis::Vertical, None, window, cx)
                                }
                                PaneCommand::Close => this.close_pane(window, cx),
                                PaneCommand::Next => this.focus_adjacent_pane(false, window, cx),
                                PaneCommand::Zoom => this.toggle_zoom(cx),
                                PaneCommand::CopyToOther => {
                                    this.send_to_other_pane(false, window, cx)
                                }
                                PaneCommand::MoveToOther => {
                                    this.send_to_other_pane(true, window, cx)
                                }
                            }
                        }
                        Destination::History => this.open_operation_history(window, cx),
                        Destination::Info => this.toggle_info_panel(window, cx),
                        Destination::Tool(None) => this.open_storage(None, cx),
                        Destination::Tool(Some(similar)) => this.open_find_duplicates(similar, cx),
                        Destination::File(action) => this.file_action(action, None, window, cx),
                    }
                    cx.notify();
                });
            });
        div()
            .id("command-palette-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(72.))
            .px_6()
            .bg(rgba(0x00000038))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.close_command_palette(window, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .id("command-palette")
                    .role(Role::Dialog)
                    .aria_label("Jump and commands")
                    .w(px(560.))
                    .max_w(relative(1.))
                    .h_auto()
                    .max_h(relative(0.85))
                    .w(px(620.))
                    .rounded(crate::tokens::RADIUS_CARD)
                    .bg(cx.theme().tokens.popover)
                    .border_1()
                    .border_color(p.line)
                    .shadow_lg()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(commands),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Destination, Entry, Group, View, confirmed_entry, daily, grouped};
    fn entry(label: &str, destination: Destination, enabled: bool) -> Entry {
        Entry {
            label: label.into(),
            destination,
            keywords: vec![],
            enabled,
        }
    }
    #[test]
    fn grouped_confirmation_uses_original_section_and_row_after_filtering() {
        let entries = vec![
            entry("Rename", Destination::File(daily::Action::Rename), true),
            entry("Recovery", Destination::History, true),
            entry("Appearance", Destination::Settings(0), true),
            entry("List", Destination::View(View::List), true),
            entry(
                "Unavailable",
                Destination::File(daily::Action::Paste),
                false,
            ),
            entry("Shortcuts", Destination::Settings(10), true),
        ];
        let groups = grouped(&entries);
        let settings = groups
            .iter()
            .position(|(group, _)| *group == Group::Settings)
            .unwrap();
        // Filtering for Shortcuts leaves its input coordinates intact, even
        // though it is the first visible result. It must never invoke Rename.
        assert!(matches!(
            confirmed_entry(&groups, settings, 1).unwrap().destination,
            Destination::Settings(10)
        ));
        let files = groups
            .iter()
            .position(|(group, _)| *group == Group::Files)
            .unwrap();
        assert!(confirmed_entry(&groups, files, 1).is_none());
        assert!(confirmed_entry(&groups, groups.len(), 0).is_none());
        assert!(confirmed_entry(&groups, settings, 99).is_none());
        for (group, rows) in groups {
            for row in rows {
                assert!(
                    row.search_keywords()
                        .iter()
                        .any(|keyword| keyword == group.label())
                );
            }
        }
    }
}
