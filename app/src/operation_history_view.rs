use super::*;
impl Seek {
    fn archive_operation_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.files_pending != 0
            || self.history_loading
            || !self
                .operation_history
                .as_ref()
                .is_ok_and(|snapshot| !snapshot.jobs.is_empty())
        {
            return;
        }
        self.history_focus.focus(window, cx);
        self.run_file_op(crate::file_ops::Operation::ArchiveHistory, window, cx);
    }
    pub(super) fn can_put_back(&self) -> bool {
        self.files_pending == 0
            && self
                .operation_history
                .as_ref()
                .ok()
                .and_then(|history| history.latest_put_back())
                .is_some()
    }
    pub(super) fn put_back_job(&mut self, job: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.files_pending == 0
            && self
                .operation_history
                .as_ref()
                .ok()
                .and_then(|history| history.put_back_plan(job))
                .is_some()
        {
            if self.history_open {
                self.history_focus.focus(window, cx);
            } else {
                self.table.read(cx).focus_handle(cx).focus(window, cx);
            }
            self.run_file_op(crate::file_ops::Operation::PutBack { job }, window, cx);
        }
    }
    pub(super) fn can_reverse_rename(&self, redo: bool) -> bool {
        self.files_pending == 0
            && self.operation_history.as_ref().ok().is_some_and(|history| {
                history.rename_plan(redo, None).is_some()
                    || history.batch_rename_plan(redo, None).is_some()
                    || history.move_plans(redo, None).is_some()
                    || (!redo && history.recycle_undo_job().is_some())
                    || (!redo && history.copy_undo_plan(None).is_some())
                    || (redo
                        && (history.copy_redo_job(None).is_some()
                            || history.recycle_redo_plan(None).is_some()))
            })
    }
    pub(super) fn reverse_file_operation(
        &mut self,
        redo: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_reverse_rename(redo) {
            return;
        }
        let history = self.operation_history.as_ref().ok();
        if history.is_some_and(|history| history.batch_rename_plan(redo, None).is_some()) {
            self.run_file_op(
                crate::file_ops::Operation::UndoBatchRename { redo },
                window,
                cx,
            );
            return;
        }
        if (!redo && history.is_some_and(|history| history.copy_undo_plan(None).is_some()))
            || (redo && history.is_some_and(|history| history.copy_redo_job(None).is_some()))
        {
            self.run_file_op(crate::file_ops::Operation::UndoCopy { redo }, window, cx);
            return;
        }
        if redo && history.is_some_and(|history| history.recycle_redo_plan(None).is_some()) {
            self.run_file_op(crate::file_ops::Operation::RedoRecycle, window, cx);
            return;
        }
        if !redo
            && let Some(job) = self
                .operation_history
                .as_ref()
                .ok()
                .and_then(|history| history.recycle_undo_job())
        {
            self.put_back_job(job, window, cx);
            return;
        }
        let moving = self
            .operation_history
            .as_ref()
            .ok()
            .is_some_and(|history| history.move_plans(redo, None).is_some());
        self.run_file_op(
            if moving {
                crate::file_ops::Operation::UndoMove { redo }
            } else {
                crate::file_ops::Operation::UndoRename { redo }
            },
            window,
            cx,
        );
    }

    pub(super) fn open_operation_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview_enlarged || self.edit.is_some() || self.permanent_delete.is_some() {
            return;
        }
        // A menu's transient focus handle disappears as its item callback opens us.
        // Restore a stable browser control rather than that disposed menu handle.
        let input_focus = self.input.read(cx).focus_handle(cx);
        self.history_previous = Some(if input_focus.contains_focused(window, cx) {
            input_focus
        } else {
            self.table.read(cx).focus_handle(cx)
        });
        self.history_open = true;
        self.history_loading = true;
        self.history_focus.focus(window, cx);
        self.run_file_op(crate::file_ops::Operation::History, window, cx);
        cx.notify();
    }
    pub(super) fn close_operation_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history_open = false;
        if let Some(focus) = self.history_previous.take() {
            focus.focus(window, cx);
        } else {
            self.table.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }
    pub(super) fn operation_history_keys(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.keystroke.key.as_str() {
            "escape" => {
                self.close_operation_history(window, cx);
                cx.stop_propagation();
            }
            "tab" => {
                for _ in 0..128 {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    if self.history_focus.contains_focused(window, cx) {
                        break;
                    }
                }
                if !self.history_focus.contains_focused(window, cx) {
                    self.history_focus.focus(window, cx);
                }
                cx.stop_propagation();
            }
            "enter" | "space" if self.history_focus.is_focused(window) => {
                self.close_operation_history(window, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }
    pub(super) fn operation_history_view(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut rows = div()
            .id("operation-history-rows")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_4()
            .py_2();
        if self.history_loading {
            rows = rows.child("Loading operation history...");
        } else {
            match &self.operation_history {
                Err(error) => {
                    rows = rows.child(div().text_color(p.text).child(error.clone()));
                }
                Ok(snapshot) => {
                    if let Some(warning) = &snapshot.warning {
                        rows = rows.child(div().mb_3().text_color(p.text).child(warning.clone()));
                    }
                    if let Some(archive) = &snapshot.archived_file {
                        let archive = archive.clone();
                        rows = rows.child(
                            div().mb_3().flex().child(
                                super::settings_view::control(
                                    "reveal-history-archive",
                                    "Reveal previous history in Explorer",
                                    p,
                                    cx,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, _| {
                                        let _ = this
                                            .backend
                                            .tx
                                            .send(Command::Open(archive.clone(), true));
                                    },
                                )),
                            ),
                        );
                    }
                    if snapshot.jobs.is_empty() {
                        rows = rows
                            .justify_center()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("No file operations yet"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(p.muted)
                                    .child("Renames, moves and copies will appear here."),
                            );
                    }
                    for job in snapshot.jobs.iter().rev().take(100) {
                        let completed = job
                            .outcomes
                            .iter()
                            .filter(|event| {
                                matches!(
                                    event,
                                    crate::operation_journal::Event::After { hresult, .. } if crate::operation_journal::item_completed(*hresult)
                                )
                            })
                            .count();
                        let skipped=job.outcomes.iter().filter(|event|matches!(event,crate::operation_journal::Event::After{hresult,..} if !crate::operation_journal::item_completed(*hresult))).count();
                        let status = if !job.finished {
                            "Interrupted - review files before retrying"
                        } else if job.error.is_some() && completed == 0 {
                            if job.label == "Rename" {
                                "Not renamed"
                            } else if job.label == "Undo rename" || job.label == "Undo move" {
                                "Not undone"
                            } else {
                                "Not completed"
                            }
                        } else if job.error.is_some() {
                            "Partially completed"
                        } else if skipped != 0 {
                            "Completed with skipped or failed items"
                        } else {
                            "Completed"
                        };
                        let mut row = div()
                            .id(("operation-job", job.id as usize))
                            .role(Role::Group)
                            .aria_label(format!("{}: {status}. {completed} completed item results, {skipped} other results",job.label))
                            .py_3()
                            .border_b_1()
                            .border_color(p.line)
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .id(("operation-job-title",job.id as usize))
                                    .font_weight(FontWeight::MEDIUM)
                                    .role(Role::Label)
                                    .aria_label(format!("{} - {}",job.label,status))
                                    .child(format!("{} - {}", job.label, status)),
                            )
                            .child(div().text_size(px(12.)).text_color(p.muted).child(format!(
                                "{completed} completed item results, {skipped} other results"
                            )));
                        if let Some(error) = &job.error {
                            row = row.child(div().text_size(px(12.)).child(error.clone()));
                        }
                        for note in &job.review {
                            row = row.child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(p.muted)
                                    .child(note.clone()),
                            );
                        }
                        if let Some(plan) = snapshot.put_back_plan(job.id) {
                            let id = job.id;
                            row = row.child(
                                div().flex().child(
                                    super::settings_view::control(
                                        ("put-back-job", job.id as usize),
                                        format!(
                                            "Put Back {} {}",
                                            plan.len(),
                                            if plan.len() == 1 { "item" } else { "items" }
                                        ),
                                        p,
                                        cx,
                                    )
                                    .disabled(self.files_pending != 0)
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.put_back_job(id, window, cx)
                                        },
                                    )),
                                ),
                            );
                        }
                        for event in job
                            .outcomes
                            .iter()
                            .filter(|event| {
                                matches!(event, crate::operation_journal::Event::After { .. })
                            })
                            .take(20)
                        {
                            if let crate::operation_journal::Event::After {
                                action,
                                source,
                                target,
                                hresult,
                                ..
                            } = event
                            {
                                let display = |path: &crate::operation_journal::WirePath| {
                                    path.path()
                                        .to_string_lossy()
                                        .trim_start_matches(r"\\?\")
                                        .to_owned()
                                };
                                let paths = match (source, target) {
                                    (Some(from), Some(to)) => {
                                        format!("{} -> {}", display(from), display(to))
                                    }
                                    (Some(from), None) => display(from),
                                    (None, Some(to)) => display(to),
                                    _ => "Windows did not provide an item path".into(),
                                };
                                row = row.child(
                                    div().text_size(px(12.)).text_color(p.muted).child(format!(
                                        "{action}: {paths}{}",
                                        if crate::operation_journal::item_completed(*hresult) {
                                            String::new()
                                        } else {
                                            format!(" (Windows result 0x{:08X})", *hresult as u32)
                                        }
                                    )),
                                );
                            }
                        }
                        rows = rows.child(row);
                    }
                }
            }
        }
        let compact = |id: &'static str, label: &'static str| {
            super::settings_view::control(id, label, p, cx)
                .h(px(28.))
                .with_size(px(28.))
                .px_2()
                .text_size(px(12.))
                .flex_shrink_0()
        };
        let interrupted = self
            .operation_history
            .as_ref()
            .map_or(0, |history| history.interrupted());
        let usage = self
            .operation_history
            .as_ref()
            .map(|history| format!("{:.1} / 32 MiB", history.bytes_used as f64 / 1_048_576.))
            .unwrap_or_else(|_| "History unavailable".into());
        let archive_enabled = self.files_pending == 0
            && !self.history_loading
            && self
                .operation_history
                .as_ref()
                .is_ok_and(|snapshot| !snapshot.jobs.is_empty());
        div().id("operation-history-overlay").absolute().inset_0().occlude().flex().items_center().justify_center().p_6().bg(rgba(0x00000055))
            .child(div().id("operation-history").role(Role::Dialog).aria_label("Operation history").track_focus(&self.history_focus)
                .w(px(760.)).max_w(relative(1.)).h(px(480.)).max_h(relative(0.88)).rounded(px(12.))
                .bg(crate::appearance::modal_surface(p)).border_1().border_color(crate::appearance::modal_border(p)).flex().flex_col().overflow_hidden()
                .child(div().px_4().py_3().flex().items_center().gap_2().border_b_1().border_color(p.line)
                    .child(div().flex_1().min_w_0().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).child("Operation history"))
                    .child(compact("undo-history-rename", "Undo").tooltip("Undo the latest eligible operation").disabled(!self.can_reverse_rename(false))
                        .on_click(cx.listener(|this, _, window, cx| this.reverse_file_operation(false, window, cx))))
                    .child(compact("redo-history-rename", "Redo").tooltip("Redo the latest eligible operation").disabled(!self.can_reverse_rename(true))
                        .on_click(cx.listener(|this, _, window, cx| this.reverse_file_operation(true, window, cx))))
                    .child(compact("close-operation-history", "Done").on_click(cx.listener(|this, _, window, cx| this.close_operation_history(window, cx)))))
                .child(rows)
                .child(div().px_4().py_3().border_t_1().border_color(p.line).flex().items_center().gap_2()
                    .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(p.muted).child(usage))
                    .when(self.files_pending != 0, |footer| footer.child(compact("stop-file-operations", "Stop")
                        .on_click(cx.listener(|this, _, window, cx| { this.history_focus.focus(window, cx); this.cancel_file_operations(cx); }))))
                    .when(interrupted != 0, |footer| footer.child(compact("review-interrupted-operations", "Review")
                        .tooltip("Inspect interrupted results without replaying operations").disabled(self.files_pending != 0)
                        .on_click(cx.listener(|this, _, window, cx| { this.history_focus.focus(window, cx); this.run_file_op(crate::file_ops::Operation::ReviewHistory, window, cx); }))))
                    .child(compact("archive-operation-history", "Archive history").disabled(!archive_enabled)
                        .tooltip("Preserve the journal as a file and start fresh Undo and Put Back records")
                        .on_click(cx.listener(|this, _, window, cx| this.archive_operation_history(window, cx)))))
                .child(div().px_4().pb_3().text_size(px(11.)).text_color(p.muted).child("Interrupted work is never replayed automatically. Esc: close.")))
            .into_any_element()
    }
}
