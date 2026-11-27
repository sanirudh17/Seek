//! Similar images in Find Duplicates (Phase 9.5): pictures side by side with
//! their sizes, a similarity level, and the same choose-what-goes rules as
//! exact duplicates. Labelled similar, not identical; nothing is selected for
//! the person; one picture of every group always stays; each ticked picture
//! must look as it did when compared; moves are journaled, so Undo works.
use super::*;
use crate::similar::{LEVELS, Picture, Scan};
use std::collections::HashSet;
use std::sync::{Mutex, atomic::AtomicBool};

/// Groups drawn per page.
const PAGE: usize = 12;

#[derive(Default)]
pub(super) struct Similar {
    folder: Option<PathBuf>,
    scan: Option<Arc<Scan>>,
    /// Index into `LEVELS`.
    level: usize,
    groups: Vec<Vec<usize>>,
    /// Pictures ticked for the Recycle Bin, by index in the scan.
    ticked: HashSet<usize>,
    running: bool,
    progress: Arc<Mutex<(usize, usize, &'static str)>>,
    cancel: Option<Arc<AtomicBool>>,
    task: Option<Task<()>>,
    shown: usize,
    moved: Option<usize>,
    checking: bool,
}

impl Similar {
    fn regroup(&mut self) {
        self.groups = self
            .scan
            .as_ref()
            .map(|scan| crate::similar::groups(&scan.pictures, LEVELS[self.level].0))
            .unwrap_or_default();
        self.ticked.clear();
        self.shown = PAGE;
    }

    /// Ticks or unticks a picture; refuses the last one left in its group.
    fn toggle(&mut self, picture: usize) -> Result<(), &'static str> {
        if self.ticked.remove(&picture) {
            return Ok(());
        }
        let Some(group) = self.groups.iter().find(|g| g.contains(&picture)) else {
            return Ok(());
        };
        if group.iter().filter(|i| !self.ticked.contains(i)).count() <= 1 {
            return Err("One picture of each group always stays.");
        }
        self.ticked.insert(picture);
        Ok(())
    }
}

