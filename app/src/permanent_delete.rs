//! App-owned confirmation using GPUI's existing modal/focus-trap primitive.
use super::*;

pub(super) struct Confirmation {
    pub paths: Vec<PathBuf>,
    pub root: FocusHandle,
    pub cancel: FocusHandle,
    pub delete: FocusHandle,
    pub restore: Option<FocusHandle>,
}

pub(super) fn selection_names(paths: &[PathBuf]) -> (Vec<String>, usize) {
    (
        paths
            .iter()
            .take(3)
            .map(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect(),
        paths.len().saturating_sub(3),
    )
}

impl Seek {
    pub(super) fn cancel_permanent_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(confirm) = self.permanent_delete.take() {
            if let Some(focus) = confirm.restore {
                window.focus(&focus, cx);
            }
            cx.notify();
        }
    }
    pub(super) fn accept_permanent_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Taking the captured selection before enqueuing makes activation one-shot.
        let Some(confirm) = self.permanent_delete.take() else {
            return;
        };
        crate::metrics::interaction(
            "permanent_delete_confirmed",
            serde_json::json!({ "items": confirm.paths.len() }),
        );
        if let Some(focus) = confirm.restore {
            window.focus(&focus, cx);
        }
        self.run_file_op(
            crate::file_ops::Operation::DeletePermanently {
                paths: confirm.paths,
            },
            window,
            cx,
        );
        cx.notify();
    }
    pub(super) fn permanent_delete_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(confirm) = self.permanent_delete.as_ref() else {
            return;
        };
        if !event.is_held {
            match event.keystroke.key.as_str() {
                "escape" => self.cancel_permanent_delete(window, cx),
                "tab" | "left" | "right" => {
                    let focus = if confirm.cancel.is_focused(window) {
                        confirm.delete.clone()
                    } else {
                        confirm.cancel.clone()
                    };
                    window.focus(&focus, cx);
                }
                "enter" | "space" | " " => {
                    if confirm.delete.is_focused(window) {
                        self.accept_permanent_delete(window, cx)
                    } else {
                        self.cancel_permanent_delete(window, cx)
                    }
                }
                _ => {}
            }
        }
        cx.stop_propagation();
    }
    pub(super) fn permanent_delete_view(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(confirm) = self.permanent_delete.as_ref() else {
            return div().into_any_element();
        };
        let p = crate::appearance::palette(&self.preferences, cx);
        let mut surface = p.sheet;
        // Let the window's acrylic show through, just like the app's menus.
        // Keep an opaque surface when the user has disabled glass.
        surface.a = if self.preferences.glass { 0.8 } else { 1. };
        let (names, more) = selection_names(&confirm.paths);
        let selection = format!(
            "{} {} ({}{})",
            confirm.paths.len(),
            if confirm.paths.len() == 1 {
                "item"
            } else {
                "items"
            },
            names
                .iter()
                .map(|n| format!("\"{n}\""))
                .collect::<Vec<_>>()
                .join(", "),
            if more > 0 {
                format!(", and {more} more")
            } else {
                String::new()
            }
        );
        let description = format!(
            "Are you sure you want to permanently delete {selection}? {} cannot be restored from the Recycle Bin.",
            if confirm.paths.len() == 1 {
                "This item"
            } else {
                "These items"
            }
        );
        let selection_label = format!("Delete permanently? {selection}");
        let cancel_focused = confirm.cancel.is_focused(window);
        let delete_focused = confirm.delete.is_focused(window);
        let cancel = gpui_base::Button::new("permanent-delete-cancel")
            .accessibility_label("Cancel")
            .track_focus(&confirm.cancel)
            .tab_index(0)
            .h(px(28.))
            .px_3()
            .rounded(px(8.))
            .border_1()
            .border_color(if cancel_focused { p.muted } else { p.line })
            .bg(p.control)
            .text_color(p.text)
            .hover(|s| s.bg(p.control_hover))
            .child(interface::button_text("Cancel"))
            .on_click(cx.listener(|this, _, window, cx| this.cancel_permanent_delete(window, cx)));
        let delete = gpui_base::Button::new("permanent-delete-accept")
            .accessibility_label("Delete permanently")
            .track_focus(&confirm.delete)
            .tab_index(1)
            .h(px(28.))
            .px_3()
            .rounded(px(8.))
            .border_1()
            .border_color(if delete_focused {
                p.text
            } else {
                rgb(0xb42318).into()
            })
            .bg(rgb(0xb42318))
            .text_color(rgb(0xffffff))
            .hover(|s| s.bg(rgb(0xc73325)))
            .child(interface::button_text("Delete permanently"))
            .on_click(cx.listener(|this, _, window, cx| this.accept_permanent_delete(window, cx)));
        let panel = div()
            .id("permanent-delete-surface")
            .role(Role::Group)
            .aria_label(selection_label)
            .occlude()
            .w(px(448.))
            .max_w((window.viewport_size().width - px(40.)).max(px(0.)))
            .max_h((window.viewport_size().height - px(40.)).max(px(0.)))
            .p_4()
            .flex()
            .flex_col()
            .gap_2()
            .rounded(px(14.))
            .border_1()
            .border_color(p.line)
            .bg(surface)
            .text_color(p.text)
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Delete permanently?"),
            )
            .child(
                div()
                    .id("permanent-delete-names")
                    .min_h_0()
                    .overflow_y_scroll()
                    .text_size(px(12.))
                    .text_color(p.muted)
                    .child(description),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .flex_shrink_0()
                    .child(cancel)
                    .child(delete),
            );
        // The unstyled GPUI host centers normal-flow content, traps focus and
        // occludes the full client area. Its generic Confirm action never deletes.
        gpui_base::Dialog::new(cx)
            .focus_handle(confirm.root.clone())
            .close_on_escape(false)
            .close_on_backdrop_press(false)
            .on_ok(|_, _, _| false)
            .backdrop(div().absolute().inset_0().occlude().bg(rgba(0x00000088)))
            .popup(panel)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::selection_names;
    use std::path::PathBuf;
    #[test]
    fn selection_preview_is_bounded_and_names_remaining_items() {
        let paths = (0..7)
            .map(|n| PathBuf::from(format!("owned-{n}.txt")))
            .collect::<Vec<_>>();
        let (names, more) = selection_names(&paths);
        assert_eq!(names, ["owned-0.txt", "owned-1.txt", "owned-2.txt"]);
        assert_eq!(more, 4);
    }
}
