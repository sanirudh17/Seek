//! Cleanup suggestions on the Storage page (Phase 9.2). Nothing is looked
//! for until Find suggestions is chosen, and nothing moves until a reviewed
//! list is confirmed; removal goes to the Recycle Bin through the journaled
//! file operations, so Ctrl+Z puts it back. Emptying the Recycle Bin is the
//! one permanent action, and it asks first.
use super::*;
use crate::cleanup::{Action, Report, Suggestion};
use gpui_component::checkbox::Checkbox;
use std::sync::{Mutex, atomic::AtomicBool};

#[derive(Default)]
pub(super) struct Cleanup {
    report: Option<Report>,
    /// When the last look finished, for "Checked at".
    checked_at: Option<chrono::DateTime<chrono::Local>>,
    looking: bool,
    /// What is being checked right now.
    stage: Arc<Mutex<String>>,
    /// The suggestion under review.
    open: Option<usize>,
    /// Items the person unticked in the open review.
    unticked: std::collections::HashSet<PathBuf>,
    /// "Clean up all" was pressed once and waits for its confirmation.
    confirm_all: bool,
    /// Items the last move sent to the Recycle Bin (offers Undo, since
    /// Ctrl+Z belongs to the file list).
    moved: Option<usize>,
    cancel: Option<Arc<AtomicBool>>,
    task: Option<Task<()>>,
}

