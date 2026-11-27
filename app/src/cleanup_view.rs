//! Cleanup suggestions on the Storage page (Phase 9.2). Nothing is looked
//! for until Find suggestions is chosen, and nothing moves until a reviewed
//! list is confirmed. Caches, temporary files and build folders are deleted
//! in place (they rebuild on their own); models, unfinished downloads and old
//! installers go to the Recycle Bin through the journaled file operations, so
//! Ctrl+Z puts them back.
use super::*;
use crate::cleanup::{Action, Report, Suggestion};
use gpui_component::checkbox::Checkbox;
use std::sync::{Mutex, atomic::AtomicBool};

/// Formats the last Cleanup check time with day context so overnight or
/// multi-day sessions always show when suggestions were gathered.
fn format_checked_at(
    at: chrono::DateTime<chrono::Local>,
    now: chrono::DateTime<chrono::Local>,
) -> String {
    use chrono::Datelike;
    let time = at.format("%H:%M");
    let at_date = at.date_naive();
    let now_date = now.date_naive();
    if at_date == now_date {
        format!("today at {time}")
    } else if at_date + chrono::Days::new(1) == now_date {
        format!("yesterday at {time}")
    } else {
        format!("{} {} at {time}", at.format("%b"), at.day())
    }
}

#[derive(Default)]
pub(super) struct Cleanup {
    report: Option<Report>,
    /// When the last look finished, for "Checked today at HH:MM".
    checked_at: Option<chrono::DateTime<chrono::Local>>,
    /// Monotonic timestamp of the last completed look, for staleness checks.
    checked_instant: Option<Instant>,
    /// True once items have been deleted or recycled since the last look, so
    /// returning to Storage refreshes Cleanup suggestions automatically.
    dirty_since_look: bool,
    looking: bool,
    /// What is being checked right now.
    stage: Arc<Mutex<String>>,
    /// The suggestion under review.
    open: Option<usize>,
    /// Items the person unticked in the open review.
    unticked: std::collections::HashSet<PathBuf>,
    /// Groups the person unticked (by title): left out of Clean up.
    skipped_groups: std::collections::HashSet<String>,
    /// A deletion running now, or the last one's result.
    removal: Option<Removal>,
    /// "Clean up all" was pressed once and waits for its confirmation.
    confirm_all: bool,
    /// Items the last move sent to the Recycle Bin (offers Undo, since
    /// Ctrl+Z belongs to the file list).
    moved: Option<usize>,
    cancel: Option<Arc<AtomicBool>>,
    task: Option<Task<()>>,
}

/// A deletion of cache folders (or emptying the Recycle Bin) running in the background.
pub(super) struct Removal {
    progress: Arc<crate::cleanup::Progress>,
    cancel: Arc<AtomicBool>,
    running: bool,
    stopped: bool,
    recycle_bin: bool,
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
        self.cleanup.confirm_all = false;
        self.cleanup.moved = None;
        self.cleanup.open = None;
        self.cleanup.unticked.clear();
        self.cleanup.skipped_groups.clear();
        if self.cleanup.removal.as_ref().is_some_and(|r| !r.running) {
            self.cleanup.removal = None;
        }
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
                this.cleanup.checked_instant = Some(Instant::now());
                this.cleanup.dirty_since_look = false;
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

    /// The ticked items of one suggestion.
    fn ticked(&self, suggestion: &Suggestion) -> Vec<PathBuf> {
        suggestion
            .items
            .iter()
            .map(|(p, _)| p.clone())
            .filter(|p| !self.cleanup.unticked.contains(p))
            .collect()
    }

    fn clean_suggestion(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(suggestion) = self.suggestions().get(index) else {
            return;
        };
        let paths = self.ticked(suggestion);
        let (delete, recycle) = if suggestion.action == Action::Delete {
            (paths, vec![])
        } else {
            (vec![], paths)
        };
        let title = suggestion.title.clone();
        self.clean(delete, recycle, window, cx);
        if let Some(report) = &mut self.cleanup.report {
            report.suggestions.retain(|s| s.title != title);
        }
        self.cleanup.open = None;
        self.cleanup.unticked.clear();
        cx.notify();
    }

