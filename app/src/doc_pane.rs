//! Document pane (3.5): a layout leaf that stays on one file beside the panes
//! you browse. It reuses the Quick Look reader and editor (rendered Markdown
//! with Edit/Read, highlighted code, text, safe-save and auto-save rules) and
//! the document preview host (PDF and Office) through a reader slot: each
//! document pane owns a reader state, preview state and preview worker, and
//! `with_reader` swaps them into the shared fields only for the duration of a
//! call, a render, a listener or a task that belongs to that pane.
use super::*;
use crate::pane_layout::PaneId;
use gpui_component_assets::IconName as Glyph;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReaderTarget {
    QuickLook,
    Document(PaneId),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Document,
    Image,
    Video,
    Other,
}

pub(super) struct DocPane {
    pub(super) id: PaneId,
    pub(super) path: PathBuf,
    row: Option<Row>,
    kind: Kind,
    error: Option<String>,
    reader: text_reader::State,
    pub(super) focus: FocusHandle,
    document_key: Option<crate::document_preview::Key>,
    document_pages: Option<u32>,
    document: Option<Result<crate::document_preview::Preview, String>>,
    documents: crate::document_preview::Worker,
    _replies: Task<()>,
}

fn row_for(path: &std::path::Path) -> Result<Row, String> {
    let meta = std::fs::metadata(path).map_err(|error| error.to_string())?;
    if meta.is_dir() {
        return Err("Folders open in a browsing pane, not a document pane.".into());
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |time| time.as_secs() as u32);
    let mut dates = std::collections::HashMap::new();
    Ok(Row::with_dates(
        seek_engine::Found {
            path: path.to_path_buf(),
            kind: seek_engine::walk::KIND_FILE,
            size: meta.len(),
            mtime,
            score: 0,
        },
        &mut dates,
    ))
}

impl Seek {
    /// Run `f` with document pane `target`'s reader and preview state in the
    /// shared reader fields. Returns None when that pane has been closed.
    /// Gives keyboard focus back to the surface showing the reader (Quick
    /// Look or its document pane), after a click on a control inside it.
    pub(super) fn focus_reader(&self, window: &mut Window, cx: &mut App) {
        match self.reader_slot {
            ReaderTarget::QuickLook => self.preview_focus.focus(window, cx),
            ReaderTarget::Document(id) => {
                if let Some(doc) = self.doc_panes.iter().find(|doc| doc.id == id) {
                    doc.focus.focus(window, cx);
                }
            }
        }
    }

    pub(super) fn with_reader<R>(
        &mut self,
        target: ReaderTarget,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        if self.reader_slot == target {
            return Some(f(self));
        }
        let ReaderTarget::Document(id) = target else {
            // Quick Look's state is always in place outside a swap.
            return Some(f(self));
        };
        let index = self.doc_panes.iter().position(|doc| doc.id == id)?;
        self.swap_reader(index);
        self.reader_slot = target;
        let result = f(self);
        // `f` cannot remove document panes (closing is deferred), so the
        // index still names the same pane.
        self.swap_reader(index);
        self.reader_slot = ReaderTarget::QuickLook;
        if let Some(id) = self.close_document.take() {
            self.remove_document_pane(id);
        }
        Some(result)
    }

    fn swap_reader(&mut self, index: usize) {
        let doc = &mut self.doc_panes[index];
        std::mem::swap(&mut self.text_reader, &mut doc.reader);
        std::mem::swap(&mut self.preview_focus, &mut doc.focus);
        std::mem::swap(&mut self.document_key, &mut doc.document_key);
        std::mem::swap(&mut self.document_pages, &mut doc.document_pages);
        std::mem::swap(&mut self.document, &mut doc.document);
        std::mem::swap(&mut self.documents, &mut doc.documents);
    }

    /// A listener that runs against the reader it was created for.
    pub(super) fn reader_listener<E: ?Sized + 'static>(
        target: ReaderTarget,
        cx: &Context<Self>,
        f: impl Fn(&mut Self, &E, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(&E, &mut Window, &mut App) + 'static {
        cx.listener(move |this, event: &E, window, cx| {
            this.with_reader(target, |this| f(this, event, window, cx));
        })
    }

