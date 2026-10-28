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
                .color(p.control)
                .foreground(p.text)
                .hover(p.control_hover)
                .active(p.control_selected),
        )
        .outline()
        .h(px(32.))
        .px_3()
        .py_0()
        .rounded(px(6.))
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
        let height = if self.preview_enlarged {
            (f32::from(window.viewport_size().height) - 200.).max(120.)
        } else {
            270.
        };
        div()
            .id("document-preview-state")
            .role(Role::Group)
            .aria_label(label)
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
                    .text_color(p.muted)
                    .text_size(px(12.))
                    .child(label)
                    .when(retry, |d| {
                        d.child(
                            control("retry-document-preview", "Retry preview", p, cx)
                                .child(interface::button_text("Retry"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.documents.request(None);
                                    this.document_key = None;
                                    this.refresh_preview(cx);
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(detail.clone()).build(window, cx)
            })
            .into_any_element()
    }
    pub(super) fn document_navigation(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let (pages, slide) = self.document_pages?;
        let page = self.document_key.as_ref()?.page;
        let label = format!(
            "{} {} of {}",
            if slide { "Slide" } else { "Page" },
            page + 1,
            pages
        );
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
                    control("pdf-previous", "Previous page or slide", p, cx)
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
                    control("pdf-next", "Next page or slide", p, cx)
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
        let total = self.all_total?;
        let offset = self.all_offset;
        let rows = self.table.read(cx).delegate().base_rows.len();
        let label = format!(
            "{}–{} of {}",
            count(if total == 0 { 0 } else { offset + 1 }),
            count((offset + rows).min(total)),
            count(total)
        );
        Some(
            div()
                .id("all-locations-pages")
                .role(Role::Group)
                .aria_label(format!("All locations: {total} items"))
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
                        .disabled(offset == 0)
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
                        .disabled(offset + rows >= total)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.all_page(offset + 500, cx)),
                        ),
                ),
        )
    }
}
