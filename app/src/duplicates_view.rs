//! Find Duplicates on the Storage page (Phase 9.3), after Monodrive's rule:
//! nothing is chosen automatically by age, folder or name. The person ticks
//! the exact copies to move to the Recycle Bin (Select extras does it by a
//! rule only when pressed, and Clear undoes the choice); one copy of every
//! file always stays, and each ticked copy must look as it did when
//! compared before anything moves. Moves are journaled, so Undo (or Ctrl+Z
//! in the file list) puts them back.
//!
//! Folders lists only folders whose whole contents are identical (see
//! `duplicates::identical_folders`). Before anything moves, a background
//! check confirms each ticked file and folder is unchanged and that every
//! file keeps a copy that isn't going anywhere.
//!
//! The whole window redraws on every hover and animation frame, so this
//! section draws little: a page of groups at a time and no per-row tooltips.
use super::*;
use crate::duplicates::{Group, Keep, Report};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, atomic::AtomicBool};

/// Groups or folder pairs drawn per page.
const PAGE: usize = 20;
const RULES: [Keep; 3] = [Keep::Oldest, Keep::Newest, Keep::ShortestPath];

#[derive(Default)]
pub(super) struct Duplicates {
    folder: Option<PathBuf>,
    /// The last search looked everywhere rather than in `folder`.
    everywhere: bool,
    report: Option<Report>,
    running: bool,
    progress: Arc<Mutex<(usize, usize, &'static str)>>,
    /// The rule Select extras uses.
    rule: usize,
    /// Showing identical folders instead of file groups.
    folders_view: bool,
    /// Showing similar images (similar_view) instead of exact copies.
    pub(super) similar_mode: bool,
    /// How many groups or folders are drawn.
    shown: usize,
    /// Copies ticked for the Recycle Bin: (group, copy).
    ticked: HashSet<(usize, usize)>,
    /// Identical folders with one side ticked: folder index → side (0, 1).
    folder_ticked: HashMap<usize, usize>,
    /// How many items the last move sent to the Recycle Bin (offers Undo).
    moved: Option<usize>,
    /// A move is being checked in the background.
    checking: bool,
    cancel: Option<Arc<AtomicBool>>,
    task: Option<Task<()>>,
}

impl Duplicates {
    fn set_report(&mut self, report: Option<Report>) {
        self.report = report;
        self.ticked.clear();
        self.folder_ticked.clear();
        self.shown = PAGE;
    }

    /// Ticks or unticks one copy; refuses to tick the last one left.
    fn toggle(&mut self, group: usize, copy: usize) -> Result<(), &'static str> {
        if self.ticked.remove(&(group, copy)) {
            return Ok(());
        }
        let copies = self
            .report
            .as_ref()
            .and_then(|r| r.groups.get(group))
            .map_or(0, |g| g.copies.len());
        let ticked = (0..copies).filter(|c| self.ticked.contains(&(group, *c))).count();
        if ticked + 1 >= copies {
            return Err("One copy of each file always stays.");
        }
        self.ticked.insert((group, copy));
        Ok(())
    }

    /// Ticks every copy but the one the rule keeps, in every group.
    fn select_extras(&mut self) {
        let Some(report) = &self.report else { return };
        self.ticked.clear();
        for (index, group) in report.groups.iter().enumerate() {
            let keep = crate::duplicates::keeper(group, RULES[self.rule]);
            for copy in 0..group.copies.len() {
                if copy != keep {
                    self.ticked.insert((index, copy));
                }
            }
        }
    }

    fn clear(&mut self) {
        self.ticked.clear();
        self.folder_ticked.clear();
    }

    /// Ticks one side of an identical pair (the other stays), or unticks it.
    fn tick_folder(&mut self, folder: usize, side: usize) {
        if self.folder_ticked.get(&folder) == Some(&side) {
            self.folder_ticked.remove(&folder);
        } else {
            self.folder_ticked.insert(folder, side);
        }
    }

