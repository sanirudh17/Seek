use super::*;
use crate::batch_rename::{Mode, Proposal};

/// Suggest Names: one editable field per file, filled from naming::suggest.
pub(super) struct Suggested {
    inputs: Vec<Entity<InputState>>,
    /// Why a file keeps its name, when no suggestion was found.
    reasons: Vec<Option<String>>,
    /// Where each filled-in name came from ("Document title").
    sources: Vec<Option<&'static str>>,
    /// Low-confidence names: offered beside the file, not filled in.
    offers: Vec<Option<(String, &'static str)>>,
    loading: bool,
    _task: Task<()>,
    _subscriptions: Vec<Subscription>,
}
/// Suggestions are edited in place, so their number is bounded.
const SUGGEST_LIMIT: usize = 200;
pub(super) struct State {
    pub(super) second: Entity<InputState>,
    paths: Vec<PathBuf>,
    pub(super) numbered: bool,
    pub(super) suggested: Option<Suggested>,
    /// Advanced rename (8.2): steps, options and presets.
    pub(super) advanced: Option<super::rename_advanced_view::Advanced>,
    skip_conflicts: bool,
    pub(super) rows: Vec<Proposal>,
    revision: u64,
    pub(super) loading: bool,
    task: Task<()>,
    _subscription: Subscription,
}
impl State {
    pub(super) fn suggesting(&self) -> bool {
        self.suggested.is_some()
    }
}
impl Seek {
    pub(super) fn install_batch_rename(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.len() > 1000 {
            self.edit = None;
            self.message = Some("Select up to 1,000 items for one batch rename.".into());
            self.table.read(cx).focus_handle(cx).focus(window, cx);
            return;
        }
        let second = cx.new(|cx| {
            InputState::new(window, cx)
                .context_menu(false)
                .placeholder("Replacement text")
        });
        let subscription = cx.subscribe(&second, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.refresh_batch_preview(cx);
            }
        });
        if let Some(edit) = &mut self.edit {
            edit.input.update(cx, |input, cx| {
                input.set_placeholder("Text to find", window, cx)
            });
            edit.batch = Some(State {
                second,
                paths,
                numbered: false,
                suggested: None,
                advanced: None,
                skip_conflicts: false,
                rows: vec![],
                revision: 0,
                loading: true,
                task: Task::ready(()),
                _subscription: subscription,
            });
        }
        self.refresh_batch_preview(cx);
    }
    pub(super) fn refresh_batch_preview(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.edit else {
            return;
        };
        let Some(batch) = &mut edit.batch else {
            return;
        };
        if edit.pending {
            return;
        }
        batch.revision = batch.revision.wrapping_add(1);
        let revision = batch.revision;
        let input_id = edit.input.entity_id();
        let first = edit.input.read(cx).value().to_string();
        let second = batch.second.read(cx).value().to_string();
        if let Some(advanced) = &batch.advanced {
            let steps: Vec<_> = advanced.rows.iter().map(|row| row.step.clone()).collect();
            let options = advanced.options.clone();
            edit.error = None;
            batch.loading = true;
            let paths = batch.paths.clone();
            batch.task = cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                let (rows, problem) = cx
                    .background_executor()
                    .spawn(async move {
                        let facts = crate::rename_steps::facts(&paths, &options);
                        let names = crate::rename_steps::plan(&paths, &facts, &steps, &options);
                        let problem = names.iter().find_map(|name| name.clone().err());
                        let names = names
                            .into_iter()
                            .zip(&paths)
                            .map(|(name, path)| {
                                name.unwrap_or_else(|_| {
                                    path.file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_default()
                                })
                            })
                            .collect();
                        (crate::batch_rename::preview(&paths, &Mode::Names(names)), problem)
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(edit) = &mut this.edit
                        && edit.input.entity_id() == input_id
                        && let Some(batch) = &mut edit.batch
                        && batch.revision == revision
                    {
                        batch.rows = rows;
                        batch.loading = false;
                        edit.error = problem;
                        cx.notify();
                    }
                });
            });
            cx.notify();
            return;
        }
        let mode = if let Some(suggested) = &batch.suggested {
            if suggested.loading {
                return;
            }
            Mode::Names(
                suggested
                    .inputs
                    .iter()
                    .map(|input| input.read(cx).value().to_string())
                    .collect(),
            )
        } else if batch.numbered {
            let Ok(start) = second.parse() else {
                edit.error = Some("Enter a nonnegative whole number for Start at.".into());
                batch.rows.clear();
                batch.loading = false;
                cx.notify();
                return;
            };
            Mode::Number { base: first, start }
        } else {
            Mode::Replace {
                find: first,
                replacement: second,
            }
        };
        edit.error = None;
        batch.loading = true;
        let paths = batch.paths.clone();
        batch.task = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
            let rows = cx
                .background_executor()
                .spawn(async move { crate::batch_rename::preview(&paths, &mode) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(edit) = &mut this.edit
                    && edit.input.entity_id() == input_id
                    && let Some(batch) = &mut edit.batch
                    && batch.revision == revision
                {
                    batch.rows = rows;
                    batch.loading = false;
                    cx.notify();
                }
            });
        });
        cx.notify();
    }
    /// Switch the batch sheet to Suggest Names: read each file's suggestion off
    /// the UI thread, then show one editable field per file.
    pub(super) fn batch_suggest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(batch) = self.edit.as_mut().and_then(|edit| edit.batch.as_mut()) else {
            return;
        };
        if batch.paths.len() > SUGGEST_LIMIT {
            if let Some(edit) = &mut self.edit {
                edit.error = Some(format!(
                    "Suggest names works on up to {SUGGEST_LIMIT} files."
                ));
            }
            cx.notify();
            return;
        }
        batch.advanced = None;
        let paths = batch.paths.clone();
        let task = cx.spawn_in(window, async move |this, cx| {
            let started = std::time::Instant::now();
            let suggestions = cx
                .background_executor()
                .spawn(async move {
                    paths
                        .iter()
                        .map(|path| crate::naming::suggest(path))
                        .collect::<Vec<_>>()
                })
                .await;
            let elapsed = started.elapsed();
            let _ = this.update_in(cx, |this, window, cx| {
                crate::metrics::interaction(
                    "suggest_names",
                    serde_json::json!({
                        "files": suggestions.len(),
                        "ms_per_file": elapsed.as_secs_f64() * 1000. / suggestions.len().max(1) as f64,
                    }),
                );
                let Some(batch) = this.edit.as_mut().and_then(|edit| edit.batch.as_mut()) else {
                    return;
                };
                let mut inputs = vec![];
                let mut reasons = vec![];
                let mut sources = vec![];
                let mut offers = vec![];
                let mut subscriptions = vec![];
                // Two files read as the same name: the later ones are numbered,
                // in the order they were selected, so applying never clashes.
                let mut used = std::collections::HashSet::new();
                for (path, suggestion) in batch.paths.iter().zip(suggestions) {
                    let original = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let unique = |name: String, used: &mut std::collections::HashSet<String>| {
                        let (stem, extension) = match name.rfind('.') {
                            Some(dot) if dot > 0 => (name[..dot].to_string(), name[dot..].to_string()),
                            _ => (name.clone(), String::new()),
                        };
                        let mut candidate = name;
                        let mut n = 2;
                        while !used.insert(candidate.to_lowercase()) {
                            candidate = format!("{stem} ({n}){extension}");
                            n += 1;
                        }
                        candidate
                    };
                    let (value, reason, source, offer) = match suggestion {
                        Ok(found) if found.confidence == crate::naming::Confidence::Low => {
                            (original, None, None, Some((found.name, found.source)))
                        }
                        Ok(found) => (unique(found.name, &mut used), None, Some(found.source), None),
                        Err(reason) => (original, Some(reason), None, None),
                    };
                    let input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .context_menu(false)
                            .default_value(value)
                    });
                    subscriptions.push(cx.subscribe(&input, |this, _, event, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.refresh_batch_preview(cx);
                        }
                    }));
                    inputs.push(input);
                    reasons.push(reason);
                    sources.push(source);
                    offers.push(offer);
                }
                if let Some(suggested) = &mut batch.suggested {
                    suggested.inputs = inputs;
                    suggested.reasons = reasons;
                    suggested.sources = sources;
                    suggested.offers = offers;
                    suggested.loading = false;
                    suggested._subscriptions = subscriptions;
                }
                this.refresh_batch_preview(cx);
            });
        });
        batch.suggested = Some(Suggested {
            inputs: vec![],
            reasons: vec![],
            sources: vec![],
            offers: vec![],
            loading: true,
            _task: task,
            _subscriptions: vec![],
        });
        batch.rows.clear();
        batch.loading = true;
        cx.notify();
    }
    fn batch_mode(&mut self, numbered: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(edit) = &mut self.edit
            && !edit.pending
            && let Some(batch) = &mut edit.batch
        {
            batch.suggested = None;
            batch.advanced = None;
            batch.numbered = numbered;
            // New mode, new inputs: preserve extension only in numbered mode.
            edit.input.update(cx, |input, cx| {
                input.set_value(if numbered { "File" } else { "" }, window, cx);
                input.set_placeholder(
                    if numbered {
                        "Base name"
                    } else {
                        "Text to find"
                    },
                    window,
                    cx,
                );
            });
            batch.second.update(cx, |input, cx| {
                input.set_value(if numbered { "1" } else { "" }, window, cx);
                input.set_placeholder(
                    if numbered { "Start at" } else { "Replace with" },
                    window,
                    cx,
                );
            });
        }
        self.refresh_batch_preview(cx);
    }
    pub(super) fn batch_apply_ready(&self) -> bool {
        let Some(batch) = self.edit.as_ref().and_then(|edit| edit.batch.as_ref()) else {
            return true;
        };
        !batch.loading
            && batch
                .rows
                .iter()
                .any(|row| row.changed && row.error.is_none())
            && (batch.skip_conflicts || batch.rows.iter().all(|row| row.error.is_none()))
    }
    pub(super) fn finish_batch_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.batch_apply_ready() {
            return;
        }
        let Some(edit) = &mut self.edit else {
            return;
        };
        if edit.pending {
            return;
        }
        let Some(batch) = &edit.batch else {
            return;
        };
        let entries = batch
            .rows
            .iter()
            .filter(|row| row.changed && row.error.is_none())
            .map(|row| (row.source.clone(), row.name.clone()))
            .collect();
        edit.pending = true;
        edit.error = None;
        self.run_file_op(
            crate::file_ops::Operation::BatchRename { entries },
            window,
            cx,
        );
        if let Some(error) = self.message.clone()
            && let Some(edit) = &mut self.edit
        {
            edit.pending = false;
            edit.error = Some(error);
        }
    }
    pub(super) fn batch_name_fields(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(edit) = &self.edit else {
            return div().into_any_element();
        };
        let Some(batch) = &edit.batch else {
            return div().into_any_element();
        };
        let p = crate::appearance::palette(&self.preferences, cx);
        let suggesting = batch.suggested.is_some();
        let advanced = batch.advanced.is_some();
        // Which way the batch is renamed, shown as one segmented choice so
        // the current mode is always visible.
        let current = if advanced {
            3
        } else if suggesting {
            2
        } else if batch.numbered {
            1
        } else {
            0
        };
        let pending = edit.pending;
        let mode_buttons = div().flex().child(super::settings_view::segmented(
            "batch-mode",
            &["Replace text", "Number names", "Suggest names", "Advanced"],
            current,
            self.segment_motion.get("batch-mode").copied(),
            self.motion_enabled,
            p,
            cx,
            move |this, index, window, cx| {
                if pending || index == current {
                    return;
                }
                this.slide_segment("batch-mode", current, index);
                match index {
                    0 => this.batch_mode(false, window, cx),
                    1 => this.batch_mode(true, window, cx),
                    2 => this.batch_suggest(window, cx),
                    _ => this.batch_advanced(window, cx),
                }
            },
        ));
        if advanced {
            return div()
                .flex()
                .flex_col()
                .gap_3()
                .child(mode_buttons)
                .child(self.advanced_fields(p, cx))
                .into_any_element();
        }
        if let Some(suggested) = &batch.suggested {
            let mut list = div()
                .id("suggest-name-rows")
                .max_h(px(360.))
                .overflow_y_scroll()
                .flex()
                .flex_col();
            for (index, path) in batch.paths.iter().enumerate() {
                let original = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let row = batch.rows.get(index);
                let note = suggested
                    .reasons
                    .get(index)
                    .cloned()
                    .flatten()
                    .or_else(|| row.and_then(|row| row.error.clone()));
                let source = suggested.sources.get(index).copied().flatten();
                let offer = suggested.offers.get(index).cloned().flatten();
                let input = suggested.inputs.get(index).cloned();
                list = list.child(
                    div()
                        .id(("suggest-name-row", index))
                        .py_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .border_b_1()
                        .border_color(p.line)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(div().flex_1().min_w_0().children(
                                    suggested.inputs.get(index).map(|input| {
                                        Input::new(input).readonly(edit.pending).min_h(px(32.))
                                    }),
                                ))
                                .child(
                                    div()
                                        .w(px(200.))
                                        .flex_shrink_0()
                                        .truncate()
                                        .text_size(px(12.))
                                        .text_color(p.muted)
                                        .child(original),
                                ),
                        )
                        .when_some(source, |row, source| {
                            row.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .text_size(px(12.))
                                    .text_color(p.muted)
                                    .child(div().size(px(5.)).rounded_full().bg(p.tints.green))
                                    .child(format!("From its {}", source.to_lowercase())),
                            )
                        })
                        .when_some(offer, |row, (name, source)| {
                            let pending = edit.pending;
                            row.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_size(px(12.))
                                    .text_color(p.muted)
                                    .child(format!("Possible, from its {}: {name}", source.to_lowercase()))
                                    .child(
                                        div()
                                            .id(("suggest-name-use", index))
                                            .role(Role::Button)
                                            .px_1()
                                            .rounded(px(4.))
                                            .cursor_pointer()
                                            .text_color(p.accent)
                                            .hover(move |s| s.bg(p.accent.opacity(0.1)))
                                            .child("Use")
                                            .on_click(cx.listener(move |_, _, window, cx| {
                                                if pending {
                                                    return;
                                                }
                                                if let Some(input) = &input {
                                                    input.update(cx, |input, cx| {
                                                        input.set_value(name.clone(), window, cx)
                                                    });
                                                }
                                            })),
                                    ),
                            )
                        })
                        .when_some(note, |row, note| {
                            row.child(div().text_size(px(12.)).text_color(p.muted).child(note))
                        }),
                );
            }
            let applicable = batch
                .rows
                .iter()
                .filter(|row| row.changed && row.error.is_none())
                .count();
            return div()
                .flex()
                .flex_col()
                .gap_3()
                .child(mode_buttons)
                .child(if suggested.loading {
                    div()
                        .py_4()
                        .text_size(px(13.))
                        .text_color(p.muted)
                        .child("Reading files…")
                        .into_any_element()
                } else {
                    list.into_any_element()
                })
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child(if suggested.loading {
                            String::new()
                        } else {
                            let found = suggested.sources.iter().flatten().count();
                            let offered = suggested.offers.iter().flatten().count();
                            let kept = batch.paths.len().saturating_sub(found + offered);
                            format!(
                                "{} · {found} read from the files{}{}",
                                names_change(applicable),
                                if offered > 0 { format!(" · {offered} possible") } else { String::new() },
                                match kept {
                                    0 => String::new(),
                                    1 => " · 1 keeps its name".to_string(),
                                    n => format!(" · {n} keep their names"),
                                },
                            )
                        }),
                )
                .into_any_element();
        }
        let mut preview = div()
            .id("batch-name-preview")
            .max_h(px(240.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2();
        for (index, row) in batch.rows.iter().enumerate() {
            preview = preview.child(
                div()
                    .id(("batch-name-row", index))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .py_2()
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .flex()
                            .gap_4()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(p.muted)
                                    .child(
                                        row.source
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .into_owned(),
                                    ),
                            )
                            // Only the names that change stand out.
                            .child({
                                let unchanged = row.source.file_name().is_some_and(|n| n.to_string_lossy() == row.name.as_str());
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .when(unchanged, |d| d.text_color(p.muted))
                                    .when(!unchanged, |d| d.font_weight(FontWeight::MEDIUM))
                                    .child(row.name.clone())
                            }),
                    )
                    .when_some(row.error.clone(), |view, error| {
                        view.child(div().text_size(px(12.)).text_color(p.muted).child(error))
                    }),
            );
        }
        let applicable = batch
            .rows
            .iter()
            .filter(|row| row.changed && row.error.is_none())
            .count();
        let conflicts = batch.rows.iter().filter(|row| row.error.is_some()).count();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(mode_buttons)
            .child(
                Input::new(&edit.input)
                    .readonly(edit.pending)
                    .min_h(px(36.)),
            )
            .child(
                Input::new(&batch.second)
                    .readonly(edit.pending)
                    .min_h(px(36.)),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(p.muted)
                    .child(if batch.loading {
                        "Checking…".into()
                    } else {
                        match conflicts {
                            0 => names_change(applicable),
                            1 => format!("{} · 1 conflict", names_change(applicable)),
                            n => format!("{} · {n} conflicts", names_change(applicable)),
                        }
                    }),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .text_size(px(12.))
                    .text_color(p.muted)
                    .child(div().flex_1().child("Current name"))
                    .child(div().flex_1().child("Proposed name")),
            )
            .child(preview)
            // Only when some names clash: skip those and rename the rest.
            .when(conflicts > 0 || batch.skip_conflicts, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            gpui_component::checkbox::Checkbox::new("batch-skip-conflicts")
                                .checked(batch.skip_conflicts)
                                .disabled(edit.pending)
                                .accessibility_label("Skip conflicting items")
                                .on_click(cx.listener(|this, _: &bool, _, cx| {
                                    if let Some(batch) =
                                        this.edit.as_mut().and_then(|edit| edit.batch.as_mut())
                                    {
                                        batch.skip_conflicts = !batch.skip_conflicts;
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .child("Skip conflicting items and rename the rest"),
                        ),
                )
            })
            .into_any_element()
    }
}

/// "1 name changes", "3 names change".
fn names_change(count: usize) -> String {
    if count == 1 {
        "1 name changes".into()
    } else {
        format!("{count} names change")
    }
}