impl Seek {
    fn find_cleanup(&mut self, cx: &mut Context<Self>) {
        let engine = crate::backend::ENGINE.get().cloned();
        if let Some(cancel) = &self.cleanup.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let stage = Arc::new(Mutex::new("Starting".to_string()));
        self.cleanup.cancel = Some(cancel.clone());
        self.cleanup.stage = stage.clone();
        self.cleanup.looking = true;
        self.cleanup.moved = None;
        self.cleanup.open = None;
        self.cleanup.unticked.clear();
        let home = PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default());
        let job = cx.background_executor().spawn(async move {
            let say = |text: &str| {
                if let Ok(mut s) = stage.lock() {
                    *s = text.to_string();
                }
            };
            say("Projects");
            let repositories: Vec<PathBuf> = crate::worktrees::find_on_disk(&home, &cancel)
                .into_iter()
                .filter_map(|g| g.parent().map(std::path::Path::to_path_buf))
                .collect();
            crate::cleanup::suggest(engine.as_ref(), &home, &repositories, &cancel, say)
        });
        self.cleanup.task = Some(cx.spawn(async move |this, cx| {
            let report = job.await;
            let _ = this.update(cx, |this, cx| {
                this.cleanup.report = Some(report);
                this.cleanup.checked_at = Some(chrono::Local::now());
                this.cleanup.looking = false;
                this.cleanup.cancel = None;
                cx.notify();
            });
        }));
        // Redraw the progress line while looking; nothing ticks afterwards.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let looking = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.cleanup.looking
                    })
                    .unwrap_or(false);
                if !looking {
                    return;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn stop_cleanup(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = &self.cleanup.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    fn suggestions(&self) -> &[Suggestion] {
        self.cleanup
            .report
            .as_ref()
            .map_or(&[], |r| r.suggestions.as_slice())
    }

    fn recycle_suggestion(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(suggestion) = self.suggestions().get(index) else {
            return;
        };
        let paths: Vec<PathBuf> = suggestion
            .items
            .iter()
            .map(|(p, _)| p.clone())
            .filter(|p| !self.cleanup.unticked.contains(p))
            .collect();
        if paths.is_empty() {
            return;
        }
        let count = paths.len();
        self.run_file_op(crate::file_ops::Operation::Recycle { paths }, window, cx);
        if self.message.is_none() {
            self.cleanup.moved = Some(count);
        }
        if self.message.is_none()
            && let Some(report) = &mut self.cleanup.report
        {
            report.suggestions.remove(index);
            self.cleanup.open = None;
            self.cleanup.unticked.clear();
            self.refresh_recycle_bin(cx);
        }
        cx.notify();
    }

    /// Moves every suggestion's items to the Recycle Bin in one undoable
    /// operation (the Recycle Bin itself is left to its own button).
    fn recycle_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cleanup.confirm_all = false;
        let paths: Vec<PathBuf> = self
            .suggestions()
            .iter()
            .filter(|s| s.action == Action::Recycle)
            .flat_map(|s| s.items.iter().map(|(p, _)| p.clone()))
            .collect();
        if paths.is_empty() {
            return;
        }
        let count = paths.len();
        self.run_file_op(crate::file_ops::Operation::Recycle { paths }, window, cx);
        if self.message.is_none() {
            self.cleanup.moved = Some(count);
            if let Some(report) = &mut self.cleanup.report {
                report.suggestions.retain(|s| s.action != Action::Recycle);
            }
            self.cleanup.open = None;
            self.refresh_recycle_bin(cx);
        }
        cx.notify();
    }

    /// The Recycle Bin's size once a move has had time to land, so the
    /// space waiting there shows with its Empty button.
    fn refresh_recycle_bin(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            for _ in 0..20 {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(1500))
                    .await;
                let busy = this.update(cx, |this, _| this.files_pending != 0).unwrap_or(false);
                if !busy {
                    break;
                }
            }
            let bytes = cx
                .background_executor()
                .spawn(async { crate::cleanup::recycle_bin_bytes() })
                .await
                .unwrap_or(0);
            let _ = this.update(cx, |this, cx| {
                if let Some(report) = &mut this.cleanup.report {
                    report.suggestions.retain(|s| s.action != Action::EmptyRecycleBin);
                    if bytes > 0 {
                        report.suggestions.push(Suggestion {
                            title: "Recycle Bin".into(),
                            detail: "Everything already moved to the Recycle Bin on all drives; the space is freed when it is emptied".into(),
                            note: None,
                            action: Action::EmptyRecycleBin,
                            items: vec![],
                            total: Some(bytes),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks before emptying the Recycle Bin, in Seek's own dialog.
    fn ask_empty_recycle_bin(&mut self, bytes: u64, window: &mut Window, cx: &mut Context<Self>) {
        let cancel = cx.focus_handle();
        let delete = cx.focus_handle();
        self.permanent_delete = Some(super::permanent_delete::Confirmation {
            paths: vec![],
            empty_recycle_bin: Some(bytes),
            root: cx.focus_handle(),
            cancel,
            delete: delete.clone(),
            restore: window.focused(cx),
        });
        window.focus(&delete, cx);
        cx.notify();
    }

    /// Called from the confirmation once accepted.
    pub(super) fn empty_recycle_bin(&mut self, cx: &mut Context<Self>) {
        let job = cx
            .background_executor()
            .spawn(async move { crate::cleanup::empty_recycle_bin() });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        if let Some(report) = &mut this.cleanup.report {
                            report
                                .suggestions
                                .retain(|s| s.action != Action::EmptyRecycleBin);
                        }
                        this.cleanup.open = None;
                        this.message = Some("Emptied the Recycle Bin.".into());
                    }
                    Err(error) => {
                        this.message = Some(format!("Couldn't empty the Recycle Bin: {error}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn cleanup_section(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        use super::storage_view::human;
        use crate::tokens::*;
        let state = &self.cleanup;
        let action = if state.looking {
            super::settings_view::control("stop-cleanup", "Stop", p, cx)
                .h(BUTTON_HEIGHT)
                .on_click(cx.listener(|this, _, _, cx| this.stop_cleanup(cx)))
        } else {
            super::settings_view::control(
                "find-cleanup",
                if state.report.is_some() {
                    "Look again"
                } else {
                    "Find suggestions"
                },
                p,
                cx,
            )
            .h(BUTTON_HEIGHT)
            .on_click(cx.listener(|this, _, _, cx| this.find_cleanup(cx)))
        };
        let status = if state.looking {
            let stage = state.stage.lock().map(|s| s.clone()).unwrap_or_default();
            Some(format!("Checking {}…", stage.to_lowercase()))
        } else {
            state.report.as_ref().map(|report| {
                let total: u64 = report.suggestions.iter().map(Suggestion::bytes).sum();
                let when = format!(
                    "{} in {:.1} s",
                    state
                        .checked_at
                        .map(|t| t.format("%H:%M").to_string())
                        .unwrap_or_default(),
                    report.elapsed.as_secs_f32()
                );
                let mut line = if report.suggestions.is_empty() {
                    format!("Checked at {when}")
                } else {
                    format!("{} to free · checked at {when}", human(total))
                };
                if let Some(why) = &report.stopped {
                    line = format!("{why} · {line}");
                }
                line
            })
        };
        let movable: Vec<&Suggestion> = self
            .suggestions()
            .iter()
            .filter(|s| s.action == Action::Recycle)
            .collect();
        let all_items: usize = movable.iter().map(|s| s.items.len()).sum();
        let all_bytes: u64 = movable.iter().map(|s| s.bytes()).sum();
        let clean_all = (!state.looking && all_items > 0).then(|| {
            super::settings_view::control("cleanup-all", "Clean up all", p, cx)
                .h(BUTTON_HEIGHT)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.cleanup.confirm_all = !this.cleanup.confirm_all;
                    cx.notify();
                }))
        });
        let action = div().flex().gap_2().children(clean_all).child(action);
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(TEXT_BODY)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Cleanup suggestions"),
                    )
                    .children(status.map(|s| {
                        div()
                            .truncate()
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child(s)
                    })),
            )
            .child(action);
        let mut card = div()
            .flex()
            .flex_col()
            .rounded(RADIUS_CARD)
            .border_1()
            .border_color(p.text.opacity(0.1))
            .bg(p.settings_card);
        let row = |id: ElementId| {
            div()
                .id(id)
                .min_h(px(56.))
                .px_4()
                .py_2()
                .flex()
                .items_center()
                .gap_3()
        };
        let note = |text: &'static str| {
            div()
                .px_4()
                .py_5()
                .text_size(TEXT_SMALL)
                .text_color(p.muted)
                .child(text)
        };
        if let Some(count) = state.moved {
            card = card.child(
                div()
                    .px_4()
                    .py_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .flex_1()
                            .text_size(TEXT_SMALL)
                            .child(format!("Moved {count} to the Recycle Bin. Look again to see what's left.")),
                    )
                    .child(
                        super::settings_view::control("cleanup-undo", "Undo", p, cx)
                            .h(BUTTON_HEIGHT)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cleanup.moved = None;
                                this.file_action(super::daily::Action::UndoRename, None, window, cx);
                                cx.notify();
                            })),
                    ),
            );
        }
        if state.confirm_all && all_items > 0 {
            card = card.child(
                div()
                    .px_4()
                    .py_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(p.line)
                    .bg(p.text.opacity(0.03))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().text_size(TEXT_BODY).child(format!(
                                "Move all {all_items} items ({}) to the Recycle Bin?",
                                human(all_bytes)
                            )))
                            .child(
                                div()
                                    .text_size(TEXT_SMALL)
                                    .text_color(p.muted)
                                    .child("Files apps have open are skipped. Ctrl+Z puts everything back; the space is freed when you empty the Recycle Bin."),
                            ),
                    )
                    .child(
                        super::settings_view::control("cleanup-all-cancel", "Cancel", p, cx)
                            .h(BUTTON_HEIGHT)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cleanup.confirm_all = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("cleanup-all-confirm")
                            .h(BUTTON_HEIGHT)
                            .px_3()
                            .rounded(RADIUS_CONTROL)
                            .bg(p.text)
                            .text_color(crate::appearance::modal_surface(p))
                            .font_weight(FontWeight::MEDIUM)
                            .disabled(self.files_pending != 0)
                            .child(super::interface::button_text("Move all"))
                            .on_click(cx.listener(|this, _, window, cx| this.recycle_all(window, cx))),
                    ),
            );
        }
        let list = self.suggestions();
        if state.report.is_none() && !state.looking {
            card = card.child(note(
                "Seek looks through developer, app and browser caches, shader caches, crash dumps, old temporary files and installers, build folders in projects you haven't touched for months, and the Recycle Bin. Nothing is removed until you review it, and everything but the Recycle Bin can be put back with Ctrl+Z.",
            ));
        } else if state.looking && list.is_empty() {
            card = card.child(note("Looking…"));
        } else if list.is_empty() {
            card = card.child(note("Nothing worth cleaning up right now."));
        }
        for (index, suggestion) in list.iter().enumerate() {
            let open = state.open == Some(index);
            let empties = suggestion.action == Action::EmptyRecycleBin;
            let count = suggestion.items.len();
            let mut text = div()
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
                        .child(div().text_size(TEXT_BODY).child(suggestion.title.clone()))
                        .when(count > 0, |d| {
                            d.child(
                                div()
                                    .text_size(TEXT_SMALL)
                                    .text_color(p.muted)
                                    .child(format!(
                                        "{count} {}",
                                        if count == 1 { "item" } else { "items" }
                                    )),
                            )
                        }),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(TEXT_SMALL)
                        .text_color(p.muted)
                        .child(suggestion.detail.clone()),
                );
            if let Some(n) = &suggestion.note {
                text = text.child(
                    div()
                        .truncate()
                        .text_size(TEXT_SMALL)
                        .text_color(p.muted)
                        .child(n.clone()),
                );
            }
            let bytes = suggestion.bytes();
            let mut line = row(("cleanup", index).into())
                .when(index > 0, |d| d.border_t_1().border_color(p.line))
                .child(text)
                .child(
                    div()
                        .font_family(FONT_MONO)
                        .text_size(TEXT_SMALL)
                        .text_color(p.muted)
                        .child(human(bytes)),
                );
            line = if empties {
                line.child(
                    super::settings_view::control("open-recycle-bin", "Open", p, cx)
                        .h(BUTTON_HEIGHT)
                        .on_click(|_, _, cx| cx.open_url("shell:RecycleBinFolder")),
                )
                .child(
                    super::settings_view::control("empty-recycle-bin", "Empty…", p, cx)
                        .h(BUTTON_HEIGHT)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.ask_empty_recycle_bin(bytes, window, cx)
                        })),
                )
            } else {
                line.cursor_pointer()
                    .hover(move |s| s.bg(p.hover))
                    .child(
                        Icon::new(if open {
                            gpui_component_assets::IconName::ChevronUp
                        } else {
                            gpui_component_assets::IconName::ChevronDown
                        })
                        .size(px(14.))
                        .text_color(p.muted),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.cleanup.open = if this.cleanup.open == Some(index) {
                            None
                        } else {
                            Some(index)
                        };
                        this.cleanup.unticked.clear();
                        cx.notify();
                    }))
            };
            card = card.child(line);
            if open && !empties {
                let mut review = div()
                    .id(("cleanup-review", index))
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .px_4()
                    .flex()
                    .flex_col();
                let mut chosen = 0u64;
                let mut count = 0usize;
                for (item, (path, bytes)) in suggestion.items.iter().enumerate() {
                    let ticked = !state.unticked.contains(path);
                    if ticked {
                        chosen += bytes;
                        count += 1;
                    }
                    let toggle = path.clone();
                    review = review.child(
                        div()
                            .h(px(30.))
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                Checkbox::new(("cleanup-item", index * 10_000 + item))
                                    .checked(ticked)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !this.cleanup.unticked.remove(&toggle) {
                                            this.cleanup.unticked.insert(toggle.clone());
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(TEXT_SMALL)
                                    .child(path.to_string_lossy().into_owned()),
                            )
                            .child(
                                div()
                                    .font_family(FONT_MONO)
                                    .text_size(TEXT_SMALL)
                                    .text_color(p.muted)
                                    .child(human(*bytes)),
                            ),
                    );
                }
                card = card.child(review).child(
                    div()
                        .px_4()
                        .py_3()
                        .flex()
                        .items_center()
                        .gap_3()
                        .border_t_1()
                        .border_color(p.line)
                        .child(
                            div()
                                .flex_1()
                                .text_size(TEXT_SMALL)
                                .text_color(p.muted)
                                .child("Goes to the Recycle Bin · Ctrl+Z puts it back"),
                        )
                        .child(
                            Button::new(("cleanup-recycle", index))
                                .h(BUTTON_HEIGHT)
                                .px_3()
                                .rounded(RADIUS_CONTROL)
                                .bg(p.text)
                                .text_color(crate::appearance::modal_surface(p))
                                .font_weight(FontWeight::MEDIUM)
                                .disabled(count == 0 || self.files_pending != 0)
                                .child(super::interface::button_text(format!(
                                    "Move {count} to Recycle Bin · {}",
                                    human(chosen)
                                )))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.recycle_suggestion(index, window, cx)
                                })),
                        ),
                );
            }
        }
        // What Windows itself can clean (needs its own permissions).
        if state.report.is_some() {
            card = card.child(
                row("cleanup-windows".into())
                    .border_t_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().text_size(TEXT_BODY).child("Windows updates and system files"))
                            .child(
                                div()
                                    .text_size(TEXT_SMALL)
                                    .text_color(p.muted)
                                    .child("Windows cleans these itself in Storage settings; Seek doesn't touch system folders"),
                            ),
                    )
                    .child(
                        super::settings_view::control("open-storage-sense", "Open", p, cx)
                            .h(BUTTON_HEIGHT)
                            .on_click(|_, _, cx| cx.open_url("ms-settings:storagepolicies")),
                    ),
            );
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().px_1().child(header))
                .child(card)
                .into_any_element(),
        )
    }
}
