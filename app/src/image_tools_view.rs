//! The info panel's Image tools menu and its confirm dialog (Phase 8.3). The
//! dialog lists each copy's name and size before anything is written; the
//! work runs off the interface thread with progress and Cancel.
use super::*;
use crate::image_tools::{Format, Planned, Size, Tool};
use gpui_component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use std::sync::atomic::AtomicBool;

const SIZES: [&str; 5] = ["Half", "Third", "Quarter", "Width", "Height"];
/// One batch at a time, bounded like batch rename.
const LIMIT: usize = 500;

pub(super) struct ImageJob {
    tool: Tool,
    paths: Vec<PathBuf>,
    plans: Vec<Result<Planned, (PathBuf, String)>>,
    loading: bool,
    size_choice: usize,
    pixels: Entity<InputState>,
    /// (done, total) while copies are being written.
    progress: Option<(usize, usize)>,
    cancel: Arc<AtomicBool>,
    revision: u64,
    task: Task<()>,
    _subscription: Subscription,
}

impl Seek {
    /// Selected paths that the image tools can read.
    pub(super) fn selected_pictures(&self, cx: &App) -> Vec<PathBuf> {
        self.inspector_selection(cx)
            .into_iter()
            .map(|key| key.path)
            .filter(|path| crate::image_tools::readable(path))
            .collect()
    }