    /// What a move would take: the ticked folders and the ticked files not
    /// already inside one, or why it can't go ahead (a file would lose
    /// every copy).
    fn plan(&self) -> Result<Plan, String> {
        let Some(report) = &self.report else {
            return Err("Nothing to move.".into());
        };
        let lower = |p: &std::path::Path| p.to_string_lossy().to_lowercase();
        let mut folders = vec![];
        for (index, side) in &self.folder_ticked {
            let Some(f) = report.folders.get(*index) else { continue };
            let (path, listing) = if *side == 0 { (&f.a, &f.listing_a) } else { (&f.b, &f.listing_b) };
            folders.push((path.clone(), listing.clone()));
        }
        let inside = |path: &std::path::Path| {
            let p = lower(path);
            folders.iter().any(|(f, _)| p.starts_with(&format!("{}\\", lower(f))))
        };
        let mut files = vec![];
        for (index, group) in report.groups.iter().enumerate() {
            let going = |c: usize| self.ticked.contains(&(index, c)) || inside(&group.copies[c].path);
            if (0..group.copies.len()).all(going) {
                let name = group.copies[0]
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                return Err(format!(
                    "This would remove every copy of {name}. Untick one of its copies or folders."
                ));
            }
            for c in 0..group.copies.len() {
                if self.ticked.contains(&(index, c)) && !inside(&group.copies[c].path) {
                    files.push((group.copies[c].clone(), group.size, index));
                }
            }
        }
        if files.is_empty() && folders.is_empty() {
            return Err("Tick the copies to move first.".into());
        }
        Ok(Plan { files, folders })
    }
}

/// A move worked out from the ticks, checked again in the background.
struct Plan {
    files: Vec<(crate::duplicates::Copy, u64, usize)>,
    folders: Vec<(PathBuf, Vec<crate::duplicates::Entry>)>,
}

