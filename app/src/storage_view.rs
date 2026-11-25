//! Storage overview (Phase 9.1), shown in place of the file list: free space,
//! a coloured bar by category, and the largest folders with drill-down. All
//! numbers come from the index (seek_engine Engine::storage), never a walk of
//! the disk; measuring is bounded and says why it stopped.
use super::*;
use super::interface::thousands;
use seek_engine::live::storage::{CATEGORIES, Storage};
use gpui_component::menu::DropdownMenu;
use gpui_component_assets::IconName as Glyph;
use std::sync::atomic::AtomicBool;

pub(super) struct StoragePage {
    /// The drive being shown, and the folder drilled into (the drive itself
    /// at first).
    pub(super) drive: PathBuf,
    pub(super) scope: PathBuf,
    report: Option<Storage>,
    measuring: bool,
    cancel: Arc<AtomicBool>,
    generation: u64,
    /// The page's scroll, and whether to bring Find duplicates (at the
    /// bottom) into view once the measurement has laid the page out.
    scroll: ScrollHandle,
    reveal_duplicates: bool,
    /// A search started from Jump: show its results when they arrive.
    reveal_results: bool,
    _task: Task<()>,
}

/// How long one measurement may run before it reports what it has.
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

pub(super) fn human(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e12 {
        format!("{:.2} TB", b / 1e12)
    } else if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{bytes} B")
    }
}

fn cluster_size(root: &std::path::Path) -> u64 {
    use std::os::windows::ffi::OsStrExt;
    let mut root = root.to_path_buf();
    if !root.to_string_lossy().ends_with('\\') {
        root.push("");
    }
    let wide: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut sectors, mut bytes, mut free, mut total) = (0u32, 0u32, 0u32, 0u32);
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceW(
            wide.as_ptr(),
            &mut sectors,
            &mut bytes,
            &mut free,
            &mut total,
        )
    } != 0;
    if ok && sectors > 0 && bytes > 0 {
        sectors as u64 * bytes as u64
    } else {
        4096
    }
}

/// Each category's tint; Windows and Other stay neutral.
pub(super) fn category_color(index: usize, p: crate::appearance::Palette) -> Hsla {
    // Ten hues spaced far apart so neighbours never blur together, lighter on
    // dark themes and deeper on light ones. Windows and Other stay quiet.
    let dark = p.text.l > 0.5;
    let (s, l) = if dark { (0.68, 0.62) } else { (0.62, 0.46) };
    let hue = |deg: f32| hsla(deg / 360., s, l, 1.);
    match index {
        0 => hue(265.), // Developer: violet
        1 => hue(214.), // Apps: blue
        2 => hsla(215. / 360., 0.16, if dark { 0.58 } else { 0.5 }, 1.), // Windows: slate
        3 => hue(40.),  // Documents: amber
        4 => hue(328.), // Pictures: pink
        5 => hue(6.),   // Videos: red
        6 => hue(145.), // Music: green
        7 => hue(188.), // Cloud: cyan
        8 => hue(82.),  // Temp and caches: lime
        _ => hsla(0., 0., if dark { 0.5 } else { 0.62 }, 1.), // Other: grey
    }
}

/// A share for the legend that never reads 0% for something that is there.
fn percent(share: f64) -> String {
    if share >= 100. || share == 0. {
        format!("{share:.0}%")
    } else if share >= 1. {
        // Rounded down, so a kind beside others never claims all of it.
        format!("{}%", (share.floor() as u32).max(1))
    } else if share >= 0.1 {
        format!("{share:.1}%")
    } else {
        "<0.1%".into()
    }
}