    /// Quick Look closes; a document pane stays open and returns keyboard
    /// focus to the file list (its draft is kept).
    pub(super) fn close_reader(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.reader_slot {
            ReaderTarget::QuickLook => self.close_quick_look(window, cx),
            ReaderTarget::Document(_) => {
                self.table.read(cx).focus_handle(cx).focus(window, cx);
                cx.notify();
            }
        }
    }

    /// Open `path` in a document pane beside the focused pane. An existing
    /// document pane without unsaved changes is reused.
    pub(super) fn open_document_pane(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row = match row_for(&path) {
            Ok(row) => row,
            Err(error) => {
                self.message = Some(error);
                cx.notify();
                return;
            }
        };
        if let Some(index) = self.doc_panes.iter().rposition(|doc| !doc.reader.blocked()) {
            let id = self.doc_panes[index].id;
            self.doc_panes[index].path = path;
            self.reload_document_pane(id, window, cx);
            self.remember_session(cx);
            return;
        }
        if self.panes.len() + self.doc_panes.len() + 1 >= crate::pane_layout::MAX_PANES {
            self.message = Some(format!(
                "Seek shows up to {} panes. Close a pane before opening a document.",
                crate::pane_layout::MAX_PANES
            ));
            cx.notify();
            return;
        }
        let (Some(pane), Some(split)) = (self.allocate_id(), self.allocate_id()) else {
            return;
        };
        let id = PaneId(pane);
        // Beside the focused pane when halving it leaves both readable;
        // otherwise beside the widest pane, so a document never opens as a
        // sliver next to a narrow column.
        let widths = self.pane_widths(window, cx);
        let focused = widths
            .iter()
            .find(|(leaf, _)| *leaf == self.pane_identity)
            .map_or(f32::MAX, |(_, width)| *width);
        let beside = if focused >= 560. {
            self.pane_identity
        } else {
            widths
                .iter()
                .filter(|(leaf, _)| !self.doc_panes.iter().any(|doc| doc.id == *leaf))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map_or(self.pane_identity, |(leaf, _)| *leaf)
        };
        let mut layout = self.layout.clone();
        if !layout.replace(
            beside,
            crate::pane_layout::Node::Split {
                id: crate::pane_layout::SplitId(split),
                axis: crate::pane_layout::Axis::Horizontal,
                fraction: 5500,
                first: Box::new(crate::pane_layout::Node::Pane {
                    id: beside,
                }),
                second: Box::new(crate::pane_layout::Node::Pane { id }),
            },
        ) || layout.validate_tree().is_err()
        {
            return;
        }
        self.layout = layout;
        self.unzoom();
        self.add_document_pane(id, path, Some(row), window, cx);
        self.pane_motion = Some((id, Instant::now()));
        self.remember_session(cx);
    }

