use super::*;
use std::sync::atomic::AtomicBool;

pub(super) struct State {
    key: Option<(Vec<media::Key>, u64, bool)>,
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
        let next = (
            keys.clone(),
            self.cache_epoch,
            self.panel && !self.preview_enlarged,
        );
        if self.inspector.key.as_ref() == Some(&next) {
            return;
        }
        self.inspector.cancel();
        self.inspector.revision = self.inspector.revision.wrapping_add(1);
        let revision = self.inspector.revision;
        self.inspector.task = Task::ready(());
        self.inspector.key = Some(next.clone());
        self.inspector.summary = None;
        self.inspector.rested = false;
        self.inspector.loading = !keys.is_empty();
        self.inspector.cancel = Arc::new(AtomicBool::new(false));
        if keys.is_empty() {
            return;
        }
        let cancel = self.inspector.cancel.clone();
        let calculate = self.panel
            && !self.preview_enlarged
            && (keys.len() > 1 || keys.iter().any(|key| key.folder));
        self.inspector.loading = calculate;
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
        let mut view = div().flex().flex_col().gap_3().pt_3();
        if self.inspector.loading {
            return view
                .child(
                    div()
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
            view = view.child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Selection totals"),
            );
            if summary.selected > 1 {
                view = view.child(format!(
                    "{} items selected: {} files, {} folders",
                    summary.selected, summary.selected_files, summary.selected_folders
                ));
                if !summary.kinds.is_empty() {
                    view = view.child(
                        div().text_color(p.muted).child(
                            summary
                                .kinds
                                .iter()
                                .map(|(kind, n)| format!("{n} {kind}"))
                                .collect::<Vec<_>>()
                                .join(", "),
                        ),
                    );
                }
            }
            view = view.child(format!(
                "{} files, {} folders",
                summary.files, summary.folders
            ));
            let prefix = if summary.complete() { "" } else { "At least " };
            view = view.child(format!("{prefix}{} bytes", summary.bytes));
            view = view.child(div().text_color(p.muted).child(format!(
                "Size on disk: {}{} bytes",
                if summary.unknown_disk > 0 || !summary.complete() {
                    "at least "
                } else {
                    ""
                },
                summary.disk
            )));
            if !summary.complete() || summary.unknown_disk > 0 {
                view = view.child(div().text_color(p.muted).child(format!(
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
        if self.inspector.summary.is_some() {
            view = view.child(
                super::settings_view::control("refresh-inspector-totals", "Recalculate", p, cx)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.inspector.invalidate();
                        this.refresh_preview(cx);
                        cx.notify();
                    })),
            );
        }
        view.into_any_element()
    }
}
