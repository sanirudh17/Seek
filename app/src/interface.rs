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
pub(super) fn file_image(
    row: &Row,
    size: f32,
    atlas: &Atlas,
    cache: &Arc<Mutex<media::Cache>>,
) -> AnyElement {
    let edge = if size <= 24. {
        32
    } else if size <= 128. {
        128
    } else {
        media::PREVIEW_EDGE
    };
    let key = media::Key::new(row, edge, false);
    let value = cache.lock().unwrap().get(&key);
    // A small association icon enlarged to preview size visibly flashes before
    // the real shell image arrives. Reserve the final bounds while it loads;
    // an icon fallback is appropriate only after an actual provider failure.
    if edge == media::PREVIEW_EDGE && value.is_none() && needs_shell(row) {
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
        _ => atlas.get(crate::icons::kind(
            &row.file.path,
            row.file.kind & 3 == seek_engine::walk::KIND_DIR,
        )),
    };
    img(image)
        .size(px(size))
        .object_fit(ObjectFit::Contain)
        .flex_shrink_0()
        .into_any_element()
}
fn fluent_icon(icon: Glyph, size: f32) -> AnyElement {
    // Native Windows symbol artwork, using Microsoft's documented Fluent font.
    let glyph = match icon {
        Glyph::PanelLeft => "\u{e8a0}",
        Glyph::ChevronLeft => "\u{e76b}",
        Glyph::ChevronRight => "\u{e76c}",
        Glyph::List => "\u{e8fd}",
        Glyph::LayoutGrid => "\u{e8a9}",
        Glyph::GalleryHorizontal => "\u{e8ff}",
        Glyph::ArrowDownUp => "\u{e8cb}",
        Glyph::Eye => "\u{e890}",
        Glyph::EyeOff => "\u{ed1a}",
        Glyph::Info => "\u{e946}",
        Glyph::Settings => "\u{e713}",
        Glyph::Search => "\u{e721}",
        _ => return Icon::new(icon).size(px(size)).into_any_element(),
    };
    div()
        .size(px(size))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .font_family("Segoe Fluent Icons")
        .font_weight(FontWeight::NORMAL)
        .text_size(px(size))
        .line_height(px(size))
        .child(glyph)
        .into_any_element()
}
// Base controls inherit a one-em line box, placing Segoe's visible ink low.
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
    div()
        .occlude()
        .flex()
        .items_center()
        .gap_0()
        .flex_shrink_0()
        .h(px(32.))
        .rounded(px(7.))
        .border_1()
        .border_color(p.line)
        .bg(linear_gradient(
            180.,
            linear_color_stop(
                Hsla::from(if dark {
                    rgba(0xffffff0e)
                } else {
                    rgba(0xffffffb0)
                }),
                0.,
            ),
            linear_color_stop(
                Hsla::from(if dark {
                    rgba(0xffffff03)
                } else {
                    rgba(0xffffff45)
                }),
                1.,
            ),
        ))
}
fn toolbar_separator(p: crate::appearance::Palette) -> Div {
    div().w(px(1.)).h(px(20.)).flex_shrink_0().bg(p.line)
}
pub(super) fn grid_row_height(view: View) -> Pixels {
    px(if view == View::Gallery { 136. } else { 166. })
}
fn heading(label: &'static str, p: crate::appearance::Palette) -> Div {
    div()
        .px_2()
        .pt_3()
        .pb_1()
        .text_size(px(11.))
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
        self.sync_document_preview(cx);
        let mut keys = HashSet::new();
        if (self.panel || self.preview_enlarged || self.view == View::Gallery)
            && let Some(row) = self.selected(cx)
        {
            keys.insert(media::Key::new(&row, 0, true));
            if needs_shell(&row)
                && (crate::document_preview::Key::new(&row, 0).is_none()
                    || self.view == View::Gallery)
            {
                keys.insert(media::Key::new(&row, media::PREVIEW_EDGE, false));
            }
        }
        let retired = self.cache.lock().unwrap().retain_previews(&keys);
        self.document_images_to_drop.extend(retired);
        *self.extra.lock().unwrap() = keys;
        request_media(&self.media, &self.visible, &self.extra, &self.cache);
    }
    fn sync_document_preview(&mut self, cx: &mut Context<Self>) {
        let mut wanted = (self.panel || self.preview_enlarged)
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
                    .timer(std::time::Duration::from_secs(6))
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
        if was_largest {
            self.submit(cx);
        }
        self.visible.lock().unwrap().clear();
        self.refresh_preview(cx);
        cx.notify();
    }
    pub(super) fn save_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preferences.show_hidden = self.show_hidden;
        self.store.save(&self.preferences);
        crate::appearance::apply(&self.preferences, Some(window), cx);
        self.table.update(cx, |table, cx| {
            table.delegate_mut().preferences = self.preferences.clone();
            cx.notify();
        });
        cx.notify();
    }
    fn sort_by(
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
    fn select_modified(
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
            data.drag_paths = Arc::new(data.selected_many.iter().cloned().collect());
        });
        self.table.update(cx, |t, cx| t.set_selected_row(index, cx));
        self.table.read(cx).focus_handle(cx).focus(window, cx);
        self.refresh_preview(cx);
        cx.notify();
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
        scroll = scroll.child(heading("Favorites", p));
        let all = self.scope.is_none();
        scroll = scroll.child(
            div()
                .id("all")
                .role(Role::Button)
                .aria_label("All locations")
                .flex()
                .items_center()
                .gap_2()
                .h(px(30.))
                .px_2()
                .rounded_md()
                .cursor_pointer()
                .bg(if all { p.hover } else { rgba(0).into() })
                .text_color(p.text)
                .hover(move |s| s.bg(p.hover))
                .child(Icon::new(Glyph::Search).size(px(16.)))
                .child("All locations")
                .on_click(cx.listener(|this, _, window, cx| this.scope_to(None, window, cx))),
        );
        for (index, (name, path)) in self.favorites.iter().enumerate() {
            let path = path.clone();
            let drop_path = path.clone();
            let tab_path = path.clone();
            let tab_owner = cx.entity().downgrade();
            let selected = self.scope.as_ref() == Some(&path);
            let icon = match name.as_str() {
                "Downloads" => Glyph::Download,
                "Desktop" => Glyph::Monitor,
                "Documents" => Glyph::FileText,
                "Pictures" => Glyph::Image,
                "Videos" => Glyph::Film,
                _ => Glyph::Music,
            };
            scroll = scroll.child(
                div()
                    .id(("favorite", index))
                    .on_drop(cx.listener(
                        move |this, drag: &super::daily::FilesDrag, window, cx| {
                            this.drop_files(drag, drop_path.clone(), window, cx)
                        },
                    ))
                    .role(Role::Button)
                    .aria_label(name.clone())
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(30.))
                    .px_2()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(if selected { p.hover } else { rgba(0).into() })
                    .text_color(p.text)
                    .hover(move |s| s.bg(p.hover))
                    .child(if let Some(image) = self.atlas.location(&path) {
                        img(image)
                            .size(px(16.))
                            .object_fit(ObjectFit::Contain)
                            .flex_shrink_0()
                            .into_any_element()
                    } else {
                        Icon::new(icon)
                            .size(px(16.))
                            .text_color(if selected { p.text } else { p.muted })
                            .into_any_element()
                    })
                    .child(name.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.scope_to(Some(path.clone()), window, cx)
                    }))
                    .context_menu(move |menu, window, cx| {
                        super::daily::actions_menu(
                            super::daily::menu_style(menu, window),
                            tab_owner.clone(),
                            Some(tab_path.clone()),
                            true,
                            cx,
                        )
                    }),
            );
        }
        scroll = scroll.child(heading("Locations", p));
        for (index, path) in self.roots.iter().enumerate() {
            let path = path.clone();
            let drop_path = path.clone();
            let tab_path = path.clone();
            let tab_owner = cx.entity().downgrade();
            let selected = self.scope.as_ref() == Some(&path);
            let name = path
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_owned();
            scroll = scroll.child(
                div()
                    .id(("drive", index))
                    .on_drop(cx.listener(
                        move |this, drag: &super::daily::FilesDrag, window, cx| {
                            this.drop_files(drag, drop_path.clone(), window, cx)
                        },
                    ))
                    .role(Role::Button)
                    .aria_label(name.clone())
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(30.))
                    .px_2()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(if selected { p.hover } else { rgba(0).into() })
                    .text_color(p.text)
                    .hover(move |s| s.bg(p.hover))
                    .child(
                        Icon::new(Glyph::HardDrive)
                            .size(px(16.))
                            .text_color(if selected { p.text } else { p.muted }),
                    )
                    .child(div().min_w_0().truncate().child(name))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.scope_to(Some(path.clone()), window, cx)
                    }))
                    .context_menu(move |menu, window, cx| {
                        super::daily::actions_menu(
                            super::daily::menu_style(menu, window),
                            tab_owner.clone(),
                            Some(tab_path.clone()),
                            true,
                            cx,
                        )
                    }),
            );
        }
        scroll = scroll.child(heading("Recent searches", p));
        for (index, (text, scope)) in self
            .history
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, (text, _))| !text.is_empty())
            .take(6)
        {
            let (text, scope) = (text.clone(), scope.clone());
            scroll = scroll.child(
                div()
                    .id(("recent", index))
                    .h(px(28.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |s| s.bg(p.hover))
                    .child(Icon::new(Glyph::Clock).size(px(15.)).text_color(p.muted))
                    .child(div().truncate().child(text.clone()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.scope = scope.clone();
                        this.input
                            .update(cx, |input, cx| input.set_value(text.clone(), window, cx));
                        this.submit(cx);
                    })),
            );
        }
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
                    .child(format!("{} indexed", self.status.entries)),
            );
        div()
            .id("sidebar-content")
            .w(px(204.))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(p.sidebar)
            .border_r_1()
            .border_color(p.line)
            .child(scroll)
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
            .token(move |token, _, _| {
                if token.token().label().starts_with("Type:") {
                    return div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .h(token.line_height())
                        .max_w(token.available_width())
                        .px_1()
                        .gap_1()
                        .rounded_md()
                        .bg(p.accent.opacity(0.14))
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
                    .bg(p.accent.opacity(0.16))
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
        let sidebar = tool("sidebar", Glyph::PanelLeft, "Toggle sidebar")
            .text_color(p.muted)
            .selected(false)
            .on_click(cx.listener(|this, _, _, cx| {
                this.sidebar_from = f32::from_bits(this.sidebar_width.load(Ordering::Relaxed));
                this.sidebar = !this.sidebar;
                this.sidebar_motion = this.sidebar_motion.wrapping_add(1);
                cx.notify();
            }));
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
        ] {
            let active = self.view == v;
            switcher = switcher.child(
                toolbar_state(tool(id, icon, label), active, p, cx)
                    .disabled(self.content_mode && v != View::List)
                    .toggled(active)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_view(v, cx))),
            );
        }
        let panel_visible = self.panel && self.selected(cx).is_some();
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
                .on_click(cx.listener(|this, _, window, cx| {
                    this.show_hidden = !this.show_hidden;
                    this.save_preferences(window, cx);
                    this.submit(cx);
                })),
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
                    tool("info", Glyph::Info, "Info panel · Space").w(px(30.)),
                    panel_visible,
                    p,
                    cx,
                )
                .toggled(panel_visible)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.panel = !this.panel;
                    this.refresh_preview(cx);
                    cx.notify();
                })),
            )
            .child(
                toolbar_state(
                    tool("settings", Glyph::Settings, "Settings").w(px(30.)),
                    self.settings,
                    p,
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.settings = true;
                    this.settings_focus.focus(window, cx);
                    let _ = this.backend.tx.send(Command::HelperStatus);
                    cx.notify();
                })),
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
        let available = f32::from(window.viewport_size().width)
            - f32::from_bits(self.sidebar_width.load(Ordering::Relaxed))
            - if self.panel && self.selected(cx).is_some() {
                self.info_width(window)
            } else {
                0.
            };
        self.grid_columns = (available / 140.)
            .floor()
            .max(if self.view == View::Gallery { 3. } else { 2. })
            as usize;
        let columns = self.grid_columns;
        let count = self.table.read(cx).delegate().rows.len().div_ceil(columns);
        let this = cx.entity().downgrade();
        let gallery = self.view == View::Gallery;
        let visible_height = if gallery {
            155.
        } else {
            f32::from(window.viewport_size().height) - 82.
        };
        let list = uniform_list("icon-grid", count, move |range, _window, cx| {
            this.update(cx, |this, cx| {
                let p = crate::appearance::palette(&this.preferences, cx);
                let table = this.table.read(cx);
                let total = table.delegate().rows.len();
                let start = (range.start * columns).min(total);
                let end = (range.end * columns).min(total);
                let rows = table.delegate().rows[start..end].to_vec();
                let _selected = table.selected_row();
                // UniformList can render visible rows in separate batches.
                // Request the whole viewport on each batch, so a later batch
                // cannot cancel the thumbnails requested by an earlier one.
                let top = scroll_top(&this.grid_scroll, grid_row_height(this.view));
                let lines = (visible_height / f32::from(grid_row_height(this.view))).ceil() as usize + 2;
                let wanted_start = (top.min(range.start) * columns).min(total);
                let wanted_end = ((top + lines).max(range.end) * columns).min(total);
                let keys = table.delegate().rows[wanted_start..wanted_end]
                    .iter()
                    .filter(|r| needs_shell(r) && r.file.kind & 3 != seek_engine::walk::KIND_DIR)
                    .map(|r| media::Key::new(r, 128, false))
                    .collect();
                *this.visible.lock().unwrap() = keys;
                request_media(&this.media, &this.visible, &this.extra, &this.cache);
                range
                    .map(|line| {
                        let mut line_view = div()
                            .h(grid_row_height(this.view))
                            .w_full()
                            .flex()
                            .px_3()
                            .gap_2();
                        for (i, row) in rows
                            .iter()
                            .enumerate()
                            .take(((line + 1) * columns).saturating_sub(start).min(rows.len()))
                            .skip((line * columns).saturating_sub(start))
                        {
                            let i = i + start;
                            let drag=super::daily::FilesDrag{paths:if table.delegate().selected_many.contains(&row.file.path){table.delegate().drag_paths.clone()}else{Arc::new(vec![row.file.path.clone()])},label:row.name.clone()};
                            let context_owner=cx.entity().downgrade();
                            let context_path=row.file.path.clone();
                            let context_folder=row.file.kind & 3 == seek_engine::walk::KIND_DIR;
                            let drop_path=row.file.path.clone();
                            let chosen = table.delegate().selected_many.contains(&row.file.path);
                            let size = if gallery { 72. } else { 96. };
                            line_view = line_view.child(
                                div()
                                    .id(("grid-item", i))
                                    .role(Role::Button).aria_label(row.name.clone())
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_1()
                                    .p_2()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .bg(if chosen {
                                        p.accent.opacity(0.16)
                                    } else {
                                        rgba(0).into()
                                    })
                                    .hover(move |s| {
                                        s.bg(if chosen {
                                            p.accent.opacity(0.16)
                                        } else {
                                            p.hover
                                        })
                                    })
                                    .child(file_image(row, size, &this.atlas, &this.cache))
                                    .child(
                                        div()
                                            .w_full()
                                            .text_center()
                                            .rounded_sm()
                                            .px_1()
                                            .text_size(px(12.))
                                            .bg(if chosen {
                                                p.accent.opacity(0.18)
                                            } else {
                                                rgba(0).into()
                                            })
                                            .text_color(p.text)
                                            .truncate()
                                            .child(row.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(p.muted)
                                            .child(row.size.clone()),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            move |this, event: &MouseDownEvent, window, cx| {
                                                this.select_modified(i,event.modifiers.shift,event.modifiers.control, window, cx);
                                                if event.click_count >= 2 {
                                                    this.open_selected(false, cx);
                                                }
                                            },
                                        ),
                                    )
                                    .on_drag(drag,|drag,_,window,cx| {
                let _=window;
                cx.new(|_|drag.clone())
            })
                                    .when(row.file.kind&3==seek_engine::walk::KIND_DIR,|tile|tile.on_drop(cx.listener(move|this,drag:&super::daily::FilesDrag,window,cx|this.drop_files(drag,drop_path.clone(),window,cx))))
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
        .track_scroll(&self.grid_scroll)
        .track_focus(&self.table.read(cx).focus_handle(cx))
        .size_full();
        if gallery && self.selected(cx).is_some() {
            let p = crate::appearance::palette(&self.preferences, cx);
            let preview = if let Some(row) = self.selected(cx) {
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(file_image(&row, 290., &self.atlas, &self.cache))
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
            div()
                .flex()
                .flex_col()
                .size_full()
                .child(div().flex_1().min_h_0().overflow_hidden().child(preview))
                .child(
                    div()
                        .h(px(155.))
                        .flex_shrink_0()
                        .border_t_1()
                        .border_color(p.line)
                        .child(list),
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
        self.all_total = None;
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
                    Some(Err(e)) => (
                        "Preview unavailable",
                        format!("{e}. Open the file in its app to view it."),
                    ),
                    _ => (
                        "Loading preview...",
                        "Selected document only; files up to 32 MiB.".into(),
                    ),
                };
                return self.document_loading(label, detail, state.is_some(), p, window, cx);
            }
        };
        let mut view = div()
            .id("document-preview")
            .flex()
            .flex_col()
            .gap_2()
            .flex_shrink_0()
            .when(!self.preview_enlarged, |view| {
                view.children(self.document_navigation(p, cx))
            });
        if preview.info.source.starts_with("PowerPoint") {
            let height = if self.preview_enlarged {
                (f32::from(window.viewport_size().height) - 200.).max(120.)
            } else {
                270.
            };
            let width = if self.preview_enlarged {
                (f32::from(window.viewport_size().width) - 80.).max(100.)
            } else {
                248.
            };
            let mut stage = div()
                .id("presentation-stage")
                .h(px(height))
                .flex_shrink_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_3();
            if let Some(image) = &preview.image {
                stage = stage.child(
                    img(image.clone())
                        .w(px(width))
                        .h(px((height - 60.).max(100.)))
                        .object_fit(ObjectFit::Contain)
                        .flex_shrink_0(),
                );
            }
            if let Some(text) = &preview.info.text {
                stage = stage.child(
                    div()
                        .text_size(px(if self.preview_enlarged { 16. } else { 12. }))
                        .child(text.clone()),
                );
            }
            return view
                .child(stage)
                .child(
                    div()
                        .h(px(18.))
                        .truncate()
                        .text_size(px(10.))
                        .text_color(p.muted)
                        .child(if preview.image.is_some() {
                            "Saved first-slide image and text"
                        } else {
                            "Slide text · formatting and artwork not reproduced"
                        }),
                )
                .into_any_element();
        }
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
                .child(div().text_size(px(11.)).text_color(p.muted).child(
                    if preview.info.source.starts_with("PowerPoint") {
                        "Slide text · formatting and artwork not reproduced"
                    } else {
                        "Beginning of document · text only"
                    },
                ))
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
                .child(div().text_size(px(10.)).text_color(p.muted).child(
                    if preview.info.source.starts_with("PowerPoint") {
                        "Up to 24,000 characters per slide."
                    } else {
                        "Up to 100 lines / 12,000 characters."
                    },
                ));
        }
        if let Some(sheet) = &preview.info.sheet {
            view = view
                .child(
                    div()
                        .id("first-sheet-title")
                        .role(Role::Group)
                        .aria_label(format!("First sheet: {}", sheet.name))
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(sheet.name.clone()),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child("First sheet · data only, no formatting"),
                );
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
                        * 6.5
                        + 16.)
                        .clamp(64., 128.)
                })
                .collect();
            let mut table = div()
                .id("sheet-table")
                .role(Role::Table)
                .aria_label("Sheet data")
                .flex()
                .flex_col()
                .w(px(34. + widths.iter().sum::<f32>()));
            let mut header = div()
                .flex()
                .bg(p.hover)
                .child(div().w(px(34.)).flex_shrink_0());
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
                        .px_2()
                        .py_1()
                        .text_size(px(11.))
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
                    .text_size(px(11.))
                    .child(
                        div()
                            .w(px(34.))
                            .flex_shrink_0()
                            .px_1()
                            .py_1()
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
                            .px_2()
                            .py_1()
                            .truncate()
                            .child(cell.clone()),
                    );
                }
                table = table.child(line);
            }
            view=view.child(div().id("sheet-scroll").max_h(px(280.)).overflow_scroll().border_1().border_color(p.line).rounded(px(6.)).child(table))
                .child(div().text_size(px(10.)).text_color(p.muted).child(format!("Showing {} of {} used rows, {} of {} columns. Cells up to 256 characters; cached formula values.",sheet.rows.len(),sheet.total_rows,cols,sheet.total_cols)));
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
    fn info_panel(
        &self,
        p: crate::appearance::Palette,
        window: &Window,
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
        if let Some(row) = self.selected(cx) {
            panel = panel.child(
                div().w_full().flex().justify_center().child(
                    super::settings_view::control("enlarge-preview", "Enlarge preview", p, cx)
                        .w(px(158.))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.preview_enlarged = true;
                            this.refresh_preview(cx);
                            this.preview_focus.focus(window, cx);
                            cx.notify();
                        })),
                ),
            );
            let details = match self
                .cache
                .lock()
                .unwrap()
                .get(&media::Key::new(&row, 0, true))
            {
                Some(media::Value::Details(d)) => Some(d),
                _ => None,
            };
            panel = panel
                .when(
                    self.view != View::Gallery
                        || crate::document_preview::Key::new(&row, 0).is_some(),
                    |panel| {
                        if crate::document_preview::Key::new(&row, 0).is_some() {
                            panel.child(self.document_view(p, window, cx))
                        } else {
                            panel.child(
                                div()
                                    .h(px(210.))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(file_image(&row, 200., &self.atlas, &self.cache)),
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
                .child(
                    div()
                        .pt_3()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Information"),
                );
            for (label, value) in [
                ("Created", details.as_ref().map(|d| d.created.clone())),
                (
                    "Modified",
                    details
                        .as_ref()
                        .map(|d| d.modified.clone())
                        .or_else(|| Some(row.modified.clone())),
                ),
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
                        .child("Full path"),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child(row.file.path.display().to_string()),
                );
            if let Some(d) = details {
                if let Some(text) = d.text {
                    panel = panel
                        .child(
                            div()
                                .pt_3()
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Preview"),
                        )
                        .child(div().font_family("Consolas").text_size(px(11.)).child(text));
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
                    .child(div().text_xs().child("Space toggles this panel")),
            );
        }
        div()
            .relative()
            .w(px(width))
            .h_full()
            .flex_shrink_0()
            .child(panel)
            .child(self.info_resize_handle(width))
            .into_any_element()
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
        let p = crate::appearance::palette(&self.preferences, cx);
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
            for (i, component) in crumb.components().enumerate() {
                path.push(component);
                if matches!(component, std::path::Component::RootDir) {
                    continue;
                }
                let target = path.clone();
                breadcrumb = breadcrumb
                    .child(
                        Icon::new(Glyph::ChevronRight)
                            .size(px(11.))
                            .text_color(p.muted),
                    )
                    .child(
                        div()
                            .id(("path", i))
                            .flex()
                            .items_center()
                            .h(px(22.))
                            .flex_shrink_0()
                            .gap_1()
                            .cursor_pointer()
                            .child(
                                Icon::new(if i == 0 {
                                    Glyph::HardDrive
                                } else {
                                    Glyph::Folder
                                })
                                .size(px(12.))
                                .text_color(p.muted),
                            )
                            .child(
                                div()
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
                    .max_w(px(210.))
                    .truncate()
                    .child(if self.content_mode {state.clone()} else {self.message.as_deref().unwrap_or(&state).to_owned()}),
            )
            .when(self.status.skipped != 0, |bar| bar.child(
                div().id("access-status").child(format!("{} inaccessible locations", self.status.skipped))
                    .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("Unique locations Windows could not list, not a file count. May include denied access, unavailable folders or failed enumeration. Accessible files are indexed, including hidden and system files.").build(window, cx))
            ));
        let contents = if self.table.read(cx).delegate().rows.is_empty() {
            let text = self.table.read(cx).delegate().empty_message.clone();
            div()
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
        results = results.child(
            div()
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
                        .bg(p.chrome)
                        .border_t_1()
                        .border_color(p.line)
                        .child(breadcrumb.flex_1().min_w_0())
                        .child(status.flex_shrink_0()),
                ),
        );
        if self.panel && selected.is_some() {
            results = results.child(self.info_panel(p, window, cx));
        }
        let main = div()
            .flex_1()
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
            .w(px(if self.sidebar { 204. } else { 0. }))
            .child(self.sidebar_view(p, cx));
        let sidebar = if self.sidebar_motion == 0 || !self.motion_enabled {
            sidebar.into_any_element()
        } else {
            let from = self.sidebar_from;
            let to = if self.sidebar { 204. } else { 0. };
            let width = self.sidebar_width.clone();
            sidebar
                .with_animation(
                    ("sidebar-motion", self.sidebar_motion as usize),
                    Animation::new(std::time::Duration::from_millis(160))
                        .with_easing(|t| t * t * (3. - 2. * t)),
                    move |panel, t| {
                        let value = from + (to - from) * t;
                        width.store(value.to_bits(), Ordering::Relaxed);
                        panel.w(px(value))
                    },
                )
                .into_any_element()
        };
        body = body.child(sidebar).child(main);
        let mut view =
            div()
                .id("seek")
                .relative()
                .size_full()
                .flex()
                .flex_col()
                .bg(p.canvas)
                .font_family("Segoe UI Variable Text")
                .text_size(px(13.))
                .text_color(p.text)
                .capture_action(cx.listener(|this, _: &NewTab, window, cx| {
                    if this.tabs_available(window,cx) { this.new_tab(None,window,cx); } cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &CloseTab, window, cx| {
                    if this.tabs_available(window,cx) { this.close_tab(this.active_tab,window,cx); } cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &NextTab, window, cx| {
                    if this.tabs_available(window,cx) { this.switch_tab(super::tabs::adjacent(this.active_tab,this.tabs.len(),false),window,cx); } cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &PreviousTab, window, cx| {
                    if this.tabs_available(window,cx) { this.switch_tab(super::tabs::adjacent(this.active_tab,this.tabs.len(),true),window,cx); } cx.stop_propagation();
                }))
                .capture_action(
                    cx.listener(|this, _: &gpui_base::actions::Cancel, window, cx| {
                        if this.permanent_delete.is_some() {this.cancel_permanent_delete(window,cx);cx.stop_propagation();return;}
                        if gpui_component::WindowExt::has_active_dialog(window,cx) { return; }
                        if this.edit.take().is_some() {
                            cx.notify();
                        } else if this.settings
                            || this.suggestions.is_some()
                            || this
                                .input
                                .read(cx)
                                .focus_handle(cx)
                                .is_focused(window)
                            || this
                                .table
                                .read(cx)
                                .focus_handle(cx)
                                .is_focused(window)
                        {
                            this.clear(window, cx);
                        } else {
                            cx.propagate();
                        }
                    }),
                )
                .capture_action(cx.listener(
                    |this, _: &gpui_component::input::Escape, window, cx| this.clear(window, cx),
                ))
                .capture_action(cx.listener(|this, _: &ExpandFolder, _, cx| {
                    if let Some(row) = this.selected(cx)
                        && row.file.kind & 3 == seek_engine::walk::KIND_DIR
                    {
                        this.expand_folder(row.file.path, true, cx);
                    }
                    cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &CollapseFolder, _, cx| {
                    if let Some(row) = this.selected(cx)
                        && row.file.kind & 3 == seek_engine::walk::KIND_DIR
                    {
                        this.expand_folder(row.file.path, false, cx);
                    }
                    cx.stop_propagation();
                }))
                .on_drag_move(cx.listener(|_, event: &DragMoveEvent<super::daily::FilesDrag>, window, cx| {
                    let position = event.event.position;
                    let size = window.viewport_size();
                    if position.x >= px(0.) && position.y >= px(0.) && position.x < size.width && position.y < size.height {
                        return;
                    }
                    let paths = event.drag(cx).paths.clone();
                    if !cx.stop_active_drag(window) { return; }
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
                .on_mouse_up(MouseButton::Left, cx.listener(Self::finish_info_resize))
                .on_mouse_up_out(MouseButton::Left, cx.listener(Self::finish_info_resize))
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
                .when(!self.preview_enlarged, |view| view
                    .child(self.toolbar(p, window, cx))
                    .child(self.tab_strip(p, cx))
                    .child(body));
        if self.settings {
            view = view.child(self.settings_sheet(p, cx));
        }
        if self.preview_enlarged {
            view = view.child(self.enlarged_preview(p, window, cx));
        }
        view.child(self.daily_overlays(window, cx))
    }
}
