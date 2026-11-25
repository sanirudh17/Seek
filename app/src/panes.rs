//! Split panes (3.1). The focused pane is the live browser: it owns the search
//! field, the search generation and every file command. Other panes keep their
//! own table, tabs, scroll position and thumbnail set. A press anywhere in one
//! swaps it in, exactly like a tab switch, so commands always act on the pane
//! the user is looking at. Late search replies for a pane that lost focus are
//! retired by bumping the shared generation before the swap.
use super::*;
use crate::pane_layout::{Axis, MAX_PANES, MAX_TABS, Node, PaneId, SplitId};
use gpui_component_assets::IconName as Glyph;
use std::collections::HashSet;

/// Gap between panes; also the divider's hit area.
pub(super) const DIVIDER: f32 = 6.;
/// Smallest pane width (side by side) and height (stacked).
pub(super) const MIN_WIDTH: f32 = 260.;
pub(super) const MIN_HEIGHT: f32 = 180.;

pub(super) struct GridSource {
    pub(super) table: Entity<TableState<Rows>>,
    pub(super) scroll: UniformListScrollHandle,
    pub(super) view: View,
    pub(super) visible: Arc<Mutex<HashSet<media::Key>>>,
    pub(super) width: f32,
    pub(super) active: bool,
    pub(super) pane: PaneId,
}

/// A pane that is not focused. The focused pane's equivalents live on `Seek`.
pub(super) struct Slot {
    pub(super) id: PaneId,
    pub(super) tabs: Vec<tabs::Tab>,
    pub(super) active_tab: usize,
    pub(super) table: Entity<TableState<Rows>>,
    pub(super) result_pane: Entity<ResultPane>,
    pub(super) grid_scroll: UniformListScrollHandle,
    pub(super) visible: Arc<Mutex<HashSet<media::Key>>>,
    pub(super) applied_view: Option<AppliedView>,
    pub(super) all_total: Option<usize>,
    pub(super) message: Option<String>,
    /// Contents may be out of date; reload before relying on them.
    pub(super) stale: bool,
    load: Task<()>,
    load_revision: u64,
    _subscription: Subscription,
}

#[derive(Clone)]
pub(super) struct DividerDrag {
    split: SplitId,
}
impl Render for DividerDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// A tab being dragged: reorder within its pane, or drop on another pane's
/// tab strip to move it there.
#[derive(Clone)]
pub(super) struct TabDrag {
    pub(super) pane: PaneId,
    pub(super) tab: usize,
    pub(super) label: String,
    pub(super) folder: bool,
}
impl Render for TabDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_component::ActiveTheme;
        div()
            .h(px(28.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(6.))
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .border_1()
            .border_color(cx.theme().border)
            .shadow_md()
            .text_size(px(13.))
            .child(
                Icon::new(if self.folder {
                    gpui_component::IconName::Folder
                } else {
                    gpui_component::IconName::Search
                })
                .size(px(13.)),
            )
            .child(div().max_w(px(200.)).truncate().child(self.label.clone()))
    }
}

/// Make a tab draggable and report the insertion slot under the pointer.
pub(super) fn tab_drag_source(
    tab: Stateful<Div>,
    drag: TabDrag,
    target: PaneId,
    position: usize,
    cx: &mut Context<Seek>,
) -> Stateful<Div> {
    tab.on_drag(drag, |drag, offset, _, cx| {
        super::drag_preview::near_cursor(drag.clone(), offset, cx)
    })
    .on_drag_move(
        cx.listener(move |this, event: &DragMoveEvent<TabDrag>, _, cx| {
            let bounds = event.bounds;
            if !bounds.contains(&event.event.position) {
                return;
            }
            let after = event.event.position.x > bounds.center().x;
            let slot = (target, position + usize::from(after));
            if this.tab_drop != Some(slot) {
                this.tab_drop = Some(slot);
                cx.notify();
            }
        }),
    )
}

/// Insertion line shown between tabs while a tab is dragged.
pub(super) fn tab_insertion(p: crate::appearance::Palette) -> Div {
    div()
        .w(px(2.))
        .h(px(22.))
        .flex_shrink_0()
        .rounded(px(1.))
        .bg(p.accent)
}

/// Fit the five list columns to the width a pane actually has, only when that
/// width or the column set changes, so manual resizing survives.
pub(super) fn fit_columns(table: &Entity<TableState<Rows>>, results_width: u32, cx: &mut App) {
    let rows = table.read(cx).delegate();
    let with_folder = !rows.query.trim().is_empty() || rows.content;
    let layout_key = (results_width, with_folder, rows.content);
    if rows.layout_key == Some(layout_key) {
        return;
    }
    table.update(cx, |table, cx| {
        let rows = table.delegate_mut();
        let minimum = if rows.content {
            1210.
        } else if with_folder {
            1040.
        } else {
            760.
        };
        let extra = (results_width as f32 - minimum).max(0.);
        let shares = if with_folder {
            [0.40, 0.12, 0.06, 0.12, 0.30]
        } else {
            [0.58, 0.14, 0.08, 0.20, 0.]
        };
        for (i, base) in [320., 190., 100., 150., 280.].into_iter().enumerate() {
            let base = if i == 1 && rows.content { 360. } else { base };
            rows.columns[i].width = px(base + extra * shares[i]);
        }
        rows.layout_key = Some(layout_key);
        table.refresh(cx);
    });
}

/// Whether any changed path is the folder itself or a direct child of it.
fn touches<'a>(folder: &std::path::Path, paths: impl IntoIterator<Item = &'a PathBuf>) -> bool {
    let same = |a: &std::path::Path| {
        seek_engine::paths::cmp_ci(
            &seek_engine::paths::internal(a),
            &seek_engine::paths::internal(folder),
        )
        .is_eq()
    };
    paths
        .into_iter()
        .any(|path| same(path) || path.parent().is_some_and(same))
}

pub(super) fn folder_tab(tab: &tabs::Tab) -> Option<PathBuf> {
    tab.scope
        .clone()
        .filter(|_| tab.text.trim().is_empty() && !tab.content && tab.largest.is_none())
}

