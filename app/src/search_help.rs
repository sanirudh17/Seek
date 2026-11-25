//! The search field's syntax popover: every filter the query language takes,
//! with one example each. Opened from the `?` button at the field's end.

use super::*;
use gpui_component::popover::Popover;

/// (syntax, what it does). Kept in the order people reach for them.
const SYNTAX: &[(&str, &str)] = &[
    ("budget report", "Names containing both words"),
    ("'budget.xlsx", "Exact name"),
    ("^read", "Names starting with"),
    ("pdf$", "Names ending with"),
    ("!draft", "Leave out names with"),
    ("ext:pdf,docx", "File extensions"),
    (
        "type:image",
        "image, video, audio, doc, code, archive, font, app",
    ),
    ("kind:folder", "file, folder or link"),
    (r"in:C:\Projects", "Only inside a folder"),
    (r"-in:C:\Projects\old", "Leave out a folder"),
    ("size:>100mb", "Size, also <, and 1mb..5mb"),
    ("mod:<7d", "Modified in the last 7 days"),
    ("name:\"t3\"", "Name contains this exact text"),
    (r"re:^IMG_\d+", "Name matches a regular expression"),
    ("path:invoices", "Full path matches a regular expression"),
];

impl Seek {
    pub(super) fn search_help(&self, p: crate::appearance::Palette) -> AnyElement {
        let trigger = Button::new("search-help")
            .ghost()
            .h(px(22.))
            .w(px(22.))
            .p_0()
            .rounded(px(6.))
            .accessibility_label("Search syntax")
            .tooltip("Search syntax")
            .child(
                Icon::empty()
                    .path("icons/help.svg")
                    .size(px(14.))
                    .text_color(p.muted),
            );
        Popover::new("search-help-popover")
            .anchor(Anchor::TopRight)
            .offset(px(8.))
            .trigger(trigger)
            .content(move |_, _, _| {
                let mut rows = div().flex().flex_col().gap(px(2.));
                for (syntax, meaning) in SYNTAX {
                    rows = rows.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .h(px(26.))
                            .child(
                                div()
                                    .w(px(150.))
                                    .flex_shrink_0()
                                    .truncate()
                                    .font_family(crate::tokens::FONT_MONO)
                                    .text_size(crate::tokens::TEXT_SMALL)
                                    .text_color(p.text)
                                    .child(*syntax),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(crate::tokens::TEXT_SMALL)
                                    .text_color(p.muted)
                                    .child(*meaning),
                            ),
                    );
                }
                div()
                    .w(px(420.))
                    .p_1()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_size(crate::tokens::TEXT_BODY)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Search syntax"),
                    )
                    .child(rows)
            })
            .into_any_element()
    }
}
