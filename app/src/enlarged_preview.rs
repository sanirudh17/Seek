use super::*;
impl Seek {
    pub(super) fn enlarged_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        match key {
            "escape" => {
                self.preview_enlarged = false;
                self.refresh_preview(cx);
                self.table.read(cx).focus_handle(cx).focus(window, cx);
            }
            "up" | "down" => {
                let current = self.table.read(cx).selected_row();
                let count = self.table.read(cx).delegate().rows.len();
                if let Some(next) = crate::selection::next(current, count, key, 1) {
                    self.table
                        .update(cx, |table, cx| table.set_selected_row(next, cx));
                    self.refresh_preview(cx);
                }
            }
            "left" | "right" => {
                if let Some(Ok(preview)) = &self.document {
                    let page = preview.info.page;
                    let pages = preview.info.pages;
                    let next = if key == "left" {
                        page.saturating_sub(1)
                    } else {
                        page.saturating_add(1)
                    };
                    if next < pages && next != page {
                        self.document_page(next, cx);
                    }
                }
            }
            _ => {}
        }
        cx.notify();
    }
    pub(super) fn enlarged_preview(
        &self,
        p: crate::appearance::Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::preview_controls::control;
        use gpui_component_assets::IconName as Glyph;
        let Some(row) = self.selected(cx) else {
            return div().into_any_element();
        };
        let mut content = div()
            .id("enlarged-preview-content")
            .flex_1()
            .min_h_0()
            .overflow_scroll()
            .p_4();
        if crate::document_preview::Key::new(&row, 0).is_some() {
            content = content.child(self.document_view(p, window, cx));
        } else {
            let details = match self
                .cache
                .lock()
                .unwrap()
                .get(&media::Key::new(&row, 0, true))
            {
                Some(media::Value::Details(d)) => Some(d),
                _ => None,
            };
            if let Some(text) = details.and_then(|d| d.text) {
                content = content
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .child("Text preview · up to 80 lines / 8 KiB"),
                    )
                    .child(
                        div()
                            .mt_3()
                            .text_size(px(16.))
                            .line_height(px(24.))
                            .font_family("Consolas")
                            .child(text),
                    );
            } else {
                let kind = crate::icons::kind(
                    &row.file.path,
                    row.file.kind & 3 == seek_engine::walk::KIND_DIR,
                );
                let visual = matches!(kind, crate::icons::Kind::Image | crate::icons::Kind::Video);
                content = content.child(
                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .items_center()
                        .justify_center()
                        .child(interface::file_image(
                            &row,
                            if visual {
                                (f32::from(window.viewport_size().height) - 180.).clamp(100., 1000.)
                            } else {
                                64.
                            },
                            &self.atlas,
                            &self.cache,
                        ))
                        .when(!visual, |d| {
                            d.child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(p.muted)
                                    .child("No preview available for this file type"),
                            )
                        }),
                );
            }
        }
        div()
            .id("enlarged-preview")
            .track_focus(&self.preview_focus)
            .role(Role::Region)
            .aria_label("Enlarged preview")
            .absolute()
            .inset_0()
            .occlude()
            .bg(p.chrome)
            .text_color(p.text)
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(56.))
                    .flex_shrink_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .id("preview-file-navigation")
                            .role(Role::Group)
                            .aria_label("Browse files")
                            .flex()
                            .gap_1()
                            .child(
                                control("preview-previous-file", "Previous file", p, cx)
                                    .w(px(34.))
                                    .px_0()
                                    .icon(Glyph::ChevronLeft)
                                    .tooltip("Previous file (Up)")
                                    .disabled(
                                        self.table.read(cx).selected_row().is_none_or(|i| i == 0),
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.enlarged_key("up", window, cx)
                                    })),
                            )
                            .child(
                                control("preview-next-file", "Next file", p, cx)
                                    .w(px(34.))
                                    .px_0()
                                    .icon(Glyph::ChevronRight)
                                    .tooltip("Next file (Down)")
                                    .disabled(self.table.read(cx).selected_row().is_none_or(|i| {
                                        i + 1 >= self.table.read(cx).delegate().rows.len()
                                    }))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.enlarged_key("down", window, cx)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(row.name),
                    )
                    .child(
                        control("close-enlarged-preview", "Close preview", p, cx)
                            .w(px(34.))
                            .px_0()
                            .icon(Glyph::WindowClose)
                            .tooltip("Close preview (Esc)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.enlarged_key("escape", window, cx)
                            })),
                    ),
            )
            .child(content)
            .children(
                self.document_navigation(p, cx)
                    .map(|bar| div().px_4().border_t_1().border_color(p.line).child(bar)),
            )
            .child(
                div()
                    .h(px(28.))
                    .flex_shrink_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child("Up/Down: files   ·   Left/Right: pages or slides   ·   Esc: close"),
            )
            .into_any_element()
    }
}