impl Seek {
    pub(super) fn pick_similar_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Find similar pictures in".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = paths.await
                && let Some(folder) = paths.pop()
            {
                let _ = this.update(cx, |this, cx| this.find_similar(Some(folder), cx));
            }
        })
        .detach();
    }

    /// Where "everywhere" looks: the home folder, and indexed locations
    /// outside it that are not a whole drive (a drive holds apps' and
    /// Windows' own pictures). Hidden folders such as AppData are skipped
    /// while listing.
    fn everywhere(&self) -> Vec<PathBuf> {
        let home = PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default());
        let mut folders = vec![home.clone()];
        for root in &self.roots {
            let drive = root.parent().is_none();
            if !drive && !root.starts_with(&home) && !home.starts_with(root) {
                folders.push(root.clone());
            }
        }
        folders
    }

    /// Compares the pictures in one folder, or (`None`) everywhere.
    pub(super) fn find_similar(&mut self, folder: Option<PathBuf>, cx: &mut Context<Self>) {
        let folders = match &folder {
            Some(folder) => vec![folder.clone()],
            None => self.everywhere(),
        };
        if let Some(cancel) = &self.similar.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new((0, 0, "Listing pictures")));
        let state = &mut self.similar;
        state.cancel = Some(cancel.clone());
        state.progress = progress.clone();
        state.folder = folder.clone();
        state.scan = None;
        state.groups.clear();
        state.ticked.clear();
        state.moved = None;
        state.running = true;
        if state.level == 0 && state.groups.is_empty() {
            // Very similar by default.
            state.level = 1;
        }
        let skip_system = folder.is_none();
        let job = cx.background_executor().spawn(async move {
            crate::similar::scan_in(&folders, skip_system, &cancel, true, |done, total, stage| {
                if let Ok(mut p) = progress.lock() {
                    *p = (done, total, stage);
                }
            })
        });
        self.similar.task = Some(cx.spawn(async move |this, cx| {
            let scan = job.await;
            let _ = this.update(cx, |this, cx| {
                this.similar.scan = Some(Arc::new(scan));
                this.similar.regroup();
                this.similar.running = false;
                this.reveal_found();
                this.similar.cancel = None;
                cx.notify();
            });
        }));
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let running = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.similar.running
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

    fn stop_similar(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = &self.similar.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    /// Moves the ticked pictures to the Recycle Bin after checking, in the
    /// background, that each still looks as it did and that every group
    /// keeps an unchanged picture.
    fn recycle_similar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = &self.similar;
        let Some(scan) = state.scan.clone() else { return };
        if state.ticked.is_empty() {
            return;
        }
        let going: Vec<Picture> = state.ticked.iter().map(|&i| scan.pictures[i].clone()).collect();
        let staying: Vec<Picture> = state
            .groups
            .iter()
            .filter(|g| g.iter().any(|i| state.ticked.contains(i)))
            .filter_map(|g| g.iter().find(|i| !state.ticked.contains(i)))
            .map(|&i| scan.pictures[i].clone())
            .collect();
        self.similar.checking = true;
        cx.notify();
        let job = cx.background_executor().spawn(async move {
            if !staying.iter().all(crate::similar::unchanged) {
                return Err("A picture that stays has changed since it was compared. Search again.".to_string());
            }
            let (paths, changed): (Vec<_>, Vec<_>) = going.into_iter().partition(crate::similar::unchanged);
            Ok((paths.into_iter().map(|p| p.path).collect::<Vec<_>>(), changed.len()))
        });
        cx.spawn_in(window, async move |this, cx| {
            let checked = job.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.similar.checking = false;
                let (paths, changed) = match checked {
                    Ok(x) => x,
                    Err(why) => {
                        this.message = Some(why);
                        cx.notify();
                        return;
                    }
                };
                if paths.is_empty() {
                    this.message = Some("These pictures changed since they were compared. Search again.".into());
                    cx.notify();
                    return;
                }
                let count = paths.len();
                let gone: HashSet<PathBuf> = paths.iter().cloned().collect();
                let pending_before = this.files_pending;
                this.run_file_op(crate::file_ops::Operation::Recycle { paths }, window, cx);
                if this.files_pending > pending_before {
                    let state = &mut this.similar;
                    if let Some(scan) = state.scan.take() {
                        state.scan = Some(Arc::new(Scan {
                            pictures: scan
                                .pictures
                                .iter()
                                .filter(|p| !gone.contains(&p.path))
                                .cloned()
                                .collect(),
                            seen: scan.seen,
                            stopped: scan.stopped.clone(),
                        }));
                    }
                    state.regroup();
                    state.moved = Some(count);
                    this.refresh_recycle_bin(cx);
                    this.refresh_capacity(cx);
                    if let Some(page) = &this.storage_page {
                        let scope = page.scope.clone();
                        this.measure_storage_again(scope, cx);
                    }
                    if changed > 0 {
                        this.message = Some(format!("Skipped {changed} that changed since they were compared."));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn similar_buttons(&self, p: crate::appearance::Palette, cx: &mut Context<Self>) -> Div {
        use crate::tokens::*;
        if self.similar.running {
            return div().child(
                super::settings_view::control("stop-similar", "Stop", p, cx)
                    .h(BUTTON_HEIGHT)
                    .on_click(cx.listener(|this, _, _, cx| this.stop_similar(cx))),
            );
        }
        // As for identical files: everywhere, or one folder; the last search
        // can be run again.
        let searched = self.similar.scan.is_some();
        let folder = self.similar.folder.clone();
        let in_folder = folder.is_some();
        div()
            .flex()
            .gap_2()
            .when(searched && in_folder, |d| {
                d.child(
                    super::settings_view::control("similar-again", "Search again", p, cx)
                        .h(BUTTON_HEIGHT)
                        .on_click(cx.listener(move |this, _, _, cx| this.find_similar(folder.clone(), cx))),
                )
            })
            .child(
                super::settings_view::control(
                    "similar-everywhere",
                    if searched && !in_folder { "Search again" } else { "Search everywhere" },
                    p,
                    cx,
                )
                .h(BUTTON_HEIGHT)
                .on_click(cx.listener(|this, _, _, cx| this.find_similar(None, cx))),
            )
            .child(
                super::settings_view::control("pick-similar", "In a folder…", p, cx)
                    .h(BUTTON_HEIGHT)
                    .on_click(cx.listener(|this, _, window, cx| this.pick_similar_folder(window, cx))),
            )
    }

    pub(super) fn similar_card(&self, mut card: Div, p: crate::appearance::Palette, cx: &mut Context<Self>) -> Div {
        use super::storage_view::human;
        use crate::tokens::*;
        use gpui_component::checkbox::Checkbox;
        let state = &self.similar;
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
                    .child(div().flex_1().min_w_0().text_size(TEXT_SMALL).child(format!("Moved {count} to the Recycle Bin.")))
                    .child(
                        div().flex_shrink_0().child(
                            super::settings_view::control("similar-undo", "Undo", p, cx)
                                .h(BUTTON_HEIGHT)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.similar.moved = None;
                                    this.file_action(super::daily::Action::UndoRename, None, window, cx);
                                    cx.notify();
                                })),
                        ),
                    ),
            );
        }
        if state.running {
            let (done, total, stage) = state.progress.lock().map(|p| *p).unwrap_or_default();
            let line = if total > 0 {
                format!("{stage} · {done} of {total}")
            } else {
                format!("{stage}…")
            };
            return card.child(note(line));
        }
        let Some(scan) = &state.scan else {
            return card.child(note(
                "Finds pictures that look nearly the same (resized, recompressed or lightly edited) in one folder and its subfolders, by comparing small fingerprints of each picture. These are similar, not identical: look at them side by side and tick the ones to move to the Recycle Bin. Nothing is selected for you, and one picture of each group always stays.".into(),
            ));
        };
        let level = state.level;
        let levels = super::settings_view::segmented(
            "similar-level",
            &[LEVELS[0].1, LEVELS[1].1, LEVELS[2].1],
            level,
            self.segment_motion.get("similar-level").copied(),
            self.motion_enabled,
            p,
            cx,
            move |this, index, _, cx| {
                this.slide_segment("similar-level", level, index);
                this.similar.level = index;
                this.similar.regroup();
                cx.notify();
            },
        );
        let groups = state.groups.len();
        let mut summary = format!(
            "{groups} {} of similar pictures among {}",
            if groups == 1 { "group" } else { "groups" },
            // Those compared: after Stop, fewer than were found.
            scan.pictures.len()
        );
        if let Some(why) = &scan.stopped {
            summary = format!("{why} · {summary}");
        }
        card = card.child(
            div()
                .px_4()
                .py_3()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap_3()
                .border_b_1()
                .border_color(p.line)
                .child(div().text_size(TEXT_SMALL).text_color(p.muted).child(summary))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().text_size(TEXT_SMALL).text_color(p.muted).child("Similarity"))
                        .child(levels)
                        .when(!state.ticked.is_empty(), |d| {
                            d.child(
                                super::settings_view::control("similar-clear", "Clear", p, cx)
                                    .h(BUTTON_HEIGHT)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.similar.ticked.clear();
                                        cx.notify();
                                    })),
                            )
                        }),
                ),
        );
        if state.groups.is_empty() {
            return card.child(note(format!(
                "No similar pictures among {} at this level.",
                scan.seen
            )));
        }
        let shown = state.shown.min(state.groups.len());
        let mut list = div().flex().flex_col();
        for (g, group) in state.groups.iter().take(shown).enumerate() {
            let mut tiles = div().flex().flex_wrap().gap_3();
            for &i in group {
                let picture = &scan.pictures[i];
                let ticked = state.ticked.contains(&i);
                let name = picture
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let folder = picture
                    .path
                    .parent()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();
                tiles = tiles.child(
                    div()
                        .id(("similar-picture", i))
                        .w(px(132.))
                        .p_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .rounded(RADIUS_CONTROL)
                        .border_1()
                        .border_color(if ticked { p.accent.opacity(0.6) } else { p.line })
                        .when(ticked, |d| d.opacity(0.6))
                        .child(
                            div()
                                .h(px(96.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(match &picture.thumbnail {
                                    Some(thumb) => img(thumb.clone())
                                        .max_w(px(116.))
                                        .max_h(px(96.))
                                        .object_fit(ObjectFit::Contain)
                                        .into_any_element(),
                                    None => div().text_size(TEXT_SMALL).text_color(p.muted).child("No preview").into_any_element(),
                                }),
                        )
                        .child(div().truncate().text_size(TEXT_SMALL).font_weight(FontWeight::MEDIUM).child(name))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(p.muted)
                                .child(format!("{}×{} · {}", picture.width, picture.height, human(picture.size))),
                        )
                        .child(div().truncate().text_size(px(11.)).text_color(p.muted).child(folder))
                        .child(
                            Checkbox::new(("similar-tick", i))
                                .checked(ticked)
                                .label("Recycle")
                                .accessibility_label(SharedString::from(format!(
                                    "Move {} to the Recycle Bin",
                                    picture.path.to_string_lossy()
                                )))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Err(why) = this.similar.toggle(i) {
                                        this.message = Some(why.into());
                                    }
                                    cx.notify();
                                })),
                        ),
                );
            }
            list = list.child(
                div()
                    .px_4()
                    .py_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .when(g > 0, |d| d.border_t_1().border_color(p.line))
                    .child(
                        div()
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child(format!("{} similar pictures", group.len())),
                    )
                    .child(tiles),
            );
        }
        let left = state.groups.len() - shown;
        if left > 0 {
            list = list.child(
                div().px_4().py_3().border_t_1().border_color(p.line).child(
                    super::settings_view::control("similar-more", format!("Show {} more", left.min(PAGE)), p, cx)
                        .h(BUTTON_HEIGHT)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.similar.shown += PAGE;
                            cx.notify();
                        })),
                ),
            );
        }
        let chosen = state.ticked.len();
        let chosen_bytes: u64 = state.ticked.iter().map(|&i| scan.pictures[i].size).sum();
        card.child(list).child(
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
                        .child("Similar, not identical: compare before you tick · one picture of each group stays"),
                )
                .child(
                    div().flex_shrink_0().child(
                        Button::new("similar-recycle")
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
                            .on_click(cx.listener(|this, _, window, cx| this.recycle_similar(window, cx))),
                    ),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Similar;

    #[test]
    fn the_last_picture_of_a_group_never_goes() {
        let mut state = Similar {
            groups: vec![vec![0, 1, 2]],
            ..Default::default()
        };
        assert!(state.toggle(0).is_ok());
        assert!(state.toggle(1).is_ok());
        assert!(state.toggle(2).is_err());
        assert!(state.toggle(1).is_ok());
        assert!(state.toggle(2).is_ok());
    }
}
