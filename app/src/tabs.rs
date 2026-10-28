//! Tabs share immutable rendered row snapshots; the index and workers are shared.
use super::*;

#[derive(Clone)]
pub(super) struct Tab {
    all_offset: usize,
    id: usize,
    text: String,
    scope: Option<PathBuf>,
    view: View,
    content: bool,
    largest: Option<bool>,
    normal: Option<NormalResults>,
    sort: Option<(usize, ColumnSort)>,
    expanded: std::collections::HashSet<PathBuf>,
    history: Vec<(String, Option<PathBuf>)>,
    history_position: usize,
    list_offset: Point<Pixels>,
    grid_offset: Point<Pixels>,
    selected: Option<PathBuf>,
    snapshot: Option<Arc<Snapshot>>,
    created: Option<std::time::Instant>,
}
impl Tab {
    pub fn initial(view: View) -> Self {
        Self {
            all_offset: 0,
            id: 0,
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
            created: None,
        }
    }
    fn label(&self) -> String {
        if !self.text.is_empty() {
            self.text.clone()
        } else {
            self.scope
                .as_ref()
                .map(|p| {
                    p.file_name()
                        .unwrap_or(p.as_os_str())
                        .to_string_lossy()
                        .into_owned()
                })
                .unwrap_or_else(|| "All locations".into())
        }
    }
}
#[derive(Clone)]
struct Snapshot {
    rows: Arc<Vec<Row>>,
    base_rows: Arc<Vec<Row>>,
    children: std::collections::HashMap<PathBuf, Vec<Row>>,
    depths: Vec<usize>,
    selected_many: std::collections::HashSet<PathBuf>,
    empty_message: String,
    message: Option<String>,
    show_hidden: bool,
    exclusions: Vec<PathBuf>,
    captured: std::time::Instant,
    content_locations: Vec<PathBuf>,
    remember_document_text: bool,
    selection_anchor: Option<PathBuf>,
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
    fn remove(&mut self, paths: &[PathBuf]) {
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
fn ease_out(t: f32) -> f32 {
    1. - (1. - t).powi(4)
}
pub(super) struct Closing {
    index: usize,
    id: usize,
    label: String,
    started: std::time::Instant,
}
impl Closing {
    fn view(&self, p: crate::appearance::Palette) -> AnyElement {
        div()
            .id(("closing-tab", self.id))
            .h(px(28.))
            .flex_shrink_0()
            .overflow_hidden()
            .rounded(px(7.))
            .bg(p.control)
            .text_color(p.muted)
            .child(self.label.clone())
            .with_animation(
                ("tab-exit", self.id),
                Animation::new(std::time::Duration::from_millis(100)).with_easing(ease_out),
                |tab, t| tab.w(px(170. * (1. - t))).opacity(1. - t),
            )
            .into_any_element()
    }
}
pub(super) fn adjacent(active: usize, count: usize, backwards: bool) -> usize {
    if backwards {
        (active + count - 1) % count
    } else {
        (active + 1) % count
    }
}
impl Seek {
    pub(super) fn tabs_available(&self, window: &mut Window, cx: &mut App) -> bool {
        use gpui_component::WindowExt;
        !self.settings
            && !self.preview_enlarged
            && self.edit.is_none()
            && self.permanent_delete.is_none()
            && !window.has_active_dialog(cx)
    }
    fn capture_tab(&self, cx: &App) -> Tab {
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
            snapshot: {
                let data = table.delegate();
                let snapshot = Arc::new(Snapshot {
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
    fn activate_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
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
        let mut tab = self.capture_tab(cx);
        self.tabs[self.active_tab] = tab.clone();
        tab.id = self.next_tab_id;
        tab.created = Some(std::time::Instant::now());
        self.next_tab_id += 1;
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
        // Closing the last tab quits the ordinary window/process.
        if self.tabs.len() == 1 {
            window.remove_window();
            return;
        }
        self.tabs[self.active_tab] = self.capture_tab(cx);
        self.closing_tab = Some(Closing {
            index,
            id: self.tabs[index].id,
            label: self.tabs[index].label(),
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
    pub(super) fn tab_strip(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut strip = div()
            .id("tabs")
            .h(px(34.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .border_b_1()
            .border_color(p.line)
            .bg(p.chrome);
        let mut tabs = div()
            .id("tab-list")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap_1()
            .overflow_x_scroll();
        for (index, saved) in self.tabs.iter().enumerate() {
            if let Some(closing) = &self.closing_tab
                && closing.index == index
                && self.motion_enabled
                && closing.started.elapsed().as_millis() < 100
            {
                tabs = tabs.child(closing.view(p));
            }
            let active = index == self.active_tab;
            let label = if active {
                {
                    let text = self.input.read(cx).value();
                    if text.is_empty() {
                        self.scope
                            .as_ref()
                            .map(|p| {
                                p.file_name()
                                    .unwrap_or(p.as_os_str())
                                    .to_string_lossy()
                                    .into_owned()
                            })
                            .unwrap_or_else(|| "All locations".into())
                    } else {
                        text.to_string()
                    }
                }
            } else {
                saved.label()
            };
            let tab = div()
                .id(("tab", saved.id))
                .h(px(28.))
                .w(px(170.))
                .min_w(px(90.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .rounded(px(7.))
                .border_1()
                .border_color(if active {
                    p.muted.opacity(0.35)
                } else {
                    p.line
                })
                .bg(if active {
                    p.control_selected
                } else {
                    p.control
                })
                .child(
                    gpui_base::Button::new(("switch-tab", saved.id))
                        .accessibility_label(format!("Switch to tab {}: {}", index + 1, label))
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .px_2()
                        .child(div().min_w_0().truncate().line_height(px(20.)).child(label))
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
            let tab = if self.motion_enabled
                && saved.created.is_some_and(|t| t.elapsed().as_millis() < 140)
            {
                tab.with_animation(
                    ("tab-enter", saved.id),
                    Animation::new(std::time::Duration::from_millis(140)).with_easing(ease_out),
                    move |tab, t| {
                        let tab = tab.w(px(108. + 62. * t));
                        if active {
                            tab.bg(p.control_selected.opacity(0.75 + 0.25 * t))
                        } else {
                            tab
                        }
                    },
                )
                .into_any_element()
            } else if self.motion_enabled
                && active
                && self.tab_motion_started.elapsed().as_millis() < 120
            {
                tab.with_animation(
                    ("tab-select", self.tab_motion as usize),
                    Animation::new(std::time::Duration::from_millis(120)).with_easing(ease_out),
                    move |tab, t| tab.bg(p.control_selected.opacity(0.75 + 0.25 * t)),
                )
                .into_any_element()
            } else {
                tab.into_any_element()
            };
            tabs = tabs.child(tab);
        }
        if let Some(closing) = &self.closing_tab
            && closing.index >= self.tabs.len()
            && self.motion_enabled
            && closing.started.elapsed().as_millis() < 100
        {
            tabs = tabs.child(closing.view(p));
        }
        strip = strip.child(tabs).child(
            gpui_base::Button::new("new-tab")
                .accessibility_label("New tab")
                .size(px(26.))
                .child(Icon::new(gpui_component::IconName::Plus).size(px(14.)))
                .on_click(cx.listener(|this, _, window, cx| this.new_tab(None, window, cx))),
        );
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
    fn cycling_wraps_in_both_directions() {
        assert_eq!(adjacent(0, 3, true), 2);
        assert_eq!(adjacent(2, 3, false), 0);
        assert_eq!(adjacent(0, 1, true), 0);
        assert_eq!(adjacent(0, 1, false), 0);
    }
}