    /// Every ticked group's ticked items: caches deleted, the person's own
    /// files to the Recycle Bin (the Recycle Bin itself has its own button).
    fn clean_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cleanup.confirm_all = false;
        let mut delete = vec![];
        let mut recycle = vec![];
        let mut done = vec![];
        for suggestion in self.suggestions() {
            if suggestion.action == Action::EmptyRecycleBin
                || self.cleanup.skipped_groups.contains(&suggestion.title)
            {
                continue;
            }
            let paths = self.ticked(suggestion);
            done.push(suggestion.title.clone());
            if suggestion.action == Action::Delete {
                delete.extend(paths);
            } else {
                recycle.extend(paths);
            }
        }
        self.clean(delete, recycle, window, cx);
        if let Some(report) = &mut self.cleanup.report {
            report.suggestions.retain(|s| !done.contains(&s.title));
        }
        self.cleanup.open = None;
        self.cleanup.unticked.clear();
        cx.notify();
    }

    fn clean(&mut self, delete: Vec<PathBuf>, recycle: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        if !recycle.is_empty() {
            let count = recycle.len();
            let pending_before = self.files_pending;
            self.run_file_op(crate::file_ops::Operation::Recycle { paths: recycle }, window, cx);
            if self.files_pending > pending_before {
                self.cleanup.moved = Some(count);
                self.cleanup.dirty_since_look = true;
                // Clear any stale finished "Emptied Recycle Bin" notice now that
                // new items are moving into the Recycle Bin.
                if self
                    .cleanup
                    .removal
                    .as_ref()
                    .is_some_and(|r| !r.running && r.recycle_bin)
                {
                    self.cleanup.removal = None;
                }
                self.refresh_recycle_bin(cx);
            }
        }
        if delete.is_empty() || self.cleanup.removal.as_ref().is_some_and(|r| r.running) {
            return;
        }
        self.cleanup.dirty_since_look = true;
        let progress = Arc::new(crate::cleanup::Progress::default());
        let cancel = Arc::new(AtomicBool::new(false));
        self.cleanup.removal = Some(Removal {
            progress: progress.clone(),
            cancel: cancel.clone(),
            running: true,
            stopped: false,
            recycle_bin: false,
        });
        let job = cx.background_executor().spawn({
            let progress = progress.clone();
            async move { crate::cleanup::delete(&delete, &cancel, &progress) }
        });
        cx.spawn(async move |this, cx| {
            job.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(removal) = &mut this.cleanup.removal {
                    removal.running = false;
                }
                // Storage's numbers and drive capacity include what was just freed.
                this.refresh_capacity(cx);
                if let Some(page) = &this.storage_page {
                    let scope = page.scope.clone();
                    this.measure_storage_again(scope, cx);
                }
                cx.notify();
            });
        })
        .detach();
        // Redraw the progress about five times a second while it runs.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(200)).await;
                let running = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.cleanup.removal.as_ref().is_some_and(|r| r.running)
                    })
                    .unwrap_or(false);
                if !running {
                    return;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn stop_removal(&mut self, cx: &mut Context<Self>) {
        if let Some(removal) = &mut self.cleanup.removal {
            removal.cancel.store(true, Ordering::Relaxed);
            removal.stopped = true;
        }
        cx.notify();
    }

    /// Called whenever the user navigates back to Storage so Cleanup
    /// suggestions never stay frozen with stale results or old finished
    /// deletion banners across tab switches or overnight sessions.
    pub(super) fn sync_cleanup_on_storage_open(&mut self, cx: &mut Context<Self>) {
        // Clear finished one-shot result banners from an earlier Storage visit
        // (never interrupt an in-flight deletion or Recycle Bin empty).
        if self.cleanup.removal.as_ref().is_some_and(|r| !r.running) {
            self.cleanup.removal = None;
        }
        self.cleanup.moved = None;
        if self.cleanup.looking || self.cleanup.removal.as_ref().is_some_and(|r| r.running) {
            return;
        }
        let Some(report) = &self.cleanup.report else {
            return;
        };
        let now_local = chrono::Local::now();
        let day_changed = self
            .cleanup
            .checked_at
            .is_some_and(|at| at.date_naive() != now_local.date_naive());
        let age = self
            .cleanup
            .checked_instant
            .map_or(std::time::Duration::MAX, |t| t.elapsed());
        let stale = self.cleanup.dirty_since_look
            || day_changed
            || age >= std::time::Duration::from_secs(300)
            || (report.suggestions.is_empty() && age >= std::time::Duration::from_secs(30));
        if stale {
            self.find_cleanup(cx);
        } else {
            // Fast (<5ms) Recycle Bin sync so items deleted in other tabs or
            // Explorer show up immediately without waiting for a full cache scan.
            self.refresh_recycle_bin_now(cx);
        }
    }

    /// Immediate single-shot Recycle Bin size check used when reopening Storage.
    fn refresh_recycle_bin_now(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let bytes = cx
                .background_executor()
                .spawn(async { crate::cleanup::recycle_bin_bytes() })
                .await
                .unwrap_or(0);
            let _ = this.update(cx, |this, cx| {
                if let Some(report) = &mut this.cleanup.report {
                    let prev = report
                        .suggestions
                        .iter()
                        .find(|s| s.action == Action::EmptyRecycleBin)
                        .map(Suggestion::bytes)
                        .unwrap_or(0);
                    if prev != bytes {
                        report
                            .suggestions
                            .retain(|s| s.action != Action::EmptyRecycleBin);
                        if bytes > 0 {
                            report.suggestions.push(Suggestion {
                                title: "Recycle Bin".into(),
                                detail: "Everything already moved to the Recycle Bin on all drives; the space is freed when it is emptied".into(),
                                note: None,
                                action: Action::EmptyRecycleBin,
                                items: vec![],
                                total: Some(bytes),
                            });
                            report
                                .suggestions
                                .sort_by_key(|s| std::cmp::Reverse(s.bytes()));
                        }
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// The Recycle Bin's size once a move has had time to land, so the
    /// space waiting there shows with its Empty button.
    pub(super) fn refresh_recycle_bin(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            // Wait quickly for the queued Recycle/Undo file op to finish (up to 30s).
            for _ in 0..120 {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(250))
                    .await;
                let busy = this
                    .update(cx, |this, _| this.files_pending != 0)
                    .unwrap_or(false);
                if !busy {
                    break;
                }
            }
            // Brief settle so Windows finishes flushing `$I*` metadata, with one retry if 0.
            let mut bytes = 0u64;
            for wait_ms in [250u64, 750u64] {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(wait_ms))
                    .await;
                bytes = cx
                    .background_executor()
                    .spawn(async { crate::cleanup::recycle_bin_bytes() })
                    .await
                    .unwrap_or(0);
                if bytes > 0 {
                    break;
                }
            }
            let _ = this.update(cx, |this, cx| {
                if bytes > 0
                    && this
                        .cleanup
                        .removal
                        .as_ref()
                        .is_some_and(|r| !r.running && r.recycle_bin)
                {
                    this.cleanup.removal = None;
                }
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
                        report
                            .suggestions
                            .sort_by_key(|s| std::cmp::Reverse(s.bytes()));
                    }
                }
                this.refresh_capacity(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks before emptying the Recycle Bin, in Seek's own dialog.
    fn ask_empty_recycle_bin(&mut self, bytes: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.cleanup.removal.as_ref().is_some_and(|r| r.running) {
            return;
        }
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

    /// Called from the confirmation once accepted. Shows immediate progress,
    /// prevents duplicate concurrent runs, and refreshes drive capacity.
    pub(super) fn empty_recycle_bin(&mut self, cx: &mut Context<Self>) {
        if self.cleanup.removal.as_ref().is_some_and(|r| r.running) {
            return;
        }
        let progress = Arc::new(crate::cleanup::Progress::default());
        let cancel = Arc::new(AtomicBool::new(false));
        self.cleanup.removal = Some(Removal {
            progress: progress.clone(),
            cancel: cancel.clone(),
            running: true,
            stopped: false,
            recycle_bin: true,
        });
        self.cleanup.open = None;
        let job = cx.background_executor().spawn({
            let progress = progress.clone();
            let cancel = cancel.clone();
            async move { crate::cleanup::empty_recycle_bin_with_progress(&cancel, &progress) }
        });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                let stopped = this.cleanup.removal.as_ref().is_some_and(|r| r.stopped);
                if let Some(removal) = &mut this.cleanup.removal {
                    removal.running = false;
                }
                match result {
                    Ok(()) => {
                        if !stopped
                            && let Some(report) = &mut this.cleanup.report
                        {
                            report
                                .suggestions
                                .retain(|s| s.action != Action::EmptyRecycleBin);
                        }
                        this.cleanup.open = None;
                        this.message = Some(if stopped {
                            "Stopped emptying the Recycle Bin.".into()
                        } else {
                            "Emptied the Recycle Bin.".into()
                        });
                    }
                    Err(error) => {
                        this.message = Some(format!("Couldn't empty the Recycle Bin: {error}"))
                    }
                }
                this.refresh_capacity(cx);
                if let Some(page) = &this.storage_page {
                    let scope = page.scope.clone();
                    this.measure_storage_again(scope, cx);
                }
                cx.notify();
            });
        })
        .detach();
        // Redraw live Recycle Bin emptying progress ~5 times a second while running.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let running = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.cleanup.removal.as_ref().is_some_and(|r| r.running)
                    })
                    .unwrap_or(false);
                if !running {
                    return;
                }
            }
        })
        .detach();
        cx.notify();
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
                let when_stamp = state
                    .checked_at
                    .map(|t| format_checked_at(t, chrono::Local::now()))
                    .unwrap_or_default();
                let when = format!("{when_stamp} in {:.1} s", report.elapsed.as_secs_f32());
                let mut line = if report.suggestions.is_empty() {
                    format!("Checked {when}")
                } else {
                    format!("{} to free · checked {when}", human(total))
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
            .filter(|s| s.action != Action::EmptyRecycleBin && !state.skipped_groups.contains(&s.title))
            .collect();
        let all_items: usize = movable.iter().map(|s| s.items.len()).sum();
        let all_bytes: u64 = movable.iter().map(|s| s.bytes()).sum();
        let recycled_items: usize = movable
            .iter()
            .filter(|s| s.action == Action::Recycle)
            .map(|s| s.items.len())
            .sum();
        let some_skipped = !state.skipped_groups.is_empty();
        let busy = state.removal.as_ref().is_some_and(|r| r.running);
        let clean_all = (!state.looking && all_items > 0 && !busy).then(|| {
            super::settings_view::control(
                "cleanup-all",
                if some_skipped { "Clean up selected" } else { "Clean up all" },
                p,
                cx,
            )
                .h(BUTTON_HEIGHT)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.cleanup.confirm_all = !this.cleanup.confirm_all;
                    cx.notify();
                }))
        });
        let action = div()
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .children(clean_all)
            .child(action);
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
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(TEXT_BODY)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Cleanup suggestions"),
                    )
                    .children(status.map(|s| {
                        div()
                            .min_w_0()
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
            .overflow_hidden()
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
                            .min_w_0()
                            .text_size(TEXT_SMALL)
                            .child(format!("Moved {count} to the Recycle Bin. Look again to see what's left.")),
                    )
                    .child(
                        div().flex_shrink_0().child(
                            super::settings_view::control("cleanup-undo", "Undo", p, cx)
                                .h(BUTTON_HEIGHT)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cleanup.moved = None;
                                    this.file_action(super::daily::Action::UndoRename, None, window, cx);
                                    this.refresh_recycle_bin(cx);
                                    cx.notify();
                                })),
                        ),
                    ),
            );
        }
        if let Some(removal) = &state.removal {
            use std::sync::atomic::Ordering::Relaxed;
            let listed = removal.progress.listed.load(Relaxed);
            let files = removal.progress.files.load(Relaxed);
            let bytes = removal.progress.bytes.load(Relaxed);
            let skipped = removal.progress.skipped.load(Relaxed);
            let listing = removal.progress.listing.load(Relaxed);
            let fraction = if listed == 0 { 0. } else { (files + skipped) as f32 / listed as f32 };
            let line = if removal.running && listing {
                if removal.recycle_bin {
                    format!(
                        "Emptying Recycle Bin… finding items ({})",
                        super::interface::thousands(listed as usize)
                    )
                } else {
                    format!("Finding files… {}", super::interface::thousands(listed as usize))
                }
            } else if removal.running {
                if removal.recycle_bin && listed == 0 {
                    "Emptying Recycle Bin…".to_string()
                } else {
                    format!(
                        "{} {} of {} files · {} freed",
                        if removal.recycle_bin {
                            "Emptying Recycle Bin…"
                        } else {
                            "Deleting…"
                        },
                        super::interface::thousands((files + skipped) as usize),
                        super::interface::thousands(listed as usize),
                        human(bytes)
                    )
                }
            } else if removal.recycle_bin && files == 0 && !removal.stopped {
                "Emptied the Recycle Bin.".to_string()
            } else {
                let mut line = format!(
                    "{} {} · {} files deleted",
                    if removal.stopped {
                        "Stopped. Freed"
                    } else if removal.recycle_bin {
                        "Emptied Recycle Bin · freed"
                    } else {
                        "Freed"
                    },
                    human(bytes),
                    super::interface::thousands(files as usize)
                );
                if skipped > 0 {
                    line.push_str(&format!(" · {} in use or protected were left", super::interface::thousands(skipped as usize)));
                }
                line
            };
            card = card.child(
                div()
                    .px_4()
                    .py_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().flex_1().min_w_0().text_size(TEXT_SMALL).child(line))
                            .when(removal.running, |d| {
                                d.child(
                                    div().flex_shrink_0().child(
                                        super::settings_view::control("cleanup-stop-removal", "Stop", p, cx)
                                            .h(BUTTON_HEIGHT)
                                            .on_click(cx.listener(|this, _, _, cx| this.stop_removal(cx))),
                                    ),
                                )
                            }),
                    )
                    .when(removal.running, |d| {
                        d.child(
                            div()
                                .h(px(6.))
                                .w_full()
                                .rounded(px(3.))
                                .overflow_hidden()
                                .bg(p.text.opacity(0.08))
                                .child(div().h_full().w(relative(fraction.clamp(0., 1.))).bg(p.accent)),
                        )
                    }),
            );
        }
        if state.confirm_all && all_items > 0 {
            card = card.child(
                div()
                    .px_4()
                    .py_3()
                    .flex()
                    .items_center()
                    .gap_4()
                    .border_b_1()
                    .border_color(p.line)
                    .bg(p.text.opacity(0.03))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().text_size(TEXT_BODY).font_weight(FontWeight::MEDIUM).child(format!(
                                "Clean up {all_items} items ({})?",
                                human(all_bytes)
                            )))
                            .child(
                                div()
                                    .text_size(TEXT_SMALL)
                                    .text_color(p.muted)
                                    .child(if recycled_items > 0 {
                                        "Caches and logs are deleted (their apps rebuild them); your downloads and models go to the Recycle Bin. Files apps have open are skipped."
                                    } else {
                                        "These are caches and logs their apps rebuild, so they're deleted rather than recycled. Files apps have open are skipped."
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .flex_shrink_0()
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
                                    .disabled(self.files_pending != 0 || busy)
                                    .child(super::interface::button_text("Clean up"))
                                    .on_click(cx.listener(|this, _, window, cx| this.clean_selected(window, cx))),
                            ),
                    ),
            );
        }
        let list = self.suggestions();
        if state.report.is_none() && !state.looking {
            card = card.child(note(
                "Seek looks through developer, app and browser caches, shader caches, crash dumps, old temporary files and installers, build folders in projects you haven't touched for months, and the Recycle Bin. Nothing is removed until you review it. Caches their apps rebuild are deleted; your own files go to the Recycle Bin.",
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
            let title = suggestion.title.clone();
            let included = !state.skipped_groups.contains(&title);
            let mut line = row(("cleanup", index).into())
                .when(index > 0, |d| d.border_t_1().border_color(p.line))
                // Which groups Clean up takes.
                .when(!empties, |d| {
                    d.child(
                        Checkbox::new(("cleanup-group", index))
                            .checked(included)
                            .accessibility_label(title.clone())
                            .on_click(cx.listener(move |this, _: &bool, _, cx| {
                                if !this.cleanup.skipped_groups.remove(&title) {
                                    this.cleanup.skipped_groups.insert(title.clone());
                                }
                                cx.notify();
                            })),
                    )
                })
                .child(text)
                .child(
                    div()
                        .flex_shrink_0()
                        .font_family(FONT_MONO)
                        .text_size(TEXT_SMALL)
                        .text_color(p.muted)
                        .child(human(bytes)),
                );
            let emptying_bin = state
                .removal
                .as_ref()
                .is_some_and(|r| r.running && r.recycle_bin);
            line = if empties {
                line.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .flex_shrink_0()
                        .child(
                            super::settings_view::control("open-recycle-bin", "Open", p, cx)
                                .h(BUTTON_HEIGHT)
                                .on_click(|_, _, cx| cx.open_url("shell:RecycleBinFolder")),
                        )
                        .child(
                            super::settings_view::control(
                                "empty-recycle-bin",
                                if emptying_bin { "Emptying…" } else { "Empty…" },
                                p,
                                cx,
                            )
                            .h(BUTTON_HEIGHT)
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.ask_empty_recycle_bin(bytes, window, cx)
                            })),
                        ),
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
                                .min_w_0()
                                .text_size(TEXT_SMALL)
                                .text_color(p.muted)
                                .child(if suggestion.action == Action::Delete {
                                    "Deleted · the apps that made these rebuild them"
                                } else {
                                    "Goes to the Recycle Bin · Ctrl+Z puts it back"
                                }),
                        )
                        .child(
                            div().flex_shrink_0().child(
                                Button::new(("cleanup-recycle", index))
                                    .h(BUTTON_HEIGHT)
                                    .px_3()
                                    .rounded(RADIUS_CONTROL)
                                    .bg(p.text)
                                    .text_color(crate::appearance::modal_surface(p))
                                    .font_weight(FontWeight::MEDIUM)
                                    .disabled(count == 0 || self.files_pending != 0 || busy)
                                    .child(super::interface::button_text(if suggestion.action == Action::Delete {
                                        format!("Delete {count} · {}", human(chosen))
                                    } else {
                                        format!("Move {count} to Recycle Bin · {}", human(chosen))
                                    }))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.clean_suggestion(index, window, cx)
                                    })),
                            ),
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
                            .min_w_0()
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
                        div().flex_shrink_0().child(
                            super::settings_view::control("open-storage-sense", "Open", p, cx)
                                .h(BUTTON_HEIGHT)
                                .on_click(|_, _, cx| cx.open_url("ms-settings:storagepolicies")),
                        ),
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

#[cfg(test)]
mod tests {
    use super::format_checked_at;
    use chrono::{Local, TimeZone};

    #[test]
    fn format_checked_at_includes_day_context() {
        let now = Local.with_ymd_and_hms(2026, 11, 27, 14, 20, 0).unwrap();
        let today = Local.with_ymd_and_hms(2026, 11, 27, 13, 15, 0).unwrap();
        let yesterday = Local.with_ymd_and_hms(2026, 11, 26, 21, 5, 0).unwrap();
        let earlier = Local.with_ymd_and_hms(2026, 11, 20, 9, 30, 0).unwrap();

        assert_eq!(format_checked_at(today, now), "today at 13:15");
        assert_eq!(format_checked_at(yesterday, now), "yesterday at 21:05");
        assert_eq!(format_checked_at(earlier, now), "Nov 20 at 09:30");
    }
}
