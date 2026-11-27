use super::*;
use gpui_component_assets::IconName as Glyph;
pub(super) fn control(
    id: &'static str,
    label: &'static str,
    p: crate::appearance::Palette,
    cx: &App,
) -> Button {
    Button::new(id)
        .custom(
            gpui_component::button::ButtonCustomVariant::new(cx)
                .color(rgba(0).into())
                .foreground(p.text)
                .hover(p.accent.opacity(0.16))
                .active(p.accent.opacity(0.24)),
        )
        .h(px(32.))
        .px_3()
        .py_0()
        .rounded(crate::tokens::RADIUS_CONTROL)
        .accessibility_label(label)
}
fn count(n: usize) -> String {
    let raw = n.to_string();
    let mut result = String::new();
    for (i, c) in raw.chars().enumerate() {
        if i > 0 && (raw.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}
/// One plain sentence for a preview failure; the raw error stays in the
/// title's tooltip and in Copy diagnostics.
fn plain_reason(detail: &str, row: Option<&crate::backend::Row>) -> String {
    let lower = detail.to_lowercase();
    let kind = row
        .map(|row| row.kind.clone())
        .filter(|kind| !kind.is_empty())
        .unwrap_or_else(|| "file".into());
    if lower.contains("zip")
        || lower.contains("eocd")
        || lower.contains("corrupt")
        || lower.contains("invalid")
    {
        format!("It may be damaged, or not a real {kind}.")
    } else if lower.contains("timeout") || lower.contains("timed out") {
        "It took too long to open.".into()
    } else if lower.contains("32 mib") || lower.contains("too large") || lower.contains("exceeds") {
        "It's too large to preview.".into()
    } else if lower.contains("denied") || lower.contains("permission") {
        "Seek doesn't have permission to read it.".into()
    } else {
        "Open it in its app to view it.".into()
    }
}
impl Seek {
    pub(super) fn document_loading(
        &self,
        label: &'static str,
        detail: String,
        retry: bool,
        p: crate::appearance::Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::tokens::*;
        let height = if self.preview_enlarged {
            (f32::from(window.viewport_size().height) - 200.).max(120.)
        } else {
            270.
        };
        let row = self.selected(cx);
        let reason = retry.then(|| plain_reason(&detail, row.as_ref()));
        let icon = row
            .as_ref()
            .map(|row| interface::file_image(row, 64., &self.atlas, &self.cache, false));
        let raw = detail.clone();
        div()
            .id("document-preview-state")
            .role(Role::Group)
            .aria_label(reason.clone().unwrap_or_else(|| label.to_string()))
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap_2()
            .when(!self.preview_enlarged, |view| {
                view.children(self.document_navigation(p, cx))
            })
            .child(
                div()
                    .h(px(height))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .px_6()
                    .when(!retry, |d| {
                        d.text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child("Loading…")
                    })
                    .when(retry, |d| {
                        d.children(icon)
                            .child(
                                div()
                                    .id("preview-error-title")
                                    .text_size(TEXT_BODY)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.text)
                                    .child("Can't preview this file")
                                    .tooltip(move |window, cx| {
                                        gpui_component::tooltip::Tooltip::new(raw.clone())
                                            .build(window, cx)
                                    }),
                            )
                            .children(reason.clone().map(|reason| {
                                div()
                                    .max_w(px(260.))
                                    .text_center()
                                    .text_size(TEXT_SMALL)
                                    .text_color(p.muted)
                                    .child(reason)
                            }))
                            .child(
                                div()
                                    .pt_1()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        control("open-from-preview", "Open", p, cx)
                                            .h(BUTTON_HEIGHT)
                                            .rounded(RADIUS_CONTROL)
                                            .child(interface::button_text("Open"))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.open_selected(false, window, cx)
                                            })),
                                    )
                                    .child(
                                        control("retry-document-preview", "Try again", p, cx)
                                            .h(BUTTON_HEIGHT)
                                            .rounded(RADIUS_CONTROL)
                                            .child(interface::button_text("Try again"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.documents.request(None);
                                                this.document_key = None;
                                                this.refresh_preview(cx);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("copy-preview-diagnostics")
                                            .ghost()
                                            .h(BUTTON_HEIGHT)
                                            .w(BUTTON_HEIGHT)
                                            .p_0()
                                            .rounded(RADIUS_CONTROL)
                                            .accessibility_label("Copy diagnostics")
                                            .tooltip("Copy diagnostics")
                                            .child(
                                                Icon::new(Glyph::Copy)
                                                    .size(px(14.))
                                                    .text_color(p.muted),
                                            )
                                            .on_click(|_, _, cx| {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    crate::preview_diagnostics::snapshot(),
                                                ))
                                            }),
                                    ),
                            )
                    }),
            )
            .into_any_element()
    }
    pub(super) fn document_navigation(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let pages = self.document_pages?;
        let page = self.document_key.as_ref()?.page;
        let label = format!("Page {} of {}", page + 1, pages);
        Some(
            div()
                .id("pdf-page-navigation")
                .role(Role::Group)
                .aria_label(label.clone())
                .h(px(40.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    control("pdf-previous", "Previous page", p, cx)
                        .w(px(32.))
                        .px_0()
                        .icon(Glyph::ChevronLeft)
                        .disabled(page == 0)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.document_page(page.saturating_sub(1), cx)
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .text_center()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child(label),
                )
                .child(
                    control("pdf-next", "Next page", p, cx)
                        .w(px(32.))
                        .px_0()
                        .icon(Glyph::ChevronRight)
                        .disabled(page + 1 >= pages)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.document_page(page + 1, cx)),
                        ),
                ),
        )
    }
    pub(super) fn all_pagination(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        if !super::tabs::is_all_locations(
            self.input.read(cx).value().as_ref(),
            self.scope.as_deref(),
            self.content_mode,
            self.largest,
        ) {
            return None;
        }
        let total = self.all_total;
        let offset = self.all_offset;
        let rows = self.table.read(cx).delegate().base_rows.len();
        let label = match total {
            Some(total) if total == 0 || rows > 0 => format!(
                "{}\u{2013}{} of {}",
                count(if total == 0 { 0 } else { offset + 1 }),
                count((offset + rows).min(total)),
                count(total)
            ),
            Some(_) => "Loading all locations...".into(),
            None => "Indexing all locations...".into(),
        };
        Some(
            div()
                .id("all-locations-pages")
                .role(Role::Group)
                .aria_label(format!("All locations: {label}"))
                .h(px(42.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .gap_3()
                .px_3()
                .border_t_1()
                .border_color(p.line)
                .child(
                    control("all-previous-page", "Previous results page", p, cx)
                        .w(px(30.))
                        .h(px(28.))
                        .px_0()
                        .icon(Glyph::ChevronLeft)
                        .disabled(offset == 0 || total.is_none() || rows == 0)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.all_page(offset.saturating_sub(500), cx)
                        })),
                )
                .child(div().text_size(px(11.)).text_color(p.muted).child(label))
                .child(
                    control("all-next-page", "Next results page", p, cx)
                        .w(px(30.))
                        .h(px(28.))
                        .px_0()
                        .icon(Glyph::ChevronRight)
                        .disabled(total.is_none_or(|total| offset + rows >= total) || rows == 0)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.all_page(offset + 500, cx)),
                        ),
                ),
        )
    }
}
