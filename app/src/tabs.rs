//! Tabs retain navigation, not copies of the index, result rows or workers.
use super::*;

#[derive(Clone)]
pub(super) struct Tab {
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
}
impl Tab {
    pub fn initial(view: View) -> Self {
        Self {
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
            && self.edit.is_none()
            && self.permanent_delete.is_none()
            && !window.has_active_dialog(cx)
    }
    fn capture_tab(&self, cx: &App) -> Tab {
        let table = self.table.read(cx);
        Tab {
            id: self.tabs[self.active_tab].id,
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
            selected: self.selected(cx).map(|r| r.file.path).or_else(|| {
                self.pending_tab_restore
                    .as_ref()
                    .and_then(|tab| tab.selected.clone())
            }),
        }
    }
    fn activate_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.tabs[index].clone();
        self.active_tab = index;
        self.scope = tab.scope.clone();
        self.view = tab.view;
        self.content_mode = tab.content;
        self.largest = tab.largest;
        self.normal_results = tab.normal.clone();
        self.history = tab.history.clone();
        self.history_position = tab.history_position;
        self.suggestions = None;
        self.suggestion_dismiss = None;
        self.applied_view = None;
        self.table.update(cx, |table, cx| {
            let data = table.delegate_mut();
            data.rows.clear();
            data.base_rows.clear();
            data.children.clear();
            data.expanded = tab.expanded.clone();
            data.sort = tab.sort;
            for (i, column) in data.columns.iter_mut().enumerate() {
                column.sort = Some(
                    tab.sort
                        .filter(|(c, _)| *c == i)
                        .map_or(ColumnSort::Default, |(_, s)| s),
                );
            }
            data.selected_many.clear();
            data.drag_paths = Arc::new(vec![]);
            data.empty_message = "Loading tab...".into();
            table.clear_selection(cx);
            table.refresh(cx);
        });
        self.restoring_input = Some(tab.text.clone());
        self.input.update(cx, |input, cx| {
            input.set_value(tab.text.clone(), window, cx);
            input.focus(window, cx);
        });
        self.decorate_filters(window, cx);
        self.pending_tab_restore = Some(tab);
        self.submit(cx);
        self.refresh_preview(cx);
        cx.notify();
        crate::metrics::interaction(
            "tab_activate",
            serde_json::json!({"active": index, "count": self.tabs.len(), "query": self.input.read(cx).value().as_str(), "scope": self.scope}),
        );
    }
    pub(super) fn switch_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index == self.active_tab || index >= self.tabs.len() {
            return;
        }
        self.tabs[self.active_tab] = self.capture_tab(cx);
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
        self.next_tab_id += 1;
        if let Some(folder) = folder {
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
        self.activate_tab(self.tabs.len() - 1, window, cx);
    }
    pub(super) fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        // Closing the last tab quits the ordinary window/process.
        if self.tabs.len() == 1 {
            window.remove_window();
            return;
        }
        self.tabs[self.active_tab] = self.capture_tab(cx);
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
        if self.table.read(cx).delegate().rows.is_empty()
            && self
                .message
                .as_deref()
                .is_some_and(|s| s.contains("Indexing") || s.contains("Searching"))
        {
            return;
        }
        if self.content_mode
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
            let selected = tab
                .selected
                .as_ref()
                .and_then(|p| table.delegate().rows.iter().position(|r| &r.file.path == p));
            if let Some(i) = selected {
                table.set_selected_row(i, cx);
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
            tabs = tabs.child(
                div()
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
                    ),
            );
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
