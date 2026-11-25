//! Tabs share immutable rendered row snapshots; the index and workers are shared.
use super::*;
use gpui_component::menu::{ContextMenuExt, PopupMenuItem};

#[derive(Clone)]
pub(super) struct Tab {
    pub(super) all_offset: usize,
    pub(super) id: usize,
    pub(super) text: String,
    pub(super) scope: Option<PathBuf>,
    pub(super) view: View,
    pub(super) content: bool,
    pub(super) largest: Option<bool>,
    pub(super) normal: Option<NormalResults>,
    pub(super) sort: Option<(usize, ColumnSort)>,
    pub(super) expanded: std::collections::HashSet<PathBuf>,
    pub(super) history: Vec<(String, Option<PathBuf>)>,
    pub(super) history_position: usize,
    pub(super) list_offset: Point<Pixels>,
    pub(super) grid_offset: Point<Pixels>,
    pub(super) selected: Option<PathBuf>,
    pub(super) snapshot: Option<Arc<Snapshot>>,
    pub(super) created: Option<std::time::Instant>,
    pub(super) column_widths: Vec<f32>,
    pub(super) related: Vec<super::related::Related>,
    pub(super) related_visible: bool,
}
impl Tab {
    pub(super) fn saved(&self) -> crate::pane_layout::Tab {
        use crate::pane_layout::{Normal, Tab, TabId, Visit};
        let sort = |sort: Option<(usize, ColumnSort)>| {
            sort.map(|(column, order)| (column, order == ColumnSort::Descending))
        };
        let start = self.history_position.saturating_sub(127);
        Tab {
            id: TabId(self.id as u64),
            query: self.text.clone(),
            scope: self.scope.clone(),
            view: self.view,
            content: self.content,
            largest: self.largest,
            normal: self.normal.as_ref().map(|(query, order, view)| Normal {
                query: query.clone(),
                sort: sort(*order),
                view: *view,
            }),
            sort: sort(self.sort),
            history: self
                .history
                .iter()
                .skip(start)
                .take(128)
                .map(|(query, scope)| Visit {
                    query: query.clone(),
                    scope: scope.clone(),
                })
                .collect(),
            history_position: self.history_position - start,
            list_offset: [
                f32::from(self.list_offset.x).min(0.),
                f32::from(self.list_offset.y).min(0.),
            ],
            grid_offset: [
                f32::from(self.grid_offset.x).min(0.),
                f32::from(self.grid_offset.y).min(0.),
            ],
            selected: self.selected.clone(),
            expanded: {
                let mut paths: Vec<_> = self.expanded.iter().cloned().collect();
                paths.sort();
                paths.truncate(256);
                paths
            },
            column_widths: self
                .column_widths
                .iter()
                .take(16)
                .map(|width| width.round().clamp(100., 1000.) as u16)
                .collect(),
            related: self
                .related
                .iter()
                .take(16)
                .map(|chip| crate::pane_layout::RelatedFolder {
                    path: Some(chip.path.clone()),
                    label: chip.label.clone(),
                })
                .collect(),
            related_visible: self.related_visible,
        }
    }
    pub(super) fn from_saved(saved: crate::pane_layout::Tab) -> Result<Self, String> {
        let mut tab = Self::initial(saved.view);
        let sort = |order: Option<(usize, bool)>| {
            order.map(|(column, descending)| {
                (
                    column,
                    if descending {
                        ColumnSort::Descending
                    } else {
                        ColumnSort::Ascending
                    },
                )
            })
        };
        tab.id = usize::try_from(saved.id.0).map_err(|error| error.to_string())?;
        tab.text = saved.query;
        tab.scope = saved.scope;
        tab.content = saved.content;
        tab.largest = saved.largest;
        tab.normal = saved
            .normal
            .map(|normal| (normal.query, sort(normal.sort), normal.view));
        tab.sort = sort(saved.sort);
        tab.history = saved
            .history
            .into_iter()
            .map(|visit| (visit.query, visit.scope))
            .collect();
        tab.history_position = saved.history_position;
        tab.list_offset = point(px(saved.list_offset[0]), px(saved.list_offset[1]));
        tab.grid_offset = point(px(saved.grid_offset[0]), px(saved.grid_offset[1]));
        tab.selected = saved.selected;
        tab.expanded = saved.expanded.into_iter().collect();
        tab.column_widths = saved.column_widths.into_iter().map(f32::from).collect();
        tab.related = saved
            .related
            .into_iter()
            .filter_map(|chip| {
                chip.path.map(|path| super::related::Related {
                    path,
                    label: chip.label,
                })
            })
            .collect();
        tab.related_visible = saved.related_visible;
        Ok(tab)
    }
    pub fn initial(view: View) -> Self {
        Self {
            all_offset: 0,
            id: 2,
            text: String::new(),
            scope: None,
            view,
            content: false,
            largest: None,
            normal: None,
            sort: None,
            expanded: Default::default(),
            history: vec![(String::new(), None)],
            history_position: 0,
            list_offset: Point::default(),
            grid_offset: Point::default(),
            selected: None,
            snapshot: None,
            column_widths: vec![],
            related: vec![],
            related_visible: false,
            created: None,
        }
    }
    pub(super) fn is_all_locations(&self) -> bool {
        is_all_locations(
            &self.text,
            self.scope.as_deref(),
            self.content,
            self.largest,
        )
    }
    pub(super) fn label(&self) -> String {
        if !self.text.is_empty() {
            self.text.clone()
        } else {
            self.scope
                .as_ref()
                .map(|p| folder_name(p))
                .unwrap_or_else(|| "All locations".into())
        }
    }
}
/// A folder's display name: its last component, or for a drive root the
/// native path without the `\\?\` prefix (`C:\`).
pub(super) fn folder_name(path: &std::path::Path) -> String {
    let native = seek_engine::paths::native(&seek_engine::paths::internal(path));
    native
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            native
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_owned()
        })
}
#[derive(Clone)]
pub(super) struct Snapshot {
    pub(super) all_total: Option<usize>,
    pub(super) rows: Arc<Vec<Row>>,
    pub(super) base_rows: Arc<Vec<Row>>,
    pub(super) children: std::collections::HashMap<PathBuf, Vec<Row>>,
    pub(super) depths: Vec<usize>,
    pub(super) selected_many: std::collections::HashSet<PathBuf>,
    pub(super) empty_message: String,
    pub(super) message: Option<String>,
    pub(super) show_hidden: bool,
    pub(super) exclusions: Vec<PathBuf>,
    pub(super) captured: std::time::Instant,
    pub(super) content_locations: Vec<PathBuf>,
    pub(super) remember_document_text: bool,
    pub(super) selection_anchor: Option<PathBuf>,
}
impl Snapshot {
    fn bytes(&self) -> usize {
        fn row_bytes(r: &Row) -> usize {
            std::mem::size_of::<Row>()
                + 2 * r.file.path.as_os_str().len()
                + r.name.len()
                + r.folder.len()
                + r.modified.len()
                + r.size.len()
                + r.kind.len()
                + r.sort_name.len()
                + r.snippet.as_ref().map_or(0, String::len)
        }
        self.rows
            .iter()
            .chain(self.base_rows.iter())
            .chain(self.children.values().flatten())
            .map(row_bytes)
            .sum()
    }
    pub(super) fn remove(&mut self, paths: &[PathBuf]) {
        if self
            .rows
            .iter()
            .any(|r| crate::file_changes::under_any(&r.file.path, paths))
        {
            Arc::make_mut(&mut self.base_rows)
                .retain(|r| !crate::file_changes::under_any(&r.file.path, paths));
            self.children
                .retain(|p, _| !crate::file_changes::under_any(p, paths));
            for rows in self.children.values_mut() {
                rows.retain(|r| !crate::file_changes::under_any(&r.file.path, paths));
            }
            // Retain the displayed order/depths rather than resetting the user's sort.
            let mut i = 0;
            self.depths.retain(|_| {
                let keep = !crate::file_changes::under_any(&self.rows[i].file.path, paths);
                i += 1;
                keep
            });
            Arc::make_mut(&mut self.rows)
                .retain(|r| !crate::file_changes::under_any(&r.file.path, paths));
            self.selected_many
                .retain(|p| !crate::file_changes::under_any(p, paths));
        }
        if self
            .selection_anchor
            .as_ref()
            .is_some_and(|p| crate::file_changes::under_any(p, paths))
        {
            self.selection_anchor = None;
        }
    }
}
pub(super) struct Closing {
    index: usize,
    id: usize,
    label: String,
    folder: bool,
    started: std::time::Instant,
}
impl Closing {
    fn view(&self, p: crate::appearance::Palette) -> AnyElement {
        div()
            .id(("closing-tab", self.id))
            .min_w_0()
            .h(px(28.))
            .flex_shrink_0()
            .overflow_hidden()
            .child(
                div()
                    .w(px(224.))
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_1()
                    .rounded(px(6.))
                    .bg(p.text.opacity(0.04))
                    .text_color(p.muted)
                    .child(
                        div()
                            .size(px(20.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .bg(p.text.opacity(0.06))
                            .child(
                                Icon::new(if self.folder {
                                    gpui_component::IconName::Folder
                                } else {
                                    gpui_component::IconName::Search
                                })
                                .size(px(13.)),
                            ),
                    )
                    .child(div().min_w_0().truncate().child(self.label.clone())),
            )
            .with_animation(("tab-exit", self.id), crate::motion::exit(), |slot, t| {
                slot.w(px(224. * (1. - t))).opacity((1. - t * 3.).max(0.))
            })
            .into_any_element()
    }
}
/// A tab showing `folder`, keeping the template's view and sort.
pub(super) fn folder_tab(template: &Tab, folder: PathBuf) -> Tab {
    let mut tab = template.clone();
    tab.snapshot = None;
    tab.created = None;
    tab.scope = Some(folder);
    tab.text.clear();
    tab.content = false;
    tab.largest = None;
    tab.normal = None;
    tab.expanded.clear();
    tab.selected = None;
    tab.list_offset = Point::default();
    tab.grid_offset = Point::default();
    tab.history = vec![(String::new(), tab.scope.clone())];
    tab.history_position = 0;
    tab.all_offset = 0;
    tab.related.clear();
    tab.related_visible = false;
    tab
}
pub(super) fn is_all_locations(
    text: &str,
    scope: Option<&std::path::Path>,
    content: bool,
    largest: Option<bool>,
) -> bool {
    text.trim().is_empty() && scope.is_none() && !content && largest.is_none()
}
pub(super) fn adjacent(active: usize, count: usize, backwards: bool) -> usize {
    if backwards {
        (active + count - 1) % count
    } else {
        (active + 1) % count
    }
}
impl Seek {
    pub(super) fn restore_saved_tabs(
        &mut self,
        session: crate::pane_layout::Session,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        session.validate()?;
        let focused = session.active;
        let mut others = vec![];
        let mut pane = None;
        for saved in session.panes {
            if saved.id == focused {
                pane = Some(saved);
            } else {
                let active = saved
                    .tabs
                    .iter()
                    .position(|tab| tab.id == saved.active_tab)
                    .unwrap();
                let tabs = saved
                    .tabs
                    .into_iter()
                    .map(Tab::from_saved)
                    .collect::<Result<Vec<_>, _>>()?;
                others.push((saved.id, tabs, active));
            }
        }
        let pane = pane.ok_or("Session has no focused pane")?;
        let active = pane
            .tabs
            .iter()
            .position(|tab| tab.id == pane.active_tab)
            .unwrap();
        let tabs = pane
            .tabs
            .into_iter()
            .map(Tab::from_saved)
            .collect::<Result<Vec<_>, _>>()?;
        let next_id = usize::try_from(session.next_id).map_err(|error| error.to_string())?;
        self.pane_identity = pane.id;
        self.tabs = tabs;
        self.next_tab_id = next_id;
        self.layout = session.root;
        self.panes = others
            .into_iter()
            .map(|(id, tabs, active)| self.new_slot(id, tabs, active, window, cx))
            .collect();
        for index in 0..self.panes.len() {
            self.load_inactive(index, cx);
        }
        for document in session.documents {
            if let Some(path) = document.path {
                self.add_document_pane(document.id, path, None, window, cx);
            }
        }
        self.activate_tab(active, window, cx);
        Ok(())
    }
    pub(super) fn remember_session(&self, cx: &App) {
        use crate::pane_layout::{Pane, Session, TabId, VERSION};
        let Some(store) = &self.session_store else {
            return;
        };
        let mut tabs: Vec<_> = self.tabs.iter().map(Tab::saved).collect();
        let active = self.capture_tab(cx).saved();
        tabs[self.active_tab] = active.clone();
        let mut panes = vec![Pane {
            id: self.pane_identity,
            active_tab: TabId(active.id.0),
            tabs,
        }];
        panes.extend(self.panes.iter().map(|slot| Pane {
            id: slot.id,
            active_tab: TabId(slot.tabs[slot.active_tab].id as u64),
            tabs: slot.tabs.iter().map(Tab::saved).collect(),
        }));
        let session = Session {
            version: VERSION,
            active: self.pane_identity,
            root: self.layout.clone(),
            panes,
            next_id: self.next_tab_id as u64,
            documents: self.saved_documents(),
        };
        if let Err(error) = store.remember(session) {
            eprintln!("Cannot remember pane session: {error}");
        }
    }
    pub(super) fn tabs_available(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.history_open || self.command_palette.is_some() || self.command_run.is_some() {
            return false;
        }
        use gpui_component::WindowExt;
        if self.settings
            || self.edit.is_some()
            || self.permanent_delete.is_some()
            || window.has_active_dialog(cx)
        {
            return false;
        }
        if self.preview_enlarged {
            if self.request_text_reader_exit(window, cx) {
                return false;
            }
            self.finish_close_quick_look(window, cx);
        }
        true
    }
    pub(super) fn capture_tab(&self, cx: &App) -> Tab {
        let table = self.table.read(cx);
        Tab {
            all_offset: self.all_offset,
            id: self.tabs[self.active_tab].id,
            created: self.tabs[self.active_tab].created,
            text: self.input.read(cx).value().to_string(),
            scope: self.scope.clone(),
            view: self.view,
            content: self.content_mode,
            largest: self.largest,
            normal: self.normal_results.clone(),
            sort: table.delegate().sort,
            expanded: table.delegate().expanded.clone(),
            history: self.history.clone(),
            history_position: self.history_position,
            list_offset: self.pending_tab_restore.as_ref().map_or_else(
                || table.vertical_scroll_handle.0.borrow().base_handle.offset(),
                |tab| tab.list_offset,
            ),
            grid_offset: self.pending_tab_restore.as_ref().map_or_else(
                || self.grid_scroll.0.borrow().base_handle.offset(),
                |tab| tab.grid_offset,
            ),
            column_widths: self.column_widths.clone(),
            related: self.related.clone(),
            related_visible: self.related_visible,
            snapshot: {
                let data = table.delegate();
                let snapshot = Arc::new(Snapshot {
                    all_total: self.all_total,
                    rows: data.rows.clone(),
                    base_rows: data.base_rows.clone(),
                    children: data.children.clone(),
                    depths: data.depths.clone(),
                    selected_many: data.selected_many.clone(),
                    empty_message: data.empty_message.clone(),
                    message: self.message.clone(),
                    show_hidden: self.show_hidden,
                    exclusions: self.preferences.exclusions.clone(),
                    captured: std::time::Instant::now(),
                    content_locations: self.preferences.content_locations.clone(),
                    remember_document_text: self.preferences.remember_document_text,
                    selection_anchor: data.selection_anchor.clone(),
                });
                (snapshot.bytes() <= 8 * 1024 * 1024
                    && self.applied_view.as_ref()
                        == Some(&(
                            self.input.read(cx).value().to_string(),
                            self.scope.clone(),
                            self.show_hidden,
                            self.preferences.exclusions.clone(),
                            self.largest,
                            self.content_mode,
                        )))
                .then_some(snapshot)
            },
            selected: self.selected(cx).map(|r| r.file.path).or_else(|| {
                self.pending_tab_restore
                    .as_ref()
                    .and_then(|tab| tab.selected.clone())
            }),
        }
    }
    pub(super) fn activate_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut tab = self.tabs[index].clone();
        self.active_tab = index;
        self.scope = tab.scope.clone();
        self.all_offset = tab.all_offset;
        self.all_total = None;
        self.view = tab.view;
        self.content_mode = tab.content;
        self.largest = tab.largest;
        self.normal_results = tab.normal.clone();
        self.history = tab.history.clone();
        self.history_position = tab.history_position;
        self.column_widths = tab.column_widths.clone();
        self.related = tab.related.clone();
        self.related_visible = tab.related_visible;
        self.related_edit = None;
        self.suggestions = None;
        self.suggestion_dismiss = None;
        let snapshot = tab
            .snapshot
            .as_ref()
            .filter(|s| {
                s.show_hidden == self.show_hidden
                    && s.exclusions == self.preferences.exclusions
                    && s.content_locations == self.preferences.content_locations
                    && s.remember_document_text == self.preferences.remember_document_text
            })
            .cloned();
        tab.snapshot = snapshot.clone();
        self.all_total = snapshot.as_ref().and_then(|s| s.all_total);
        self.applied_view = snapshot.as_ref().map(|_| {
            (
                tab.text.clone(),
                tab.scope.clone(),
                self.show_hidden,
                self.preferences.exclusions.clone(),
                tab.largest,
                tab.content,
            )
        });
        self.tab_motion = self.tab_motion.wrapping_add(1);
        self.tab_motion_started = std::time::Instant::now();
        self.message = snapshot.as_ref().and_then(|s| s.message.clone());
        self.table.update(cx, |table, cx| {
            let data = table.delegate_mut();
            data.rows = snapshot
                .as_ref()
                .map_or_else(|| Arc::new(vec![]), |s| s.rows.clone());
            data.base_rows = snapshot
                .as_ref()
                .map_or_else(|| Arc::new(vec![]), |s| s.base_rows.clone());
            data.children = snapshot
                .as_ref()
                .map_or_else(Default::default, |s| s.children.clone());
            data.depths = snapshot
                .as_ref()
                .map_or_else(Vec::new, |s| s.depths.clone());
            data.content = tab.content;
            data.query = tab.text.clone();
            data.selection_anchor = snapshot.as_ref().and_then(|s| s.selection_anchor.clone());
            data.expanded = tab.expanded.clone();
            data.sort = tab.sort;
            for (i, column) in data.columns.iter_mut().enumerate() {
                column.sort = Some(
                    tab.sort
                        .filter(|(c, _)| *c == i)
                        .map_or(ColumnSort::Default, |(_, s)| s),
                );
            }
            data.selected_many = snapshot
                .as_ref()
                .map_or_else(Default::default, |s| s.selected_many.clone());
            data.drag_paths = Arc::new(vec![]);
            data.empty_message = snapshot
                .as_ref()
                .map_or_else(|| "Loading tab...".into(), |s| s.empty_message.clone());
            if snapshot.is_none() {
                table.clear_selection(cx);
            }
            table.refresh(cx);
        });
        self.restoring_input = Some(tab.text.clone());
        self.input.update(cx, |input, cx| {
            input.set_value(tab.text.clone(), window, cx);
            input.focus(window, cx);
        });
        self.decorate_filters(window, cx);
        self.pending_tab_restore = Some(tab);
        if snapshot.is_some() {
            let owner = cx.entity().downgrade();
            let id = self.tabs[index].id;
            window.defer(cx, move |_, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if this.tabs[this.active_tab].id == id {
                        this.restore_tab_viewport(cx);
                    }
                });
            });
        }
        self.table.read(cx).delegate().refresh_media();
        self.submit(cx);
        self.refresh_preview(cx);
        cx.notify();
        crate::metrics::interaction(
            "tab_activate",
            serde_json::json!({"active": index, "count": self.tabs.len(), "query": self.input.read(cx).value().as_str(), "scope": self.scope}),
        );
    }
    fn trim_tab_snapshots(&mut self) {
        while self
            .tabs
            .iter()
            .filter_map(|t| t.snapshot.as_ref())
            .map(|s| s.bytes())
            .sum::<usize>()
            > 24 * 1024 * 1024
            || self.tabs.iter().filter(|t| t.snapshot.is_some()).count() > 8
        {
            let oldest = self
                .tabs
                .iter()
                .enumerate()
                .filter_map(|(i, t)| t.snapshot.as_ref().map(|s| (i, s.captured)))
                .min_by_key(|(_, time)| *time)
                .map(|(i, _)| i);
            if let Some(i) = oldest {
                self.tabs[i].snapshot = None;
            } else {
                break;
            }
        }
    }
    pub(super) fn remove_confirmed_files(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        for tab in &mut self.tabs {
            if let Some(s) = &mut tab.snapshot {
                Arc::make_mut(s).remove(paths);
            }
            if tab
                .selected
                .as_ref()
                .is_some_and(|p| crate::file_changes::under_any(p, paths))
            {
                tab.selected = None;
            }
        }
        self.table.update(cx, |table, cx| {
            let old = table.selected_row();
            let selected_path = old
                .and_then(|i| table.delegate().rows.get(i))
                .map(|r| r.file.path.clone());
            let data = table.delegate_mut();
            let had_selection = !data.selected_many.is_empty();
            Arc::make_mut(&mut data.base_rows)
                .retain(|r| !crate::file_changes::under_any(&r.file.path, paths));
            data.children
                .retain(|p, _| !crate::file_changes::under_any(p, paths));
            for rows in data.children.values_mut() {
                rows.retain(|r| !crate::file_changes::under_any(&r.file.path, paths));
            }
            data.selected_many
                .retain(|p| !crate::file_changes::under_any(p, paths));
            if data
                .selection_anchor
                .as_ref()
                .is_some_and(|p| crate::file_changes::under_any(p, paths))
            {
                data.selection_anchor = None;
            }
            data.drag_paths = Arc::new(vec![]);
            data.sorted();
            let surviving =
                selected_path.and_then(|p| data.rows.iter().position(|r| r.file.path == p));
            if data.rows.is_empty() {
                table.clear_selection(cx);
            } else if let Some(old) = old {
                let next = surviving.unwrap_or_else(|| old.min(table.delegate().rows.len() - 1));
                let data = table.delegate_mut();
                if had_selection && data.selected_many.is_empty() {
                    let path = data.rows[next].file.path.clone();
                    data.selection_anchor = Some(path.clone());
                    data.selected_many.insert(path);
                }
                table.set_selected_row(next, cx);
            }
            table.refresh(cx);
        });
        self.refresh_preview(cx);
        cx.notify();
    }
    pub(super) fn switch_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index == self.active_tab || index >= self.tabs.len() {
            return;
        }
        self.tabs[self.active_tab] = self.capture_tab(cx);
        self.trim_tab_snapshots();
        self.activate_tab(index, window, cx);
    }
    pub(super) fn new_tab(
        &mut self,
        folder: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.len() >= crate::pane_layout::MAX_TABS {
            self.message =
                Some("This pane already has 32 tabs. Close a tab before opening another.".into());
            cx.notify();
            return;
        }
        let Some(next_id) = self.next_tab_id.checked_add(1) else {
            return;
        };
        let mut tab = self.capture_tab(cx);
        self.tabs[self.active_tab] = tab.clone();
        tab.id = self.next_tab_id;
        tab.created = Some(std::time::Instant::now());
        // A new tab starts without the current tab's related folders.
        tab.related.clear();
        tab.related_visible = false;
        self.next_tab_id = next_id;
        if let Some(folder) = folder {
            tab.snapshot = None;
            tab.scope = Some(folder);
            tab.text.clear();
            tab.content = false;
            tab.largest = None;
            tab.normal = None;
            tab.expanded.clear();
            tab.selected = None;
            tab.list_offset = Point::default();
            tab.grid_offset = Point::default();
            tab.history = vec![(String::new(), tab.scope.clone())];
            tab.history_position = 0;
        }
        self.tabs.push(tab);
        self.trim_tab_snapshots();
        self.activate_tab(self.tabs.len() - 1, window, cx);
    }
    pub(super) fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        // Closing a pane's last tab closes the pane; with one pane left it
        // quits the ordinary window/process.
        if self.tabs.len() == 1 {
            if !self.panes.is_empty() {
                self.close_pane(window, cx);
            } else {
                window.remove_window();
            }
            return;
        }
        self.tabs[self.active_tab] = self.capture_tab(cx);
        self.closing_tab = Some(Closing {
            index,
            id: self.tabs[index].id,
            label: self.tabs[index].label(),
            folder: !self.tabs[index].is_all_locations(),
            started: std::time::Instant::now(),
        });
        self.tabs.remove(index);
        if index == self.active_tab {
            self.activate_tab(index.min(self.tabs.len() - 1), window, cx);
        } else {
            if index < self.active_tab {
                self.active_tab -= 1;
            }
            self.input.update(cx, |input, cx| input.focus(window, cx));
            self.remember_session(cx);
            cx.notify();
        }
    }
    pub(super) fn restore_tab_viewport(&mut self, cx: &mut Context<Self>) {
        if self.pending_tab_restore.is_none() {
            return;
        }
        let restoring_snapshot = self
            .pending_tab_restore
            .as_ref()
            .is_some_and(|t| t.snapshot.is_some());
        if !restoring_snapshot
            && self.table.read(cx).delegate().rows.is_empty()
            && self
                .message
                .as_deref()
                .is_some_and(|s| s.contains("Indexing") || s.contains("Searching"))
        {
            return;
        }
        if !restoring_snapshot
            && self.content_mode
            && self.message.as_deref().is_some_and(|s| {
                !s.ends_with("Complete")
                    && !s.ends_with("Cancelled")
                    && (s.contains("Searched ")
                        || s.contains("Searching")
                        || s.contains("Finding files"))
            })
        {
            return;
        }
        let tab = self.pending_tab_restore.take().unwrap();
        self.table.update(cx, |table, cx| {
            if let Some(snapshot) = &tab.snapshot {
                table.delegate_mut().selected_many = snapshot.selected_many.clone();
                table.delegate_mut().selection_anchor = snapshot.selection_anchor.clone();
            }
            let selected = tab
                .selected
                .as_ref()
                .and_then(|p| table.delegate().rows.iter().position(|r| &r.file.path == p));
            if let Some(i) = selected {
                table.set_selected_row(i, cx);
            } else {
                table.clear_selection(cx);
            }
            let mut state = table.vertical_scroll_handle.0.borrow_mut();
            state.deferred_scroll_to_item = None;
            state.base_handle.set_offset(tab.list_offset);
        });
        let mut state = self.grid_scroll.0.borrow_mut();
        state.deferred_scroll_to_item = None;
        state.base_handle.set_offset(tab.grid_offset);
        crate::metrics::interaction(
            "tab_restored",
            serde_json::json!({"active": self.active_tab, "list_y": f32::from(tab.list_offset.y), "grid_y": f32::from(tab.grid_offset.y)}),
        );
    }
    fn hover_tab_with_files(
        &mut self,
        bounds: Bounds<Pixels>,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !bounds.contains(&window.mouse_position()) {
            if self.tab_hover == Some(index) {
                self.tab_hover = None;
                self.tab_hover_task = Task::ready(());
            }
            return;
        }
        if index == self.active_tab || self.tab_hover == Some(index) {
            return;
        }
        self.tab_hover = Some(index);
        let delay = self.preferences.drag_hover_ms as u64;
        self.tab_hover_task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(delay))
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.tab_hover == Some(index)
                    && cx.has_active_drag()
                    && bounds.contains(&window.mouse_position())
                    && index < this.tabs.len()
                {
                    this.tab_hover = None;
                    this.switch_tab(index, window, cx);
                }
            });
        });
    }
    pub(super) fn tab_strip(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pane = self.pane_identity;
        let mut strip = div()
            .id("tabs")
            .h(px(38.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(2.))
            .px_2()
            .border_b_1()
            .border_color(p.line)
            // Follows the pane's rounded corners; the pane clips only to its
            // rectangle, so a square bar would show past the curve.
            .rounded_t(crate::tokens::RADIUS_CARD - px(1.))
            .bg(p.chrome);
        let mut tabs = div()
            .id("tab-list")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap(px(2.))
            .overflow_x_scroll()
            .on_drop(
                cx.listener(move |this, drag: &super::panes::TabDrag, window, cx| {
                    this.drop_tab(drag, pane, window, cx)
                }),
            );
        for (index, saved) in self.tabs.iter().enumerate() {
            if let Some(closing) = &self.closing_tab
                && closing.index == index
                && self.motion_enabled
                && closing.started.elapsed() < crate::motion::EXIT
            {
                tabs = tabs.child(closing.view(p));
            }
            let active = index == self.active_tab;
            let folder = if active {
                !is_all_locations(
                    self.input.read(cx).value().as_ref(),
                    self.scope.as_deref(),
                    self.content_mode,
                    self.largest,
                )
            } else {
                !saved.is_all_locations()
            };
            let label = if active {
                {
                    let text = self.input.read(cx).value();
                    if text.trim().is_empty() {
                        self.scope
                            .as_ref()
                            .map(|p| folder_name(p))
                            .unwrap_or_else(|| "All locations".into())
                    } else {
                        text.to_string()
                    }
                }
            } else {
                saved.label()
            };
            let drag = super::panes::TabDrag {
                pane,
                tab: saved.id,
                label: label.clone(),
                folder,
            };
            let tab = div()
                .id(("tab", saved.id))
                .h(px(28.))
                .w(px(224.))
                // Narrow split panes shrink tabs before scrolling them.
                .flex_shrink_1()
                .min_w(px(96.))
                .flex()
                .items_center()
                .overflow_hidden()
                .rounded(px(6.))
                .bg(p.text.opacity(if active { 0.10 } else { 0.04 }))
                .hover(move |d| d.bg(p.text.opacity(if active { 0.10 } else { 0.07 })))
                .text_color(if active { p.text } else { p.muted })
                .child(
                    gpui_base::Button::new(("switch-tab", saved.id))
                        .accessibility_label(format!("Switch to tab {}: {}", index + 1, label))
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .px_1()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .min_w_0()
                                .child(
                                    div()
                                        .size(px(20.))
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(4.))
                                        .bg(p.text.opacity(0.06))
                                        .child(
                                            Icon::new(if folder {
                                                gpui_component::IconName::Folder
                                            } else {
                                                gpui_component::IconName::Search
                                            })
                                            .size(px(13.)),
                                        ),
                                )
                                .child(
                                    div().min_w_0().truncate().line_height(px(20.)).child(label),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.switch_tab(index, window, cx)
                        })),
                )
                .child(
                    gpui_base::Button::new(("close-tab", saved.id))
                        .accessibility_label(format!("Close tab {}", index + 1))
                        .size(px(24.))
                        .child(Icon::new(gpui_component::IconName::Close).size(px(12.)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.close_tab(index, window, cx)
                        })),
                );
            // Files dragged onto another tab switch to it after the delay.
            let tab = tab.on_drag_move(cx.listener(
                move |this, event: &DragMoveEvent<super::daily::FilesDrag>, window, cx| {
                    this.hover_tab_with_files(event.bounds, index, window, cx)
                },
            ));
            let tab = super::panes::tab_drag_source(tab, drag, pane, index, cx);
            let tab = if self.motion_enabled
                && saved
                    .created
                    .is_some_and(|t| t.elapsed() < crate::motion::ENTER)
            {
                div()
                    .id(("tab-opening-slot", saved.id))
                    .min_w_0()
                    .h(px(28.))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .child(tab)
                    .with_animation(
                        ("tab-enter", saved.id),
                        crate::motion::enter(),
                        |slot, t| slot.w(px(224. * t)),
                    )
                    .into_any_element()
            } else if self.motion_enabled
                && active
                && self.tab_motion_started.elapsed() < crate::motion::STATE
            {
                tab.with_animation(
                    ("tab-select", self.tab_motion as usize),
                    crate::motion::state(),
                    move |tab, t| tab.bg(p.text.opacity(0.075 + 0.025 * t)),
                )
                .into_any_element()
            } else {
                tab.into_any_element()
            };
            if self.tab_drop == Some((pane, index)) {
                tabs = tabs.child(super::panes::tab_insertion(p));
            }
            let owner = cx.entity().downgrade();
            let showing = active && self.related_visible;
            let tab = div()
                .id(("tab-menu", saved.id))
                .flex()
                .min_w(px(96.))
                .flex_shrink_1()
                .child(tab)
                .context_menu(move |menu, window, _| {
                    let owner = owner.clone();
                    super::daily::menu_style(menu, window).item(
                        PopupMenuItem::new(if showing {
                            "Hide related folders"
                        } else {
                            "Show related folders"
                        })
                        .on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                if this.active_tab != index {
                                    this.switch_tab(index, window, cx);
                                }
                                this.toggle_related(cx);
                            });
                        }),
                    )
                });
            tabs = tabs.child(tab);
        }
        if self.tab_drop == Some((pane, self.tabs.len())) {
            tabs = tabs.child(super::panes::tab_insertion(p));
        }
        if let Some(closing) = &self.closing_tab
            && closing.index >= self.tabs.len()
            && self.motion_enabled
            && closing.started.elapsed() < crate::motion::EXIT
        {
            tabs = tabs.child(closing.view(p));
        }
        tabs = tabs.child(
            gpui_base::Button::new("new-tab")
                .accessibility_label("New tab")
                .size(px(28.))
                .rounded(px(6.))
                .bg(p.text.opacity(0.04))
                .hover(move |d| d.bg(p.hover))
                .child(Icon::new(gpui_component::IconName::Plus).size(px(14.)))
                .on_click(cx.listener(|this, _, window, cx| this.new_tab(None, window, cx))),
        );
        strip = strip
            .child(tabs)
            .children(self.zoom_button(self.pane_identity, p, cx));
        strip.into_any_element()
    }
}
#[cfg(test)]
mod tests {
    use super::adjacent;
    #[test]
    fn snapshots_share_rows_and_remove_deleted_subtrees_without_changing_other_tabs() {
        use super::*;
        let root = PathBuf::from(r"C:\Owned");
        let rows = Arc::new(
            (0..12000)
                .map(|i| {
                    Row::new(seek_engine::Found {
                        path: root.join(format!("Row-{i:05}.txt")),
                        kind: 0,
                        mtime: 0,
                        size: 1,
                        score: 0,
                    })
                })
                .collect::<Vec<_>>(),
        );
        let mut snapshot = Snapshot {
            all_total: Some(12000),
            rows: rows.clone(),
            base_rows: rows.clone(),
            children: Default::default(),
            depths: vec![0; rows.len()],
            selected_many: [rows[4].file.path.clone()].into(),
            empty_message: String::new(),
            message: None,
            show_hidden: false,
            exclusions: vec![],
            captured: std::time::Instant::now(),
            content_locations: vec![],
            remember_document_text: true,
            selection_anchor: Some(rows[4].file.path.clone()),
        };
        let shared = Arc::new(snapshot.clone());
        let mut times = vec![];
        for _ in 0..100 {
            let began = std::time::Instant::now();
            let tab = std::hint::black_box(shared.clone());
            let restored = std::hint::black_box(tab.rows.clone());
            times.push(began.elapsed().as_secs_f64() * 1000.);
            assert!(Arc::ptr_eq(&restored, &rows));
        }
        times.sort_by(f64::total_cmp);
        snapshot.remove(&[rows[4].file.path.clone()]);
        assert_eq!(snapshot.rows.len(), 11999);
        assert_eq!(snapshot.depths.len(), snapshot.rows.len());
        assert!(snapshot.selected_many.is_empty());
        assert!(snapshot.selection_anchor.is_none());
        assert_eq!(shared.rows.len(), 12000);
        assert_eq!(shared.all_total, Some(12000));
        assert!(Arc::ptr_eq(&shared.rows, &rows));
        println!(
            "snapshot handoff: 12000 shared rows, p95 {:.6} ms (100 handoffs, test profile); this excludes GPUI layout/paint",
            times[94]
        );
    }
    #[test]
    fn saved_tab_navigation_does_not_change_when_another_tab_is_edited() {
        use super::*;
        let mut first = Tab::initial(View::List);
        first.text = "note ext:pdf".into();
        first.scope = Some(PathBuf::from(r"D:\Documents"));
        first.list_offset = point(px(0.), px(-1234.));
        first.content = true;
        let mut second = first.clone();
        second.text = "report".into();
        second.scope = None;
        second.list_offset = Point::default();
        second.history.push(("other".into(), None));
        assert_eq!(first.text, "note ext:pdf");
        assert!(first.scope.is_some());
        assert_eq!(first.list_offset.y, px(-1234.));
        assert_eq!(first.history.len(), 1);
    }
    #[test]
    fn only_the_virtual_all_locations_tab_uses_search() {
        use super::*;
        let mut tab = Tab::initial(View::List);
        assert!(tab.is_all_locations());
        tab.text = "report".into();
        assert!(!tab.is_all_locations());
        tab.text.clear();
        tab.scope = Some(PathBuf::from(r"C:\All locations"));
        assert!(!tab.is_all_locations());
        tab.scope = None;
        tab.content = true;
        assert!(!tab.is_all_locations());
        tab.content = false;
        tab.largest = Some(false);
        assert!(!tab.is_all_locations());
    }
    #[test]
    fn cycling_wraps_in_both_directions() {
        assert_eq!(adjacent(0, 3, true), 2);
        assert_eq!(adjacent(2, 3, false), 0);
        assert_eq!(adjacent(0, 1, true), 0);
        assert_eq!(adjacent(0, 1, false), 0);
    }
}
