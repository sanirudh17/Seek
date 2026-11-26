//! Git worktrees page (Phase 9.4), shown in place of the file list like
//! Storage. Repositories come from the index's `.git` entries plus any the
//! person added; each worktree shows its branch, Git's notes, whether it is
//! clean and what rebuildable folders it holds. Removing one shows a plan of
//! what would be lost, asks for explicit permanent-delete confirmation,
//! works the plan out again before anything is removed, and permanently
//! deletes the worktree folder and Git's admin record of it.
use super::*;
use crate::worktrees::{Git, Plan, Repository, State, Status};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

/// The last scan's results, so reopening the page shows them at once while
/// a fresh scan runs.
type Remembered = (
    Vec<Repository>,
    HashMap<PathBuf, Result<Status, String>>,
    HashMap<PathBuf, u64>,
);
static REMEMBERED: std::sync::Mutex<Option<Remembered>> = std::sync::Mutex::new(None);

/// Worktrees whose status and sizes are read after a scan.
const MAX_CHECKED: usize = 80;
/// `.git` entries asked of the index.
const MAX_GIT_ENTRIES: usize = 20_000;

pub(super) struct WorktreesPage {
    repos: Option<Vec<Repository>>,
    /// A scan is running (results on screen may be from the last one).
    scanning: bool,
    error: Option<String>,
    status: HashMap<PathBuf, Result<Status, String>>,
    sizes: HashMap<PathBuf, u64>,
    review: Option<Review>,
    generation: u64,
    cancel: Arc<AtomicBool>,
    _task: Task<()>,
}

struct Review {
    repo: usize,
    worktree: usize,
    plan: Option<Result<Plan, String>>,
    /// Second-step explicit confirmation before permanent deletion.
    confirm_permanent: bool,
    busy: bool,
}

fn found_git() -> Result<Git, String> {
    let git = Git::find().ok_or("Git isn't installed, or Seek can't find git.exe on PATH.")?;
    let version = git.version()?;
    if version < crate::worktrees::MIN_VERSION {
        return Err(format!(
            "Git {}.{} is too old to list worktrees; Seek needs 2.36 or later.",
            version.0, version.1
        ));
    }
    Ok(git)
}

fn index_size(path: &std::path::Path, cancel: &AtomicBool) -> u64 {
    crate::backend::ENGINE.get().map_or(0, |engine| {
        engine
            .storage(
                path,
                4096,
                std::time::Instant::now() + std::time::Duration::from_secs(2),
                cancel,
            )
            .bytes
    })
}

impl Seek {
    pub(super) fn open_worktrees(&mut self, cx: &mut Context<Self>) {
        self.close_storage();
        let cancel = Arc::new(AtomicBool::new(false));
        let remembered = REMEMBERED.lock().ok().and_then(|r| r.clone());
        let (repos, status, sizes) = match remembered {
            Some((repos, status, sizes)) => (Some(repos), status, sizes),
            None => (None, HashMap::new(), HashMap::new()),
        };
        self.worktrees_page = Some(WorktreesPage {
            repos,
            scanning: false,
            error: None,
            status,
            sizes,
            review: None,
            generation: 0,
            cancel,
            _task: Task::ready(()),
        });
        self.scan_worktrees(cx);
    }