impl Seek {
    pub(super) fn open_storage(&mut self, drive: Option<PathBuf>, cx: &mut Context<Self>) {
        let Some(drive) = drive.or_else(|| self.roots.first().cloned()) else {
            self.message = Some("No locations are indexed yet.".into());
            cx.notify();
            return;
        };
        self.close_worktrees();
        self.storage_page = Some(StoragePage {
            drive: drive.clone(),
            scope: drive.clone(),
            report: None,
            measuring: false,
            cancel: Arc::new(AtomicBool::new(false)),
            generation: 0,
            scroll: ScrollHandle::new(),
            reveal_duplicates: false,
            reveal_results: false,
            _task: Task::ready(()),
        });
        self.measure_storage(drive, cx);
    }

    /// Storage with Find duplicates in view, already searching the folder in
    /// view (or everywhere) for identical files or similar pictures: what
    /// Jump's Find duplicates and Find similar images do.
    pub(super) fn open_find_duplicates(&mut self, similar: bool, cx: &mut Context<Self>) {
        let folder = self.scope.clone();
        self.open_storage(None, cx);
        self.duplicates.similar_mode = similar;
        if similar {
            self.find_similar(folder, cx);
        } else {
            self.find_duplicates(folder, cx);
        }
        if let Some(page) = &mut self.storage_page {
            page.reveal_duplicates = true;
            page.reveal_results = true;
            page.scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    /// Duplicates or similar pictures have been found: when the search came
    /// from Jump, scroll down to the results (once).
    pub(super) fn reveal_found(&mut self) {
        if let Some(page) = &mut self.storage_page
            && std::mem::take(&mut page.reveal_results)
        {
            page.scroll.scroll_to_bottom();
        }
    }

    pub(super) fn close_storage(&mut self) {
        if let Some(page) = self.storage_page.take() {
            page.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Measures the page's folder again (after a cleanup freed space).
    pub(super) fn measure_storage_again(&mut self, scope: PathBuf, cx: &mut Context<Self>) {
        self.measure_storage(scope, cx);
    }

    fn measure_storage(&mut self, scope: PathBuf, cx: &mut Context<Self>) {
        let Some(page) = &mut self.storage_page else {
            return;
        };
        page.cancel.store(true, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        page.cancel = cancel.clone();
        page.scope = scope.clone();
        page.measuring = true;
        page.generation += 1;
        let generation = page.generation;
        let drive = page.drive.clone();
        let job = cx.background_executor().spawn(async move {
            let Some(engine) = crate::backend::ENGINE.get() else {
                return Storage {
                    stopped: Some("The index is still loading".into()),
                    ..Default::default()
                };
            };
            engine.storage(
                &scope,
                cluster_size(&drive),
                std::time::Instant::now() + DEADLINE,
                &cancel,
            )
        });
        page._task = cx.spawn(async move |this, cx| {
            let report = job.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(page) = &mut this.storage_page
                    && page.generation == generation
                {
                    // The index is still loading or hasn't reached this
                    // folder (after an update or on first start): measure
                    // again shortly instead of stopping at a blank page.
                    let indexed = crate::file_changes::under_any(&page.scope, &this.roots);
                    let mut report = report;
                    if !indexed && report.stopped.as_deref() == Some("This folder isn't in the index yet") {
                        report.stopped = Some("Not indexed · add it in Settings > Locations to see what uses space".into());
                    }
                    let waiting = indexed
                        && report.stopped.as_deref().is_some_and(|why| {
                            why == "The index is still loading" || why == "This folder isn't in the index yet"
                        });
                    page.report = Some(report);
                    page.measuring = false;
                    if std::mem::take(&mut page.reveal_duplicates) {
                        page.scroll.scroll_to_bottom();
                    }
                    if waiting {
                        let scope = page.scope.clone();
                        cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(std::time::Duration::from_secs(2)).await;
                            let _ = this.update(cx, |this, cx| {
                                if this.storage_page.as_ref().is_some_and(|p| p.generation == generation) {
                                    this.measure_storage(scope, cx);
                                }
                            });
                        })
                        .detach();
                    }
                    cx.notify();
                }
            });
        });
        cx.notify();
    }

    pub(super) fn storage_view(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::tokens::*;
        let Some(page) = &self.storage_page else {
            return div().into_any_element();
        };
        let capacity = self.places.capacity.get(&page.drive).cloned();
        let report = page.report.clone().unwrap_or_default();
        let at_drive = page.scope == page.drive;
        // Header: title, the drive picker, and the measuring line.
        let owner = cx.entity().downgrade();
        // Every drive on the PC, and any other indexed location.
        let mut roots = drives();
        for root in &self.roots {
            if !roots.iter().any(|r| r.as_os_str().eq_ignore_ascii_case(root.as_os_str())) {
                roots.push(root.clone());
            }
        }
        let drive_name = page.drive.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        let picker = super::settings_view::choice_control("storage-drive", drive_name, p, cx)
            .h(BUTTON_HEIGHT)
            .w(px(180.))
            .dropdown_menu(move |menu, window, _| {
                let mut menu = super::daily::menu_style(menu, window);
                for root in &roots {
                    let owner = owner.clone();
                    let chosen = root.clone();
                    menu = menu.item(
                        gpui_component::menu::PopupMenuItem::new(
                            root.to_string_lossy().trim_start_matches(r"\\?\").to_string(),
                        )
                        .on_click(move |_, _, cx| {
                            let chosen = chosen.clone();
                            let _ = owner.update(cx, |this, cx| this.open_storage(Some(chosen), cx));
                        }),
                    );
                }
                menu
            });
        let status = if page.measuring {
            "Measuring…".to_string()
        } else if let Some(reason) = &report.stopped {
            match reason.as_str() {
                "The index is still loading" | "This folder isn't in the index yet" => {
                    "Waiting for the index; this fills in by itself".to_string()
                }
                _ => reason.clone(),
            }
        } else {
            format!(
                "{} files · {} · {} on disk",
                thousands(report.files as usize),
                human(report.bytes),
                human(report.on_disk)
            )
        };
        // Two bars: the drive (used against free), then what this
        // measurement found, by category, scaled to fill its own bar so the
        // shares can be read even when it covers a small part of the drive.
        let used = capacity
            .as_ref()
            .map(|c| c.total.saturating_sub(c.free));
        let measured: u64 = report.categories.iter().map(|(b, _)| *b).sum();
        let mut order: Vec<usize> = (0..CATEGORIES.len()).collect();
        order.sort_by(|a, b| report.categories[*b].0.cmp(&report.categories[*a].0));
        let grow = page.generation;
        let motion_on = self.motion_enabled;
        let track = |h: f32| {
            div()
                .h(px(h))
                .w_full()
                .flex()
                .rounded(px(h / 2.))
                .overflow_hidden()
                .bg(p.text.opacity(0.08))
        };
        // One pill, clipped round at both ends. Every kind in the key shows
        // in the bar in its own colour: a kind under a pixel would otherwise
        // blend into its neighbours as a colour the key doesn't have, so each
        // gets at least 3px, taken from the largest (flex shrinks in
        // proportion to size), and a 2px gap keeps colours apart. The key
        // gives the exact shares.
        let segment = |key: String, share: f32, color: Hsla| {
            div()
                .h_full()
                .w(relative(share))
                .min_w(px(3.))
                .flex_shrink_1()
                .bg(color)
                .with_animation(
                    SharedString::from(key),
                    crate::motion::enter(),
                    move |d, t| if motion_on { d.w(relative(share * t)) } else { d },
                )
        };
        let drive_bar = used.zip(capacity.as_ref()).map(|(used, c)| {
            track(6.).child(segment(
                format!("storage-drive-{grow}"),
                used as f32 / c.total.max(1) as f32,
                p.text.opacity(0.55),
            ))
        });
        let mut bar = track(10.).gap(px(2.));
        for (n, index) in order.iter().enumerate() {
            let bytes = report.categories[*index].0;
            if bytes == 0 {
                continue;
            }
            bar = bar.child(segment(
                format!("storage-bar-{grow}-{n}"),
                bytes as f32 / measured.max(1) as f32,
                category_color(*index, p),
            ));
        }
        let mut legend = div().w_full().grid().grid_cols(3).gap_x_6().gap_y_2();
        for index in order {
            let (bytes, files) = report.categories[index];
            if bytes == 0 {
                continue;
            }
            let share = bytes as f64 * 100. / measured.max(1) as f64;
            legend = legend.child(
                div()
                    .id(("storage-category", index))
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .child(
                        div()
                            .size(px(10.))
                            .flex_shrink_0()
                            .rounded(px(3.))
                            .bg(category_color(index, p)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(TEXT_SMALL)
                            .child(CATEGORIES[index]),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family(FONT_MONO)
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child(format!("{} · {}", human(bytes), percent(share))),
                    )
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(format!(
                            "{} files",
                            thousands(files as usize)
                        ))
                        .build(window, cx)
                    }),
            );
        }
        let measured_label = div()
            .flex()
            .items_baseline()
            .justify_between()
            .text_size(TEXT_SMALL)
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(if page.scope == page.drive { "By kind" } else { "By kind, in this folder" }),
            )
            .child(div().text_color(p.muted).child(format!("{} measured", human(measured))));
        let summary = div()
            .p_5()
            .flex()
            .flex_col()
            .gap_4()
            .rounded(RADIUS_CARD)
            .border_1()
            .border_color(p.text.opacity(0.1))
            .bg(p.settings_card)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap_3()
                            .child(
                                div()
                                    .text_size(TEXT_TITLE)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(match &capacity {
                                        Some(c) => format!("{} free of {}", human(c.free), human(c.total)),
                                        None => human(report.bytes),
                                    }),
                            )
                            .when_some(capacity.as_ref(), |d, c| {
                                let used = c.total.saturating_sub(c.free);
                                d.child(
                                    div()
                                        .text_size(TEXT_SMALL)
                                        .text_color(p.muted)
                                        .child(format!(
                                            "{} used · {:.0}%",
                                            human(used),
                                            used as f64 * 100. / c.total.max(1) as f64
                                        )),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child(status),
                    ),
            )
            .children(drive_bar)
            .when(measured > 0, |d| d.child(measured_label).child(bar).child(legend));
        // Where we are: the drive, then each folder drilled into.
        let mut crumbs = div().flex().items_center().gap_1().text_size(TEXT_SMALL);
        let mut walk = page.drive.clone();
        // Compared without case: the index and the sidebar may spell the
        // same drive differently.
        let lower = |p: &std::path::Path| PathBuf::from(p.to_string_lossy().to_lowercase());
        let at_top = lower(&page.scope) == lower(&page.drive);
        let rest: Vec<String> = if at_top {
            vec![]
        } else {
            match lower(&page.scope).strip_prefix(lower(&page.drive)) {
                Ok(r) => {
                    // Take the real spelling from the scope's own tail.
                    let n = r.components().count();
                    let all: Vec<String> = page
                        .scope
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect();
                    all[all.len().saturating_sub(n)..].to_vec()
                }
                Err(_) => vec![page.scope.to_string_lossy().into_owned()],
            }
        };
        let mut parts = vec![(page.drive.clone(), "Largest folders".to_string())];
        let outside = !at_top && rest.len() == 1 && PathBuf::from(&rest[0]).is_absolute();
        for part in rest {
            walk = if outside { PathBuf::from(&part) } else { walk.join(&part) };
            parts.push((walk.clone(), part));
        }
        let last = parts.len() - 1;
        for (index, (path, label)) in parts.into_iter().enumerate() {
            if index > 0 {
                crumbs = crumbs.child(div().text_color(p.muted).child("›"));
            }
            crumbs = crumbs.child(
                div()
                    .id(("storage-crumb", index))
                    .px_1()
                    .rounded(px(4.))
                    .when(index == last, |d| d.font_weight(FontWeight::SEMIBOLD))
                    .when(index != last, |d| {
                        d.text_color(p.muted)
                            .cursor_pointer()
                            .hover(move |s| s.text_color(p.text))
                    })
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.measure_storage(path.clone(), cx)
                    })),
            );
        }
        let largest = report.children.first().map_or(1, |(_, b)| (*b).max(1));
        let mut rows = div().flex().flex_col();
        for (index, (path, bytes)) in report.children.iter().take(40).enumerate() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            let target = path.clone();
            let reveal = path.clone();
            let share = *bytes as f64 / largest as f64;
            rows = rows.child(
                div()
                    .id(("storage-folder", index))
                    .h(px(38.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded(RADIUS_ROW)
                    .cursor_pointer()
                    .hover(move |s| s.bg(p.hover))
                    .child(Icon::new(Glyph::Folder).size(px(15.)).text_color(p.muted))
                    .child(div().w(px(220.)).flex_shrink_0().truncate().text_size(TEXT_BODY).child(name))
                    .child(
                        div()
                            .flex_1()
                            .h(px(6.))
                            .rounded(px(3.))
                            .bg(p.text.opacity(0.06))
                            .child(
                                div()
                                    .h_full()
                                    .rounded(px(3.))
                                    .w(relative(share as f32))
                                    .bg(p.accent.opacity(0.7)),
                            ),
                    )
                    .child(
                        div()
                            .w(px(84.))
                            .flex_shrink_0()
                            .text_right()
                            .font_family(FONT_MONO)
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child(human(*bytes)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.measure_storage(target.clone(), cx)
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, window, cx| {
                            this.close_storage();
                            this.scope_to(Some(reveal.clone()), window, cx);
                        }),
                    )
                    .tooltip(|window, cx| {
                        gpui_component::tooltip::Tooltip::new("Click to look inside · right-click to open")
                            .build(window, cx)
                    }),
            );
        }
        if !at_drive && report.here > 0 {
            rows = rows.child(
                div()
                    .h(px(34.))
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(TEXT_SMALL)
                    .text_color(p.muted)
                    .child("Files in this folder")
                    .child(div().font_family(FONT_MONO).child(human(report.here))),
            );
        }
        let folders = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .px_1()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(crumbs)
                    .when(!at_top, |d| {
                        let drive = page.drive.clone();
                        let up = page
                            .scope
                            .parent()
                            .filter(|parent| parent.starts_with(&page.drive) && parent != &page.drive.as_path())
                            .map(std::path::Path::to_path_buf)
                            .unwrap_or(drive);
                        d.child(
                            super::settings_view::control("storage-back", "Back", p, cx)
                                .h(BUTTON_HEIGHT)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.measure_storage(up.clone(), cx)
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .p_2()
                    .rounded(RADIUS_CARD)
                    .border_1()
                    .border_color(p.text.opacity(0.1))
                    .bg(p.settings_card)
                    .child(if report.children.is_empty() && !page.measuring {
                        div()
                            .py_6()
                            .text_center()
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child("No folders here")
                            .into_any_element()
                    } else {
                        rows.into_any_element()
                    }),
            );
        div()
            .id("storage-page")
            .track_focus(&self.page_focus)
            .role(Role::Region)
            .aria_label("Storage")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&page.scroll)
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
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(TEXT_PAGE)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Storage"),
                            )
                            .child(picker),
                    )
                    .child(summary)
                    .child(folders)
                        .children(self.cleanup_section(p, cx))
                    .child(self.duplicates_section(p, cx)),
            )
            .into_any_element()
    }
}

/// The PC's drives with something in them: fixed, removable and network.
fn drives() -> Vec<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i) as char))
        .filter(|root| {
            let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
            // 2 removable, 3 fixed, 4 network.
            matches!(unsafe { GetDriveTypeW(wide.as_ptr()) }, 2..=4)
        })
        .map(PathBuf::from)
        .collect()
}
