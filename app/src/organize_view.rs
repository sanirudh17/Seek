//! Organize Folder review sheet and the short-lived Undo toast that follows it.
//! The plan is read off the UI thread; nothing moves until Organize is pressed.
use super::*;
use crate::organize::Proposal;
use gpui_component::checkbox::Checkbox;
use std::path::Path;

pub(super) struct Review {
    pub folder: PathBuf,
    /// By type, by date taken, or by name pattern.
    pub mode: crate::organize::Mode,
    /// `None` while the folder is being read.
    pub plan: Option<Result<Proposal, String>>,
    pub enabled: Vec<bool>,
    pub rows: Vec<FocusHandle>,
    pub root: FocusHandle,
    pub cancel: FocusHandle,
    pub accept: FocusHandle,
    pub restore: Option<FocusHandle>,
    _task: Task<()>,
}

pub(super) struct Toast {
    pub text: String,
    pub undo: bool,
    pub generation: u64,
    _timer: Task<()>,
}

/// How long a toast stays before it leaves on its own.
const TOAST_SECONDS: u64 = 6;

/// The latest Organize job's completed moves, from the saved journal.
pub(super) fn organized(history: &crate::operation_journal::Snapshot) -> Option<usize> {
    use crate::operation_journal::Event;
    let job = history.jobs.last()?;
    (job.label == crate::file_ops::ORGANIZE_LABEL).then(|| {
        job.outcomes
            .iter()
            .filter(|event| {
                matches!(event, Event::After { action, hresult, .. }
                    if action == "move" && crate::operation_journal::item_completed(*hresult))
            })
            .count()
    })
}