    pub(super) fn close_worktrees(&mut self) {
        if let Some(page) = self.worktrees_page.take() {
            page.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn scan_worktrees(&mut self, cx: &mut Context<Self>) {
        let registered = self.preferences.worktree_repos.clone();
        let Some(page) = &mut self.worktrees_page else {
            return;
        };
        page.cancel.store(true, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        page.cancel = cancel.clone();
        page.generation += 1;
        // What's on screen stays until the new results arrive.
        page.scanning = true;
        page.error = None;
        page.review = None;
        let generation = page.generation;
        let walk_cancel = cancel.clone();
        let job = cx.background_executor().spawn(async move {
            let git = found_git()?;
            let mut dot_gits = vec![];
            if let Some(engine) = crate::backend::ENGINE.get()
                && let Ok(mut query) = seek_engine::Query::parse("'.git", "")
            {
                query.limit = MAX_GIT_ENTRIES;
                dot_gits = engine
                    .search(&query)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|f| f.path)
                    .filter(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(".git")))
                    .collect();
            }
            // The person's own folders too, for repositories the index
            // doesn't cover (bounded; caches and app data are skipped).
            if let Some(home) = std::env::var_os("USERPROFILE") {
                dot_gits.extend(crate::worktrees::find_on_disk(std::path::Path::new(&home), &walk_cancel));
            }
            let mut commons = crate::worktrees::repositories_from(dot_gits);
            // Added by hand: kept whether or not they have linked worktrees.
            commons.extend(registered.into_iter().filter(|c| c.is_dir()));
            let repos = crate::worktrees::scan(&git, commons);
            Ok::<_, String>((git, repos))
        });
        page._task = cx.spawn(async move |this, cx| {
            let result = job.await;
            let git = match result {
                Ok((git, repos)) => {
                    let ok = this.update(cx, |this, cx| {
                        if let Some(page) = &mut this.worktrees_page
                            && page.generation == generation
                        {
                            // Keep last time's status for worktrees still listed.
                            let listed: std::collections::HashSet<PathBuf> = repos
                                .iter()
                                .flat_map(|r| r.worktrees.iter().map(|w| w.path.clone()))
                                .collect();
                            page.status.retain(|p, _| listed.contains(p));
                            page.sizes.retain(|p, _| listed.contains(p));
                            page.repos = Some(repos.clone());
                            cx.notify();
                        }
                    });
                    if ok.is_err() {
                        return;
                    }
                    (git, repos)
                }
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(page) = &mut this.worktrees_page
                            && page.generation == generation
                        {
                            page.error = Some(error);
                            page.repos = Some(vec![]);
                            page.scanning = false;
                            cx.notify();
                        }
                    });
                    return;
                }
            };
            // Then each worktree's status and rebuildable folders.
            let (git, repos) = git;
            let checks = cx
                .background_executor()
                .spawn(async move {
                    use rayon::prelude::*;
                    let worktrees: Vec<_> = repos
                        .iter()
                        .flat_map(|r| r.worktrees.iter())
                        .filter(|w| w.state == State::Ok && !w.bare)
                        .take(MAX_CHECKED)
                        .collect();
                    // Each worktree is its own git process; run them side by side.
                    let checked: Vec<_> = worktrees
                        .par_iter()
                        .filter(|_| !cancel.load(Ordering::Relaxed))
                        .map(|w| {
                            (
                                w.path.clone(),
                                crate::worktrees::status(&git, &w.path),
                                crate::worktrees::reclaimable(&w.path, |p| index_size(p, &cancel)),
                            )
                        })
                        .collect();
                    let mut status = HashMap::new();
                    let mut sizes = HashMap::new();
                    for (path, s, b) in checked {
                        status.insert(path.clone(), s);
                        sizes.insert(path, b);
                    }
                    (status, sizes, repos)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(page) = &mut this.worktrees_page
                    && page.generation == generation
                {
                    let (status, sizes, repos) = checks;
                    page.status = status;
                    page.sizes = sizes;
                    page.scanning = false;
                    if let Ok(mut remembered) = REMEMBERED.lock() {
                        *remembered = Some((repos, page.status.clone(), page.sizes.clone()));
                    }
                    cx.notify();
                }
            });
        });
        cx.notify();
    }

    fn add_worktree_repo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add repository".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = paths.await else {
                return;
            };
            let Some(folder) = paths.pop() else { return };
            let found = cx
                .background_executor()
                .spawn(async move { found_git().and_then(|git| crate::worktrees::common_dir(&git, &folder)) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match found {
                Ok(common) => {
                    let key = crate::worktrees::normal(&common);
                    if !this.preferences.worktree_repos.contains(&key) {
                        this.preferences.worktree_repos.push(key);
                        this.save_preferences(window, cx);
                    }
                    this.scan_worktrees(cx);
                }
                Err(error) => {
                    this.message = Some(error);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn forget_worktree_repo(&mut self, common: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.preferences.worktree_repos.retain(|c| *c != common);
        self.save_preferences(window, cx);
        self.scan_worktrees(cx);
    }

    /// Works out what removing a worktree would take, in the background.
    fn review_worktree(&mut self, repo: usize, worktree: usize, cx: &mut Context<Self>) {
        let Some(page) = &mut self.worktrees_page else {
            return;
        };
        let Some(r) = page.repos.as_ref().and_then(|r| r.get(repo)).cloned() else {
            return;
        };
        let Some(w) = r.worktrees.get(worktree).cloned() else {
            return;
        };
        page.review = Some(Review {
            repo,
            worktree,
            plan: None,
            confirm_permanent: false,
            busy: true,
        });
        let generation = page.generation;
        let job = cx.background_executor().spawn(async move {
            found_git().and_then(|git| crate::worktrees::plan(&git, &r, &w))
        });
        cx.spawn(async move |this, cx| {
            let plan = job.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(page) = &mut this.worktrees_page
                    && page.generation == generation
                    && let Some(review) = &mut page.review
                    && (review.repo, review.worktree) == (repo, worktree)
                {
                    review.plan = Some(plan);
                    review.busy = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Works the plan out again and permanently deletes the worktree only if
    /// nothing changed since it was reviewed.
    fn remove_worktree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = &mut self.worktrees_page else {
            return;
        };
        let Some(review) = &mut page.review else {
            return;
        };
        let Some(Ok(shown)) = review.plan.clone() else {
            return;
        };
        let Some(repo) = page.repos.as_ref().and_then(|r| r.get(review.repo)).cloned() else {
            return;
        };
        let Some(w) = repo.worktrees.get(review.worktree).cloned() else {
            return;
        };
        let wt_name = w
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| w.path.to_string_lossy().into_owned());
        review.busy = true;
        let job = cx.background_executor().spawn(async move {
            let git = found_git()?;
            // List again so a lock or a new worktree since the scan counts.
            let fresh = crate::worktrees::list(&git, &repo.common);
            let w = fresh
                .worktrees
                .iter()
                .find(|f| f.path == w.path)
                .cloned()
                .unwrap_or(w);
            let now = crate::worktrees::plan(&git, &fresh, &w)?;
            if now != shown {
                return Ok(Err(now));
            }
            crate::worktrees::delete_permanently(&git, &repo.common, &now)?;
            Ok(Ok(()))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(Ok(())) => {
                        if let Some(page) = &mut this.worktrees_page {
                            page.review = None;
                        }
                        let _ = this.backend.tx.send(Command::Changed);
                        this.scan_worktrees(cx);
                        this.show_quiet_toast(
                            format!("Permanently deleted worktree {wt_name}"),
                            window,
                            cx,
                        );
                    }
                    Ok(Err(changed)) => {
                        if let Some(review) =
                            this.worktrees_page.as_mut().and_then(|p| p.review.as_mut())
                        {
                            review.busy = false;
                            review.confirm_permanent = false;
                            review.plan = Some(Ok(changed));
                        }
                        this.message = Some(
                            "The worktree changed since you reviewed it, so nothing was deleted. Review the new list."
                                .into(),
                        );
                    }
                    Err(err) => {
                        if let Some(review) =
                            this.worktrees_page.as_mut().and_then(|p| p.review.as_mut())
                        {
                            review.busy = false;
                            review.confirm_permanent = false;
                        }
                        this.message = Some(err);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn worktrees_view(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::tokens::*;
        let Some(page) = &self.worktrees_page else {
            return div().into_any_element();
        };
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(TEXT_PAGE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Worktrees"),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        super::settings_view::control("worktrees-add", "Add repository…", p, cx)
                            .h(BUTTON_HEIGHT)
                            .on_click(cx.listener(|this, _, window, cx| this.add_worktree_repo(window, cx))),
                    )
                    .child(
                        super::settings_view::control(
                            "worktrees-refresh",
                            if page.scanning { "Refreshing…" } else { "Refresh" },
                            p,
                            cx,
                        )
                            .h(BUTTON_HEIGHT)
                            .disabled(page.scanning)
                            .on_click(cx.listener(|this, _, _, cx| this.scan_worktrees(cx))),
                    ),
            );
        let note = |text: String| {
            div()
                .px_4()
                .py_5()
                .text_size(TEXT_SMALL)
                .text_color(p.muted)
                .child(text)
        };
        let card = || {
            div()
                .flex()
                .flex_col()
                .rounded(RADIUS_CARD)
                .border_1()
                .border_color(p.text.opacity(0.1))
                .bg(p.settings_card)
        };
        let mut body = div().flex().flex_col().gap_6();
        if let Some(error) = &page.error {
            body = body.child(card().child(note(error.clone())));
        }
        match &page.repos {
            None => body = body.child(card().child(note("Finding repositories…".into()))),
            Some(repos) if repos.is_empty() && page.error.is_none() => {
                body = body.child(card().child(note(
                    "No repositories with linked worktrees on this PC. Add a repository by hand to see its worktrees.".into(),
                )))
            }
            Some(repos) => {
                for (ri, repo) in repos.iter().enumerate() {
                    body = body.child(self.worktree_repo(ri, repo, page, p, cx));
                }
            }
        }
        div()
            .id("worktrees-page")
            .track_focus(&self.page_focus)
            .role(Role::Region)
            .aria_label("Worktrees")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .bg(p.pane)
            .child(
                div()
                    .max_w(px(880.))
                    .mx_auto()
                    .px_8()
                    .py_6()
                    .flex()
                    .flex_col()
                    .gap_6()
                    .child(header)
                    .child(body),
            )
            .into_any_element()
    }

    fn worktree_repo(
        &self,
        ri: usize,
        repo: &Repository,
        page: &WorktreesPage,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        use crate::tokens::*;
        let registered = self.preferences.worktree_repos.contains(&repo.common);
        let common = repo.common.clone();
        let title = div()
            .px_1()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .child(
                        div()
                            .text_size(TEXT_BODY)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(repo.name()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child(repo.common.to_string_lossy().into_owned()),
                    ),
            )
            .when(registered, |d| {
                d.child(
                    super::settings_view::control(("worktrees-forget", ri), "Forget", p, cx)
                        .h(BUTTON_HEIGHT)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.forget_worktree_repo(common.clone(), window, cx)
                        })),
                )
            });
        let mut card = div()
            .flex()
            .flex_col()
            .rounded(RADIUS_CARD)
            .border_1()
            .border_color(p.text.opacity(0.1))
            .bg(p.settings_card);
        if let Some(error) = &repo.error {
            // Git's own words, dubious ownership included.
            card = card.child(
                div()
                    .px_4()
                    .py_4()
                    .text_size(TEXT_SMALL)
                    .text_color(p.muted)
                    .child(error.clone()),
            );
        }
        for (wi, w) in repo.worktrees.iter().enumerate() {
            card = card.child(self.worktree_row(ri, wi, w, page, p, cx).when(wi > 0, |d| {
                d.border_t_1().border_color(p.line)
            }));
            if let Some(review) = &page.review
                && (review.repo, review.worktree) == (ri, wi)
            {
                card = card.child(self.worktree_review(review, p, cx));
            }
        }
        div().flex().flex_col().gap_2().child(title).child(card)
    }

    fn worktree_row(
        &self,
        ri: usize,
        wi: usize,
        w: &crate::worktrees::Worktree,
        page: &WorktreesPage,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        use crate::tokens::*;
        let name = w
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| w.path.to_string_lossy().into_owned());
        let mut detail = vec![if w.bare {
            "bare".to_string()
        } else if let Some(branch) = &w.branch {
            branch.clone()
        } else {
            format!(
                "detached at {}",
                w.head.as_deref().map(|h| &h[..h.len().min(7)]).unwrap_or("?")
            )
        }];
        // Words, not colour, carry each state.
        if w.main {
            detail.push("main checkout".into());
        }
        match w.state {
            State::Missing => detail.push("missing (drive not connected?)".into()),
            State::Stale => detail.push("stale".into()),
            State::Replaced => detail.push("holds another checkout".into()),
            State::Unreadable => detail.push("unreadable".into()),
            State::Ok => {}
        }
        if let Some(reason) = &w.locked {
            detail.push(if reason.is_empty() {
                "locked".into()
            } else {
                format!("locked: {reason}")
            });
        }
        if w.prunable.is_some() {
            detail.push("prunable".into());
        }
        let status = match page.status.get(&w.path) {
            Some(Ok(s)) if s.clean() => "Clean".to_string(),
            Some(Ok(s)) => format!("{} changed", s.changes.len()),
            Some(Err(_)) => "Status skipped".into(),
            None if w.state == State::Ok && !w.bare => "…".into(),
            None => String::new(),
        };
        let status_reason = match page.status.get(&w.path) {
            Some(Err(e)) => Some(e.clone()),
            _ => None,
        };
        let size = page
            .sizes
            .get(&w.path)
            .filter(|b| **b > 0)
            .map(|b| format!("{} rebuildable", super::storage_view::human(*b)));
        let reveal = w.path.clone();
        let removable = !w.main && !w.bare;
        let reviewing = page
            .review
            .as_ref()
            .is_some_and(|r| (r.repo, r.worktree) == (ri, wi));
        div()
            .id(("worktree", ri * 1_000 + wi))
            .min_h(px(52.))
            .px_4()
            .py_2()
            .flex()
            .items_center()
            .gap_3()
            .child(
                Icon::empty()
                    .path("seek/git-branch.svg")
                    .size(px(15.))
                    .text_color(p.muted),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().truncate().text_size(TEXT_BODY).child(name))
                    .child(
                        div()
                            .truncate()
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child(detail.join(" · ")),
                    ),
            )
            .children(size.map(|s| {
                div()
                    .font_family(FONT_MONO)
                    .text_size(TEXT_SMALL)
                    .text_color(p.muted)
                    .child(s)
            }))
            .child(
                div()
                    .id(("worktree-status", ri * 1_000 + wi))
                    .w(px(96.))
                    .text_right()
                    .text_size(TEXT_SMALL)
                    .text_color(p.muted)
                    .child(status)
                    .when_some(status_reason, |d, reason| {
                        d.tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(reason.clone()).build(window, cx)
                        })
                    }),
            )
            .when(w.state != State::Missing, |d| {
                d.child(
                    super::settings_view::control(("worktree-open", ri * 1_000 + wi), "Open", p, cx)
                        .h(BUTTON_HEIGHT)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.close_worktrees();
                            this.scope_to(Some(reveal.clone()), window, cx);
                        })),
                )
            })
            .when(removable, |d| {
                d.child(
                    super::settings_view::control(
                        ("worktree-remove", ri * 1_000 + wi),
                        if reviewing { "Close" } else { "Remove…" },
                        p,
                        cx,
                    )
                    .h(BUTTON_HEIGHT)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let open = this
                            .worktrees_page
                            .as_ref()
                            .and_then(|p| p.review.as_ref())
                            .is_some_and(|r| (r.repo, r.worktree) == (ri, wi));
                        if open {
                            if let Some(page) = &mut this.worktrees_page {
                                page.review = None;
                            }
                            cx.notify();
                        } else {
                            this.review_worktree(ri, wi, cx);
                        }
                    })),
                )
            })
    }

    fn worktree_review(&self, review: &Review, p: crate::appearance::Palette, cx: &mut Context<Self>) -> Div {
        use crate::tokens::*;
        let line = |text: String| {
            div()
                .truncate()
                .text_size(TEXT_SMALL)
                .child(text)
        };
        let mut panel = div()
            .mx_4()
            .mb_3()
            .p_3()
            .rounded(RADIUS_CONTROL)
            .bg(p.text.opacity(0.04))
            .flex()
            .flex_col()
            .gap_2();
        let plan = match &review.plan {
            None => return panel.child(line("Working out what removing it would take…".into())),
            Some(Err(error)) => {
                return panel.child(
                    div()
                        .text_size(TEXT_SMALL)
                        .child(error.clone()),
                );
            }
            Some(Ok(plan)) => plan,
        };
        if plan.at_risk() {
            panel = panel.child(
                div()
                    .text_size(TEXT_SMALL)
                    .font_weight(FontWeight::MEDIUM)
                    .child("Removing it would lose work the repository doesn't keep:"),
            );
        } else {
            panel = panel.child(
                div()
                    .text_size(TEXT_SMALL)
                    .text_color(p.muted)
                    .child("Nothing is at risk: no changed files, and every commit is on another branch, tag or remote."),
            );
        }
        if !plan.changes.is_empty() {
            panel = panel.child(
                div()
                    .text_size(TEXT_SMALL)
                    .text_color(p.muted)
                    .child(format!("{} changed or untracked files", plan.changes.len())),
            );
            for change in plan.changes.iter().take(10) {
                panel = panel.child(line(change.clone()).font_family(FONT_MONO));
            }
            if plan.changes.len() > 10 {
                panel = panel.child(line(format!("and {} more", plan.changes.len() - 10)));
            }
        }
        if !plan.commits.is_empty() {
            panel = panel.child(
                div()
                    .text_size(TEXT_SMALL)
                    .text_color(p.muted)
                    .child(format!("{} commits found nowhere else", plan.commits.len())),
            );
            for (hash, subject) in plan.commits.iter().take(10) {
                panel = panel.child(line(format!("{} {subject}", &hash[..hash.len().min(7)])).font_family(FONT_MONO));
            }
            if plan.commits.len() > 10 {
                panel = panel.child(line(format!("and {} more", plan.commits.len() - 10)));
            }
            if plan.detached {
                panel = panel.child(
                    div()
                        .text_size(TEXT_SMALL)
                        .child("This worktree's HEAD is detached. Once deleted, Git may drop these commits at its next cleanup."),
                );
            }
        }
        if !plan.folder_exists {
            panel = panel.child(
                div()
                    .text_size(TEXT_SMALL)
                    .text_color(p.muted)
                    .child("The folder is already gone; only Git's admin record will be deleted."),
            );
        }

        let danger_bg = rgba(0xE11D4826);
        let danger_border = rgba(0xE11D4866);
        let danger_text = rgb(0xFDA4AF);

        if review.confirm_permanent {
            let wt_path = plan.worktree.display().to_string();
            panel.child(
                div()
                    .mt_1()
                    .p_3()
                    .rounded(RADIUS_CONTROL)
                    .bg(danger_bg)
                    .border_1()
                    .border_color(danger_border)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_size(TEXT_SMALL)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child("Permanently delete worktree? This cannot be undone."),
                    )
                    .child(
                        div()
                            .truncate()
                            .font_family(FONT_MONO)
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .child(wt_path),
                    )
                    .child(
                        div()
                            .pt_1()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .child(
                                super::settings_view::control("worktree-cancel-perm", "Cancel", p, cx)
                                    .h(BUTTON_HEIGHT)
                                    .disabled(review.busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(review) = this
                                            .worktrees_page
                                            .as_mut()
                                            .and_then(|p| p.review.as_mut())
                                        {
                                            review.confirm_permanent = false;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("worktree-confirm-permanent")
                                    .role(Role::Button)
                                    .aria_label("Delete permanently")
                                    .h(BUTTON_HEIGHT)
                                    .px_3()
                                    .rounded(RADIUS_CONTROL)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .bg(rgba(0xE11D4838))
                                    .border_1()
                                    .border_color(danger_border)
                                    .text_size(TEXT_SMALL)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(danger_text)
                                    .hover(|s| s.bg(rgba(0xE11D4855)))
                                    .when(review.busy, |d| d.opacity(0.5).cursor_default())
                                    .child(super::interface::button_text(if review.busy {
                                        "Deleting…"
                                    } else {
                                        "Delete permanently"
                                    }))
                                    .when(!review.busy, |d| {
                                        d.on_click(cx.listener(|this, _, window, cx| {
                                            this.remove_worktree(window, cx);
                                        }))
                                    }),
                            ),
                    ),
            )
        } else {
            panel.child(
                div()
                    .pt_1()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child("Permanently deletes the worktree folder and Git's record of it (bypasses the Recycle Bin)"),
                    )
                    .child(
                        div()
                            .id("worktree-confirm")
                            .role(Role::Button)
                            .aria_label("Delete permanently")
                            .h(BUTTON_HEIGHT)
                            .px_3()
                            .rounded(RADIUS_CONTROL)
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .bg(danger_bg)
                            .border_1()
                            .border_color(danger_border)
                            .text_size(TEXT_SMALL)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(danger_text)
                            .hover(|s| s.bg(rgba(0xE11D4840)))
                            .when(review.busy, |d| d.opacity(0.5).cursor_default())
                            .child(super::interface::button_text(if plan.at_risk() {
                                "Delete permanently & lose work…"
                            } else {
                                "Delete permanently…"
                            }))
                            .when(!review.busy, |d| {
                                d.on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(review) = this
                                        .worktrees_page
                                        .as_mut()
                                        .and_then(|p| p.review.as_mut())
                                    {
                                        review.confirm_permanent = true;
                                    }
                                    cx.notify();
                                }))
                            }),
                    ),
            )
        }
    }
}
