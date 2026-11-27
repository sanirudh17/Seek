use super::*;
use std::sync::atomic::AtomicBool;

pub(super) struct State {
    key: Option<(Vec<media::Key>, bool)>,
    revision: u64,
    cancel: Arc<AtomicBool>,
    pub summary: Option<crate::inspector::Summary>,
    pub loading: bool,
    pub rested: bool,
    task: Task<()>,
}
impl State {
    pub fn new() -> Self {
        Self {
            key: None,
            revision: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            summary: None,
            loading: false,
            rested: false,
            task: Task::ready(()),
        }
    }
    pub fn invalidate(&mut self) {
        self.key = None;
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl Seek {
    pub(super) fn inspector_selection(&self, cx: &App) -> Vec<media::Key> {
        let table = self.table.read(cx);
        let data = table.delegate();
        if data.selected_many.is_empty() {
            return self
                .selected(cx)
                .map(|r| vec![media::Key::new(&r, 0, true)])
                .unwrap_or_default();
        }
        data.rows
            .iter()
            .filter(|r| data.selected_many.contains(&r.file.path))
            .map(|r| media::Key::new(r, 0, true))
            .collect()
    }
    pub(super) fn refresh_inspector(&mut self, cx: &mut Context<Self>) {
        let keys = if self.preview_enlarged {
            self.selected(cx)
                .map(|row| vec![media::Key::new(&row, 0, true)])
                .unwrap_or_default()
        } else if self.panel || matches!(self.view, View::Gallery | View::Columns) {
            self.inspector_selection(cx)
        } else {
            vec![]
        };
        let next = (keys.clone(), self.panel && !self.preview_enlarged);
        if self.inspector.key.as_ref() == Some(&next) {
            return;
        }
        self.inspector.cancel();
        self.inspector.revision = self.inspector.revision.wrapping_add(1);
        let revision = self.inspector.revision;
        self.inspector.task = Task::ready(());
        self.inspector.key = Some(next.clone());
        self.inspector.rested = false;
        self.inspector.cancel = Arc::new(AtomicBool::new(false));
        if keys.is_empty() {
            self.inspector.summary = None;
            self.inspector.loading = false;
            return;
        }
        let cancel = self.inspector.cancel.clone();
        let calculate = self.panel
            && !self.preview_enlarged
            && (keys.len() > 1 || keys.iter().any(|key| key.folder));
        // Compute an immediate in-memory summary from the selected rows so
        // multi-selection updates seamlessly on frame 0 with zero "Calculating..."
        // flash; the background scan then refines exact disk/subfolder totals in place.
        if calculate {
            let table = self.table.read(cx);
            let data = table.delegate();
            let mut instant = crate::inspector::Summary {
                selected: keys.len(),
                ..Default::default()
            };
            let selected_set: std::collections::HashSet<&std::path::PathBuf> =
                keys.iter().map(|k| &k.path).collect();
            for row in data.rows.iter() {
                if !selected_set.contains(&row.file.path) {
                    continue;
                }
                let is_dir = row.file.kind & 3 == seek_engine::walk::KIND_DIR;
                if is_dir {
                    instant.selected_folders += 1;
                    instant.folders += 1;
                    *instant.kinds.entry("Folder".into()).or_default() += 1;
                } else {
                    instant.selected_files += 1;
                    instant.files += 1;
                    instant.bytes = instant.bytes.saturating_add(row.file.size);
                    let est_disk = if row.file.kind & seek_engine::walk::FLAG_CLOUD != 0
                        || row.file.size == 0
                    {
                        0
                    } else {
                        row.file.size.div_ceil(4096) * 4096
                    };
                    instant.disk = instant.disk.saturating_add(est_disk);
                    let kind = row
                        .file
                        .path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_ascii_uppercase())
                        .unwrap_or_else(|| "File".into());
                    *instant.kinds.entry(kind).or_default() += 1;
                }
            }
            self.inspector.summary = Some(instant);
            self.inspector.loading = false;
        } else {
            self.inspector.summary = None;
            self.inspector.loading = false;
        }
        self.inspector.task = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(120))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.inspector.revision == revision {
                    this.inspector.rested = true;
                    this.refresh_preview(cx);
                    cx.notify();
                }
            });
            if !calculate {
                return;
            }
            let paths = keys.into_iter().map(|k| k.path).collect::<Vec<_>>();
            let summary = cx
                .background_executor()
                .spawn(async move { crate::inspector::scan(&paths, &cancel) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.inspector.revision == revision && this.inspector.key.as_ref() == Some(&next)
                {
                    this.inspector.summary = Some(summary);
                    this.inspector.loading = false;
                    cx.notify();
                }
            });
        });
    }
    pub(super) fn inspector_totals(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut view = div().flex().flex_col().gap_2().flex_shrink_0();
        if self.inspector.loading {
            return view
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Selection"),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child("Calculating selection totals..."),
                )
                .child(
                    super::settings_view::control("cancel-inspector-totals", "Cancel", p, cx)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.inspector.cancel();
                            cx.notify();
                        })),
                )
                .into_any_element();
        }
        if let Some(summary) = &self.inspector.summary {
            let mut header = div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(if summary.selected > 1 {
                            format!("{} items selected", summary.selected)
                        } else {
                            "Folder totals".into()
                        }),
                );
            header = header.child(
                div()
                    .id("refresh-inspector-totals")
                    .role(Role::Button)
                    .aria_label("Recalculate totals")
                    .px_2()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .rounded(crate::tokens::RADIUS_CONTROL)
                    .cursor_pointer()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .hover(move |s| s.bg(p.hover).text_color(p.text))
                    .child("Recalculate")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.inspector.invalidate();
                        this.refresh_preview(cx);
                        cx.notify();
                    })),
            );
            view = view.child(header);
            if summary.selected > 1 && !summary.kinds.is_empty() {
                view = view.child(
                    div().text_size(px(11.)).text_color(p.muted).child(
                        summary
                            .kinds
                            .iter()
                            .map(|(kind, n)| format!("{n} {kind}"))
                            .collect::<Vec<_>>()
                            .join(" · "),
                    ),
                );
            }
            let prefix = if summary.complete() { "" } else { "At least " };
            let disk_prefix = if summary.unknown_disk > 0 || !summary.complete() {
                "At least "
            } else {
                ""
            };
            let rows = [
                (
                    "Contents",
                    format!("{} files, {} folders", summary.files, summary.folders),
                ),
                (
                    "Total size",
                    format!("{prefix}{}", super::storage_view::human(summary.bytes)),
                ),
                (
                    "Size on disk",
                    format!("{disk_prefix}{}", super::storage_view::human(summary.disk)),
                ),
            ];
            let mut card = div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .p_3()
                .rounded(crate::tokens::RADIUS_CONTROL)
                .bg(p.text.opacity(0.035))
                .border_1()
                .border_color(p.line);
            for (label, value) in rows {
                card = card.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .text_size(px(11.))
                        .child(div().text_color(p.muted).child(label))
                        .child(
                            div()
                                .font_family(crate::tokens::FONT_MONO)
                                .text_right()
                                .child(value),
                        ),
                );
            }
            view = view.child(card);
            if !summary.complete() || summary.unknown_disk > 0 {
                view = view.child(div().text_size(px(11.)).text_color(p.muted).child(format!(
                    "{}{} skipped, {} unreadable, {} allocation sizes unavailable",
                    if summary.cancelled {
                        "Cancelled. "
                    } else {
                        "Partial. "
                    },
                    summary.skipped,
                    summary.errors,
                    summary.unknown_disk
                )));
            }
        }
        view.into_any_element()
    }

    /// Scrollable breakdown of selected items shown in the Info panel during
    /// multi-selection so vertical space is purposefully occupied with zero dead gap.
    pub(super) fn inspector_multi_list(
        &self,
        p: crate::appearance::Palette,
        cx: &App,
    ) -> AnyElement {
        const MAX_SHOWN: usize = 60;
        let table = self.table.read(cx);
        let data = table.delegate();
        let selected_rows: Vec<Row> = data
            .rows
            .iter()
            .filter(|r| data.selected_many.contains(&r.file.path))
            .take(MAX_SHOWN + 1)
            .cloned()
            .collect();
        let total_selected = data.selected_many.len();
        let overflow = total_selected.saturating_sub(MAX_SHOWN);
        let mut list = div()
            .id("inspector-multi-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(2.))
            .pt_1();
        for (idx, row) in selected_rows.iter().take(MAX_SHOWN).enumerate() {
            let is_dir = row.file.kind & 3 == seek_engine::walk::KIND_DIR;
            let size_label = if is_dir {
                "Folder".to_string()
            } else {
                super::storage_view::human(row.file.size)
            };
            list = list.child(
                div()
                    .id(("multi-item", idx))
                    .h(px(28.))
                    .px_2()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(crate::tokens::RADIUS_ROW)
                    .hover(move |s| s.bg(p.hover))
                    .child(super::interface::file_image(
                        row,
                        16.,
                        &self.atlas,
                        &self.cache,
                        false,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(p.text)
                            .child(row.name.clone()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family(crate::tokens::FONT_MONO)
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .child(size_label),
                    ),
            );
        }
        if overflow > 0 {
            list = list.child(
                div()
                    .px_2()
                    .py_1()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(format!("+{overflow} more items selected")),
            );
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_1()
            .pt_2()
            .border_t_1()
            .border_color(p.line)
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.muted)
                    .child("Selected items"),
            )
            .child(list)
            .into_any_element()
    }
}