    /// Create a document pane's state and start loading its file.
    pub(super) fn add_document_pane(
        &mut self,
        id: PaneId,
        path: PathBuf,
        row: Option<Row>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (documents, replies) = crate::document_preview::Worker::start();
        let replies = cx.spawn_in(window, async move |this, cx| {
            while let Ok(reply) = replies.recv().await {
                let alive = this.update_in(cx, |this, window, cx| {
                    let Some(doc) = this.doc_panes.iter_mut().find(|doc| doc.id == id) else {
                        return;
                    };
                    if !doc.documents.accepts(&reply) {
                        return;
                    }
                    if let Some(Ok(old)) = doc.document.take()
                        && let Some(image) = old.image
                    {
                        let _ = window.drop_image(image);
                    }
                    if let Ok(preview) = &reply.result {
                        doc.document_pages = (preview.info.pages > 0).then_some(preview.info.pages);
                    }
                    doc.document = Some(reply.result);
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        self.doc_panes.push(DocPane {
            id,
            path,
            row,
            kind: Kind::Other,
            error: None,
            reader: text_reader::State::new(),
            focus: cx.focus_handle(),
            document_key: None,
            document_pages: None,
            document: None,
            documents,
            _replies: replies,
        });
        self.reload_document_pane(id, window, cx);
    }

    /// Re-read the file from disk. Refused while the draft has unsaved changes.
    pub(super) fn reload_document_pane(
        &mut self,
        id: PaneId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.doc_panes.iter().position(|doc| doc.id == id) else {
            return;
        };
        if self.doc_panes[index].reader.blocked() {
            self.doc_panes[index].error =
                Some("Save or discard your changes before reloading.".into());
            cx.notify();
            return;
        }
        let path = self.doc_panes[index].path.clone();
        let row = row_for(&path);
        let doc = &mut self.doc_panes[index];
        let revision = doc.reader.revision_next();
        doc.reader = text_reader::State::new();
        doc.reader.set_revision(revision);
        doc.document = None;
        doc.document_pages = None;
        match row {
            Ok(row) => {
                doc.error = None;
                doc.kind = if super::text_reader::text_file(&row) {
                    Kind::Text
                } else if crate::document_preview::Key::new(&row, 0).is_some() {
                    Kind::Document
                } else if crate::icons::kind(&row.file.path, false) == crate::icons::Kind::Image {
                    Kind::Image
                } else if crate::video_edit::is_video(&row.file.path) {
                    Kind::Video
                } else {
                    Kind::Other
                };
                doc.row = Some(row.clone());
                match doc.kind {
                    Kind::Text => {
                        let key = media::Key::new(&row, 0, true);
                        self.with_reader(ReaderTarget::Document(id), |this| {
                            this.text_reader.key = Some(key.clone());
                            this.start_text_load(key, cx);
                        });
                    }
                    Kind::Document => {
                        let key = crate::document_preview::Key::new(&row, 0);
                        self.with_reader(ReaderTarget::Document(id), |this| {
                            this.document_key = key.clone();
                            // The worker ignores a request equal to its current
                            // one; reset first so Reload always reads again.
                            this.documents.request(None);
                            this.documents.request(key);
                        });
                    }
                    Kind::Image | Kind::Video | Kind::Other => self.refresh_preview(cx),
                }
            }
            Err(error) => {
                doc.row = None;
                doc.error = Some(format!("This file is unavailable: {error}"));
            }
        }
        cx.notify();
    }

    /// Close a document pane. Refused while its draft has unsaved changes.
    pub(super) fn close_document_pane(&mut self, id: PaneId, cx: &mut Context<Self>) {
        if let Some(ed) = self
            .picture_edit
            .as_mut()
            .filter(|e| e.host == picture_edit_view::EditHost::DocumentPane(id))
        {
            if ed.is_dirty() {
                ed.confirm_discard = true;
                cx.notify();
                return;
            }
            self.picture_edit = None;
        }
        if let Some(v) = self
            .video_edit
            .as_mut()
            .filter(|v| v.host == video_edit_view::VideoHost::DocumentPane(id))
        {
            if v.is_dirty() {
                v.confirm_discard = true;
                cx.notify();
                return;
            }
            self.video_edit = None;
        }
        let Some(doc) = self.doc_panes.iter_mut().find(|doc| doc.id == id) else {
            return;
        };
        if doc.reader.blocked() {
            doc.error = Some("Save or discard your changes before closing this document.".into());
            cx.notify();
            return;
        }
        if self.reader_slot == ReaderTarget::Document(id) {
            self.close_document = Some(id);
        } else {
            self.remove_document_pane(id);
        }
        self.remember_session(cx);
        cx.notify();
    }

    fn remove_document_pane(&mut self, id: PaneId) {
        if let Some(layout) = self.layout.clone().without(id) {
            self.layout = layout;
            self.doc_panes.retain(|doc| doc.id != id);
        }
    }

    /// Any document pane with unsaved changes keeps the window open.
    pub(super) fn documents_block_close(&mut self, cx: &mut Context<Self>) -> bool {
        let mut blocked = false;
        if let Some(ed) = self.picture_edit.as_mut()
            && ed.is_dirty()
        {
            ed.confirm_discard = true;
            blocked = true;
        }
        for doc in &mut self.doc_panes {
            if doc.reader.blocked() {
                doc.error = Some("Save or discard your changes before closing Seek.".into());
                blocked = true;
            }
        }
        if blocked {
            cx.notify();
        }
        blocked
    }

    /// The document pane whose reader holds keyboard focus, if any.
    /// The file a focused document pane shows, for Space to open in Quick
    /// Look, with the pane's focus to return to. None while editing text.
    pub(super) fn document_for_quick_look(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<(Row, FocusHandle)> {
        let id = self.focused_document(window, cx)?;
        let doc = self.doc_panes.iter().find(|doc| doc.id == id)?;
        if doc.reader.is_editing() {
            return None;
        }
        Some((doc.row.clone()?, doc.focus.clone()))
    }

    pub(super) fn focused_document(&self, window: &Window, cx: &App) -> Option<PaneId> {
        self.doc_panes.iter().find_map(|doc| {
            let editor = doc.reader.editor_focus(cx);
            (doc.focus.contains_focused(window, cx)
                || editor.is_some_and(|focus| focus.contains_focused(window, cx)))
            .then_some(doc.id)
        })
    }

    pub(super) fn document_keys(&self) -> Vec<media::Key> {
        self.doc_panes
            .iter()
            .filter(|doc| doc.kind == Kind::Image)
            .filter_map(|doc| doc.row.as_ref())
            .map(|row| media::Key::new(row, media::PREVIEW_EDGE, false))
            .collect()
    }

    pub(super) fn saved_documents(&self) -> Vec<crate::pane_layout::DocumentPane> {
        self.doc_panes
            .iter()
            .map(|doc| crate::pane_layout::DocumentPane {
                id: doc.id,
                path: Some(doc.path.clone()),
            })
            .collect()
    }

    pub(super) fn document_pane(
        &mut self,
        id: PaneId,
        p: crate::appearance::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(index) = self.doc_panes.iter().position(|doc| doc.id == id) else {
            return div().into_any_element();
        };
        let focused = self.focused_document(window, cx) == Some(id);
        let doc = &self.doc_panes[index];
        let name = doc
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let details = doc
            .row
            .as_ref()
            .map(|row| format!("{} · {} · {}", row.kind, row.size, row.modified))
            .unwrap_or_default();
        let error = doc.error.clone();
        let kind = doc.kind;
        let row = doc.row.clone();
        let focus = doc.focus.clone();
        let header = div()
            .h(px(44.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_b_1()
            .border_color(p.line)
            .bg(p.chrome)
            .child(match &row {
                Some(row) => div()
                    .size(px(20.))
                    .flex_shrink_0()
                    .child(super::interface::file_image(
                        row,
                        20.,
                        &self.atlas,
                        &self.cache,
                        false,
                    ))
                    .into_any_element(),
                None => Icon::new(Glyph::FileText)
                    .size(px(15.))
                    .text_color(p.muted)
                    .into_any_element(),
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(name.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .child(details),
                    ),
            )
            .when(
                kind == Kind::Image
                    && !self.picture_edit_for(picture_edit_view::EditHost::DocumentPane(id))
                    && row
                        .as_ref()
                        .is_some_and(|r| crate::image_tools::readable(&r.file.path)),
                |hdr| {
                    let edit_path = row.as_ref().unwrap().file.path.clone();
                    hdr.child(
                        gpui_base::Button::new(("document-edit-picture", id.0 as usize))
                            .accessibility_label(format!("Edit picture {name}"))
                            .h(px(28.))
                            .px_2()
                            .rounded(px(6.))
                            .text_size(px(12.))
                            .hover(move |d| d.bg(p.hover))
                            .child("Edit")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_picture_edit(
                                    picture_edit_view::EditHost::DocumentPane(id),
                                    edit_path.clone(),
                                    window,
                                    cx,
                                );
                            })),
                    )
                },
            )
            .child(
                gpui_base::Button::new(("document-reload", id.0 as usize))
                    .accessibility_label(format!("Reload {name}"))
                    .size(px(28.))
                    .rounded(px(6.))
                    .hover(move |d| d.bg(p.hover))
                    .child(Icon::new(Glyph::RefreshCw).size(px(14.)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.reload_document_pane(id, window, cx)
                    })),
            )
            .child(
                gpui_base::Button::new(("document-close", id.0 as usize))
                    .accessibility_label(format!("Close document pane {name}"))
                    .size(px(28.))
                    .rounded(px(6.))
                    .hover(move |d| d.bg(p.hover))
                    .child(Icon::new(gpui_component::IconName::Close).size(px(13.)))
                    .on_click(cx.listener(move |this, _, _, cx| this.close_document_pane(id, cx))),
            );
        let body: AnyElement = match (kind, row) {
            (_, None) => div()
                .p_4()
                .text_color(p.muted)
                .child(error.clone().unwrap_or_else(|| "Loading...".into()))
                .into_any_element(),
            (Kind::Text, _) => self
                .with_reader(ReaderTarget::Document(id), |this| {
                    this.text_reader_view(p, window, cx)
                })
                .unwrap_or_else(|| div().into_any_element()),
            (Kind::Document, _) => self
                .with_reader(ReaderTarget::Document(id), |this| {
                    // A page image fills the pane under its page controls;
                    // text-only documents and loading states use the shared view.
                    let page = this
                        .document
                        .as_ref()
                        .and_then(|d| d.as_ref().ok())
                        .and_then(|preview| preview.image.clone());
                    match page {
                        Some(image) => {
                            let size = image.size(0);
                            let ratio = size.width.0.max(1) as f32 / size.height.0.max(1) as f32;
                            div()
                                .size_full()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .px_4()
                                .pt_2()
                                .pb_4()
                                .children(this.document_navigation(p, cx))
                                .child(
                                    // Top-aligned: the page spans the pane's width
                                    // at its own proportions, up to the pane's height.
                                    div()
                                        .flex_1()
                                        .min_h_0()
                                        .w_full()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .child(
                                            div().w_full().max_h_full().aspect_ratio(ratio).child(
                                                img(image)
                                                    .size_full()
                                                    .object_fit(ObjectFit::Contain),
                                            ),
                                        ),
                                )
                                .into_any_element()
                        }
                        None => div()
                            .size_full()
                            .px_4()
                            .pt_2()
                            .child(this.document_view(p, window, cx))
                            .into_any_element(),
                    }
                })
                .unwrap_or_else(|| div().into_any_element()),
            (Kind::Image, Some(row)) => {
                if self.picture_edit_for(picture_edit_view::EditHost::DocumentPane(id)) {
                    self.picture_edit_view(p, window, cx)
                } else {
                    let image = match self.cache.lock().unwrap().get(&media::Key::new(
                        &row,
                        media::PREVIEW_EDGE,
                        false,
                    )) {
                        Some(media::Value::Image(Some(image))) => Some(image),
                        _ => None,
                    };
                    match image {
                        Some(image) => div()
                            .size_full()
                            .p_4()
                            .child(img(image).size_full().object_fit(ObjectFit::Contain))
                            .into_any_element(),
                        None => div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(p.muted)
                            .child("Loading image...")
                            .into_any_element(),
                    }
                }
            }
            (Kind::Video, Some(row)) => self.video_edit_view(
                video_edit_view::VideoHost::DocumentPane(id),
                row.file.path.clone(),
                p,
                window,
                cx,
            ),
            (Kind::Other, Some(row)) => div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .text_color(p.muted)
                .child(super::interface::file_image(
                    &row,
                    96.,
                    &self.atlas,
                    &self.cache,
                    true,
                ))
                .child("No document preview for this kind of file.")
                .into_any_element(),
        };
        div()
            .id(("document-pane", id.0 as usize))
            .role(Role::Region)
            .aria_label(format!("Document pane: {name}"))
            .track_focus(&focus)
            .size_full()
            .flex()
            .flex_col()
            .min_w_0()
            .min_h_0()
            .child(header)
            .when_some(
                error.filter(|_| kind == Kind::Text || kind == Kind::Document),
                |d, error| {
                    d.child(
                        div()
                            .px_3()
                            .py_1()
                            .text_size(px(12.))
                            .text_color(p.muted)
                            .child(error),
                    )
                },
            )
            .child(div().flex_1().min_h_0().child(body))
            .when(focused, |d| d.key_context("SeekDocument"))
            .into_any_element()
    }
}