    pub(super) fn image_tools_menu(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.selected_pictures(cx).is_empty() {
            return None;
        }
        let owner = cx.entity().downgrade();
        let trigger = super::settings_view::choice_control("image-tools", "Image tools", p, cx)
            .h(crate::tokens::BUTTON_HEIGHT)
            .w_full();
        Some(
            trigger
                .dropdown_menu(move |menu, window, _| {
                    let mut menu = super::daily::menu_style(menu, window);
                    let item = |menu: PopupMenu, label: &str, tool: Tool| {
                        let owner = owner.clone();
                        menu.item(PopupMenuItem::new(label.to_string()).on_click(
                            move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.open_image_tool(tool, window, cx)
                                });
                            },
                        ))
                    };
                    for format in [Format::Png, Format::Jpeg, Format::Webp] {
                        menu = item(menu, &format!("Convert to {}", format.label()), Tool::Convert(format));
                    }
                    menu = menu.separator();
                    menu = item(menu, "Resize…", Tool::Resize(Size::Fraction(2)));
                    menu = menu.separator();
                    menu = item(menu, "Rotate left", Tool::RotateLeft);
                    menu = item(menu, "Rotate right", Tool::RotateRight);
                    menu = item(menu, "Rotate 180°", Tool::Rotate180);
                    menu = menu.separator();
                    menu = item(menu, "Flip horizontally", Tool::FlipHorizontal);
                    item(menu, "Flip vertically", Tool::FlipVertical)
                })
                .into_any_element(),
        )
    }

    pub(super) fn open_image_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        let paths = self.selected_pictures(cx);
        if paths.is_empty() {
            return;
        }
        if paths.len() > LIMIT {
            self.message = Some(format!("Select up to {LIMIT} pictures at a time."));
            cx.notify();
            return;
        }
        let pixels = cx.new(|cx| {
            InputState::new(window, cx)
                .context_menu(false)
                .placeholder("Pixels")
                .default_value("1280")
        });
        let subscription = cx.subscribe(&pixels, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.plan_image_tool(cx);
            }
        });
        self.image_job = Some(ImageJob {
            tool,
            paths,
            plans: vec![],
            loading: true,
            size_choice: 0,
            pixels,
            progress: None,
            cancel: Arc::new(AtomicBool::new(false)),
            revision: 0,
            task: Task::ready(()),
            _subscription: subscription,
        });
        self.image_job_focus.focus(window, cx);
        self.plan_image_tool(cx);
    }

    fn image_tool(&self, job: &ImageJob, cx: &App) -> Option<Tool> {
        if !matches!(job.tool, Tool::Resize(_)) {
            return Some(job.tool);
        }
        let pixels = || {
            job.pixels
                .read(cx)
                .value()
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|n| (1..=20_000).contains(n))
        };
        Some(Tool::Resize(match job.size_choice {
            0 => Size::Fraction(2),
            1 => Size::Fraction(3),
            2 => Size::Fraction(4),
            3 => Size::Width(pixels()?),
            _ => Size::Height(pixels()?),
        }))
    }

    /// Re-plan off the interface thread: names, sizes, skips.
    fn plan_image_tool(&mut self, cx: &mut Context<Self>) {
        let Some(job) = &self.image_job else {
            return;
        };
        let Some(tool) = self.image_tool(job, cx) else {
            if let Some(job) = &mut self.image_job {
                job.plans.clear();
                job.loading = false;
            }
            cx.notify();
            return;
        };
        let paths = job.paths.clone();
        let job = self.image_job.as_mut().unwrap();
        job.tool = tool;
        job.revision += 1;
        job.loading = true;
        let revision = job.revision;
        job.task = cx.spawn(async move |this, cx| {
            let plans = cx
                .background_executor()
                .spawn(async move { crate::image_tools::plan(&paths, tool) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(job) = &mut this.image_job
                    && job.revision == revision
                {
                    job.plans = plans;
                    job.loading = false;
                    cx.notify();
                }
            });
        });
        cx.notify();
    }

    fn run_image_tool(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(job) = &mut self.image_job else {
            return;
        };
        if job.loading || job.progress.is_some() {
            return;
        }
        let work: Vec<Planned> = job.plans.iter().filter_map(|p| p.as_ref().ok().cloned()).collect();
        if work.is_empty() {
            return;
        }
        let tool = job.tool;
        let cancel = job.cancel.clone();
        let total = work.len();
        job.progress = Some((0, total));
        let (send, receive) = async_channel::unbounded();
        cx.background_executor()
            .spawn(async move {
                let mut made = 0;
                let mut failed = vec![];
                for (index, planned) in work.iter().enumerate() {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    match crate::image_tools::run(planned, tool) {
                        Ok(_) => made += 1,
                        Err(error) => failed.push(format!(
                            "{}: {error}",
                            planned.source.file_name().unwrap_or_default().to_string_lossy()
                        )),
                    }
                    let _ = send.try_send(Err(index + 1));
                }
                let _ = send.try_send(Ok((made, failed)));
            })
            .detach();
        job.task = cx.spawn_in(window, async move |this, cx| {
            while let Ok(event) = receive.recv().await {
                let done = event.is_ok();
                let _ = this.update_in(cx, |this, window, cx| match event {
                    Err(count) => {
                        if let Some(job) = &mut this.image_job {
                            job.progress = Some((count, total));
                        }
                        cx.notify();
                    }
                    Ok((made, failed)) => {
                        this.image_job = None;
                        this.table.read(cx).focus_handle(cx).focus(window, cx);
                        let text = match made {
                            1 => "Saved 1 copy".to_string(),
                            n => format!("Saved {n} copies"),
                        };
                        this.message = failed.first().map(|first| {
                            if failed.len() == 1 {
                                first.clone()
                            } else {
                                format!("{first} (and {} more)", failed.len() - 1)
                            }
                        });
                        this.show_toast(text, false, window, cx);
                        cx.notify();
                    }
                });
                if done {
                    break;
                }
            }
        });
        cx.notify();
    }

    pub(super) fn close_image_tool(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(job) = &self.image_job
            && job.progress.is_some()
        {
            // Stop after the copy being written; finished copies stay.
            job.cancel.store(true, Ordering::Relaxed);
            return;
        }
        self.image_job = None;
        self.table.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    pub(super) fn image_tool_view(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::tokens::*;
        let Some(job) = &self.image_job else {
            return div().into_any_element();
        };
        let ready = job.plans.iter().filter(|p| p.is_ok()).count();
        let skipped = job.plans.len() - ready;
        let title = match job.tool {
            Tool::Resize(_) => format!(
                "Resize {} picture{}",
                job.paths.len(),
                if job.paths.len() == 1 { "" } else { "s" }
            ),
            tool => tool.title(),
        };
        let mut body = div().flex().flex_col().gap_3();
        if matches!(job.tool, Tool::Resize(_)) {
            let choice = job.size_choice;
            let sizes = super::settings_view::segmented(
                "image-size",
                &SIZES,
                choice,
                self.segment_motion.get("image-size").copied(),
                self.motion_enabled,
                p,
                cx,
                move |this, index, _, cx| {
                    this.slide_segment("image-size", choice, index);
                    if let Some(job) = &mut this.image_job {
                        job.size_choice = index;
                    }
                    this.plan_image_tool(cx);
                },
            );
            body = body.child(sizes).when(choice >= 3, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .w(px(140.))
                                .child(Input::new(&job.pixels).h(FIELD_HEIGHT)),
                        )
                        .child(
                            div()
                                .text_size(TEXT_SMALL)
                                .text_color(p.muted)
                                .child(if choice == 3 {
                                    "pixels wide, height keeps the proportions"
                                } else {
                                    "pixels high, width keeps the proportions"
                                }),
                        ),
                )
            });
        }
        let mut rows = div()
            .id("image-tool-rows")
            .max_h(px(300.))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        for (index, plan) in job.plans.iter().enumerate() {
            let (name, detail, ok) = match plan {
                Ok(planned) => (
                    planned
                        .target
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    format!(
                        "{}×{}  →  {}×{}",
                        planned.from.0, planned.from.1, planned.to.0, planned.to.1
                    ),
                    true,
                ),
                Err((path, reason)) => (
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    format!("Skipped: {reason}"),
                    false,
                ),
            };
            rows = rows.child(
                div()
                    .id(("image-tool-row", index))
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(TEXT_BODY)
                            .text_color(if ok { p.text } else { p.muted })
                            .child(name),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family(FONT_MONO)
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child(detail),
                    ),
            );
        }
        body = body.child(if job.loading {
            div()
                .py_3()
                .text_size(TEXT_SMALL)
                .text_color(p.muted)
                .child("Reading pictures…")
                .into_any_element()
        } else {
            rows.into_any_element()
        });
        let footer_note = if let Some((done, total)) = job.progress {
            format!("Saving {done} of {total}…")
        } else if skipped > 0 {
            format!("{ready} copies · {skipped} skipped · originals are kept")
        } else {
            format!(
                "{ready} cop{} beside the original{} · originals are kept",
                if ready == 1 { "y" } else { "ies" },
                if ready == 1 { "" } else { "s" }
            )
        };
        let progress_bar = job.progress.map(|(done, total)| {
            div()
                .h(px(4.))
                .w_full()
                .rounded_full()
                .bg(p.text.opacity(0.1))
                .child(
                    div()
                        .h_full()
                        .rounded_full()
                        .bg(p.accent)
                        .w(relative(done as f32 / total.max(1) as f32)),
                )
        });
        let running = job.progress.is_some();
        div()
            .id("image-tool-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000055))
            .child(
                div()
                    .id("image-tool")
                    .role(Role::Dialog)
                    .aria_label(title.clone())
                    .track_focus(&self.image_job_focus)
                    .w(px(560.))
                    .max_w(relative(0.94))
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .rounded(RADIUS_CARD)
                    .bg(crate::appearance::modal_surface(p))
                    .border_1()
                    .border_color(crate::appearance::modal_border(p))
                    .text_color(p.text)
                    .child(
                        div()
                            .text_size(TEXT_TITLE)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(body)
                    .children(progress_bar)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(TEXT_SMALL)
                                    .text_color(p.muted)
                                    .child(footer_note),
                            )
                            .child(
                                Button::new("image-tool-cancel")
                                    .ghost()
                                    .h(BUTTON_HEIGHT)
                                    .px_3()
                                    .rounded(RADIUS_CONTROL)
                                    .accessibility_label(if running { "Stop" } else { "Cancel" })
                                    .child(super::interface::button_text(if running {
                                        "Stop"
                                    } else {
                                        "Cancel"
                                    }))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_image_tool(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("image-tool-save")
                                    .h(BUTTON_HEIGHT)
                                    .px_3()
                                    .rounded(RADIUS_CONTROL)
                                    .bg(p.text)
                                    .text_color(crate::appearance::modal_surface(p))
                                    .font_weight(FontWeight::MEDIUM)
                                    .accessibility_label("Save copies")
                                    .disabled(job.loading || running || ready == 0)
                                    .child(super::interface::button_text(if ready == 1 {
                                        "Save copy"
                                    } else {
                                        "Save copies"
                                    }))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.run_image_tool(window, cx)
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}