fn names(items: &[crate::organize::Item]) -> String {
    let shown = items
        .iter()
        .take(3)
        .filter_map(|item| item.path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(", ");
    match items.len().saturating_sub(3) {
        0 => shown,
        more => format!("{shown} and {more} more"),
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{} {}", count, if count == 1 { one } else { many })
}

impl Seek {
    pub(super) fn open_organize(
        &mut self,
        folder: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_organize_as(folder, crate::organize::Mode::Kind, window, cx);
    }

    pub(super) fn open_organize_as(
        &mut self,
        folder: Option<PathBuf>,
        mode: crate::organize::Mode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(folder) = folder.or_else(|| self.scope.clone()) else {
            self.message = Some("Open a folder to organize it.".into());
            cx.notify();
            return;
        };
        if self.files_pending != 0 {
            self.message = Some("Wait for the current file operation to finish.".into());
            cx.notify();
            return;
        }
        let read = folder.clone();
        let task = cx.spawn_in(window, async move |this, cx| {
            let plan = cx
                .background_executor()
                .spawn(async move { crate::organize::propose_by(&read, mode) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                let Some(review) = &mut this.organize else {
                    return;
                };
                if let Ok(plan) = &plan {
                    review.enabled = vec![true; plan.groups.len()];
                    review.rows = plan.groups.iter().map(|_| cx.focus_handle()).collect();
                }
                review.plan = Some(plan);
                let accept = review.accept.clone();
                window.focus(&accept, cx);
                cx.notify();
            });
        });
        let root = cx.focus_handle();
        window.focus(&root, cx);
        // Switching modes keeps the review open and where focus came from.
        let restore = self
            .organize
            .take()
            .map_or_else(|| window.focused(cx), |review| review.restore);
        self.organize = Some(Review {
            folder,
            mode,
            plan: None,
            enabled: vec![],
            rows: vec![],
            root,
            cancel: cx.focus_handle(),
            accept: cx.focus_handle(),
            restore,
            _task: task,
        });
        cx.notify();
    }
    pub(super) fn close_organize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(review) = self.organize.take() {
            if let Some(focus) = review.restore {
                window.focus(&focus, cx);
            }
            cx.notify();
        }
    }
    fn organize_count(&self) -> usize {
        self.organize
            .as_ref()
            .and_then(|review| {
                let plan = review.plan.as_ref()?.as_ref().ok()?;
                Some(
                    plan.groups
                        .iter()
                        .zip(&review.enabled)
                        .filter(|(_, on)| **on)
                        .map(|(group, _)| group.items.len())
                        .sum(),
                )
            })
            .unwrap_or(0)
    }
    pub(super) fn accept_organize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.organize_count() == 0 {
            return;
        }
        let Some(review) = self.organize.take() else {
            return;
        };
        let Some(Ok(plan)) = review.plan else {
            return;
        };
        if let Some(focus) = review.restore {
            window.focus(&focus, cx);
        }
        let groups = plan
            .groups
            .into_iter()
            .zip(review.enabled)
            .filter(|(_, on)| *on)
            .map(|(group, _)| (group.name, group.items))
            .collect();
        self.organizing = true;
        self.run_file_op(
            crate::file_ops::Operation::Organize {
                folder: review.folder,
                groups,
            },
            window,
            cx,
        );
        if self.message.is_some() {
            self.organizing = false;
        }
    }
    fn toggle_organize_group(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(on) = self
            .organize
            .as_mut()
            .and_then(|review| review.enabled.get_mut(index))
        {
            *on = !*on;
            cx.notify();
        }
    }
    pub(super) fn organize_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(review) = self.organize.as_ref() else {
            return;
        };
        let mut order = review.rows.clone();
        order.push(review.cancel.clone());
        order.push(review.accept.clone());
        let current = order.iter().position(|focus| focus.is_focused(window));
        let step = |forward: bool| -> FocusHandle {
            let next = match (current, forward) {
                (None, _) => order.len() - 1,
                (Some(i), true) => (i + 1) % order.len(),
                (Some(i), false) => (i + order.len() - 1) % order.len(),
            };
            order[next].clone()
        };
        match event.keystroke.key.as_str() {
            "escape" => self.close_organize(window, cx),
            "tab" => {
                let focus = step(!event.keystroke.modifiers.shift);
                window.focus(&focus, cx);
            }
            "down" | "right" => window.focus(&step(true), cx),
            "up" | "left" => window.focus(&step(false), cx),
            "space" | " " | "enter" => {
                if let Some(i) = current.filter(|i| *i < review.rows.len()) {
                    self.toggle_organize_group(i, cx);
                } else if review.cancel.is_focused(window) {
                    self.close_organize(window, cx);
                } else if event.keystroke.key == "enter" || review.accept.is_focused(window) {
                    self.accept_organize(window, cx);
                }
            }
            _ => {}
        }
        cx.stop_propagation();
    }
    /// Called with each finished file reply: shows "Organized N files" with Undo.
    pub(super) fn finish_organize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.organizing) {
            return;
        }
        let moved = self
            .operation_history
            .as_ref()
            .ok()
            .and_then(organized)
            .unwrap_or(0);
        if moved == 0 {
            return;
        }
        self.show_toast(
            format!("Organized {}", plural(moved, "file", "files")),
            true,
            window,
            cx,
        );
    }
    pub(super) fn show_toast(
        &mut self,
        text: String,
        undo: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_toast_for(
            text,
            undo,
            std::time::Duration::from_secs(TOAST_SECONDS),
            window,
            cx,
        );
    }
    /// Brief non-stacking confirmation toast for quiet actions (11.6),
    /// respecting the "Show confirmations" preference and auto-dismissing
    /// after `motion::TOAST_QUIET` (~2.5 s).
    pub(super) fn show_quiet_toast(
        &mut self,
        text: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.preferences.show_confirmations {
            return;
        }
        self.show_toast_for(text.into(), false, crate::motion::TOAST_QUIET, window, cx);
    }
    fn show_toast_for(
        &mut self,
        text: String,
        undo: bool,
        duration: std::time::Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toast_generation = self.toast_generation.wrapping_add(1);
        let generation = self.toast_generation;
        let timer = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(duration).await;
            let _ = this.update(cx, |this, cx| {
                if this.toast.as_ref().is_some_and(|t| t.generation == generation) {
                    this.toast = None;
                    cx.notify();
                }
            });
        });
        self.toast = Some(Toast {
            text,
            undo,
            generation,
            _timer: timer,
        });
        cx.notify();
    }
    pub(super) fn toast_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let toast = self.toast.as_ref()?;
        let p = crate::appearance::palette(&self.preferences, cx);
        let undo = toast.undo && self.can_reverse_rename(false);
        let pill = div()
            .id("toast")
            .role(Role::Status)
            .aria_label(toast.text.clone())
            .occlude()
            .h(px(36.))
            .pl_4()
            .pr(px(if undo { 8. } else { 16. }))
            .flex()
            .items_center()
            .justify_center()
            .gap_3()
            .rounded(px(10.))
            .border_1()
            .border_color(crate::appearance::modal_border(p))
            .bg(crate::appearance::modal_surface(p))
            .shadow_lg()
            .text_size(px(13.))
            .text_color(p.text)
            .when(toast.undo, |d| {
                // A finished operation: a green check beside the words.
                d.child(
                    Icon::new(gpui_component_assets::IconName::Check)
                        .size(px(14.))
                        .text_color(p.tints.green),
                )
            })
            .child(
                div()
                    .whitespace_nowrap()
                    .child(interface::button_text(toast.text.clone())),
            )
            .when(undo, |toast| {
                toast.child(
                    gpui_base::Button::new("toast-undo")
                        .accessibility_label("Undo")
                        .h(px(26.))
                        .px_2()
                        .rounded(px(6.))
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.text)
                        .hover(|s| s.bg(p.hover))
                        .child(interface::button_text("Undo"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toast = None;
                            this.reverse_file_operation(false, window, cx);
                        })),
                )
            });
        let pill: AnyElement = if self.motion_enabled {
            pill.with_animation(
                ("toast-enter", toast.generation as usize),
                crate::motion::enter(),
                |toast, t| toast.opacity(t).mt(px(8. * (1. - t))),
            )
            .into_any_element()
        } else {
            pill.into_any_element()
        };
        Some(
            div()
                .absolute()
                .bottom(px(40.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(pill)
                .into_any_element(),
        )
    }
    pub(super) fn organize_view(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(review) = self.organize.as_ref() else {
            return div().into_any_element();
        };
        let p = crate::appearance::palette(&self.preferences, cx);
        let title = format!(
            "Organize {}",
            review
                .folder
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| interface_path(&review.folder))
        );
        let count = self.organize_count();
        let body: AnyElement = match &review.plan {
            None => div()
                .py_6()
                .text_size(px(13.))
                .text_color(p.muted)
                .child("Reading folder…")
                .into_any_element(),
            Some(Err(error)) => div()
                .py_4()
                .text_size(px(13.))
                .text_color(p.text)
                .child(error.clone())
                .into_any_element(),
            Some(Ok(plan)) if plan.groups.is_empty() => div()
                .py_6()
                .text_size(px(13.))
                .text_color(p.muted)
                .child(match review.mode {
                    crate::organize::Mode::Kind => "Nothing here to organize by type.",
                    crate::organize::Mode::Date => "No photos or videos here to sort by date.",
                    crate::organize::Mode::Names => {
                        "No camera sequences (three or more) or versions of one name here."
                    }
                })
                .into_any_element(),
            Some(Ok(plan)) => {
                let mut list = div()
                    .id("organize-groups")
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .overflow_y_scroll();
                for (i, group) in plan.groups.iter().enumerate() {
                    let on = review.enabled.get(i).copied().unwrap_or(false);
                    let focused = review.rows.get(i).is_some_and(|f| f.is_focused(window));
                    let tone = if on { p.text } else { p.muted };
                    let mut row = div()
                        .id(("organize-group", i))
                        .role(Role::CheckBox)
                        .aria_label(format!(
                            "{}, {}",
                            group.name,
                            plural(group.items.len(), "file", "files")
                        ))
                        .aria_toggled(if on {
                            gpui::Toggled::True
                        } else {
                            gpui::Toggled::False
                        })
                        .px_3()
                        .py(px(10.))
                        .flex()
                        .items_start()
                        .gap_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(if focused { p.muted } else { rgba(0).into() })
                        .cursor_pointer()
                        .hover(|s| s.bg(p.hover))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.toggle_organize_group(i, cx)),
                        )
                        // The box sits on the group name's line.
                        .child(
                            div().h(px(18.)).flex().items_center().child(
                                Checkbox::new(("organize-check", i))
                                    .checked(on)
                                    .accessibility_label(group.name.clone())
                                    .on_click(cx.listener(move |this, _: &bool, _, cx| {
                                        this.toggle_organize_group(i, cx)
                                    })),
                            ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .flex()
                                        .items_baseline()
                                        .gap_2()
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .truncate()
                                                .text_size(px(13.))
                                                .line_height(px(18.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(tone)
                                                .child(group.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .text_size(px(12.))
                                                .text_color(p.muted)
                                                .child(group.items.len().to_string()),
                                        ),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(12.))
                                        .text_color(p.muted)
                                        .child(group.detail.clone()),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(12.))
                                        .text_color(tone.opacity(0.85))
                                        .child(names(&group.items)),
                                ),
                        );
                    if let Some(focus) = review.rows.get(i) {
                        row = row.track_focus(focus);
                    }
                    list = list.child(row);
                }
                list.into_any_element()
            }
        };
        let unplaced = review
            .plan
            .as_ref()
            .and_then(|plan| plan.as_ref().ok())
            .map_or(0, |plan| plan.unplaced);
        let ready = count > 0 && self.files_pending == 0;
        let button = |id: &'static str, focus: &FocusHandle, primary: bool| {
            let focused = focus.is_focused(window);
            gpui_base::Button::new(id)
                .track_focus(focus)
                .h(px(30.))
                .px_3()
                .rounded(px(8.))
                .border_1()
                .text_size(px(13.))
                .map(|b| {
                    if primary {
                        b.bg(p.text)
                            .text_color(crate::appearance::modal_surface(p))
                            .border_color(if focused { p.accent } else { p.text })
                            .font_weight(FontWeight::MEDIUM)
                            .hover(|s| s.opacity(0.9))
                    } else {
                        b.bg(p.control)
                            .text_color(p.text)
                            .border_color(if focused { p.muted } else { p.line })
                            .hover(|s| s.bg(p.control_hover))
                    }
                })
        };
        let panel = div()
            .id("organize-review")
            .role(Role::Dialog)
            .aria_label(title.clone())
            .occlude()
            .w(px(520.))
            .max_w((window.viewport_size().width - px(40.)).max(px(0.)))
            .max_h((window.viewport_size().height - px(80.)).max(px(0.)))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .rounded(px(14.))
            .border_1()
            .border_color(crate::appearance::modal_border(p))
            .bg(crate::appearance::modal_surface(p))
            .text_color(p.text)
            .child(
                div()
                    .px_1()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child({
                use crate::organize::Mode;
                const MODES: [Mode; 3] = [Mode::Kind, Mode::Date, Mode::Names];
                let folder = review.folder.clone();
                let current = MODES.iter().position(|m| *m == review.mode).unwrap_or(0);
                div().px_1().flex().child(super::settings_view::segmented(
                    "organize-mode",
                    &["By type", "By date taken", "By name"],
                    current,
                    self.segment_motion.get("organize-mode").copied(),
                    self.motion_enabled,
                    p,
                    cx,
                    move |this, index, window, cx| {
                        if index != current {
                            this.slide_segment("organize-mode", current, index);
                            this.open_organize_as(Some(folder.clone()), MODES[index], window, cx);
                        }
                    },
                ))
            })
            .child(div().flex_1().min_h_0().flex().flex_col().child(body))
            .child(
                div()
                    .px_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .flex_shrink_0()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(p.muted)
                            .when(unplaced > 0, |d| {
                                d.child(format!(
                                    "{} stay where they are",
                                    plural(unplaced, "file", "files")
                                ))
                            }),
                    )
                    .child(
                        button("organize-cancel", &review.cancel, false)
                            .accessibility_label("Cancel")
                            .child(interface::button_text("Cancel"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_organize(window, cx)),
                            ),
                    )
                    .child(
                        button("organize-accept", &review.accept, true)
                            .accessibility_label(format!("Organize {count}"))
                            .disabled(!ready)
                            .when(!ready, |b| b.opacity(0.45))
                            .child(interface::button_text(if count == 0 {
                                "Organize".to_string()
                            } else {
                                format!("Organize {count}")
                            }))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.accept_organize(window, cx)),
                            ),
                    ),
            );
        gpui_base::Dialog::new(cx)
            .focus_handle(review.root.clone())
            .close_on_escape(false)
            .close_on_backdrop_press(false)
            .on_ok(|_, _, _| false)
            .backdrop(div().absolute().inset_0().occlude().bg(rgba(0x00000066)))
            .popup(panel)
            .into_any_element()
    }
}

fn interface_path(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_list_names_three_and_counts_the_rest() {
        let items = (0..5)
            .map(|n| crate::organize::Item {
                path: format!(r"C:\x\f{n}.pdf").into(),
                stamp: (0, 0),
            })
            .collect::<Vec<_>>();
        assert_eq!(super::names(&items), "f0.pdf, f1.pdf, f2.pdf and 2 more");
        assert_eq!(super::names(&items[..1]), "f0.pdf");
    }
}
