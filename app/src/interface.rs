use super::*;
use gpui_component::Selectable;
use gpui_component::menu::{ContextMenuExt, DropdownMenu};
use gpui_component_assets::IconName as Glyph;
use std::collections::HashSet;

pub(super) fn needs_shell(row: &Row) -> bool {
    row.file.kind & 3 != seek_engine::walk::KIND_LINK
}
pub(super) fn request_media(
    worker: &media::Worker,
    visible: &Arc<Mutex<HashSet<media::Key>>>,
    extra: &Arc<Mutex<HashSet<media::Key>>>,
    cache: &Arc<Mutex<media::Cache>>,
) {
    let mut keys = visible.lock().unwrap().clone();
    keys.extend(extra.lock().unwrap().iter().cloned());
    // Keep completed, visible keys wanted. A reply or filesystem event must
    // not cancel the rest of a thumbnail batch just because one image loaded.
    let _ = cache;
    worker.request(keys);
}
pub(super) fn thumbnail_allowed(row: &Row, settings: &Settings) -> bool {
    settings.image_thumbnails
        || !matches!(
            crate::icons::kind(&row.file.path, false),
            crate::icons::Kind::Image | crate::icons::Kind::Video
        )
}
pub(super) fn file_image(
    row: &Row,
    size: f32,
    atlas: &Atlas,
    cache: &Arc<Mutex<media::Cache>>,
    thumbnails: bool,
) -> AnyElement {
    let edge = if size <= 24. {
        32
    } else if size <= 128. {
        128
    } else {
        media::PREVIEW_EDGE
    };
    let key = media::Key::new(row, edge, false);
    let value = if thumbnails {
        cache.lock().unwrap().get(&key)
    } else {
        None
    };
    // A small association icon enlarged to preview size visibly flashes before
    // the real shell image arrives. Reserve the final bounds while it loads;
    // an icon fallback is appropriate only after an actual provider failure.
    if thumbnails && edge == media::PREVIEW_EDGE && value.is_none() && needs_shell(row) {
        return div()
            .size(px(size))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.))
            .child("Loading preview...")
            .into_any_element();
    }
    let image = match value {
        Some(media::Value::Image(Some(i))) => i,
        _ => {
            let folder = row.file.kind & 3 == seek_engine::walk::KIND_DIR;
            // The file type's own Windows icon (32 px) when it's already
            // known, else Seek's icon for the kind.
            (size <= 32.)
                .then(|| crate::icons::type_icon(&row.file.path, folder))
                .flatten()
                .unwrap_or_else(|| atlas.get(crate::icons::kind(&row.file.path, folder)))
        }
    };
    img(image)
        .size(px(size))
        .object_fit(ObjectFit::Contain)
        .flex_shrink_0()
        .into_any_element()
}
fn fluent_icon(icon: Glyph, size: f32) -> AnyElement {
    Icon::new(icon).size(px(size)).into_any_element()
}
// Base controls inherit a one-em line box, placing Segoe's visible ink low.
/// A download glyph after the name of a cloud placeholder that is not on
/// this PC (OneDrive and other sync providers).
pub(super) fn cloud_badge(row: &Row, p: crate::appearance::Palette) -> Option<AnyElement> {
    (row.file.kind & seek_engine::walk::FLAG_CLOUD != 0).then(|| {
        div()
            .flex_shrink_0()
            .id("cloud-badge")
            .tooltip(|window, cx| {
                gpui_component::tooltip::Tooltip::new("Not downloaded").build(window, cx)
            })
            .child(
                Icon::empty()
                    .path("icons/cloud-download.svg")
                    .size(px(13.))
                    .text_color(p.tints.blue),
            )
            .into_any_element()
    })
}
pub(super) fn button_text(label: impl Into<SharedString>) -> Div {
    div()
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .line_height(px(20.))
        .child(label.into())
}
fn tool(id: &'static str, icon: Glyph, label: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .w(px(32.))
        .h(px(30.))
        .px_0()
        .py_0()
        .rounded(px(5.))
        .child(
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    if id == "hidden" || matches!(icon, Glyph::Settings | Glyph::Info) {
                        Icon::default()
                            .path(if id == "hidden" {
                                "seek/hidden-files.svg"
                            } else if icon == Glyph::Info {
                                "icons/info.svg"
                            } else {
                                "icons/settings.svg"
                            })
                            .size(px(14.))
                            .flex_shrink_0()
                            .into_any_element()
                    } else {
                        fluent_icon(
                            icon,
                            if matches!(icon, Glyph::PanelLeft) {
                                16.
                            } else {
                                14.
                            },
                        )
                    },
                ),
        )
        .tooltip(label)
        .accessibility_label(label)
}
fn toolbar_state(button: Button, active: bool, p: crate::appearance::Palette, cx: &App) -> Button {
    button
        .custom(
            gpui_component::button::ButtonCustomVariant::new(cx)
                .foreground(if active { p.text } else { p.muted })
                .hover(p.hover)
                .active(p.accent.opacity(0.14)),
        )
        .selected(active)
}
fn toolbar_group(p: crate::appearance::Palette) -> Div {
    let dark = p.text.l > 0.5;
    // One faint fill groups related controls; no border or gradient.
    div()
        .occlude()
        .flex()
        .items_center()
        .gap_0()
        .flex_shrink_0()
        .h(px(32.))
        .rounded(px(8.))
        .bg(Hsla::from(if dark {
            rgba(0xffffff0a)
        } else {
            rgba(0x0000000a)
        }))
}
fn toolbar_separator(p: crate::appearance::Palette) -> Div {
    div().w(px(1.)).h(px(20.)).flex_shrink_0().bg(p.line)
}
pub(super) fn grid_row_height(view: View, options: crate::icon_view::IconView) -> Pixels {
    px(if view == View::Gallery {
        136.
    } else {
        options.row_height()
    })
}
pub(super) fn heading(label: &'static str, p: crate::appearance::Palette) -> Div {
    div()
        .px_2()
        .h(px(30.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .mb_2()
        .when(label != "Favorites", |d| d.mt_3())
        .line_height(px(20.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.muted)
        .child(label)
}
fn window_controls(p: crate::appearance::Palette, window: &Window) -> AnyElement {
    let mut controls = toolbar_group(p);
    for (id, area, icon, label) in [
        (
            "minimize",
            WindowControlArea::Min,
            Glyph::WindowMinimize,
            "Minimize",
        ),
        (
            "maximize",
            WindowControlArea::Max,
            if window.is_maximized() {
                Glyph::WindowRestore
            } else {
                Glyph::WindowMaximize
            },
            "Maximize or restore",
        ),
        (
            "close",
            WindowControlArea::Close,
            Glyph::WindowClose,
            "Close",
        ),
    ] {
        let close = id == "close";
        controls = controls.child(
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .window_control_area(area)
                .w(px(26.))
                .h(px(30.))
                .rounded(px(5.))
                .flex()
                .items_center()
                .justify_center()
                .hover(move |s| {
                    s.bg(if close { rgb(0xc42b1c).into() } else { p.hover })
                        .text_color(if close { rgb(0xffffff).into() } else { p.text })
                })
                .child(Icon::new(icon).size(px(11.))),
        );
    }
    controls.into_any_element()
}
pub(super) fn disclosure(
    row: &Row,
    depth: usize,
    expanded: bool,
    owner: WeakEntity<Seek>,
) -> AnyElement {
    let mut gutter = div()
        .w(px(20. + depth as f32 * 16.))
        .flex_shrink_0()
        .flex()
        .justify_end()
        .items_center();
    if row.file.kind & 3 == seek_engine::walk::KIND_DIR {
        let path = row.file.path.clone();
        gutter = gutter.child(
            Button::new(format!("expand-{}", path.display()))
                .ghost()
                .with_size(px(22.))
                .w(px(20.))
                .h(px(24.))
                .accessibility_label(format!(
                    "{} {}",
                    if expanded { "Collapse" } else { "Expand" },
                    row.name
                ))
                .icon(if expanded {
                    Glyph::ChevronDown
                } else {
                    Glyph::ChevronRight
                })
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    let _ = owner.update(cx, |this, cx| {
                        this.expand_folder(path.clone(), !expanded, cx);
                        this.table.read(cx).focus_handle(cx).focus(window, cx);
                    });
                }),
        );
    }
    gutter.into_any_element()
}

impl Seek {
    pub(super) fn expand_folder(&mut self, path: PathBuf, expand: bool, cx: &mut Context<Self>) {
        let selected = self.selected(cx).map(|r| r.file.path);
        self.table.update(cx, |table, cx| {
            let data = table.delegate_mut();
            if expand {
                data.expanded.insert(path.clone());
            } else {
                data.expanded.retain(|p| !p.starts_with(&path));
                data.children.retain(|p, _| !p.starts_with(&path));
            }
            data.sorted();
            let selection = selected
                .as_ref()
                .and_then(|p| data.rows.iter().position(|r| &r.file.path == p))
                .or_else(|| data.rows.iter().position(|r| r.file.path == path));
            if let Some(i) = selection {
                table.set_selected_row(i, cx);
            }
            table.refresh(cx);
        });
        self.submit(cx);
        self.refresh_preview(cx);
        cx.notify();
    }
    pub(super) fn refresh_preview(&mut self, cx: &mut Context<Self>) {
        self.refresh_inspector(cx);
        self.sync_text_reader(cx);
        self.sync_document_preview(cx);
        let mut keys = HashSet::new();
        // Gallery and the column view's summary show the selection's preview
        // even with the info panel closed.
        let shows_preview = matches!(self.view, View::Gallery | View::Columns);
        if (self.panel || self.preview_enlarged || shows_preview)
            && let Some(row) = self.selected(cx)
        {
            if self.inspector.rested {
                keys.insert(media::Key::info(
                    &row,
                    self.preferences.automatic_previews || self.preview_enlarged,
                ));
            }
            if (self.preferences.automatic_previews || self.preview_enlarged || shows_preview)
                && needs_shell(&row)
                && (crate::document_preview::Key::new(&row, 0).is_none()
                    || self.view == View::Gallery)
            {
                keys.insert(media::Key::new(&row, media::PREVIEW_EDGE, false));
                if self.preview_enlarged
                    && row.file.kind & 3 == seek_engine::walk::KIND_FILE
                    && crate::icons::kind(&row.file.path, false) == crate::icons::Kind::Image
                {
                    keys.insert(media::Key::new(
                        &row,
                        crate::document_preview::ENLARGED_EDGE,
                        false,
                    ));
                }
            }
        }
        keys.extend(self.document_keys());
        let retired = self.cache.lock().unwrap().retain_previews(&keys);
        self.document_images_to_drop.extend(retired);
        *self.extra.lock().unwrap() = keys;
        request_media(&self.media, &self.visible, &self.extra, &self.cache);
    }
    fn sync_document_preview(&mut self, cx: &mut Context<Self>) {
        let mut wanted = ((self.panel || self.view == View::Columns)
            && self.preferences.automatic_previews
            || self.preview_enlarged)
            .then(|| self.selected(cx))
            .flatten()
            .and_then(|r| crate::document_preview::Key::new(&r, 0));
        if let Some(key) = &mut wanted
            && self.preview_enlarged
        {
            key.file.edge = crate::document_preview::ENLARGED_EDGE;
        }
        if let (Some(old), Some(next)) = (&self.document_key, &mut wanted)
            && old.file.path == next.file.path
            && old.file.mtime == next.file.mtime
            && old.file.size == next.file.size
        {
            next.page = old.page;
        }
        if self.document_key != wanted {
            if self.document_key.as_ref().map(|k| &k.file.path)
                != wanted.as_ref().map(|k| &k.file.path)
            {
                self.document_pages = None;
            }
            if let Some(Ok(old)) = self.document.take()
                && let Some(image) = old.image
            {
                self.document_images_to_drop.push(image);
            }
            self.document_key = wanted.clone();
            self.documents.request(wanted.clone());
            self.watch_preview(wanted, cx);
            #[cfg(feature = "benchmarks")]
            crate::metrics::interaction(
                "document_request",
                serde_json::json!({"active":self.document_key.is_some(),"retained":self.document.is_some()}),
            );
        }
    }
    fn watch_preview(&mut self, key: Option<crate::document_preview::Key>, cx: &mut Context<Self>) {
        self.preview_watchdog = if key.is_some() {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(crate::preview_diagnostics::UI_WATCHDOG)
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.document_key == key && this.document.is_none() {
                        this.documents.request(None);
                        this.document = Some(Err(
                            "Preview timed out. Try again or open the document in its app".into(),
                        ));
                        cx.notify();
                    }
                });
            })
        } else {
            Task::ready(())
        };
    }
    pub(super) fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        if self.content_mode && view != View::List {
            return;
        }
        let was_largest = self.largest.take().is_some();
        #[cfg(feature = "benchmarks")]
        crate::metrics::interaction("view", serde_json::json!({"view":format!("{view:?}")}));
        self.view = view;
        self.remember_session(cx);
        if was_largest {
            self.submit(cx);
        }
        self.visible.lock().unwrap().clear();
        self.refresh_preview(cx);
        cx.notify();
    }
    /// The empty-space menu's state for the focused pane, given its table's sort.
    pub(super) fn background_state(
        &self,
        sort: Option<(usize, ColumnSort)>,
        cx: &App,
    ) -> super::daily::Background {
        super::daily::Background {
            folder: self.scope.clone().filter(|_| {
                self.input.read(cx).value().trim().is_empty()
                    && !self.content_mode
                    && self.largest.is_none()
            }),
            view: self.view,
            sort,
            hidden: self.show_hidden,
        }
    }
    pub(super) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_from = f32::from_bits(self.sidebar_width.load(Ordering::Relaxed));
        self.sidebar = !self.sidebar;
        self.sidebar_motion = self.sidebar_motion.wrapping_add(1);
        cx.notify();
    }
    pub(super) fn toggle_hidden(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_hidden = !self.show_hidden;
        self.save_preferences(window, cx);
        self.submit(cx);
    }
    pub(super) fn save_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_background(cx);
        *crate::mcp::owner::POLICY.write().unwrap() = crate::mcp::owner::Policy {
            can_change: self.preferences.agents_can_change,
            folders: self.preferences.agent_folders.clone(),
        };
        self.preferences.show_hidden = self.show_hidden;
        self.store.save(&self.preferences);
        crate::appearance::apply(&self.preferences, Some(window), cx);
        self.table.update(cx, |table, cx| {
            table.delegate_mut().preferences = self.preferences.clone();
            table.delegate().refresh_media();
            cx.notify();
        });
        for slot in &self.panes {
            slot.table.update(cx, |table, cx| {
                table.delegate_mut().preferences = self.preferences.clone();
                cx.notify();
            });
        }
        self.columns
            .sync_filters(self.show_hidden, &self.preferences.exclusions);
        // Unfocused folder panes re-read when hidden files or exclusions change.
        for index in 0..self.panes.len() {
            let filters_changed = self.panes[index].applied_view.as_ref().is_some_and(
                |(_, _, hidden, exclusions, _, _)| {
                    *hidden != self.show_hidden || *exclusions != self.preferences.exclusions
                },
            );
            if filters_changed {
                self.load_inactive(index, cx);
            }
        }
        self.refresh_preview(cx);
        cx.notify();
    }
    pub(super) fn sort_by(
        &mut self,
        column: usize,
        direction: ColumnSort,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = self.selected(cx).map(|r| r.file.path);
        self.table.update(cx, |table, cx| {
            let rows = table.delegate_mut();
            rows.sort = Some((column, direction));
            for (i, c) in rows.columns.iter_mut().enumerate() {
                c.sort = Some(if i == column {
                    direction
                } else {
                    ColumnSort::Default
                });
            }
            rows.sorted();
            rows.refresh_media();
            let next = selected.and_then(|p| rows.rows.iter().position(|r| r.file.path == p));
            if let Some(i) = next {
                table.set_selected_row(i, cx);
            }
            table.refresh(cx);
        });
        self.table.read(cx).focus_handle(cx).focus(window, cx);
        self.sort_open = false;
        self.refresh_preview(cx);
        cx.notify();
    }
    /// Release after a press on a multi-selected grid item: select only it,
    /// unless the press became a drag.
    pub(super) fn release_selection(&mut self, index: usize, cx: &mut Context<Self>) {
        if cx.has_active_drag() {
            return;
        }
        let collapsed = self.table.update(cx, |table, _| {
            let data = table.delegate_mut();
            if data.pending_collapse.take() != Some(index) {
                return false;
            }
            crate::selection::change(
                &data.rows,
                &mut data.selected_many,
                &mut data.selection_anchor,
                Some(index),
                index,
                crate::selection::Input {
                    shift: false,
                    control: false,
                    keyboard: false,
                },
            );
            data.drag_paths = Arc::new(crate::selection::ordered(&data.rows, &data.selected_many));
            true
        });
        if collapsed {
            self.refresh_preview(cx);
            cx.notify();
        }
    }
    pub(super) fn select_modified(
        &mut self,
        index: usize,
        shift: bool,
        control: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.table.update(cx, |table, _| {
            let current = table.selected_row();
            let data = table.delegate_mut();
            let keep = data.rows.get(index).is_some_and(|row| {
                crate::selection::press_keeps_selection(
                    &data.selected_many,
                    &row.file.path,
                    shift,
                    control,
                )
            });
            data.pending_collapse = keep.then_some(index);
            if !keep {
                crate::selection::change(
                    &data.rows,
                    &mut data.selected_many,
                    &mut data.selection_anchor,
                    current,
                    index,
                    crate::selection::Input {
                        shift,
                        control,
                        keyboard: false,
                    },
                );
            }
            data.drag_paths = Arc::new(crate::selection::ordered(&data.rows, &data.selected_many));
        });
        self.table.update(cx, |t, cx| t.set_selected_row(index, cx));
        self.table.read(cx).focus_handle(cx).focus(window, cx);
        self.refresh_preview(cx);
        cx.notify();
    }
    /// Put `moved` where `before` is in the Locations order.
    fn move_location(
        &mut self,
        moved: PathBuf,
        before: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if moved == before || !self.roots.contains(&moved) {
            return;
        }
        let order = &self.preferences.location_order;
        let mut all: Vec<PathBuf> = order
            .iter()
            .filter(|p| self.roots.contains(p))
            .cloned()
            .collect();
        all.extend(self.roots.iter().filter(|p| !order.contains(p)).cloned());
        all.retain(|p| *p != moved);
        let at = all.iter().position(|p| *p == before).unwrap_or(all.len());
        all.insert(at, moved);
        self.preferences.location_order = all;
        self.save_preferences(window, cx);
    }
    fn toggle_location_hidden(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let hidden = &mut self.preferences.hidden_locations;
        if let Some(at) = hidden.iter().position(|p| *p == path) {
            hidden.remove(at);
        } else {
            hidden.push(path);
        }
        self.save_preferences(window, cx);
    }
    fn sidebar_view(&self, p: crate::appearance::Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut scroll = div()
            .id("sidebar-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_2()
            .pb_3()
            .flex()
            .flex_col();
        scroll = scroll.children(self.favorites_section(p, cx));
        scroll = scroll.child(heading("Locations", p));
        // The user's order first, then any new locations; hidden ones only
        // while "Show hidden" is on.
        let order = &self.preferences.location_order;
        let mut roots: Vec<PathBuf> = order
            .iter()
            .filter(|p| self.roots.contains(p))
            .cloned()
            .collect();
        roots.extend(self.roots.iter().filter(|p| !order.contains(p)).cloned());
        let hidden_count = roots
            .iter()
            .filter(|p| self.preferences.hidden_locations.contains(p))
            .count();
        if !self.show_hidden_locations {
            roots.retain(|p| !self.preferences.hidden_locations.contains(p));
        }
        for (index, path) in roots.iter().enumerate() {
            let path = path.clone();
            let hidden = self.preferences.hidden_locations.contains(&path);
            let reorder_target = path.clone();
            let menu_path = path.clone();
            let menu_owner = cx.entity().downgrade();
            let drop_path = path.clone();
            let feedback_path = path.clone();
            let tab_path = path.clone();
            let tab_owner = cx.entity().downgrade();
            let selected = self.scope.as_ref() == Some(&path);
            let capacity = self.places.capacity.get(&path).cloned();
            let name = path
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_owned();
            scroll = scroll.child(
                div()
                    .id(("drive", index))
                    .on_drag(
                        super::drag_split::PlaceDrag {
                            path: Some(path.clone()),
                            label: name.clone(),
                            item: None,
                        },
                        |drag, offset, _, cx| {
                            super::drag_preview::near_cursor(drag.clone(), offset, cx)
                        },
                    )
                    .drag_over::<super::daily::FilesDrag>(move |style, drag, _, _| {
                        if super::daily::drop_allowed(&drag.paths, &feedback_path) {
                            style.bg(p.accent.opacity(0.22))
                        } else {
                            style
                        }
                    })
                    .on_drop(cx.listener(
                        move |this, drag: &super::daily::FilesDrag, window, cx| {
                            this.drop_files(drag, drop_path.clone(), window, cx)
                        },
                    ))
                    // Drop one location on another to put it there.
                    .drag_over::<super::drag_split::PlaceDrag>(move |style, drag, _, _| {
                        if drag.item.is_none() && drag.path.is_some() {
                            style.bg(p.accent.opacity(0.12))
                        } else {
                            style
                        }
                    })
                    .on_drop(cx.listener(
                        move |this, drag: &super::drag_split::PlaceDrag, window, cx| {
                            if drag.item.is_none()
                                && let Some(moved) = drag.path.clone()
                            {
                                this.move_location(moved, reorder_target.clone(), window, cx);
                            }
                        },
                    ))
                    .when(hidden, |d| d.opacity(0.55))
                    .role(Role::Button)
                    .aria_label(name.clone())
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(30.))
                    .px_2()
                    .rounded(crate::tokens::RADIUS_ROW)
                    .cursor_pointer()
                    .bg(if selected { p.hover } else { rgba(0).into() })
                    .text_color(p.text)
                    .hover(move |s| s.bg(p.hover))
                    .child(
                        Icon::new(Glyph::HardDrive)
                            .size(px(16.))
                            .text_color(if selected { p.text } else { p.muted }),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(div().truncate().child(name))
                            .when_some(capacity.clone(), |view, disk| {
                                view.child(div().text_size(px(11.)).text_color(p.muted).child(
                                    format!(
                                        "{} free of {}",
                                        super::storage_view::human(disk.free),
                                        super::storage_view::human(disk.total),
                                    ),
                                ))
                            }),
                    )
                    .when(capacity.is_some(), |view| view.h(px(44.)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.scope_to(Some(path.clone()), window, cx)
                    }))
                    .context_menu(move |menu, window, cx| {
                        let owner = menu_owner.clone();
                        let path = menu_path.clone();
                        let menu = super::daily::menu_style(menu, window)
                            .item(
                                gpui_component::menu::PopupMenuItem::new(if hidden {
                                    "Show in sidebar"
                                } else {
                                    "Hide from sidebar"
                                })
                                .on_click(move |_, window, cx| {
                                    let _ = owner.update(cx, |this, cx| {
                                        this.toggle_location_hidden(path.clone(), window, cx)
                                    });
                                }),
                            )
                            .separator();
                        super::daily::actions_menu(
                            menu,
                            tab_owner.clone(),
                            Some(tab_path.clone()),
                            true,
                            cx,
                        )
                    }),
            );
        }
        if hidden_count > 0 {
            let showing = self.show_hidden_locations;
            scroll = scroll.child(
                div()
                    .id("show-hidden-locations")
                    .role(Role::Button)
                    .h(px(26.))
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded(crate::tokens::RADIUS_ROW)
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(p.muted)
                    .hover(move |s| s.bg(p.hover).text_color(p.text))
                    .child(if showing {
                        "Hide hidden locations".to_string()
                    } else {
                        format!("Show {hidden_count} hidden")
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_hidden_locations = !this.show_hidden_locations;
                        cx.notify();
                    })),
            );
        }
        // Tools that look across locations, apart from the places you go.
        scroll = scroll
            .child(heading("Tools", p))
            .child(self.storage_row(p, cx))
            .child(self.worktrees_row(p, cx));
        let footer = div()
            .h(px(30.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .border_t_1()
            .border_color(p.line)
            .text_size(px(11.))
            .text_color(p.muted)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .truncate()
                    .child(Icon::new(Glyph::Search).size(px(13.)).text_color(p.muted))
                    .child(format!("{} indexed", thousands(self.status.entries)))
                    .child(
                        div()
                            .id("index-mode")
                            .text_color(p.muted.opacity(0.8))
                            .child(index_mode(&self.status.source).0)
                            .tooltip({
                                let detail = index_mode(&self.status.source).1;
                                move |window, cx| {
                                    gpui_component::tooltip::Tooltip::new(detail).build(window, cx)
                                }
                            }),
                    ),
            )
            .children(self.update_indicator(p, cx));
        div()
            .id("sidebar-content")
            .w(px(220.))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(p.sidebar)
            .border_r_1()
            .border_color(p.line)
            .child(scroll)
            .child(
                div().px_2().pb_2().child(
                    Button::new("sidebar-jump")
                        .ghost()
                        .w_full()
                        .h(px(32.))
                        .px_2()
                        .py_0()
                        .rounded(px(8.))
                        .accessibility_label("Jump to location or command (Ctrl+K)")
                        .child(
                            div()
                                .w_full()
                                .h_full()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(Icon::new(Glyph::Keyboard).size(px(14.)).text_color(p.muted))
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(13.))
                                        .line_height(px(20.))
                                        .child("Jump..."),
                                ),
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| {
                                this.open_command_palette(window, cx)
                            }),
                        ),
                ),
            )
            .child(footer)
            .into_any_element()
    }
    fn toolbar(
        &self,
        p: crate::appearance::Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = cx.entity().downgrade();
        let content_owner = cx.entity().downgrade();
        let prefix = div()
            .flex()
            .items_center()
            .gap_1()
            .child(fluent_icon(Glyph::Search, 16.))
            .when(self.content_mode, |d| {
                d.child(
                    Button::new("leave-content")
                        .ghost()
                        .h(px(22.))
                        .px_2()
                        .py_0()
                        .text_size(px(11.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child("Content")
                                .child(Icon::new(Glyph::X).size(px(11.))),
                        )
                        .accessibility_label("Exit content search")
                        .on_click(move |_, window, cx| {
                            let _ = content_owner
                                .update(cx, |this, cx| this.set_content_mode(false, window, cx));
                        }),
                )
            });
        let search = Input::new(&self.input)
            .appearance(false)
            .bordered(false)
            .small()
            .h(px(30.))
            .py_0()
            .prefix(prefix)
            .suffix(self.search_help(p))
            .text_size(px(13.))
            .line_height(px(18.))
            .token(move |token, _, _| {
                if token.token().label().starts_with("Type:") {
                    let label = token.token().label().to_lowercase();
                    let t = p.tints;
                    let tint = [
                        ("image", t.purple),
                        ("video", t.pink),
                        ("audio", t.orange),
                        ("doc", t.blue),
                        ("code", t.cyan),
                        ("archive", t.green),
                        ("folder", t.orange),
                    ]
                    .into_iter()
                    .find(|(kind, _)| label.contains(kind))
                    .map_or(p.accent, |(_, tint)| tint);
                    return div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .h(token.line_height())
                        .max_w(token.available_width())
                        .px_1()
                        .gap_1()
                        .rounded_md()
                        .bg(tint.opacity(0.16))
                        .text_color(p.text)
                        .text_size(px(11.))
                        .line_height(relative(1.))
                        .child(token.token().label().clone())
                        .child(Icon::new(Glyph::ChevronDown).size(px(10.)))
                        .child(Icon::new(Glyph::X).size(px(11.)))
                        .into_any_element();
                }
                div()
                    .px_2()
                    .rounded_md()
                    .bg(p.accent.opacity(0.26))
                    .text_color(p.text)
                    .child(format!("{} ×", token.token().label()))
                    .into_any_element()
            })
            .on_token_click(move |event, window, cx| {
                let _ = view.update(cx, |this, cx| {
                    let mut text = this.input.read(cx).value().to_string();
                    if text.get(event.range()).is_none() {
                        return;
                    }
                    if text.get(event.range()).is_some_and(|v| {
                        ["type:", "kind:", "ext:"].iter().any(|p| v.starts_with(p))
                    }) {
                        if matches!(event.click(), ClickEvent::Mouse(_))
                            && event.click().position().x >= event.bounds().right() - px(20.)
                        {
                            text.replace_range(event.range(), "");
                            this.input
                                .update(cx, |input, cx| input.set_value(text, window, cx));
                            this.submit(cx);
                        } else {
                            this.kind_popup(event.bounds(), window, cx);
                        }
                        return;
                    }
                    text.replace_range(event.range(), "");
                    this.input
                        .update(cx, |input, cx| input.set_value(text, window, cx));
                    this.decorate_filters(window, cx);
                    this.submit(cx);
                });
            });
        let sidebar = tool("sidebar", Glyph::PanelLeft, "Toggle sidebar (Ctrl+B)")
            .text_color(p.muted)
            .selected(false)
            .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)));
        let nav = toolbar_group(p)
            .child(
                tool("back", Glyph::ChevronLeft, "Back")
                    .text_color(p.muted)
                    .disabled(self.history_position == 0)
                    .on_click(cx.listener(|this, _, window, cx| this.history(-1, window, cx))),
            )
            .child(
                tool("forward", Glyph::ChevronRight, "Forward")
                    .text_color(p.muted)
                    .disabled(self.history_position + 1 >= self.history.len())
                    .on_click(cx.listener(|this, _, window, cx| this.history(1, window, cx))),
            );
        let mut switcher = toolbar_group(p);
        for (id, icon, v, label) in [
            ("list-view", Glyph::List, View::List, "List · Ctrl+1"),
            (
                "icons-view",
                Glyph::LayoutGrid,
                View::Icons,
                "Icons · Ctrl+2",
            ),
            (
                "gallery-view",
                Glyph::GalleryHorizontal,
                View::Gallery,
                "Gallery · Ctrl+3",
            ),
            (
                "columns-view",
                Glyph::Columns3,
                View::Columns,
                "Columns · Ctrl+4",
            ),
        ] {
            let active = self.view == v;
            switcher = switcher.child(
                toolbar_state(tool(id, icon, label), active, p, cx)
                    .disabled(self.content_mode && v != View::List)
                    .toggled(active)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_view(v, cx))),
            );
        }
        let panel_visible = self.panel && !self.info_closing && self.selected(cx).is_some();
        let sort_owner = cx.entity().downgrade();
        let sort_open_owner = sort_owner.clone();
        let tools = toolbar_group(p)
            .child(
                toolbar_state(
                    tool(
                        "hidden",
                        if self.show_hidden {
                            Glyph::Eye
                        } else {
                            Glyph::EyeOff
                        },
                        "Show hidden and system files",
                    )
                    .w(px(30.)),
                    self.show_hidden,
                    p,
                    cx,
                )
                .toggled(self.show_hidden)
                .on_click(cx.listener(|this, _, window, cx| this.toggle_hidden(window, cx))),
            )
            .child(
                toolbar_state(
                    tool("sort", Glyph::ArrowDownUp, "Sort results").w(px(30.)),
                    self.sort_open,
                    p,
                    cx,
                )
                .dropdown_menu(move |mut menu, window, _| {
                    menu = super::daily::menu_style(menu, window);
                    for (i, label, direction) in [
                        (0, "Name A–Z", ColumnSort::Ascending),
                        (0, "Name Z–A", ColumnSort::Descending),
                        (1, "Newest first", ColumnSort::Descending),
                        (1, "Oldest first", ColumnSort::Ascending),
                        (2, "Size: largest first", ColumnSort::Descending),
                        (3, "Type", ColumnSort::Ascending),
                    ] {
                        let owner = sort_owner.clone();
                        menu = menu.item(gpui_component::menu::PopupMenuItem::new(label).on_click(
                            move |_, window, cx| {
                                let _ = owner
                                    .update(cx, |this, cx| this.sort_by(i, direction, window, cx));
                            },
                        ));
                    }
                    menu
                })
                .on_open_change(move |open, _, cx| {
                    let _ = sort_open_owner.update(cx, |this, cx| {
                        this.sort_open = *open;
                        cx.notify();
                    });
                }),
            )
            .child(
                toolbar_state(
                    tool("info", Glyph::Info, "Info panel (Ctrl+I)").w(px(30.)),
                    panel_visible,
                    p,
                    cx,
                )
                .toggled(panel_visible)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_info_panel(window, cx);
                })),
            )
            .child(
                toolbar_state(
                    tool("settings", Glyph::Settings, "Settings (Ctrl+,)").w(px(30.)),
                    self.settings,
                    p,
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
            )
            .child(self.daily_controls(p, cx));
        div()
            .h(px(52.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .pl(px(16.))
            .pr(px(12.))
            .gap(px(8.))
            .bg(p.chrome)
            .window_control_area(WindowControlArea::Drag)
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .w(px(160.))
                    .flex_shrink_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(div().occlude().child(sidebar))
                    .child(nav),
            )
            .child(
                div().flex_1().min_w_0().flex().justify_center().child(
                    div()
                        .relative()
                        .occlude()
                        .w_full()
                        .max_w(px(560.))
                        .min_w(px(180.))
                        .h(px(32.))
                        .flex()
                        .items_center()
                        .px_2()
                        .rounded(px(12.))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(p.hover, 0.),
                            linear_color_stop(p.hover.opacity(0.65), 1.),
                        ))
                        .border_1()
                        .border_color(if self.input.read(cx).focus_handle(cx).is_focused(window) {
                            p.muted.opacity(0.55)
                        } else {
                            p.line
                        })
                        .child(search)
                        .child(self.search_suggestions(window, cx)),
                ),
            )
            .child(toolbar_separator(p))
            .child(switcher)
            .child(toolbar_separator(p))
            .child(tools)
            .child(toolbar_separator(p))
            .child(window_controls(p, window))
            .into_any_element()
    }
    fn grid(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let available = self.results_viewport_width(window, cx);
        let source = super::panes::GridSource {
            table: self.table.clone(),
            scroll: self.grid_scroll.clone(),
            view: self.view,
            visible: self.visible.clone(),
            width: available,
            active: true,
            pane: self.pane_identity,
        };
        self.grid_view(source, window, cx)
    }
    /// Icons/Gallery for any pane. Inactive panes pass their own table, scroll
    /// handle and thumbnail set; a press in them focuses that pane first.
    pub(super) fn grid_view(
        &mut self,
        source: super::panes::GridSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let super::panes::GridSource {
            table: source_table,
            scroll: grid_scroll,
            view,
            visible,
            width: available,
            active,
            pane,
        } = source;
        let options = if view == View::Gallery {
            crate::icon_view::IconView::default()
        } else {
            self.icon_view_for(pane)
        };
        let columns = if view == View::Gallery {
            ((available / 140.).floor() as usize).max(3)
        } else {
            options.columns(available)
        };
        // A size change animates each icon from the old size to the new one.
        let resize = self
            .icon_resize
            .filter(|(id, ..)| *id == pane.0 && self.motion_enabled && view == View::Icons)
            .map(|(_, from, generation)| (from as f32, generation));
        if active {
            self.grid_columns = columns;
        }
        let count = source_table
            .read(cx)
            .delegate()
            .rows
            .len()
            .div_ceil(columns);
        let this = cx.entity().downgrade();
        let gallery = view == View::Gallery;
        let list_table = source_table.clone();
        let list_scroll = grid_scroll.clone();
        let visible_height = if gallery {
            155.
        } else {
            f32::from(window.viewport_size().height) - 82.
        };
        let list = uniform_list("icon-grid", count, move |range, _window, cx| {
            let source_table = list_table.clone();
            let grid_scroll = list_scroll.clone();
            let visible = visible.clone();
            this.update(cx, |this, cx| {
                let p = crate::appearance::palette(&this.preferences, cx);
                let table = source_table.read(cx);
                let total = table.delegate().rows.len();
                let start = (range.start * columns).min(total);
                let end = (range.end * columns).min(total);
                let rows = table.delegate().rows[start..end].to_vec();
                let _selected = table.selected_row();
                // UniformList can render visible rows in separate batches.
                // Request the whole viewport on each batch, so a later batch
                // cannot cancel the thumbnails requested by an earlier one.
                let top = scroll_top(&grid_scroll, grid_row_height(view, options));
                let lines = (visible_height / f32::from(grid_row_height(view, options))).ceil() as usize + 2;
                let wanted_start = (top.min(range.start) * columns).min(total);
                let wanted_end = ((top + lines).max(range.end) * columns).min(total);
                let keys = table.delegate().rows[wanted_start..wanted_end]
                    .iter()
                    .filter(|r| needs_shell(r) && options.thumbnails && thumbnail_allowed(r, &this.preferences) && r.file.kind & 3 != seek_engine::walk::KIND_DIR)
                    .map(|r| media::Key::new(r, if !gallery && options.size > 128 { media::PREVIEW_EDGE } else { 128 }, false))
                    .collect();
                *visible.lock().unwrap() = keys;
                request_media(&this.media, &visible, &this.extra, &this.cache);
                range
                    .map(|line| {
                        let mut line_view = div()
                            .h(grid_row_height(view, options))
                            .w_full()
                            .flex()
                            .px_3()
                            .when(!gallery, |d| d.pb(px(options.spacing as f32)))
                            .gap(px(if gallery { 8. } else { options.spacing as f32 }));
                        for (i, row) in rows
                            .iter()
                            .enumerate()
                            .take(((line + 1) * columns).saturating_sub(start).min(rows.len()))
                            .skip((line * columns).saturating_sub(start))
                        {
                            let i = i + start;
                            let drag_paths=if table.delegate().selected_many.contains(&row.file.path){table.delegate().drag_paths.clone()}else{Arc::new(vec![row.file.path.clone()])};
                            let drag=super::daily::FilesDrag{label:if drag_paths.len()>1{format!("{} items",drag_paths.len())}else{row.name.clone()},paths:drag_paths};
                            let context_owner=cx.entity().downgrade();
                            let context_path=row.file.path.clone();
                            let context_folder=row.file.kind & 3 == seek_engine::walk::KIND_DIR;
                            let drop_path=row.file.path.clone();
                            let feedback_path=row.file.path.clone();
                            let chosen = table.delegate().selected_many.contains(&row.file.path);
                            let size = if gallery { 72. } else { options.size as f32 };
                            let right = !gallery && options.labels == crate::icon_view::Labels::Right;
                            let thumbnails = (gallery || options.thumbnails) && thumbnail_allowed(row, &this.preferences);
                            let icon = file_image(row, size, &this.atlas, &this.cache, thumbnails);
                            let icon = match resize {
                                Some((from, generation)) if !gallery => div()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .overflow_hidden()
                                    .size(px(size))
                                    .child(icon)
                                    .with_animation(
                                        ("icon-resize", generation as usize * 100_000 + i),
                                        crate::motion::state(),
                                        move |d, t| {
                                            let now = from + (size - from) * t;
                                            d.size(px(now))
                                        },
                                    )
                                    .into_any_element(),
                                _ => icon,
                            };
                            let mut details = div()
                                .flex()
                                .flex_col()
                                .when(!right, |d| d.items_center())
                                .text_size(px(11.))
                                .line_height(px(15.))
                                .text_color(p.muted);
                            if gallery || options.show_size {
                                details = details.child(
                                    div().flex().items_center().gap_1().children(cloud_badge(row, p)).child(row.size.clone()),
                                );
                            }
                            if !gallery && options.show_kind {
                                details = details.child(div().truncate().child(row.kind.clone()));
                            }
                            if !gallery && options.show_modified {
                                details = details.child(div().truncate().child(row.modified.clone()));
                            }
                            let label = div()
                                .w_full()
                                .when(!right, |d| d.text_center().px_1())
                                .rounded_sm()
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .when(!right, |d| d.bg(if chosen {
                                    p.accent.opacity(0.26)
                                } else {
                                    rgba(0).into()
                                }))
                                .text_color(p.text)
                                .truncate()
                                .child(row.name.clone());
                            let text = div()
                                .min_w_0()
                                .when(right, |d| d.flex_1())
                                .when(!right, |d| d.w_full())
                                .flex()
                                .flex_col()
                                .when(!right, |d| d.items_center())
                                .child(label)
                                .child(details);
                            line_view = line_view.child(
                                div()
                                    .id(("grid-item", i))
                                    .role(Role::Button).aria_label(row.name.clone())
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .flex()
                                    .when(!right, |d| d.flex_col().justify_center())
                                    .items_center()
                                    .gap(px(if right { 10. } else { 4. }))
                                    .p_2()
                                    .rounded(crate::tokens::RADIUS_ROW)
                                    .cursor_pointer()
                                    .bg(if chosen {
                                        p.accent.opacity(0.26)
                                    } else {
                                        rgba(0).into()
                                    })
                                    .hover(move |s| {
                                        s.bg(if chosen {
                                            p.accent.opacity(0.26)
                                        } else {
                                            p.hover
                                        })
                                    })
                                    .child(icon)
                                    .child(text)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            move |this, event: &MouseDownEvent, window, cx| {
                                                this.select_modified(i,event.modifiers.shift,event.modifiers.control, window, cx);
                                                if event.click_count >= 2 {
                                                    this.open_selected(false, window, cx);
                                                }
                                            },
                                        ),
                                    )
                                    .on_mouse_up(MouseButton::Left, cx.listener(move |this, _: &MouseUpEvent, _, cx| this.release_selection(i, cx)))
                                    .on_drag(drag,|drag,offset,_,cx| super::drag_preview::near_cursor(drag.clone(), offset, cx))
                                    .when(row.file.kind&3==seek_engine::walk::KIND_DIR,|tile|tile.drag_over::<super::daily::FilesDrag>(move|style,drag,_,_|if super::daily::drop_allowed(&drag.paths,&feedback_path){style.bg(p.accent.opacity(0.22))}else{style}).on_drop(cx.listener(move|this,drag:&super::daily::FilesDrag,window,cx|this.drop_files(drag,drop_path.clone(),window,cx))))
                                    .context_menu(move|menu,window,cx|super::daily::actions_menu(super::daily::menu_style(menu,window),context_owner.clone(),Some(context_path.clone()),context_folder,cx)),
                            );
                        }
                        let used = ((line + 1) * columns)
                            .min(total)
                            .saturating_sub(line * columns);
                        for _ in used..columns {
                            line_view = line_view.child(div().flex_1());
                        }
                        line_view.into_any_element()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
        })
        .key_context("SeekFiles")
        .track_scroll(&grid_scroll)
        .track_focus(&source_table.read(cx).focus_handle(cx))
        .size_full();
        // Items open their own menu first; the rest of the grid is empty space.
        let background = self.background_state(source_table.read(cx).delegate().sort, cx);
        let background_owner = cx.entity().downgrade();
        let list = div()
            .id("grid-background")
            .size_full()
            .context_menu(move |menu, window, _| {
                super::daily::background_menu(
                    super::daily::menu_style(menu, window),
                    background_owner.clone(),
                    background.clone(),
                )
            })
            .child(list);
        let selected = if active {
            self.selected(cx)
        } else {
            let table = source_table.read(cx);
            table
                .selected_row()
                .and_then(|row| table.delegate().rows.get(row).cloned())
        };
        if gallery && selected.is_some() && self.preferences.gallery_preview {
            let p = crate::appearance::palette(&self.preferences, cx);
            let preview = if let Some(row) = selected {
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(file_image(&row, 290., &self.atlas, &self.cache, true))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(row.name))
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(p.muted)
                    .child("Select a file to preview")
                    .into_any_element()
            };
            let hide = div()
                .id("gallery-hide-preview")
                .role(Role::Button)
                .aria_label("Hide preview")
                .absolute()
                .top(px(10.))
                .right(px(10.))
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(crate::tokens::RADIUS_CONTROL)
                .cursor_pointer()
                .text_color(p.muted)
                .hover(move |s| s.bg(p.hover).text_color(p.text))
                .tooltip(|window, cx| {
                    gpui_component::tooltip::Tooltip::new("Hide preview")
                        .build(window, cx)
                })
                .child(Icon::new(gpui_component_assets::IconName::Close).size(px(14.)))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.preferences.gallery_preview = false;
                    this.save_preferences(window, cx);
                }));
            div()
                .flex()
                .flex_col()
                .size_full()
                .child(div().relative().flex_1().min_h_0().overflow_hidden().child(preview).child(hide))
                .child(
                    div()
                        .h(px(155.))
                        .flex_shrink_0()
                        .border_t_1()
                        .border_color(p.line)
                        .child(list),
                )
                .into_any_element()
        } else if gallery && selected.is_some() {
            // The preview is hidden: the grid takes the space, with a button
            // to bring the preview back.
            let p = crate::appearance::palette(&self.preferences, cx);
            div()
                .relative()
                .size_full()
                .child(list)
                .child(
                    div()
                        .id("gallery-show-preview")
                        .role(Role::Button)
                        .aria_label("Show preview")
                        .absolute()
                        .bottom(px(12.))
                        .right(px(12.))
                        .h(px(28.))
                        .px_3()
                        .flex()
                        .items_center()
                        .gap_1()
                        .rounded_full()
                        .border_1()
                        .border_color(p.line)
                        .bg(p.chrome)
                        .shadow_md()
                        .cursor_pointer()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .hover(move |s| s.text_color(p.text))
                        .child(Icon::new(gpui_component_assets::IconName::Eye).size(px(13.)))
                        .child("Show preview")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.preferences.gallery_preview = true;
                            this.save_preferences(window, cx);
                        })),
                )
                .into_any_element()
        } else {
            list.into_any_element()
        }
    }
    pub(super) fn document_page(&mut self, page: u32, cx: &mut Context<Self>) {
        let Some(mut key) = self.document_key.clone() else {
            return;
        };
        key.page = page;
        if let Some(Ok(old)) = self.document.take()
            && let Some(image) = old.image
        {
            self.document_images_to_drop.push(image);
        }
        self.document_key = Some(key.clone());
        self.documents.request(Some(key.clone()));
        self.watch_preview(Some(key), cx);
        cx.notify();
    }
    pub(super) fn all_page(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.all_offset = offset;
        self.message = Some("Loading all locations...".into());
        self.table.update(cx, |table, cx| {
            table.clear_selection(cx);
            table.scroll_to_row(0, cx);
            let data = table.delegate_mut();
            data.selected_many.clear();
            data.selection = None;
            data.rows = Arc::new(vec![]);
            data.base_rows = Arc::new(vec![]);
            data.empty_message = "Loading all locations...".into();
            table.refresh(cx);
        });
        self.refresh_preview(cx);
        self.submit(cx);
        cx.notify();
    }
    pub(super) fn document_view(
        &self,
        p: crate::appearance::Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let preview = match &self.document {
            Some(Ok(preview)) => preview,
            state => {
                let (label, detail) = match state {
                    Some(Err(e)) => ("Can't preview this file", e.to_string()),
                    _ => (
                        "Loading preview...",
                        "Selected document only; files up to 32 MiB.".into(),
                    ),
                };
                return self.document_loading(label, detail, state.is_some(), p, window, cx);
            }
        };
        if self.preview_enlarged
            && let Some(text) = &preview.info.text
        {
            return div()
                .id("word-reader")
                .w_full()
                .flex_shrink_0()
                .flex()
                .flex_col()
                .gap_5()
                .px_6()
                .py_6()
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child("Document reader"),
                )
                .child(
                    div()
                        .id("word-reader-text")
                        .role(Role::Group)
                        .aria_label(text.clone())
                        .w_full()
                        .text_size(px(16.))
                        .line_height(px(28.))
                        .child(if text.is_empty() {
                            "No readable text in this document.".to_owned()
                        } else {
                            text.clone()
                        }),
                )
                .child(div().text_size(px(11.)).text_color(p.muted).child(
                    "Beginning of document - up to 100 lines / 12,000 characters. Text only.",
                ))
                .into_any_element();
        }
        let mut view = div()
            .id("document-preview")
            .flex()
            .flex_col()
            .gap_2()
            .flex_shrink_0()
            .when(!self.preview_enlarged, |view| {
                view.children(self.document_navigation(p, cx))
            });
        if let Some(image) = &preview.image {
            let height = if self.preview_enlarged {
                (f32::from(window.viewport_size().height) - 180.).max(100.)
            } else {
                270.
            };
            let width = if self.preview_enlarged {
                (f32::from(window.viewport_size().width) - 100.).max(100.)
            } else {
                248.
            };
            view = view.child(
                div()
                    .h(px(height))
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .child(
                        img(image.clone())
                            .w(px(width))
                            .h(px(height))
                            .object_fit(ObjectFit::Contain),
                    ),
            );
        }
        if let Some(text) = &preview.info.text {
            view = view
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child("Beginning of document · text only"),
                )
                .child(
                    div()
                        .id("word-text-preview")
                        .role(Role::Group)
                        .aria_label(text.clone())
                        .max_h(px(if self.preview_enlarged {
                            (f32::from(window.viewport_size().height) - 240.).max(100.)
                        } else {
                            320.
                        }))
                        .overflow_y_scroll()
                        .text_size(px(if self.preview_enlarged { 16. } else { 12. }))
                        .child(text.clone()),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(p.muted)
                        .child("Up to 100 lines / 12,000 characters."),
                );
        }
        if let Some(sheet) = &preview.info.sheet {
            let expanded = self.preview_enlarged;
            view = view.child(
                div()
                    .id("first-sheet-title")
                    .role(Role::Group)
                    .aria_label(format!("First sheet: {}", sheet.name))
                    .text_size(px(if expanded { 16. } else { 12. }))
                    .font_weight(FontWeight::MEDIUM)
                    .child(sheet.name.clone())
                    .tooltip({
                        let detail = format!(
                            "First sheet, data only. {} of {} rows, {} of {} columns.",
                            sheet.rows.len(),
                            sheet.total_rows,
                            sheet.rows.iter().map(Vec::len).max().unwrap_or(0),
                            sheet.total_cols
                        );
                        move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(detail.clone()).build(window, cx)
                        }
                    }),
            );
            view = view.when(expanded, |view| view.w_full().items_center().gap_3());
            let row_label_width = if expanded { 44. } else { 34. };
            let cols = sheet.rows.iter().map(Vec::len).max().unwrap_or(0);
            let widths: Vec<f32> = (0..cols)
                .map(|col| {
                    (sheet
                        .rows
                        .iter()
                        .filter_map(|r| r.get(col))
                        .map(|s| s.chars().count().min(24))
                        .max()
                        .unwrap_or(1) as f32
                        * if expanded { 8. } else { 6.5 }
                        + if expanded { 32. } else { 16. })
                    .clamp(
                        if expanded { 112. } else { 64. },
                        if expanded { 220. } else { 128. },
                    )
                })
                .collect();
            let mut table = div()
                .id("sheet-table")
                .role(Role::Table)
                .aria_label("Sheet data")
                .flex()
                .flex_col()
                .w(px(row_label_width + widths.iter().sum::<f32>()));
            let mut header = div()
                .flex()
                .bg(p.hover)
                .child(div().w(px(row_label_width)).flex_shrink_0());
            for (col, width) in widths.iter().enumerate() {
                let mut n = sheet.start_col as usize + col + 1;
                let mut letters = Vec::new();
                while n > 0 {
                    n -= 1;
                    letters.push((b'A' + (n % 26) as u8) as char);
                    n /= 26;
                }
                header = header.child(
                    div()
                        .w(px(*width))
                        .flex_shrink_0()
                        .px(px(if expanded { 16. } else { 8. }))
                        .py(px(if expanded { 10. } else { 4. }))
                        .text_size(px(if expanded { 13. } else { 11. }))
                        .line_height(px(if expanded { 20. } else { 16. }))
                        .child(letters.iter().rev().collect::<String>()),
                );
            }
            table = table.child(header);
            for (index, row) in sheet.rows.iter().enumerate() {
                let mut line = div()
                    .id(format!("sheet-row-{index}"))
                    .role(Role::Row)
                    .flex()
                    .border_b_1()
                    .border_color(p.line)
                    .text_size(px(if expanded { 13. } else { 11. }))
                    .line_height(px(if expanded { 20. } else { 16. }))
                    .child(
                        div()
                            .w(px(row_label_width))
                            .flex_shrink_0()
                            .px_1()
                            .py(px(if expanded { 10. } else { 4. }))
                            .text_color(p.muted)
                            .child((sheet.start_row as usize + index + 1).to_string()),
                    );
                for (col, cell) in row.iter().enumerate() {
                    line = line.child(
                        div()
                            .id(format!("sheet-cell-{index}-{col}"))
                            .role(Role::Cell)
                            .aria_label(cell.clone())
                            .w(px(widths[col]))
                            .flex_shrink_0()
                            .px(px(if expanded { 16. } else { 8. }))
                            .py(px(if expanded { 10. } else { 4. }))
                            .truncate()
                            .child(cell.clone()),
                    );
                }
                table = table.child(line);
            }
            view = view.child(
                div()
                    .id("sheet-scroll")
                    .w(px(row_label_width + 2. + widths.iter().sum::<f32>()))
                    .max_w(relative(1.))
                    .flex_shrink_0()
                    .max_h(px(if expanded {
                        (f32::from(window.viewport_size().height) - 220.).max(160.)
                    } else {
                        280.
                    }))
                    .overflow_scroll()
                    .border_1()
                    .border_color(p.line)
                    .rounded(px(6.))
                    .child(table),
            );
            if sheet.rows.is_empty() {
                view = view.child(
                    div()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child("This sheet has no data."),
                );
            }
        }
        view.into_any_element()
    }
    pub(super) fn toggle_info_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let opening = !self.panel || self.info_closing;
        self.info_close_task = Task::ready(());
        self.info_from = f32::from_bits(self.info_progress.load(Ordering::Relaxed));
        self.info_motion = self.info_motion.wrapping_add(1);
        self.info_closing = !opening;
        if !self.motion_enabled {
            self.panel = opening;
            self.info_closing = false;
            self.info_progress.store(
                (if opening { 1f32 } else { 0f32 }).to_bits(),
                Ordering::Relaxed,
            );
        } else if opening {
            self.panel = true;
        } else {
            let generation = self.info_motion;
            self.info_close_task = cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(crate::motion::EXIT).await;
                let _ = this.update_in(cx, |this, _, cx| {
                    if this.info_motion != generation || !this.info_closing {
                        return;
                    }
                    this.panel = false;
                    this.info_closing = false;
                    this.info_progress.store(0f32.to_bits(), Ordering::Relaxed);
                    this.refresh_preview(cx);
                    cx.notify();
                });
            });
        }
        self.refresh_preview(cx);
        cx.notify();
    }
    fn info_panel(
        &mut self,
        p: crate::appearance::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let width = self.info_width(window);
        let mut panel = div()
            .id("info-panel")
            .role(Role::Region)
            .aria_label("Information panel")
            .w_full()
            .flex_shrink_0()
            .h_full()
            .bg(p.chrome)
            .border_l_1()
            .border_color(p.line)
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .overflow_y_scroll();
        let selection = self.inspector_selection(cx);
        if selection.len() > 1 {
            panel = panel.child(self.inspector_totals(p, cx));
        } else if let Some(row) = self.selected(cx) {
            // Quick Look, the document pane and the other file actions live
            // in one icon row at the foot of the panel.
            let details = match self.cache.lock().unwrap().get(&media::Key::info(
                &row,
                self.preferences.automatic_previews || self.preview_enlarged,
            )) {
                Some(media::Value::Details(d)) => Some(d),
                _ => None,
            };
            panel = panel
                .when(
                    self.preferences.automatic_previews
                        && (self.view != View::Gallery
                            || !self.preferences.gallery_preview
                            || crate::document_preview::Key::new(&row, 0).is_some()),
                    |panel| {
                        if super::text_reader::text_file(&row) {
                            panel.child(self.compact_text_preview(&row, p, window, cx))
                        } else if crate::document_preview::Key::new(&row, 0).is_some() {
                            panel.child(self.document_view(p, window, cx))
                        } else {
                            panel.child(
                                div()
                                    .h(px(210.))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(file_image(&row, 200., &self.atlas, &self.cache, true)),
                            )
                        }
                    },
                )
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(row.name.clone()),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child(format!("{} · {}", row.kind, row.size)),
                )
                .when(self.content_mode && row.snippet.is_some(), |panel| {
                    let mut matches = div().flex().flex_col().gap_2().pt_3().child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Search matches"),
                    );
                    let snippet = row.snippet.as_deref().unwrap_or_default();
                    let snippet = if self.preferences.hide_env_secrets
                        && crate::env_secrets::is_env_file(&row.file.path)
                    {
                        std::borrow::Cow::Owned(crate::env_secrets::mask_snippet(snippet))
                    } else {
                        std::borrow::Cow::Borrowed(snippet)
                    };
                    for line in snippet.lines() {
                        matches = matches.child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(p.muted)
                                .child(line.to_owned()),
                        );
                    }
                    panel.child(matches)
                })
                .child(
                    div()
                        .pt_3()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Details"),
                );
            for (label, value) in [
                (
                    "Size",
                    details.as_ref().map(|data| {
                        data.bytes
                            .map(|bytes| format!("{bytes} bytes"))
                            .unwrap_or_else(|| {
                                if row.file.kind & 3 == seek_engine::walk::KIND_DIR {
                                    "See selection totals".into()
                                } else {
                                    "Unavailable".into()
                                }
                            })
                    }),
                ),
                (
                    "Size on disk",
                    details.as_ref().map(|data| {
                        data.size_on_disk
                            .map(|bytes| format!("{bytes} bytes"))
                            .unwrap_or_else(|| {
                                if row.file.kind & 3 == seek_engine::walk::KIND_DIR {
                                    "See selection totals".into()
                                } else {
                                    "Unavailable".into()
                                }
                            })
                    }),
                ),
            ] {
                panel = panel.child(
                    div()
                        .flex()
                        .gap_3()
                        .justify_between()
                        .text_size(px(11.))
                        .child(div().text_color(p.muted).child(label))
                        .child(
                            div()
                                .flex_1()
                                .text_right()
                                .child(value.unwrap_or_else(|| "Loading...".into())),
                        ),
                );
            }
            panel = panel.child(
                div()
                    .pt_3()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Dates"),
            );
            for (label, value) in [
                ("Created", details.as_ref().map(|d| d.created.clone())),
                ("Modified", details.as_ref().map(|d| d.modified.clone())),
                ("Last opened", details.as_ref().map(|d| d.opened.clone())),
            ] {
                panel = panel.child(
                    div()
                        .flex()
                        .gap_3()
                        .justify_between()
                        .text_size(px(11.))
                        .child(div().text_color(p.muted).child(label))
                        .child(
                            div()
                                .flex_1()
                                .text_right()
                                .child(value.unwrap_or_else(|| "Loading…".into())),
                        ),
                );
            }
            panel = panel
                .child(
                    div()
                        .pt_3()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Storage"),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child(row.file.path.display().to_string()),
                );
            if row.file.kind & 3 == seek_engine::walk::KIND_DIR {
                panel = panel.child(self.inspector_totals(p, cx));
            }
            if let Some(d) = details {
                panel = panel
                    .child(div().text_size(px(11.)).child(format!(
                        "Volume: {}",
                        d.volume.unwrap_or_else(|| "Unavailable".into())
                    )))
                    .child(div().text_size(px(11.)).child(format!(
                        "Owner: {}",
                        d.owner.unwrap_or_else(|| "Unavailable".into())
                    )));
                if let Some(attributes) = d.attributes {
                    panel = panel.child(
                        div()
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .child(format!("Attributes: {attributes}")),
                    );
                }
                if let Some(text) = d.text
                    && !super::text_reader::text_file(&row)
                {
                    panel = panel
                        .child(
                            div()
                                .pt_3()
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Preview"),
                        )
                        .child(
                            div()
                                .font_family(crate::tokens::FONT_MONO)
                                .text_size(px(11.))
                                .child(text),
                        );
                }
                if let Some(error) = d.error {
                    panel = panel.child(div().text_color(p.muted).child(error));
                }
            }
        } else {
            panel = panel.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .text_color(p.muted)
                    .child(Icon::new(Glyph::Info).size(px(32.)))
                    .child("Select an item")
                    .child(div().text_xs().child("Ctrl+I toggles this panel")),
            );
        }
        if let Some(tools) = self.image_tools_menu(p, cx) {
            panel = panel.child(div().pt_3().mt_2().border_t_1().border_color(p.line).child(tools));
        }
        if !selection.is_empty() {
            // One row of icon actions; Open is a click away on the row itself.
            use super::daily::Action;
            let mut actions = div()
                .flex()
                .items_center()
                .justify_between()
                .pt_3()
                .mt_2()
                .border_t_1()
                .border_color(p.line);
            for (id, icon, label, action) in [
                (
                    "inspector-quick-look",
                    "icons/eye.svg",
                    "Quick Look",
                    Action::QuickLook,
                ),
                (
                    "inspector-document-pane",
                    "icons/panel-right.svg",
                    "Open in document pane",
                    Action::DocumentPane,
                ),
                (
                    "inspector-open-with",
                    "icons/external-link.svg",
                    "Open with…",
                    Action::OpenWith,
                ),
                (
                    "inspector-rename",
                    "icons/pencil.svg",
                    "Rename",
                    Action::Rename,
                ),
                (
                    "inspector-copy-path",
                    "icons/copy.svg",
                    "Copy path",
                    Action::Path,
                ),
                (
                    "inspector-reveal",
                    "icons/folder-open.svg",
                    "Show in Explorer",
                    Action::Reveal,
                ),
                ("inspector-share", "icons/share.svg", "Share", Action::Share),
            ] {
                actions = actions.child(
                    Button::new(id)
                        .ghost()
                        .h(crate::tokens::BUTTON_HEIGHT)
                        .w(crate::tokens::BUTTON_HEIGHT)
                        .p_0()
                        .rounded(crate::tokens::RADIUS_CONTROL)
                        .accessibility_label(label)
                        .tooltip(label)
                        .child(Icon::empty().path(icon).size(px(15.)).text_color(p.muted))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.file_action(action, None, window, cx);
                        })),
                );
            }
            panel = panel.child(actions);
        }
        let view = div()
            .relative()
            .w(px(width))
            .h_full()
            .flex_shrink_0()
            .child(panel)
            .child(self.info_resize_handle(width));
        if self.info_motion == 0 || !self.motion_enabled {
            view.into_any_element()
        } else {
            let from = self.info_from;
            let to = if self.info_closing { 0. } else { 1. };
            let progress = self.info_progress.clone();
            view.with_animation(
                ("info-panel-motion", self.info_motion),
                if self.info_closing {
                    crate::motion::exit()
                } else {
                    crate::motion::enter()
                },
                move |view, t| {
                    let shown = from + (to - from) * t;
                    progress.store(shown.to_bits(), Ordering::Relaxed);
                    view.left(px(width * (1. - shown))).opacity(shown)
                },
            )
            .into_any_element()
        }
    }
}
impl Render for Seek {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _profile = crate::metrics::span("shell_render");
        // Restore/programmatic selection changes must also request a preview.
        self.sync_document_preview(cx);
        for image in self.document_images_to_drop.drain(..) {
            let _ = window.drop_image(image);
        }
        let _ = &self.subscriptions;
        self.sync_live_folders(cx);
        // Shortcuts (Ctrl+B, Ctrl+W…) reach Seek only through the focused
        // element. After a click on something that takes no focus, or when a
        // focused view closes (Settings, a dialog), nothing holds focus and
        // keys go nowhere; give it back to the file list (or Settings).
        // On Storage and Worktrees the file list isn't shown, so focus left on
        // it reaches nothing either; the page takes it (never from a field).
        let page = self.storage_page.is_some() || self.worktrees_page.is_some();
        if page
            && !self.settings
            && window.is_window_active()
            && self.table.read(cx).focus_handle(cx).is_focused(window)
        {
            let page_focus = self.page_focus.clone();
            window.defer(cx, move |window, cx| page_focus.focus(window, cx));
        }
        if window.focused(cx).is_none() && window.is_window_active() {
            let fallback = if self.settings {
                self.settings_focus.clone()
            } else if self.storage_page.is_some() || self.worktrees_page.is_some() {
                self.page_focus.clone()
            } else {
                self.table.read(cx).focus_handle(cx)
            };
            window.defer(cx, move |window, cx| {
                if window.focused(cx).is_none() {
                    fallback.focus(window, cx);
                }
            });
        }
        let p = crate::appearance::palette(&self.preferences, cx);
        // Fill the results viewport rather than leaving a fixed-width table at its left.
        // Only recalculate when the viewport or column set changes, preserving manual resizing.
        let results_width = (self.results_viewport_width(window, cx) - 16.)
            .max(0.)
            .round() as u32;
        super::panes::fit_columns(&self.table, results_width, cx);
        let count = self.table.read(cx).delegate().rows.len();
        let selected = self.selected(cx);
        let selected_count = self
            .table
            .read(cx)
            .delegate()
            .selected_many
            .len()
            .max(usize::from(selected.is_some()));
        let state = self.status.error.clone().unwrap_or_else(|| {
            if !self.status.ready {
                if self.status.progress.completed == 0 {
                    format!(
                        "Building index · {}",
                        if self.status.progress.phase.is_empty() {
                            "Preparing"
                        } else {
                            &self.status.progress.phase
                        }
                    )
                } else {
                    format!(
                        "Building index · {} folders read",
                        self.status.progress.completed
                    )
                }
            } else if self.status.catching_up {
                if let Some(total) = self.status.progress.total.filter(|n| *n > 0) {
                    format!(
                        "Catching up {}% · {}",
                        (self.status.progress.completed * 100 / total).min(99),
                        self.status.progress.phase
                    )
                } else if self.status.journal_directories > 0 {
                    format!(
                        "Catching up · {} changed folders",
                        self.status.journal_directories
                    )
                } else {
                    format!(
                        "Catching up · {}",
                        if self.status.progress.phase.is_empty() {
                            "Applying changes"
                        } else {
                            &self.status.progress.phase
                        }
                    )
                }
            } else {
                "Up to date".into()
            }
        });
        let mut breadcrumb = div()
            .h(px(30.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .px_3()
            .gap_1()
            .text_size(px(11.))
            .line_height(px(14.))
            .overflow_hidden();
        let crumb_path = selected
            .as_ref()
            .map(|r| r.file.path.clone())
            .or_else(|| self.scope.clone());
        if let Some(crumb) = crumb_path {
            // Display DOS components instead of the canonical \\?\ prefix;
            // Shell paths keep their full identity in the captured target.
            let crumb = seek_engine::paths::native(&seek_engine::paths::internal(&crumb));
            let mut path = PathBuf::new();
            // Split panes are narrow: keep the nearest folders, which identify
            // the location, and mark the elided ancestors.
            let named = crumb
                .components()
                .filter(|c| !matches!(c, std::path::Component::RootDir))
                .count();
            let hidden = if self.split_mode() && !self.zoomed {
                named.saturating_sub(2)
            } else {
                0
            };
            if hidden > 0 {
                breadcrumb = breadcrumb.child(div().text_color(p.muted).child("…"));
            }
            let mut shown = 0;
            let last = crumb.components().count().saturating_sub(1);
            for (i, component) in crumb.components().enumerate() {
                path.push(component);
                if matches!(component, std::path::Component::RootDir) {
                    continue;
                }
                shown += 1;
                if shown <= hidden {
                    continue;
                }
                let target = path.clone();
                // Too long for the bar: folders give way first, each ending
                // in an ellipsis, then the item's own name; nothing is cut
                // off at the edge.
                let item = i == last;
                breadcrumb = breadcrumb
                    .child(
                        Icon::new(Glyph::ChevronRight)
                            .size(px(11.))
                            .text_color(p.muted)
                            .flex_shrink_0(),
                    )
                    .child(
                        div()
                            .id(("path", i))
                            .flex()
                            .items_center()
                            .h(px(22.))
                            .min_w(px(if item { 40. } else { 24. }))
                            .when(!item, |d| d.flex_shrink(1.))
                            .when(item, |d| d.flex_shrink_0().max_w_full())
                            .gap_1()
                            .cursor_pointer()
                            .child(
                                Icon::new(if i == 0 {
                                    Glyph::HardDrive
                                } else {
                                    Glyph::Folder
                                })
                                .size(px(12.))
                                .text_color(p.muted)
                                .flex_shrink_0(),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .h(px(14.))
                                    .line_height(px(14.))
                                    .child(component.as_os_str().to_string_lossy().into_owned()),
                            )
                            .on_click(cx.listener(move |this, _, _, _| {
                                let _ = this.backend.tx.send(Command::Open(target.clone(), true));
                            })),
                    );
            }
        }
        let status = div()
            .h(px(30.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .px_3()
            .gap_3()
            .text_size(px(11.))
            .text_color(p.muted)
            .child(format!(
                "{count} items, {selected_count} selected{}",
                if self.all_total.is_none() && !self.content_mode && count == 200 && (self.largest.is_some() || self.scope.is_none() || !self.input.read(cx).value().trim().is_empty()) {
                    " · showing top 200"
                } else {
                    ""
                }
            ))
            .when(self.largest.is_some(), |bar| bar.child(
                Button::new("normal-results").ghost().small().accessibility_label("All items").child(button_text("All items"))
                    .on_click(cx.listener(|this, _, window, cx| this.leave_largest(window, cx)))
            ))
            .when(self.largest.is_some(),|bar|bar.child(div().id("largest-rounding").child("≈ Indexed sizes").text_size(px(10.)).tooltip(|window,cx|gpui_component::tooltip::Tooltip::new("Files below 2 GB have exact indexed sizes. Larger files are rounded down by less than 2 MiB each. Folder totals sum those indexed sizes; they are logical sizes, not allocated disk space.").build(window,cx))))
            .child(div().flex_1())
            .child(
                div()
                    .max_w(px(if self.split_mode() && !self.zoomed { 110. } else { 210. }))
                    .truncate()
                    .child({
                        // Only what is worth reading: no "Folder contents" or "Up to date".
                        let text = if self.content_mode { state.clone() } else {
                            self.message.as_deref().filter(|m| *m != "Folder contents").unwrap_or(&state).to_owned()
                        };
                        if text == "Up to date" { String::new() } else { text }
                    }),
            )
            .when(self.status.skipped != 0, |bar| bar.child(
                div().id("access-status").child(format!("{} inaccessible locations", self.status.skipped))
                    .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("Unique locations Windows could not list, not a file count. May include denied access, unavailable folders or failed enumeration. Accessible files are indexed, including hidden and system files.").build(window, cx))
            ));
        let contents = if self.view == View::Columns {
            let source = super::columns::Source {
                pane: self.pane_identity,
                table: self.table.clone(),
                scroll: self.grid_scroll.clone(),
                scope: self.scope.clone(),
                folder_view: self.scope.is_some()
                    && self.input.read(cx).value().trim().is_empty()
                    && !self.content_mode
                    && self.largest.is_none(),
                widths: self.column_widths.clone(),
                width: self.results_viewport_width(window, cx),
                active: true,
            };
            self.columns_view(source, p, window, cx)
        } else if self.table.read(cx).delegate().rows.is_empty() {
            let text = self.table.read(cx).delegate().empty_message.clone();
            let state = self.background_state(self.table.read(cx).delegate().sort, cx);
            let owner = cx.entity().downgrade();
            div()
                .id("folder-empty-background")
                .context_menu(move |menu, window, _| {
                    super::daily::background_menu(
                        super::daily::menu_style(menu, window),
                        owner.clone(),
                        state.clone(),
                    )
                })
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .id("folder-empty-state")
                        .role(Role::Group)
                        .aria_label(text.clone())
                        .text_color(p.muted)
                        .child(text),
                )
                .into_any_element()
        } else if self.view == View::List {
            div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .relative()
                .overflow_hidden()
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .child(if window.is_a11y_active() {
                            DataTable::new(&self.table)
                                .focus_indicator(false)
                                .stripe(false)
                                .bordered(false)
                                .small()
                                .into_any_element()
                        } else {
                            self.result_pane
                                .clone()
                                .cached(StyleRefinement::default().size_full())
                                .into_any_element()
                        }),
                )
                .into_any_element()
        } else {
            self.grid(window, cx)
        };
        let mut results = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .w_full()
            .overflow_hidden()
            .flex();
        let split = self.split_mode();
        let focused_pane = div()
            .id("results-scroll-fallback")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                if this.settings || this.edit.is_some() || this.permanent_delete.is_some() {
                    return;
                }
                // A bubbled, otherwise unhandled wheel event must not require
                // keyboard focus or an extra click to reach the result list.
                let handle = if this.view == View::List {
                    this.table.read(cx).vertical_scroll_handle.clone()
                } else {
                    this.grid_scroll.clone()
                };
                let delta = event.delta.pixel_delta(px(20.));
                let mut state = handle.0.borrow_mut();
                let before = state.base_handle.offset();
                let max = state.base_handle.max_offset();
                let next = point(
                    (before.x + delta.x).clamp(-max.x, px(0.)),
                    (before.y + delta.y).clamp(-max.y, px(0.)),
                );
                if next != before {
                    state.deferred_scroll_to_item = None;
                    state.base_handle.set_offset(next);
                    drop(state);
                    this.table.update(cx, |_, cx| cx.notify());
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .child(contents)
            .children(self.all_pagination(p, cx))
            .child(
                div()
                    .h(px(30.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .rounded_b(crate::tokens::RADIUS_CARD - px(1.))
                    .bg(p.chrome)
                    .border_t_1()
                    .border_color(p.line)
                    .child(breadcrumb.flex_1().min_w_0())
                    .child(status.flex_shrink_0()),
            );
        let focused_pane = if split {
            focused_pane
        } else {
            let pane = self.pane_identity;
            let overlay = self.split_overlay(pane, false, p);
            let target = focused_pane.relative().children(overlay);
            self.split_target(target, pane, false, cx)
        };
        results = if split {
            let focused_pane = div()
                .flex()
                .flex_col()
                .size_full()
                .min_w_0()
                .min_h_0()
                .child(self.tab_strip(p, cx))
                .children(self.related_row(p, cx))
                .child(focused_pane)
                .into_any_element();
            results.child(self.pane_area(focused_pane, p, window, cx))
        } else {
            results.child(focused_pane)
        };
        if self.panel && selected.is_some() {
            results = results.child(self.info_panel(p, window, cx));
        }
        let main = div()
            .flex_1()
            .bg(p.pane)
            .min_w_0()
            .min_h_0()
            .h_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .when(self.content_mode && crate::location::address(self.input.read(cx).value().as_str()).is_none(), |d| {
                let paths=&self.preferences.content_locations;
                let names=paths.iter().filter_map(|p|p.file_name()).map(|n|n.to_string_lossy()).collect::<Vec<_>>();
                let locations=if names.len()<=2 {names.join(" and ")} else {format!("{} locations",names.len())};
                let detail=self.message.clone().unwrap_or_else(||"Type to search contents".into());
                let tooltip=format!("{}\n4 MiB per file, 8 MiB expanded document text, Newest files first; continues until finished, cancelled or edited. Protected files, links, cloud-only files and unsupported types are skipped. Press F5 to rescan. Skipped and unreadable files are counted; PDF extraction is limited to 1 second per file.",paths.iter().map(|p|p.to_string_lossy()).collect::<Vec<_>>().join("; "));
                d.child(div().id("content-status").role(Role::Group).aria_label(detail.clone()).flex_shrink_0().h(px(30.)).px_3().flex().items_center().gap_3().text_size(px(11.)).text_color(p.muted)
                    .tooltip(move|window,cx|gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window,cx))
                    .child(div().flex_1().min_w_0().truncate().child(if locations.is_empty(){"No content locations".into()}else{format!("Search in {locations}")}))
                    .child(div().min_w_0().truncate().child(detail))
                    .when(!self.content_failures.is_empty(), |d| d.child(Button::new("content-unreadable-files").accessibility_label(format!("Unreadable files ({})",self.content_failures.len())).child(button_text(format!("Unreadable files ({})",self.content_failures.len()))).small().on_click(cx.listener(|this,_,window,cx|this.show_content_failures(window,cx)))))
                    .child(Button::new("cancel-content").accessibility_label("Cancel").child(button_text("Cancel")).small().on_click(cx.listener(|this,_,_,cx| {this.pending_tab_restore=None;let cancel_generation=this.backend.generation.fetch_add(1,Ordering::Relaxed)+1;let _=this.backend.tx.send(Command::CancelContent(cancel_generation));this.message=Some("Cancelling content search...".into());cx.notify();}))))
            })
            .child(results);
        let mut body = div()
            .flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden();
        let sidebar = div()
            .h_full()
            .flex_shrink_0()
            .overflow_hidden()
            .w(px(if self.sidebar { 220. } else { 0. }))
            .child(self.sidebar_view(p, cx));
        let sidebar = if self.sidebar_motion == 0 || !self.motion_enabled {
            sidebar.into_any_element()
        } else {
            let from = self.sidebar_from;
            let to = if self.sidebar { 220. } else { 0. };
            let width = self.sidebar_width.clone();
            sidebar
                .with_animation(
                    ("sidebar-motion", self.sidebar_motion as usize),
                    crate::motion::reversible(),
                    move |panel, t| {
                        let value = from + (to - from) * t;
                        width.store(value.to_bits(), Ordering::Relaxed);
                        panel.w(px(value))
                    },
                )
                .into_any_element()
        };
        // The Storage page takes the file list's place while it is open.
        let main = if self.storage_page.is_some() {
            self.storage_view(p, cx)
        } else if self.worktrees_page.is_some() {
            self.worktrees_view(p, cx)
        } else {
            main.into_any_element()
        };
        body = body.child(sidebar).child(main);
        let glass_layers = self.glass_layers(p, window, cx);
        let mut view =
            div()
                .id("seek")
                .relative()
                .size_full()
                .flex()
                .flex_col()
                .bg(p.canvas)
                .children(glass_layers)
                .font_family(crate::tokens::FONT_UI)
                .text_size(px(13.))
                .text_color(p.text)
                .capture_action(cx.listener(|this, _: &NewTab, window, cx| {
                    if this.tabs_available(window,cx) { this.new_tab(None,window,cx); } cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &CloseTab, window, cx| {
                    // A focused document pane closes itself, not the file
                    // pane's tab behind it.
                    if let Some(doc) = this.focused_document(window, cx) {
                        this.close_document_pane(doc, cx);
                    } else if this.tabs_available(window,cx) { this.close_tab(this.active_tab,window,cx); }
                    cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &SplitRight, window, cx| {
                    this.split_pane(crate::pane_layout::Axis::Horizontal, None, window, cx); cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &SplitDown, window, cx| {
                    this.split_pane(crate::pane_layout::Axis::Vertical, None, window, cx); cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &ClosePane, window, cx| {
                    this.close_pane(window, cx); cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &NextPane, window, cx| {
                    this.focus_adjacent_pane(false, window, cx); cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &PreviousPane, window, cx| {
                    this.focus_adjacent_pane(true, window, cx); cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &CopyToOtherPane, window, cx| {
                    this.send_to_other_pane(false, window, cx); cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &MoveToOtherPane, window, cx| {
                    this.send_to_other_pane(true, window, cx); cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &NextTab, window, cx| {
                    if this.tabs_available(window,cx) { this.switch_tab(super::tabs::adjacent(this.active_tab,this.tabs.len(),false),window,cx); } cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &PreviousTab, window, cx| {
                    if this.tabs_available(window,cx) { this.switch_tab(super::tabs::adjacent(this.active_tab,this.tabs.len(),true),window,cx); } cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this,_:&gpui_base::actions::Cancel,window,cx|{
                    if this.escape_action(window,cx){cx.stop_propagation();}else{cx.propagate();}
                }))
                .capture_action(cx.listener(|this,_:&gpui_component::input::Escape,window,cx|{
                    if this.escape_action(window,cx){cx.stop_propagation();}else{cx.propagate();}
                }))
                .capture_action(cx.listener(|this,_:&PanImageLeft,_,cx|{if this.quick_look_pan_key("left",cx){cx.stop_propagation();}}))
                .capture_action(cx.listener(|this,_:&PanImageRight,_,cx|{if this.quick_look_pan_key("right",cx){cx.stop_propagation();}}))
                .capture_action(cx.listener(|this,_:&PanImageUp,_,cx|{if this.quick_look_pan_key("up",cx){cx.stop_propagation();}}))
                .capture_action(cx.listener(|this,_:&PanImageDown,_,cx|{if this.quick_look_pan_key("down",cx){cx.stop_propagation();}}))
                .capture_action(cx.listener(|this, _: &ExpandFolder, window, cx| {
                    if this.view == View::Columns {
                        this.column_key("right", window, cx);
                        cx.stop_propagation();
                        return;
                    }
                    if let Some(row) = this.selected(cx)
                        && row.file.kind & 3 == seek_engine::walk::KIND_DIR
                    {
                        this.expand_folder(row.file.path, true, cx);
                    }
                    cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &CollapseFolder, window, cx| {
                    if this.view == View::Columns {
                        this.column_key("left", window, cx);
                        cx.stop_propagation();
                        return;
                    }
                    if let Some(row) = this.selected(cx)
                        && row.file.kind & 3 == seek_engine::walk::KIND_DIR
                    {
                        this.expand_folder(row.file.path, false, cx);
                    }
                    cx.stop_propagation();
                }))
                .on_drag_move(cx.listener(|this, event: &DragMoveEvent<super::daily::FilesDrag>, window, cx| {
                    let position = event.event.position;
                    let size = window.viewport_size();
                    if position.x >= px(0.) && position.y >= px(0.) && position.x < size.width && position.y < size.height {
                        return;
                    }
                    let paths = event.drag(cx).paths.clone();
                    if !cx.stop_active_drag(window) { return; }
                    this.drag_hover = None; this.drag_hover_task = Task::ready(());
                    // OLE must own capture on the source window's thread. Run after
                    // this event dispatch, so no Seek entity remains borrowed.
                    unsafe { let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture(); }
                    let owner = cx.entity().downgrade();
                    cx.defer(move |cx| {
                        let result = crate::ole_drag::start(paths);
                        let _ = owner.update(cx, |this, cx| {
                            if let Err(error) = result { this.message = Some(format!("Could not drag files: {error}")); }
                            cx.notify();
                        });
                    });
                    cx.stop_propagation();
                }))
                .on_drag_move(cx.listener(Self::resize_info))
                .on_mouse_up(MouseButton::Left, cx.listener(|this, event, window, cx| {
                    if std::mem::take(&mut this.layout_dirty) { this.remember_session(cx); }
                    if this.tab_drop.take().is_some() | this.clear_split_preview() | this.clear_sidebar_drop() { cx.notify(); }
                    this.drag_hover = None; this.drag_hover_task = Task::ready(());
                    this.finish_info_resize(event, window, cx);
                }))
                .on_mouse_up_out(MouseButton::Left, cx.listener(|this, event, window, cx| {
                    if std::mem::take(&mut this.layout_dirty) { this.remember_session(cx); }
                    if this.tab_drop.take().is_some() | this.clear_split_preview() | this.clear_sidebar_drop() { cx.notify(); }
                    this.drag_hover = None; this.drag_hover_task = Task::ready(());
                    this.finish_info_resize(event, window, cx);
                }))
                .capture_key_down(cx.listener(Self::keys))
                .on_key_down(cx.listener(Self::button_keys))
                .capture_action(
                    cx.listener(|this, _: &CopyFiles, window, cx| {
                        #[cfg(feature="benchmarks")]
                        crate::metrics::interaction("copy_route",serde_json::json!({"input":this.input.read(cx).focus_handle(cx).is_focused(window),"list":this.table.read(cx).focus_handle(cx).is_focused(window)}));
                        if !this.settings
                            && this.edit.is_none()
                            && this
                                .table
                                .read(cx)
                                .focus_handle(cx)
                                .is_focused(window)
                        {
                            this.file_action(super::daily::Action::Copy, None, window, cx);
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    }),
                )
                .capture_action(
                    cx.listener(|this, _: &CutFiles, window, cx| {
                        if !this.settings
                            && this.edit.is_none()
                            && this
                                .table
                                .read(cx)
                                .focus_handle(cx)
                                .is_focused(window)
                        {
                            this.file_action(super::daily::Action::Cut, None, window, cx);
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    }),
                )
                .capture_action(cx.listener(
                    |this, _: &PasteFiles, window, cx| {
                        if !this.settings
                            && this.edit.is_none()
                            && this
                                .table
                                .read(cx)
                                .focus_handle(cx)
                                .is_focused(window)
                        {
                            this.file_action(super::daily::Action::Paste, None, window, cx);
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    },
                ))
                .capture_action(cx.listener(|this, _: &CopyPath, window, cx| {
                    this.file_action(super::daily::Action::Path, None, window, cx);
                    cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &ShowInFolder, window, cx| {
                    this.file_action(super::daily::Action::ShowInFolder, None, window, cx);
                    cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &UndoFile, window, cx| {
                    this.file_action(super::daily::Action::UndoRename,None,window,cx);cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &RedoFile, window, cx| {
                    this.file_action(super::daily::Action::RedoRename,None,window,cx);cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &gpui_component::input::Paste, window, cx| {
                    if this.paste_address(window, cx) {
                        cx.stop_propagation();
                    }
                }))
                .capture_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                    this.toggle_sidebar(cx);
                    cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &NewFolder, window, cx| {
                    if this.scope.is_some() {
                        this.file_action(super::daily::Action::NewFolder, None, window, cx);
                        cx.stop_propagation();
                    }
                }))
                .capture_action(cx.listener(|this, _: &RenameFile, window, cx| {
                    this.file_action(super::daily::Action::Rename, None, window, cx);
                    cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &RecycleFiles, window, cx| {
                    if gpui_component::WindowExt::has_active_dialog(window,cx) {return;}
                    this.file_action(super::daily::Action::Recycle, None, window, cx);
                    cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &DeletePermanently, window, cx| {
                    this.file_action(super::daily::Action::DeletePermanently, None, window, cx);
                    cx.stop_propagation();
                }))
                .when(!self.preview_enlarged && !self.settings, |view| view
                    .child(self.toolbar(p, window, cx))
                    .when(!split, |view| {
                        view.child(self.tab_strip(p, cx))
                            .children(self.related_row(p, cx))
                    })
                    .child(body));
        if self.settings {
            view = view
                .child(
                    div()
                        .h(px(52.))
                        .w_full()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .px_4()
                        .gap_3()
                        .window_control_area(WindowControlArea::Drag)
                        .bg(p.sheet)
                        .border_b_1()
                        .border_color(p.line)
                        .child(div().flex_1().text_color(p.muted).child("Seek / Settings"))
                        .child(window_controls(p, window)),
                )
                .child(self.settings_sheet(p, cx));
        }
        if self.preview_enlarged {
            view = view.child(self.enlarged_preview(p, window, cx));
        }
        let view = view.child(self.daily_overlays(window, cx));
        let view = view.child(self.command_result_view(p, cx));
        let view = if self.history_open {
            view.child(self.operation_history_view(p, cx))
        } else {
            view
        };
        let view = if self.image_job.is_some() {
            view.child(self.image_tool_view(p, cx))
        } else {
            view
        };
        let view = if self.icon_options_open {
            view.child(self.icon_options_view(p, cx))
        } else {
            view
        };
        let view = if self.command_palette.is_some() {
            view.child(self.command_palette_view(p, cx))
        } else {
            view
        };
        if self.entered.elapsed() < crate::motion::STATE && crate::motion::enabled() {
            view.with_animation("seek-window-enter", crate::motion::state(), |view, t| {
                view.opacity(0.88 + 0.12 * t)
            })
            .into_any_element()
        } else {
            view.into_any_element()
        }
    }
}

/// `n` with thousands separators: 3,541,474.
pub(super) fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The index's source as a short label and what it means.
fn index_mode(source: &str) -> (&'static str, &'static str) {
    match source {
        "ntfs + usn" => (
            "· Live",
            "Read from the NTFS file table by the Seek helper and kept current from the change journal",
        ),
        "walker + usn" => (
            "· Live",
            "Folders scanned, then kept current from the NTFS change journal by the Seek helper",
        ),
        _ => (
            "· Scan",
            "Folders scanned and watched for changes. Install the Seek helper in Settings for journal updates",
        ),
    }
}