impl Seek {
    pub(super) fn split_mode(&self) -> bool {
        !self.panes.is_empty() || !self.doc_panes.is_empty()
    }
    fn is_document(&self, id: PaneId) -> bool {
        self.doc_panes.iter().any(|doc| doc.id == id)
    }
    /// Browsing panes in on-screen order; document panes are not focusable
    /// browsers and are skipped by pane commands.
    fn browser_leaves(&self) -> Vec<PaneId> {
        self.layout
            .leaves()
            .into_iter()
            .filter(|id| !self.is_document(*id))
            .collect()
    }
    pub(super) fn allocate_id(&mut self) -> Option<u64> {
        let id = self.next_tab_id;
        self.next_tab_id = id.checked_add(1)?;
        Some(id as u64)
    }
    /// Width shared by all panes: the window minus sidebar and info panel.
    pub(super) fn panes_width(&self, window: &Window, cx: &App) -> f32 {
        (f32::from(window.viewport_size().width)
            - f32::from_bits(self.sidebar_width.load(Ordering::Relaxed))
            - if self.panel && self.selected(cx).is_some() {
                self.info_width(window)
            } else {
                0.
            })
        .max(0.)
    }
    /// Approximate on-screen width of each pane, from the layout fractions.
    pub(super) fn pane_widths(&self, window: &Window, cx: &App) -> Vec<(PaneId, f32)> {
        let width = self.panes_width(window, cx);
        let bounds = crate::pane_layout::Rect {
            x: 0.,
            y: 0.,
            width,
            height: f32::from(window.viewport_size().height).max(1.),
        };
        self.layout
            .arrange(bounds, (1., 1.), DIVIDER)
            .map(|rows| rows.into_iter().map(|(id, r)| (id, r.width)).collect())
            .unwrap_or_else(|_| {
                self.layout
                    .leaves()
                    .into_iter()
                    .map(|id| (id, width))
                    .collect()
            })
    }
    pub(super) fn results_viewport_width(&self, window: &Window, cx: &App) -> f32 {
        if !self.split_mode() {
            return self.panes_width(window, cx);
        }
        self.pane_widths(window, cx)
            .into_iter()
            .find(|(id, _)| *id == self.pane_identity)
            .map_or_else(|| self.panes_width(window, cx), |(_, width)| width)
    }

    /// Table events from any pane. Events from a pane that is not focused
    /// focus it first, so selection and opening always act on that pane.
    pub(super) fn results_event(
        &mut self,
        emitter: &Entity<TableState<Rows>>,
        event: &TableEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if emitter != &self.table {
            let Some(id) = self
                .panes
                .iter()
                .find(|slot| &slot.table == emitter)
                .map(|slot| slot.id)
            else {
                return;
            };
            if !self.focus_pane(id, window, cx) {
                return;
            }
        }
        if matches!(event, TableEvent::ClearSelection) {
            self.table.update(cx, |table, _| {
                let data = table.delegate_mut();
                data.selection = None;
                data.selected_many.clear();
                data.drag_paths = Arc::new(vec![]);
            });
        }
        if let TableEvent::SelectRow(index) = event {
            self.table.update(cx, |table, _| {
                table.delegate_mut().selection = table
                    .delegate()
                    .rows
                    .get(*index)
                    .map(|row| row.file.path.clone());
            });
        }
        #[cfg(feature = "benchmarks")]
        if let TableEvent::SelectRow(index) = event {
            crate::metrics::interaction(
                "select",
                serde_json::json!({"row": index, "name": self.table.read(cx).delegate().rows.get(*index).map(|row| &row.name) }),
            );
        }
        if matches!(event, TableEvent::DoubleClickedRow(_)) {
            self.open_selected(false, window, cx);
        }
        self.refresh_preview(cx);
        cx.notify();
    }

    /// Make `id` the focused pane. Returns false when an editor, dialog or
    /// overlay must keep focus where it is.
    pub(super) fn focus_pane(
        &mut self,
        id: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if id == self.pane_identity {
            return true;
        }
        let Some(index) = self.panes.iter().position(|slot| slot.id == id) else {
            return false;
        };
        if !self.tabs_available(window, cx) {
            return false;
        }
        self.tabs[self.active_tab] = self.capture_tab(cx);
        // Retire the focused pane's search before its table leaves; late
        // replies are dropped by the generation check instead of landing here.
        self.backend.generation.fetch_add(1, Ordering::Relaxed);
        let mut slot = self.panes.remove(index);
        slot.stale = false;
        slot.load = Task::ready(());
        std::mem::swap(&mut self.table, &mut slot.table);
        std::mem::swap(&mut self.result_pane, &mut slot.result_pane);
        std::mem::swap(&mut self.grid_scroll, &mut slot.grid_scroll);
        std::mem::swap(&mut self.visible, &mut slot.visible);
        std::mem::swap(&mut self.tabs, &mut slot.tabs);
        std::mem::swap(&mut self.active_tab, &mut slot.active_tab);
        std::mem::swap(&mut self.applied_view, &mut slot.applied_view);
        std::mem::swap(&mut self.all_total, &mut slot.all_total);
        std::mem::swap(&mut self.message, &mut slot.message);
        std::mem::swap(&mut self.pane_identity, &mut slot.id);
        self.closing_tab = None;
        self.panes.insert(index, slot);
        self.adopt_tab(window, cx);
        // The pane that just lost focus may have had a search in flight.
        let previous = &self.panes[index];
        let tab = &previous.tabs[previous.active_tab];
        let current = (
            tab.text.clone(),
            tab.scope.clone(),
            self.show_hidden,
            self.preferences.exclusions.clone(),
            tab.largest,
            tab.content,
        );
        if previous.applied_view.as_ref() != Some(&current) {
            self.load_inactive(index, cx);
        }
        true
    }