impl Seek {
    fn pick_duplicates_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Find duplicates in".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = paths.await
                && let Some(folder) = paths.pop()
            {
                let _ = this.update(cx, |this, cx| this.find_duplicates(Some(folder), cx));
            }
        })
        .detach();
    }

    /// One folder, or (`None`) everything indexed outside system folders
    /// and projects.
    pub(super) fn find_duplicates(&mut self, folder: Option<PathBuf>, cx: &mut Context<Self>) {
        if let Some(cancel) = &self.duplicates.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new((0, 0, "Listing files")));
        let state = &mut self.duplicates;
        state.cancel = Some(cancel.clone());
        state.progress = progress.clone();
        state.folder = folder.clone();
        state.everywhere = folder.is_none();
        state.set_report(None);
        state.moved = None;
        state.running = true;
        let engine = crate::backend::ENGINE.get().cloned();
        let home = PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default());
        let job = cx.background_executor().spawn(async move {
            let report = |done, total, stage| {
                if let Ok(mut p) = progress.lock() {
                    *p = (done, total, stage);
                }
            };
            match folder {
                Some(folder) => crate::duplicates::find(&folder, &cancel, report),
                None => crate::duplicates::find_everywhere(engine.as_ref(), &home, &cancel, report),
            }
        });
        self.duplicates.task = Some(cx.spawn(async move |this, cx| {
            let report = job.await;
            let _ = this.update(cx, |this, cx| {
                this.duplicates.set_report(Some(report));
                this.duplicates.running = false;
                this.reveal_found();
                this.duplicates.cancel = None;
                cx.notify();
            });
        }));
        // Redraw the progress line twice a second while the search runs;
        // nothing ticks once it is done.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let running = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.duplicates.running
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

    fn stop_duplicates(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = &self.duplicates.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    /// Moves the ticked copies and folders to the Recycle Bin, after a
    /// background check that each still looks as it did when compared and
    /// that every file keeps a copy staying where it is.
    fn recycle_duplicates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let plan = match self.duplicates.plan() {
            Ok(plan) => plan,
            Err(why) => {
                self.message = Some(why);
                cx.notify();
                return;
            }
        };
        let kept: Vec<(crate::duplicates::Copy, u64)> = {
            // For each group a file leaves, one copy that stays must be intact.
            let report = self.duplicates.report.as_ref().expect("planned");
            let leaving: HashSet<usize> = plan.files.iter().map(|(_, _, g)| *g).collect();
            let going: HashSet<String> = plan
                .files
                .iter()
                .map(|(c, _, _)| c.path.to_string_lossy().to_lowercase())
                .collect();
            leaving
                .iter()
                .filter_map(|g| {
                    let group = &report.groups[*g];
                    group
                        .copies
                        .iter()
                        .find(|c| !going.contains(&c.path.to_string_lossy().to_lowercase()))
                        .map(|c| (c.clone(), group.size))
                })
                .collect()
        };
        self.duplicates.checking = true;
        cx.notify();
        let job = cx.background_executor().spawn(async move {
            let mut changed = 0usize;
            let mut paths = vec![];
            let kept_ok = kept
                .iter()
                .all(|(c, size)| crate::duplicates::unchanged(c, *size));
            if !kept_ok {
                return Err("A copy that stays has changed since it was compared. Search again.".to_string());
            }
            for (path, listing) in plan.folders {
                if crate::duplicates::listing_unchanged(&path, &listing) {
                    paths.push(path);
                } else {
                    changed += 1;
                }
            }
            for (copy, size, _) in plan.files {
                if crate::duplicates::unchanged(&copy, size) {
                    paths.push(copy.path);
                } else {
                    changed += 1;
                }
            }
            Ok((paths, changed))
        });
        cx.spawn_in(window, async move |this, cx| {
            let checked = job.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.duplicates.checking = false;
                let (paths, changed) = match checked {
                    Ok(x) => x,
                    Err(why) => {
                        this.message = Some(why);
                        cx.notify();
                        return;
                    }
                };
                if paths.is_empty() {
                    this.message =
                        Some("These files changed since they were compared. Search again.".into());
                    cx.notify();
                    return;
                }
                let count = paths.len();
                this.run_file_op(crate::file_ops::Operation::Recycle { paths }, window, cx);
                if this.message.is_none() {
                    this.duplicates_moved(count);
                    if changed > 0 {
                        this.message = Some(format!(
                            "Skipped {changed} that changed since they were compared."
                        ));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// After a move: groups and folders that were acted on are done; the
    /// rest stay to review.
    fn duplicates_moved(&mut self, count: usize) {
        let state = &mut self.duplicates;
        if let Some(report) = state.report.take() {
            let ticked = std::mem::take(&mut state.ticked);
            let folder_ticked = std::mem::take(&mut state.folder_ticked);
            let groups: Vec<Group> = report
                .groups
                .into_iter()
                .enumerate()
                .filter(|(i, g)| !(0..g.copies.len()).any(|c| ticked.contains(&(*i, c))))
                .map(|(_, g)| g)
                .collect();
            let folders = report
                .folders
                .into_iter()
                .enumerate()
                .filter(|(i, _)| !folder_ticked.contains_key(i))
                .map(|(_, f)| f)
                .collect();
            state.set_report(Some(Report {
                groups,
                folders,
                ..report
            }));
        }
        state.moved = Some(count);
    }

    fn undo_duplicates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.duplicates.moved = None;
        self.file_action(super::daily::Action::UndoRename, None, window, cx);
        cx.notify();
    }

    pub(super) fn duplicates_section(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::tokens::*;
        let state = &self.duplicates;
        let similar = state.similar_mode;
        let mode = super::settings_view::segmented(
            "duplicates-mode",
            &["Identical files", "Similar images"],
            similar as usize,
            self.segment_motion.get("duplicates-mode").copied(),
            self.motion_enabled,
            p,
            cx,
            move |this, index, _, cx| {
                this.slide_segment("duplicates-mode", similar as usize, index);
                this.duplicates.similar_mode = index == 1;
                cx.notify();
            },
        );
        let buttons = if similar {
            self.similar_buttons(p, cx)
        } else if state.running {
            div().child(
                super::settings_view::control("stop-duplicates", "Stop", p, cx)
                    .h(BUTTON_HEIGHT)
                    .on_click(cx.listener(|this, _, _, cx| this.stop_duplicates(cx))),
            )
        } else {
            div()
                .flex()
                .gap_2()
                .child(
                    super::settings_view::control(
                        "duplicates-everywhere",
                        if state.everywhere && state.report.is_some() {
                            "Search again"
                        } else {
                            "Search everywhere"
                        },
                        p,
                        cx,
                    )
                    .h(BUTTON_HEIGHT)
                    .on_click(cx.listener(|this, _, _, cx| this.find_duplicates(None, cx))),
                )
                .child(
                    super::settings_view::control("pick-duplicates", "In a folder…", p, cx)
                        .h(BUTTON_HEIGHT)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.pick_duplicates_folder(window, cx)
                        })),
                )
        };
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_size(TEXT_BODY)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Duplicates"),
                    )
                    .child(mode),
            )
            .child(buttons);
        let mut card = div()
            .flex()
            .flex_col()
            .rounded(RADIUS_CARD)
            .border_1()
            .border_color(p.text.opacity(0.1))
            .bg(p.settings_card);
        if similar {
            return div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().px_1().child(header))
                .child(self.similar_card(card, p, cx))
                .into_any_element();
        }
        let note = |text: String| {
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
                            .child(format!("Moved {count} to the Recycle Bin.")),
                    )
                    .child(
                        super::settings_view::control("duplicates-undo", "Undo", p, cx)
                            .h(BUTTON_HEIGHT)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.undo_duplicates(window, cx)
                            })),
                    ),
            );
        }
        let place = if state.everywhere {
            "your files".to_string()
        } else {
            state
                .folder
                .as_ref()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        if state.running {
            let (done, total, stage) = state.progress.lock().map(|p| *p).unwrap_or_default();
            let line = if total > 0 {
                format!("{stage} · {done} of {total} files in {place}")
            } else if done > 0 {
                format!("{stage} · {done} files in {place}")
            } else {
                format!("{stage}…")
            };
            card = card.child(note(line));
        } else {
            match &state.report {
                None => {
                    card = card.child(note(
                        "Search everywhere compares every file of 1 MB or more in your own folders, byte for byte. Windows, programs, app data (including hidden folders such as .codex), projects (Git repositories) and app document bundles are left out, because their copies are needed where they are. In a folder… checks one folder completely. Nothing is selected for you: tick the copies you want to move to the Recycle Bin.".into(),
                    ))
                }
                Some(report) if report.groups.is_empty() => {
                    let mut text = format!("No duplicates among {} files in {place}.", report.files);
                    if let Some(why) = &report.stopped {
                        text = format!("{why}. {text}");
                    }
                    card = card.child(note(text));
                }
                Some(report) => card = self.duplicate_results(report, card, p, cx),
            }
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().px_1().child(header))
            .child(card)
            .into_any_element()
    }

    fn duplicate_results(
        &self,
        report: &Report,
        card: Div,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        use super::storage_view::human;
        use crate::tokens::*;
        let state = &self.duplicates;
        let total: u64 = report.groups.iter().map(Group::reclaimable).sum();
        let (mut chosen, mut chosen_bytes) = state.ticked.iter().fold((0usize, 0u64), |(n, b), (g, _)| {
            (n + 1, b + report.groups.get(*g).map_or(0, |g| g.size))
        });
        for index in state.folder_ticked.keys() {
            if let Some(f) = report.folders.get(*index) {
                chosen += 1;
                chosen_bytes += f.bytes;
            }
        }
        let groups = report.groups.len();
        let mut summary = format!(
            "{groups} {} · up to {} to free",
            if groups == 1 { "group" } else { "groups" },
            human(total)
        );
        if let Some(why) = &report.stopped {
            summary = format!("{why} · {summary}");
        }
        let rule = state.rule;
        let rules = super::settings_view::segmented(
            "duplicates-keep",
            &["Oldest", "Newest", "Shortest path"],
            rule,
            self.segment_motion.get("duplicates-keep").copied(),
            self.motion_enabled,
            p,
            cx,
            move |this, index, _, cx| {
                this.slide_segment("duplicates-keep", rule, index);
                this.duplicates.rule = index;
                cx.notify();
            },
        );
        let folders_view = state.folders_view && !report.folders.is_empty();
        let view = (!report.folders.is_empty()).then(|| {
            let selected = folders_view as usize;
            super::settings_view::segmented(
                "duplicates-view",
                &["Files", "Identical folders"],
                selected,
                self.segment_motion.get("duplicates-view").copied(),
                self.motion_enabled,
                p,
                cx,
                move |this, index, _, cx| {
                    this.slide_segment("duplicates-view", selected, index);
                    this.duplicates.folders_view = index == 1;
                    this.duplicates.shown = PAGE;
                    cx.notify();
                },
            )
        });
        let toolbar = div()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_3()
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(div().text_size(TEXT_SMALL).text_color(p.muted).child(summary))
                    .children(view),
            )
            .child(
                // Select extras is a one-time action, never a standing rule.
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        super::settings_view::control("duplicates-select", "Select extras, keeping", p, cx)
                            .h(BUTTON_HEIGHT)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.duplicates.select_extras();
                                cx.notify();
                            })),
                    )
                    .child(rules)
                    .when(chosen > 0, |d| {
                        d.child(
                            super::settings_view::control("duplicates-clear", "Clear", p, cx)
                                .h(BUTTON_HEIGHT)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.duplicates.clear();
                                    cx.notify();
                                })),
                        )
                    }),
            );
        let body = if folders_view {
            self.identical_folders(report, p, cx)
        } else {
            self.duplicate_groups(report, p, cx)
        };
        let footer = div()
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
                    .child(if chosen == 0 {
                        "Tick the copies to move to the Recycle Bin".to_string()
                    } else {
                        "One copy of each file always stays · Undo puts them back".to_string()
                    }),
            )
            .child(
                Button::new("duplicates-recycle")
                    .h(BUTTON_HEIGHT)
                    .px_3()
                    .rounded(RADIUS_CONTROL)
                    .bg(p.text)
                    .text_color(crate::appearance::modal_surface(p))
                    .font_weight(FontWeight::MEDIUM)
                    .disabled(chosen == 0 || self.files_pending != 0 || state.checking)
                    .child(super::interface::button_text(if state.checking {
                        "Checking…".to_string()
                    } else if chosen == 0 {
                        "Move to Recycle Bin".to_string()
                    } else {
                        format!("Move {chosen} to Recycle Bin · {}", human(chosen_bytes))
                    }))
                    .on_click(cx.listener(|this, _, window, cx| this.recycle_duplicates(window, cx))),
            );
        card.child(toolbar).child(body).child(footer)
    }

    fn show_more(
        id: &'static str,
        left: usize,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        use crate::tokens::*;
        (left > 0).then(|| {
            div().px_4().py_3().border_t_1().border_color(p.line).child(
                super::settings_view::control(id, format!("Show {} more", left.min(PAGE)), p, cx)
                    .h(BUTTON_HEIGHT)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.duplicates.shown += PAGE;
                        cx.notify();
                    })),
            )
        })
    }

    fn duplicate_groups(&self, report: &Report, p: crate::appearance::Palette, cx: &mut Context<Self>) -> Div {
        use super::storage_view::human;
        use crate::tokens::*;
        use gpui_component::checkbox::Checkbox;
        let state = &self.duplicates;
        let mut list = div().flex().flex_col();
        let shown = state.shown.min(report.groups.len());
        for (index, group) in report.groups.iter().take(shown).enumerate() {
            // When copies have different names, each row shows its own, so
            // it is clear these are the same contents under other names.
            let names_differ = group.copies.iter().any(|c| {
                !c.path
                    .file_name()
                    .unwrap_or_default()
                    .eq_ignore_ascii_case(group.copies[0].path.file_name().unwrap_or_default())
            });
            let name = group.copies[0]
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut block = div()
                .px_4()
                .py_3()
                .flex()
                .flex_col()
                .gap_1()
                .when(index > 0, |d| d.border_t_1().border_color(p.line))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(TEXT_BODY)
                                .font_weight(FontWeight::MEDIUM)
                                .child(name),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(TEXT_SMALL)
                                .text_color(p.muted)
                                .child(format!(
                                    "{} copies · {} each{}",
                                    group.copies.len(),
                                    human(group.size),
                                    if names_differ { " · same contents, different names" } else { "" }
                                )),
                        ),
                );
            for (i, copy) in group.copies.iter().enumerate() {
                let ticked = state.ticked.contains(&(index, i));
                let folder = if names_differ {
                    copy.path.to_string_lossy().into_owned()
                } else {
                    copy.path
                        .parent()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default()
                };
                block = block.child(
                    div()
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            Checkbox::new(("duplicate", index * 1_000 + i))
                                .checked(ticked)
                                .accessibility_label(SharedString::from(format!(
                                    "Move {} to the Recycle Bin",
                                    copy.path.to_string_lossy()
                                )))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Err(why) = this.duplicates.toggle(index, i) {
                                        this.message = Some(why.into());
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
                                .text_color(if ticked { p.muted } else { p.text })
                                .child(folder),
                        )
                        .when(ticked, |d| {
                            d.child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(TEXT_SMALL)
                                    .text_color(p.muted)
                                    .child("Recycle"),
                            )
                        }),
                );
            }
            list = list.child(block);
        }
        list.children(Self::show_more("duplicates-more", report.groups.len() - shown, p, cx))
    }

    fn identical_folders(&self, report: &Report, p: crate::appearance::Palette, cx: &mut Context<Self>) -> Div {
        use super::storage_view::human;
        use crate::tokens::*;
        use gpui_component::checkbox::Checkbox;
        let state = &self.duplicates;
        let mut list = div().flex().flex_col().child(
            div()
                .px_4()
                .pt_3()
                .text_size(TEXT_SMALL)
                .text_color(p.muted)
                .child("Every file and subfolder matches by name, size and contents. Tick the copy to move; the other stays."),
        );
        let shown = state.shown.min(report.folders.len());
        for (index, folder) in report.folders.iter().take(shown).enumerate() {
            let ticked = state.folder_ticked.get(&index).copied();
            let side = |which: usize, path: &PathBuf| {
                let chosen = ticked == Some(which);
                div()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        Checkbox::new(("folder-side", index * 2 + which))
                            .checked(chosen)
                            .accessibility_label(SharedString::from(format!(
                                "Move the folder {} to the Recycle Bin",
                                path.to_string_lossy()
                            )))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.duplicates.tick_folder(index, which);
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(TEXT_SMALL)
                            .text_color(if chosen { p.muted } else { p.text })
                            .child(path.to_string_lossy().into_owned()),
                    )
                    .when(chosen, |d| {
                        d.child(
                            div()
                                .flex_shrink_0()
                                .text_size(TEXT_SMALL)
                                .text_color(p.muted)
                                .child("Recycle folder"),
                        )
                    })
            };
            list = list.child(
                div()
                    .px_4()
                    .py_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when(index > 0, |d| d.border_t_1().border_color(p.line))
                    .child(
                        div()
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child(format!(
                                "Identical folders · {} files · {}",
                                folder.files,
                                human(folder.bytes)
                            )),
                    )
                    .child(side(0, &folder.a))
                    .child(side(1, &folder.b)),
            );
        }
        list.children(Self::show_more("folders-more", report.folders.len() - shown, p, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::{Duplicates, Group, Report};
    use crate::duplicates::{Copy, FolderCopy};
    use std::path::PathBuf;

    fn state() -> Duplicates {
        let copy = |p: &str| Copy {
            path: PathBuf::from(p),
            modified: None,
        };
        let report = Report {
            groups: vec![
                Group {
                    size: 10,
                    copies: vec![copy(r"C:\a\x.bin"), copy(r"C:\b\x.bin")],
                },
                Group {
                    size: 5,
                    copies: vec![copy(r"C:\a\y.bin"), copy(r"C:\b\y.bin"), copy(r"C:\c\y.bin")],
                },
            ],
            folders: vec![FolderCopy {
                a: PathBuf::from(r"C:\a"),
                b: PathBuf::from(r"C:\b"),
                files: 2,
                bytes: 15,
                listing_a: vec![],
                listing_b: vec![],
            }],
            ..Default::default()
        };
        let mut state = Duplicates::default();
        state.set_report(Some(report));
        state
    }

    #[test]
    fn the_last_copy_never_goes_even_through_a_folder() {
        let mut s = state();
        assert!(s.toggle(0, 0).is_ok());
        assert!(s.toggle(0, 1).is_err(), "both copies of x");
        s.clear();
        // Folder a goes, and b's copy of x is ticked too: nothing keeps x.
        s.tick_folder(0, 0);
        s.ticked.insert((0, 1));
        assert!(s.plan().is_err());
        // Folder a alone: b keeps both files.
        s.ticked.clear();
        let plan = s.plan().unwrap();
        assert_eq!(plan.folders.len(), 1);
        assert!(plan.files.is_empty());
        // A ticked file inside the ticked folder isn't moved twice.
        s.ticked.insert((1, 0));
        s.ticked.insert((1, 2));
        let plan = s.plan().unwrap();
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].0.path, PathBuf::from(r"C:\c\y.bin"));
    }

    #[test]
    fn ticking_a_folder_side_switches_rather_than_takes_both() {
        let mut s = state();
        s.tick_folder(0, 0);
        s.tick_folder(0, 1);
        assert_eq!(s.folder_ticked.get(&0), Some(&1));
        s.tick_folder(0, 1);
        assert!(s.folder_ticked.is_empty());
    }
}