    /// Bind the focused pane's active tab to the shared controls without
    /// discarding the rows its table already shows.
    fn adopt_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.tabs[self.active_tab].clone();
        self.scope = tab.scope.clone();
        self.all_offset = tab.all_offset;
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
        self.pending_tab_restore = None;
        let preferences = self.preferences.clone();
        self.table.update(cx, |table, _| {
            table.delegate_mut().preferences = preferences
        });
        if self.input.read(cx).value().as_str() != tab.text {
            self.restoring_input = Some(tab.text.clone());
            self.input.update(cx, |input, cx| {
                input.set_value(tab.text.clone(), window, cx)
            });
        }
        self.decorate_filters(window, cx);
        // Always re-issue the focused pane's query. The swap retired the
        // previous generation, and the search worker only delivers live
        // updates (file changes, refreshes) for its current request; rows the
        // table already shows stay until the new results replace them.
        self.submit(cx);
        self.table.read(cx).focus_handle(cx).focus(window, cx);
        self.table.read(cx).delegate().refresh_media();
        self.refresh_preview(cx);
        cx.notify();
    }

    /// Read a folder for a pane that is not focused, with the same filters
    /// and order as the search worker, without touching the search generation.
    /// Search tabs stay as they are and refresh when focused.
    pub(super) fn load_inactive(&mut self, index: usize, cx: &mut Context<Self>) {
        let hidden = self.show_hidden;
        let excluded = self.preferences.exclusions.clone();
        let Some(slot) = self.panes.get_mut(index) else {
            return;
        };
        let tab = &slot.tabs[slot.active_tab];
        let Some(folder) = folder_tab(tab) else {
            // Searches run only in the focused pane. Show the tab's last
            // complete results if it has them, never rows from another query.
            slot.stale = true;
            slot.applied_view = None;
            let snapshot = tab.snapshot.clone();
            let (query, content) = (tab.text.clone(), tab.content);
            slot.load = Task::ready(());
            slot.table.update(cx, |table, cx| {
                let data = table.delegate_mut();
                match &snapshot {
                    Some(saved) => {
                        data.rows = saved.rows.clone();
                        data.base_rows = saved.base_rows.clone();
                        data.children = saved.children.clone();
                        data.depths = saved.depths.clone();
                        data.selected_many = saved.selected_many.clone();
                        data.empty_message = saved.empty_message.clone();
                    }
                    None => {
                        data.rows = Arc::new(vec![]);
                        data.base_rows = Arc::new(vec![]);
                        data.children.clear();
                        data.depths.clear();
                        data.selected_many.clear();
                        data.empty_message = "This search runs when its pane is focused.".into();
                    }
                }
                data.query = query;
                data.content = content;
                data.selection = None;
                data.drag_paths = Arc::new(vec![]);
                table.refresh(cx);
            });
            return;
        };
        let key: AppliedView = (
            tab.text.clone(),
            tab.scope.clone(),
            hidden,
            excluded.clone(),
            tab.largest,
            tab.content,
        );
        // The same folder again (a change on disk): keep what is expanded.
        let same_view = slot.applied_view.as_ref() == Some(&key);
        slot.load_revision = slot.load_revision.wrapping_add(1);
        let revision = slot.load_revision;
        let id = slot.id;
        slot.load = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut dates = std::collections::HashMap::new();
                    crate::browse::listing(&folder, hidden, &excluded).map(|files| {
                        files
                            .into_iter()
                            .map(|file| Row::with_dates(file, &mut dates))
                            .collect::<Vec<_>>()
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(slot) = this
                    .panes
                    .iter_mut()
                    .find(|slot| slot.id == id && slot.load_revision == revision)
                else {
                    return;
                };
                let (rows, message) = match result {
                    Ok(rows) if rows.is_empty() => {
                        (rows, "This folder has no visible items.".into())
                    }
                    Ok(rows) => (rows, "Folder contents".to_string()),
                    Err(error) => (vec![], format!("Folder unavailable: {error}")),
                };
                slot.table.update(cx, |table, cx| {
                    let data = table.delegate_mut();
                    data.base_rows = Arc::new(rows);
                    if !same_view {
                        data.children.clear();
                    }
                    data.empty_message = message.clone();
                    data.sorted();
                    let present: HashSet<&PathBuf> =
                        data.rows.iter().map(|row| &row.file.path).collect();
                    let keep: HashSet<PathBuf> = data
                        .selected_many
                        .iter()
                        .filter(|path| present.contains(path))
                        .cloned()
                        .collect();
                    data.selected_many = keep;
                    if data
                        .selection
                        .as_ref()
                        .is_some_and(|path| !data.rows.iter().any(|row| &row.file.path == path))
                    {
                        data.selection = None;
                    }
                    table.refresh(cx);
                });
                slot.applied_view = Some(key);
                slot.all_total = None;
                slot.message = Some(message);
                slot.stale = false;
                cx.notify();
            });
        });
    }

    /// After file operations: unfocused folder panes touching these paths
    /// reload; removed items leave every unfocused pane at once.
    pub(super) fn refresh_inactive_panes(
        &mut self,
        changed: &[PathBuf],
        removed: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        for index in 0..self.panes.len() {
            if !removed.is_empty() {
                let slot = &mut self.panes[index];
                for tab in &mut slot.tabs {
                    if let Some(snapshot) = &mut tab.snapshot {
                        Arc::make_mut(snapshot).remove(removed);
                    }
                }
                slot.table.update(cx, |table, cx| {
                    let data = table.delegate_mut();
                    Arc::make_mut(&mut data.base_rows)
                        .retain(|row| !crate::file_changes::under_any(&row.file.path, removed));
                    data.children
                        .retain(|path, _| !crate::file_changes::under_any(path, removed));
                    for rows in data.children.values_mut() {
                        rows.retain(|row| !crate::file_changes::under_any(&row.file.path, removed));
                    }
                    data.selected_many
                        .retain(|path| !crate::file_changes::under_any(path, removed));
                    data.sorted();
                    table.refresh(cx);
                });
            }
            let slot = &self.panes[index];
            let tab = &slot.tabs[slot.active_tab];
            let touched = folder_tab(tab)
                .is_some_and(|folder| touches(&folder, changed.iter().chain(removed)));
            if touched {
                self.load_inactive(index, cx);
            }
        }
    }

    pub(super) fn split_pane(
        &mut self,
        axis: Axis,
        folder: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.tabs_available(window, cx) {
            return;
        }
        if self.panes.len() + self.doc_panes.len() + 1 >= MAX_PANES {
            self.message = Some(format!(
                "Seek shows up to {MAX_PANES} panes. Close a pane before splitting again."
            ));
            cx.notify();
            return;
        }
        let (Some(pane), Some(tab_id), Some(split)) =
            (self.allocate_id(), self.allocate_id(), self.allocate_id())
        else {
            return;
        };
        let pane = PaneId(pane);
        let mut tab = self.capture_tab(cx);
        self.tabs[self.active_tab] = tab.clone();
        tab.id = tab_id as usize;
        tab.created = None;
        tab.snapshot = None;
        if let Some(folder) = folder {
            let id = tab.id;
            tab = tabs::folder_tab(&tab, folder);
            tab.id = id;
        }
        let slot = self.new_slot(pane, vec![tab], 0, window, cx);
        let mut layout = self.layout.clone();
        layout.replace(
            self.pane_identity,
            Node::Split {
                id: SplitId(split),
                axis,
                fraction: 5000,
                first: Box::new(Node::Pane {
                    id: self.pane_identity,
                }),
                second: Box::new(Node::Pane { id: pane }),
            },
        );
        if let Err(error) = layout.validate_tree() {
            self.message = Some(error);
            cx.notify();
            return;
        }
        self.layout = layout;
        self.panes.push(slot);
        self.unzoom();
        self.pane_motion = Some((pane, Instant::now()));
        self.focus_pane(pane, window, cx);
        self.remember_session(cx);
    }

    pub(super) fn new_slot(
        &mut self,
        id: PaneId,
        tabs: Vec<tabs::Tab>,
        active_tab: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Slot {
        let visible = Arc::new(Mutex::new(HashSet::new()));
        let (table, result_pane) = results_table(
            Rows::new(
                "Loading...",
                &self.preferences,
                &self.atlas,
                &self.cache,
                &self.media,
                &visible,
                &self.extra,
                cx.entity().downgrade(),
            ),
            window,
            cx,
        );
        let subscription = cx.subscribe_in(&table, window, Self::results_event);
        Slot {
            id,
            tabs,
            active_tab,
            table,
            result_pane,
            grid_scroll: UniformListScrollHandle::new(),
            visible,
            applied_view: None,
            all_total: None,
            message: None,
            stale: true,
            load: Task::ready(()),
            load_revision: 0,
            _subscription: subscription,
        }
    }

    /// Close the focused pane and its tabs; focus moves to its neighbour.
    pub(super) fn close_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.split_mode() || !self.tabs_available(window, cx) {
            return;
        }
        let closing = self.pane_identity;
        let leaves = self.browser_leaves();
        let Some(position) = leaves.iter().position(|id| *id == closing) else {
            return;
        };
        let next = if position + 1 < leaves.len() {
            leaves[position + 1]
        } else {
            leaves[position - 1]
        };
        if !self.focus_pane(next, window, cx) {
            return;
        }
        let Some(layout) = self.layout.clone().without(closing) else {
            return;
        };
        self.layout = layout;
        self.panes.retain(|slot| slot.id != closing);
        if self.panes.is_empty() {
            self.unzoom();
        }
        self.remember_session(cx);
        cx.notify();
    }

    /// F6 / Shift+F6: move focus between panes in on-screen order.
    pub(super) fn focus_adjacent_pane(
        &mut self,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let leaves = self.browser_leaves();
        if leaves.len() < 2 {
            return;
        }
        let position = leaves
            .iter()
            .position(|id| *id == self.pane_identity)
            .unwrap_or(0);
        let next = if backwards {
            (position + leaves.len() - 1) % leaves.len()
        } else {
            (position + 1) % leaves.len()
        };
        self.focus_pane(leaves[next], window, cx);
    }

    /// The folder shown by the next pane in on-screen order.
    fn other_pane_folder(&self) -> Result<PathBuf, String> {
        let leaves = self.browser_leaves();
        let position = leaves
            .iter()
            .position(|id| *id == self.pane_identity)
            .ok_or("Split the view to use the other pane.")?;
        let next = leaves
            .get((position + 1) % leaves.len())
            .filter(|id| **id != self.pane_identity)
            .ok_or("Split the view to use the other pane.")?;
        let slot = self
            .panes
            .iter()
            .find(|slot| slot.id == *next)
            .ok_or("The other pane is no longer open.")?;
        slot.tabs[slot.active_tab]
            .scope
            .clone()
            .ok_or_else(|| "The other pane is not showing a folder.".into())
    }

    /// Alt+F5 / Alt+F6: copy or move the selection into the other pane's folder.
    pub(super) fn send_to_other_pane(
        &mut self,
        moving: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.tabs_available(window, cx) {
            return;
        }
        let folder = match self.other_pane_folder() {
            Ok(folder) => folder,
            Err(error) => {
                self.message = Some(error);
                cx.notify();
                return;
            }
        };
        let paths: Vec<PathBuf> = {
            let data = self.table.read(cx).delegate();
            if data.selected_many.is_empty() {
                self.selected(cx)
                    .map(|row| row.file.path)
                    .into_iter()
                    .collect()
            } else {
                data.rows
                    .iter()
                    .filter(|row| data.selected_many.contains(&row.file.path))
                    .map(|row| row.file.path.clone())
                    .collect()
            }
        };
        self.transfer(paths, folder, !moving, window, cx);
    }

    /// Copy or move into a folder. Never overwrites: conflicts are resolved by
    /// the shared operation worker.
    pub(super) fn transfer(
        &mut self,
        paths: Vec<PathBuf>,
        folder: PathBuf,
        copy: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            self.message = Some("Select items to send to the other pane.".into());
            cx.notify();
            return;
        }
        if !super::daily::drop_allowed(&paths, &folder) {
            self.message = Some("Choose a different folder outside the selected items.".into());
            cx.notify();
            return;
        }
        let operation = if copy {
            crate::file_ops::Operation::Copy { paths, folder }
        } else {
            crate::file_ops::Operation::Move { paths, folder }
        };
        self.run_file_op(operation, window, cx);
    }

    /// Drop on an unfocused pane's background: its folder is the destination.
    /// Ctrl copies; otherwise the items move, as within one pane.
    fn drop_on_pane(
        &mut self,
        id: PaneId,
        drag: &super::daily::FilesDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag_hover = None;
        let Some(folder) = self
            .panes
            .iter()
            .find(|slot| slot.id == id)
            .and_then(|slot| slot.tabs[slot.active_tab].scope.clone())
        else {
            self.message = Some("That pane is not showing a folder.".into());
            cx.notify();
            return;
        };
        if self.edit.is_some() || self.history_open || self.settings {
            return;
        }
        let copy = window.modifiers().control;
        self.transfer(drag.paths.as_ref().clone(), folder, copy, window, cx);
    }

    /// Drop a dragged tab on `pane`'s strip at the insertion slot.
    pub(super) fn drop_tab(
        &mut self,
        drag: &TabDrag,
        pane: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let slot = self
            .tab_drop
            .take()
            .filter(|(target, _)| *target == pane)
            .map(|(_, slot)| slot);
        if !self.tabs_available(window, cx) {
            cx.notify();
            return;
        }
        if drag.pane == pane {
            self.reorder_tab(pane, drag.tab, slot, cx);
        } else {
            self.move_tab_to_pane(drag, pane, slot, window, cx);
        }
        cx.notify();
    }

    fn reorder_tab(
        &mut self,
        pane: PaneId,
        tab: usize,
        slot: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        if pane == self.pane_identity {
            self.tabs[self.active_tab] = self.capture_tab(cx);
            let active = self.tabs[self.active_tab].id;
            reorder(&mut self.tabs, |t| t.id == tab, slot);
            self.active_tab = self.tabs.iter().position(|t| t.id == active).unwrap_or(0);
        } else if let Some(target) = self.panes.iter_mut().find(|s| s.id == pane) {
            let active = target.tabs[target.active_tab].id;
            reorder(&mut target.tabs, |t| t.id == tab, slot);
            target.active_tab = target.tabs.iter().position(|t| t.id == active).unwrap_or(0);
        }
        self.remember_session(cx);
    }

    fn move_tab_to_pane(
        &mut self,
        drag: &TabDrag,
        pane: PaneId,
        slot: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Focus the destination first; the source is then an unfocused pane.
        if !self.focus_pane(pane, window, cx) {
            return;
        }
        let Some(source) = self.panes.iter().position(|s| s.id == drag.pane) else {
            return;
        };
        if self.tabs.len() >= MAX_TABS {
            self.message = Some(format!(
                "This pane already has {MAX_TABS} tabs. Close a tab before moving another here."
            ));
            return;
        }
        let Some(tab) = self.take_from_slot(source, drag.tab, cx) else {
            return;
        };
        self.tabs[self.active_tab] = self.capture_tab(cx);
        let index = slot.unwrap_or(self.tabs.len()).min(self.tabs.len());
        self.tabs.insert(index, tab);
        self.activate_tab(index, window, cx);
        self.remember_session(cx);
    }

    /// Remove a tab from an unfocused pane. The pane shows its next tab, or
    /// closes when that was its last tab.
    pub(super) fn take_from_slot(
        &mut self,
        index: usize,
        tab: usize,
        cx: &mut Context<Self>,
    ) -> Option<tabs::Tab> {
        let slot = self.panes.get_mut(index)?;
        let from = slot.tabs.iter().position(|t| t.id == tab)?;
        let was_active = from == slot.active_tab;
        let active = slot.tabs[slot.active_tab].id;
        let pane = slot.id;
        let taken = slot.tabs.remove(from);
        if slot.tabs.is_empty() {
            if let Some(layout) = self.layout.clone().without(pane) {
                self.layout = layout;
                self.panes.remove(index);
            }
        } else {
            slot.active_tab = if was_active {
                from.min(slot.tabs.len() - 1)
            } else {
                slot.tabs.iter().position(|t| t.id == active).unwrap_or(0)
            };
            if was_active {
                self.load_inactive(index, cx);
            }
        }
        Some(taken)
    }

    /// The pane area: the layout tree with the focused pane's live contents.
    /// Zoom the focused pane to fill the pane area, or show all panes again.
    pub(super) fn toggle_zoom(&mut self, cx: &mut Context<Self>) {
        if !self.split_mode() {
            self.zoomed = false;
            self.message = Some("Split the view to zoom a pane.".into());
            cx.notify();
            return;
        }
        self.zoomed = !self.zoomed;
        let generation = self.zoom_motion.map_or(1, |(_, _, g)| g.wrapping_add(1));
        self.zoom_motion = Some((Instant::now(), self.zoomed, generation));
        cx.notify();
    }

    /// While zooming out: eased progress from the zoomed pane filling the
    /// area (0) to the split layout (1).
    fn unzoom_progress(&self) -> Option<f32> {
        const UNZOOM: std::time::Duration = std::time::Duration::from_millis(260);
        let (at, zoomed, _) = self.zoom_motion?;
        let elapsed = at.elapsed();
        (!zoomed && self.motion_enabled && elapsed < UNZOOM)
            .then(|| crate::motion::ease_out(elapsed.as_secs_f32() / UNZOOM.as_secs_f32()))
    }

    pub(super) fn unzoom(&mut self) {
        self.zoomed = false;
        self.zoom_motion = None;
    }

    /// Zoom toggle shown at the end of a pane's tab strip while split.
    pub(super) fn zoom_button(
        &self,
        pane: PaneId,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.split_mode() {
            return None;
        }
        let zoomed = self.zoomed && pane == self.pane_identity;
        let label = if zoomed {
            format!("Show all {} panes (Ctrl+Shift+Enter)", self.panes.len() + 1)
        } else {
            "Zoom pane (Ctrl+Shift+Enter)".to_string()
        };
        Some(
            gpui_base::Button::new(("zoom-pane", pane.0 as usize))
                .accessibility_label(label.clone())
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(label.clone()).build(window, cx)
                })
                .size(px(28.))
                .flex_shrink_0()
                .rounded(px(6.))
                .when(zoomed, |b| b.bg(p.accent.opacity(0.18)))
                .hover(move |d| d.bg(p.hover))
                .child(
                    Icon::new(if zoomed {
                        Glyph::Minimize2
                    } else {
                        Glyph::Maximize2
                    })
                    .size(px(14.)),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    if this.focus_pane(pane, window, cx) {
                        this.toggle_zoom(cx);
                    }
                }))
                .into_any_element(),
        )
    }

    pub(super) fn pane_area(
        &mut self,
        active: AnyElement,
        p: crate::appearance::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Only the pane that holds focus shows it: a focused document pane
        // takes the outline from the browsing pane.
        self.focused_doc_now = self.focused_document(window, cx);
        let motion = self
            .zoom_motion
            .filter(|(at, _, _)| self.motion_enabled && at.elapsed() < crate::motion::ENTER);
        let area = div()
            .id("panes")
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .p(px(DIVIDER / 2.))
            .flex();
        if self.zoomed {
            // Only the focused pane, expanding from a slight inset.
            let frame = self.pane_frame(self.pane_identity, true, active, p, cx);
            let body = div().size_full().min_w_0().min_h_0().flex().child(frame);
            let body = match motion {
                Some((_, true, generation)) => body
                    .with_animation(
                        ("pane-zoom", generation as usize),
                        crate::motion::enter(),
                        |d, t| d.p(px(18. * (1. - t))).opacity(0.6 + 0.4 * t),
                    )
                    .into_any_element(),
                _ => body.into_any_element(),
            };
            return area.child(body).into_any_element();
        }
        let widths = self.pane_widths(window, cx);
        let layout = self.layout.clone();
        let mut active = Some(active);
        let tree = self.pane_node(&layout, &mut active, &widths, p, window, cx);
        // Zooming out: the zoomed pane shrinks back to its place while the
        // others grow in beside it (the split ratios ease from all-to-it to
        // their own), redrawn each frame until it settles.
        if self.unzoom_progress().is_some() {
            window.request_animation_frame();
        }
        let _ = motion;
        area.child(tree).into_any_element()
    }

    fn pane_node(
        &mut self,
        node: &Node,
        active: &mut Option<AnyElement>,
        widths: &[(PaneId, f32)],
        p: crate::appearance::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            Node::Pane { id } if *id == self.pane_identity => {
                let body = active.take().unwrap_or_else(|| div().into_any_element());
                self.pane_frame(*id, true, body, p, cx)
            }
            Node::Pane { id } if self.is_document(*id) => {
                let focused = self.focused_document(window, cx) == Some(*id);
                let body = self.document_pane(*id, p, window, cx);
                self.pane_frame(*id, focused, body, p, cx)
            }
            Node::Pane { id } => {
                let width = widths
                    .iter()
                    .find(|(pane, _)| pane == id)
                    .map_or(600., |(_, width)| *width);
                let body = self.inactive_pane(*id, width, p, window, cx);
                self.pane_frame(*id, false, body, p, cx)
            }
            Node::Split {
                id,
                axis,
                fraction,
                first,
                second,
            } => {
                let split = *id;
                let horizontal = *axis == Axis::Horizontal;
                let a = self.pane_node(first, active, widths, p, window, cx);
                let b = self.pane_node(second, active, widths, p, window, cx);
                let mut share = f32::from(*fraction) / 10000.;
                if let Some(t) = self.unzoom_progress() {
                    let focus = self.pane_identity;
                    if first.leaves().contains(&focus) {
                        share = 1. + (share - 1.) * t;
                    } else if second.leaves().contains(&focus) {
                        share *= t;
                    }
                }
                // Minimums apply while dragging a divider; at render time panes
                // share whatever space exists, so a small window never makes
                // them spill over each other.
                let side = |d: Div| {
                    d.flex()
                        .min_w_0()
                        .min_h_0()
                        .overflow_hidden()
                        .when(horizontal, |d| d.h_full())
                        .when(!horizontal, |d| d.w_full())
                };
                let divider = div()
                    .id(("pane-divider", split.0 as usize))
                    .role(Role::Splitter)
                    .aria_label("Resize panes. Double-click to share equally.")
                    .flex_shrink_0()
                    .rounded(px(3.))
                    .when(horizontal, |d| {
                        d.w(px(DIVIDER)).h_full().cursor_col_resize()
                    })
                    .when(!horizontal, |d| {
                        d.h(px(DIVIDER)).w_full().cursor_row_resize()
                    })
                    .hover(move |d| d.bg(p.accent.opacity(0.28)))
                    .on_drag(DividerDrag { split }, |drag, _, _, cx| {
                        cx.new(|_| drag.clone())
                    })
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        if event.click_count() == 2 && this.layout.resize(split, 5000) {
                            this.remember_session(cx);
                            cx.notify();
                        }
                    }));
                div()
                    .id(("pane-split", split.0 as usize))
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .when(horizontal, |d| d.flex_row())
                    .when(!horizontal, |d| d.flex_col())
                    .on_drag_move(cx.listener(
                        move |this, event: &DragMoveEvent<DividerDrag>, _, cx| {
                            if event.drag(cx).split != split {
                                return;
                            }
                            let bounds = event.bounds;
                            let position = event.event.position;
                            let (offset, total, minimum) = if horizontal {
                                (
                                    f32::from(position.x - bounds.origin.x),
                                    f32::from(bounds.size.width),
                                    MIN_WIDTH,
                                )
                            } else {
                                (
                                    f32::from(position.y - bounds.origin.y),
                                    f32::from(bounds.size.height),
                                    MIN_HEIGHT,
                                )
                            };
                            let fraction = divider_fraction(offset, total, minimum);
                            if this.layout.resize(split, fraction) {
                                this.layout_dirty = true;
                                cx.notify();
                            }
                        },
                    ))
                    .child(
                        side(div())
                            .flex_basis(relative(share))
                            .flex_grow(0.)
                            .flex_shrink(1.)
                            .child(a),
                    )
                    .child(divider)
                    .child(side(div()).flex_1().flex_basis(px(0.)).child(b))
                    .into_any_element()
            }
        }
    }

    fn pane_frame(
        &self,
        id: PaneId,
        active: bool,
        body: AnyElement,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entering = self
            .pane_motion
            .filter(|(pane, started)| *pane == id && started.elapsed() < crate::motion::SETTLE)
            .is_some()
            && self.motion_enabled;
        let overlay = self.split_overlay(id, true, p);
        // The focused pane's edge is a touch brighter than the others: no
        // glow, nothing that tints its contents.
        let highlight = active && self.focused_doc_now.is_none_or(|doc| doc == id);
        let frame = div()
            .id(("pane", id.0 as usize))
            .relative()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .rounded(crate::tokens::RADIUS_CARD)
            .border_1()
            // The accent is kept for selection and drop targets.
            .border_color(if highlight { p.text.opacity(0.16) } else { p.line })
            .bg(p.pane)
            // Panes coming back while zooming out fade in as they grow.
            .when_some(self.unzoom_progress().filter(|_| id != self.pane_identity), |frame, t| frame.opacity(0.25 + 0.75 * t))
            // The browsing pane takes keyboard focus back from a document
            // pane on any press, even on empty space that focuses nothing.
            .when(active && id == self.pane_identity, |frame| {
                frame.capture_any_mouse_down(cx.listener(|this, _, window, cx| {
                    if this.focused_document(window, cx).is_some() {
                        this.table.read(cx).focus_handle(cx).focus(window, cx);
                        cx.notify();
                    }
                }))
            })
            .when(!active, |frame| {
                frame
                    // Any press makes this the focused pane before its
                    // contents see it, so keys (Ctrl+W, Ctrl+T) go here. A
                    // document pane takes keyboard focus itself; a control
                    // inside it could otherwise swallow the click.
                    .capture_any_mouse_down(cx.listener(move |this, _, window, cx| {
                        if let Some(doc) = this.doc_panes.iter().find(|doc| doc.id == id) {
                            doc.focus.focus(window, cx);
                            cx.notify();
                        } else {
                            this.focus_pane(id, window, cx);
                        }
                    }))
                    .drag_over::<super::daily::FilesDrag>(move |style, _, _, _| {
                        style.border_color(p.accent).bg(p.accent.opacity(0.08))
                    })
                    .on_drop(cx.listener(
                        move |this, drag: &super::daily::FilesDrag, window, cx| {
                            this.drop_on_pane(id, drag, window, cx)
                        },
                    ))
            })
            .child(body)
            .children(overlay);
        let frame = self.split_target(frame, id, true, cx);
        if entering {
            frame
                .with_animation(
                    ("pane-enter", id.0 as usize),
                    crate::motion::settle(),
                    |frame, t| frame.opacity(0.55 + 0.45 * t),
                )
                .into_any_element()
        } else {
            frame.into_any_element()
        }
    }

    fn inactive_pane(
        &mut self,
        id: PaneId,
        width: f32,
        p: crate::appearance::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(index) = self.panes.iter().position(|slot| slot.id == id) else {
            return div().into_any_element();
        };
        let strip = self.pane_tab_strip(index, p, cx);
        let slot = &self.panes[index];
        let table = slot.table.clone();
        let tab = &slot.tabs[slot.active_tab];
        let view = tab.view;
        fit_columns(&table, (width - 16.).max(0.).round() as u32, cx);
        let count = table.read(cx).delegate().rows.len();
        let location = tab.scope.as_ref().map_or_else(
            || {
                if tab.text.is_empty() {
                    "All locations".to_string()
                } else {
                    format!("Search: {}", tab.text)
                }
            },
            |scope| {
                // The nearest folders identify a location; elide ancestors.
                let native = seek_engine::paths::native(&seek_engine::paths::internal(scope));
                let parts: Vec<String> = native
                    .components()
                    .filter(|c| !matches!(c, std::path::Component::RootDir))
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                if parts.len() > 2 {
                    format!("…\\{}", parts[parts.len() - 2..].join("\\"))
                } else {
                    native.to_string_lossy().into_owned()
                }
            },
        );
        let note = if slot.stale && folder_tab(tab).is_none() {
            "Updates when focused".to_string()
        } else {
            // Routine states ("Folder contents") are noise in a side pane;
            // show only problems.
            slot.message
                .clone()
                .filter(|message| message.starts_with("Folder unavailable"))
                .unwrap_or_default()
        };
        let contents = if count == 0 {
            let text = table.read(cx).delegate().empty_message.clone();
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(p.muted)
                .child(text)
                .into_any_element()
        } else if view == View::Columns {
            let source = super::columns::Source {
                pane: id,
                table: table.clone(),
                scroll: slot.grid_scroll.clone(),
                scope: tab.scope.clone(),
                folder_view: folder_tab(tab).is_some(),
                widths: tab.column_widths.clone(),
                width,
                active: false,
            };
            self.columns_view(source, p, window, cx)
        } else if view == View::List {
            // Cached paint does not replay accessibility nodes; assistive
            // technology gets the fully rendered table, as in the focused pane.
            if window.is_a11y_active() {
                DataTable::new(&table)
                    .focus_indicator(false)
                    .stripe(false)
                    .bordered(false)
                    .small()
                    .into_any_element()
            } else {
                slot.result_pane
                    .clone()
                    .cached(StyleRefinement::default().size_full())
                    .into_any_element()
            }
        } else {
            let source = GridSource {
                table: table.clone(),
                scroll: slot.grid_scroll.clone(),
                view,
                visible: slot.visible.clone(),
                width,
                active: false,
                pane: id,
            };
            self.grid_view(source, window, cx)
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(strip)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .relative()
                    .overflow_hidden()
                    .child(div().absolute().inset_0().child(contents)),
            )
            .child(
                div()
                    .h(px(30.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .rounded_b(crate::tokens::RADIUS_CARD - px(1.))
                    .bg(p.chrome)
                    .border_t_1()
                    .border_color(p.line)
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .flex_1()
                            .min_w_0()
                            .child(Icon::new(Glyph::Folder).size(px(12.)))
                            .child(div().min_w_0().truncate().child(location)),
                    )
                    .child(div().flex_shrink_0().child(format!("{count} items")))
                    .when(!note.is_empty(), |bar| {
                        bar.child(div().max_w(px(180.)).truncate().child(note))
                    }),
            )
            .into_any_element()
    }

    /// Tab strip for an unfocused pane. Any press focuses the pane first
    /// (through the frame's capture handler), then acts on that tab.
    fn pane_tab_strip(
        &self,
        index: usize,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let slot = &self.panes[index];
        let pane = slot.id;
        let mut tabs = div()
            .id(("pane-tab-list", pane.0 as usize))
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap(px(2.))
            .overflow_x_scroll()
            .on_drop(cx.listener(move |this, drag: &TabDrag, window, cx| {
                this.drop_tab(drag, pane, window, cx)
            }));
        for (position, tab) in slot.tabs.iter().enumerate() {
            let active = position == slot.active_tab;
            let label = tab.label();
            let folder = !tab.is_all_locations();
            if self.tab_drop == Some((pane, position)) {
                tabs = tabs.child(tab_insertion(p));
            }
            let drag = TabDrag {
                pane,
                tab: tab.id,
                label: label.clone(),
                folder,
            };
            tabs = tabs.child(tab_drag_source(
                div()
                    .id(("pane-tab", tab.id))
                    .h(px(28.))
                    .w(px(200.))
                    .flex_shrink_1()
                    .min_w(px(96.))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .rounded(px(6.))
                    .bg(p.text.opacity(if active { 0.08 } else { 0.03 }))
                    .hover(move |d| d.bg(p.text.opacity(0.07)))
                    .text_color(if active { p.text } else { p.muted })
                    .child(
                        gpui_base::Button::new(("pane-switch-tab", tab.id))
                            .accessibility_label(format!(
                                "Switch to tab {}: {}",
                                position + 1,
                                label
                            ))
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
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .line_height(px(20.))
                                            .child(label.clone()),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this.focus_pane(pane, window, cx) {
                                    this.switch_tab(position, window, cx);
                                }
                            })),
                    )
                    .child(
                        gpui_base::Button::new(("pane-close-tab", tab.id))
                            .accessibility_label(format!("Close tab {}", position + 1))
                            .size(px(24.))
                            .child(Icon::new(gpui_component::IconName::Close).size(px(12.)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this.focus_pane(pane, window, cx) {
                                    this.close_tab(position, window, cx);
                                }
                            })),
                    ),
                drag,
                pane,
                position,
                cx,
            ));
        }
        if self.tab_drop == Some((pane, slot.tabs.len())) {
            tabs = tabs.child(tab_insertion(p));
        }
        tabs = tabs.child(
            gpui_base::Button::new(("pane-new-tab", pane.0 as usize))
                .accessibility_label("New tab in this pane")
                .size(px(28.))
                .rounded(px(6.))
                .bg(p.text.opacity(0.04))
                .hover(move |d| d.bg(p.hover))
                .child(Icon::new(gpui_component::IconName::Plus).size(px(14.)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if this.focus_pane(pane, window, cx) {
                        this.new_tab(None, window, cx);
                    }
                })),
        );
        div()
            .id(("pane-tabs", pane.0 as usize))
            .h(px(38.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(2.))
            .px_2()
            .border_b_1()
            .border_color(p.line)
            .rounded_t(crate::tokens::RADIUS_CARD - px(1.))
            .bg(p.chrome)
            .child(tabs)
            .children(self.zoom_button(pane, p, cx))
            .into_any_element()
    }
}

/// Move the item matching `is` to insertion slot `slot` (an index into the
/// list before removal; `None` means the end). Returns its new index.
pub(super) fn reorder<T>(
    items: &mut Vec<T>,
    is: impl Fn(&T) -> bool,
    slot: Option<usize>,
) -> Option<usize> {
    let from = items.iter().position(is)?;
    let slot = slot.unwrap_or(items.len()).min(items.len());
    let item = items.remove(from);
    let to = if from < slot { slot - 1 } else { slot };
    items.insert(to, item);
    Some(to)
}

/// Divider position in basis points, keeping both sides at least `minimum`
/// wide where the space allows, and always within the persisted 10-90% bounds.
pub(super) fn divider_fraction(offset: f32, total: f32, minimum: f32) -> u16 {
    let available = (total - DIVIDER).max(1.);
    let first = offset - DIVIDER / 2.;
    let low = minimum.min(available / 2.);
    let high = (available - minimum).max(available / 2.);
    let first = first.clamp(low, high);
    ((first / available) * 10000.).round().clamp(1000., 9000.) as u16
}

#[cfg(test)]
mod tests {
    use super::{divider_fraction, reorder};
    #[test]
    fn reorder_uses_insertion_slots() {
        let mut items = vec![1, 2, 3, 4];
        assert_eq!(reorder(&mut items, |i| *i == 1, Some(3)), Some(2));
        assert_eq!(items, [2, 3, 1, 4]);
        assert_eq!(reorder(&mut items, |i| *i == 4, Some(0)), Some(0));
        assert_eq!(items, [4, 2, 3, 1]);
        assert_eq!(reorder(&mut items, |i| *i == 2, None), Some(3));
        assert_eq!(items, [4, 3, 1, 2]);
        assert_eq!(reorder(&mut items, |i| *i == 3, Some(99)), Some(3));
        assert_eq!(items, [4, 1, 2, 3]);
        assert_eq!(reorder(&mut items, |i| *i == 9, Some(0)), None);
        assert_eq!(items, [4, 1, 2, 3]);
    }
    #[test]
    fn divider_keeps_both_sides_usable_and_within_bounds() {
        assert_eq!(divider_fraction(503., 1006., 260.), 5000);
        let left = divider_fraction(-50., 1006., 260.);
        assert!((f32::from(left) / 10000. * 1000.) >= 259.);
        let right = divider_fraction(5000., 1006., 260.);
        assert!((1. - f32::from(right) / 10000.) * 1000. >= 259.);
        // A pane area narrower than two minimums still splits within bounds.
        let narrow = divider_fraction(10., 300., 260.);
        assert!((1000..=9000).contains(&narrow));
        assert!(divider_fraction(f32::MAX, 1006., 260.) <= 9000);
    }
}
