use super::doc_pane::ReaderTarget;
use super::*;
use gpui_component::{
    input::{Editor, EditorState},
    menu::{ContextMenuExt, DropdownMenu, PopupMenuItem},
    text::{MarkdownExtensions, TextViewState},
};

fn fence_language(raw: &str) -> &'static str {
    let lower = raw.trim().to_ascii_lowercase();
    match lower.as_str() {
        "rust" | "rs" => "rust",
        "javascript" | "typescript" | "js" | "ts" | "jsx" | "tsx" => "js",
        "json" | "jsonc" => "json",
        "python" | "py" => "python",
        "c" | "cpp" | "c++" | "cc" | "h" | "hpp" => "c",
        "csharp" | "c#" | "cs" => "csharp",
        "java" => "java",
        "go" | "golang" => "go",
        "kotlin" | "kt" => "kotlin",
        "swift" => "swift",
        "ruby" | "rb" => "ruby",
        "php" => "php",
        "lua" => "lua",
        "powershell" | "ps1" | "pwsh" => "powershell",
        "shell" | "sh" | "bash" | "zsh" | "fish" | "bat" | "cmd" => "shell",
        "sql" => "sql",
        "html" | "htm" => "html",
        "xml" | "svg" | "xaml" => "xml",
        "css" | "scss" | "sass" | "less" => "css",
        "dockerfile" | "docker" => "dockerfile",
        "toml" | "yaml" | "yml" => "config",
        "ini" | "conf" | "cfg" => "ini",
        "diff" | "patch" => "diff",
        "env" | "dotenv" => "env",
        "markdown" | "md" => "markdown",
        _ => crate::code_highlight::language(std::path::Path::new(&format!("a.{lower}"))),
    }
}

pub(super) struct InfoEditor {
    path: PathBuf,
    mtime: u32,
    size: u64,
    formatted: bool,
    wrap: bool,
    text_hash: u64,
    editor: Entity<EditorState>,
}

/// A 28 px icon button for the reader toolbar; the name and shortcut are in
/// the tooltip and the accessible label.
fn reader_icon(
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    p: crate::appearance::Palette,
) -> Button {
    Button::new(id)
        .ghost()
        .h(crate::tokens::BUTTON_HEIGHT)
        .w(crate::tokens::BUTTON_HEIGHT)
        .p_0()
        .rounded(crate::tokens::RADIUS_CONTROL)
        .accessibility_label(label)
        .tooltip(label)
        .child(Icon::empty().path(icon).size(px(15.)).text_color(p.text))
}
fn reader_control(id: &'static str, label: &'static str, p: crate::appearance::Palette) -> Button {
    Button::new(id)
        .ghost()
        .h(px(28.))
        .px_2()
        .py_0()
        .rounded(px(5.))
        .text_size(px(12.))
        .text_color(p.text)
        .accessibility_label(label)
        .child(super::interface::button_text(label))
}

pub(super) struct State {
    pub(super) key: Option<media::Key>,
    revision: usize,
    task: Task<()>,
    pub document: Option<crate::text_document::Document>,
    editor: Option<Entity<EditorState>>,
    rendered: Option<Entity<TextViewState>>,
    subscription: Option<Subscription>,
    extensions: MarkdownExtensions,
    image_sources: Arc<Mutex<std::collections::HashMap<SharedUri, Arc<gpui::Image>>>>,
    /// The reader chose Load images for this document.
    remote_images: bool,
    tasks: Vec<crate::markdown_tasks::Task>,
    pub dirty: bool,
    pub saving: bool,
    editing: bool,
    source: bool,
    pub error: Option<String>,
    exit_focus: Option<[FocusHandle; 3]>,
    autosave_task: Task<()>,
    draft_revision: u64,
    autosave_paused: bool,
    edit_lock: bool,
    close_requested: bool,
    /// Whether the last frame drew unsaved changes, and how many times Save
    /// has filled in (its fade restarts each time).
    shown_dirty: bool,
    save_motion: u64,
    /// The code block whose Copy shows "Copied", and a generation that
    /// restarts its fade.
    copied: Option<(u64, u64)>,
    /// Prettify analysis for JSON/XML files (11.2), computed off the UI thread.
    pretty: Option<crate::prettify::Outcome>,
    /// Whether the read view currently displays the prettified copy (never touches disk).
    show_formatted: bool,
    /// Guard so programmatic view switches (Formatted/Raw) never mark the draft dirty.
    syncing_display: bool,
    /// Parsed `.env` structure for secret masking (11.3).
    env_masked: Option<crate::env_secrets::MaskedEnv>,
    /// Whether the whole `.env` preview is temporarily revealed.
    env_reveal_all: bool,
    /// Individual `.env` lines temporarily revealed by clicking their masked value.
    env_revealed_lines: std::collections::HashSet<usize>,
    /// Confirmation notice shown before entering Edit mode on a masked `.env` file.
    env_edit_prompt: bool,
    /// Last applied soft-wrap setting on `editor` (11.5).
    applied_wrap: Option<bool>,
}
impl State {
    /// Editing text: Space types a space instead of opening Quick Look.
    pub(super) fn is_editing(&self) -> bool {
        self.editing
    }
    pub fn new() -> Self {
        Self {
            key: None,
            revision: 0,
            task: Task::ready(()),
            document: None,
            editor: None,
            rendered: None,
            subscription: None,
            tasks: vec![],
            extensions: MarkdownExtensions::default(),
            image_sources: Arc::new(Mutex::new(std::collections::HashMap::new())),
            remote_images: false,
            dirty: false,
            saving: false,
            editing: false,
            source: true,
            error: None,
            exit_focus: None,
            autosave_task: Task::ready(()),
            draft_revision: 0,
            autosave_paused: false,
            edit_lock: false,
            close_requested: false,
            shown_dirty: false,
            save_motion: 0,
            copied: None,
            pretty: None,
            show_formatted: false,
            syncing_display: false,
            env_masked: None,
            env_reveal_all: false,
            env_revealed_lines: std::collections::HashSet::new(),
            env_edit_prompt: false,
            applied_wrap: None,
        }
    }
    pub fn reset_env_reveal(&mut self) {
        self.env_reveal_all = false;
        self.env_revealed_lines.clear();
        self.env_edit_prompt = false;
    }
    pub fn blocked(&self) -> bool {
        self.dirty || self.saving
    }
    pub fn revision_next(&self) -> usize {
        self.revision.wrapping_add(1)
    }
    pub fn set_revision(&mut self, revision: usize) {
        self.revision = revision;
    }
    pub fn editor_focus(&self, cx: &App) -> Option<FocusHandle> {
        self.editor
            .as_ref()
            .map(|editor| editor.read(cx).focus_handle(cx))
    }
}
pub(super) fn text_file(row: &Row) -> bool {
    row.file.kind & 3 == seek_engine::walk::KIND_FILE
        && matches!(
            crate::icons::kind(&row.file.path, false),
            crate::icons::Kind::Text | crate::icons::Kind::Code
        )
}
fn markdown(path: &std::path::Path) -> bool {
    crate::code_highlight::language(path) == "markdown"
}
impl Seek {
    pub(super) fn sync_text_reader(&mut self, cx: &mut Context<Self>) {
        let wanted = self
            .preview_enlarged
            .then(|| self.selected(cx))
            .flatten()
            .filter(text_file)
            .map(|row| media::Key::new(&row, 0, true));
        if self.text_reader.key == wanted
            || self.text_reader.document.is_some()
                && self.text_reader.key.as_ref().map(|k| &k.path)
                    == wanted.as_ref().map(|k| &k.path)
        {
            return;
        }
        if self.text_reader.blocked() {
            return;
        }
        let revision = self.text_reader.revision.wrapping_add(1);
        self.text_reader = State::new();
        self.text_reader.revision = revision;
        self.text_reader.key = wanted.clone();
        let Some(key) = wanted else {
            return;
        };
        self.start_text_load(key, cx);
    }
    /// Load a text file into the reader that is currently in the slot
    /// (Quick Look or a document pane).
    pub(super) fn start_text_load(&mut self, key: media::Key, cx: &mut Context<Self>) {
        let target = self.reader_slot;
        let revision = self.text_reader.revision;
        self.text_reader.source = !markdown(&key.path);
        self.text_reader.remote_images = false;
        self.text_reader.task = cx.spawn(async move |this, cx| {
            let path = key.path.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let doc = crate::text_document::load(&path)?;
                    let pretty = crate::prettify::analyze(&doc.path, &doc.text);
                    let env_masked = crate::env_secrets::is_env_file(&doc.path)
                        .then(|| crate::env_secrets::MaskedEnv::parse(&doc.text));
                    Ok::<_, String>((doc, pretty, env_masked))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.with_reader(target, |this| {
                    if this.text_reader.revision != revision
                        || this.text_reader.key.as_ref() != Some(&key)
                    {
                        return;
                    }
                    match result {
                        Ok((document, pretty, env_masked)) => {
                            this.text_reader.tasks = crate::markdown_tasks::tasks(&document.text);
                            this.text_reader.show_formatted = matches!(
                                &pretty,
                                Some(crate::prettify::Outcome::Formatted {
                                    was_minified: true,
                                    ..
                                })
                            );
                            this.text_reader.pretty = pretty;
                            this.text_reader.env_masked = env_masked;
                            this.text_reader.reset_env_reveal();
                            this.text_reader.document = Some(document);
                        }
                        Err(error) => this.text_reader.error = Some(error),
                    }
                    cx.notify();
                });
            });
        });
    }
    pub(super) fn text_reader_blocks_navigation(&mut self, cx: &mut Context<Self>) -> bool {
        if self.text_reader.blocked() {
            self.text_reader.error = Some(
                if self.text_reader.saving {
                    "Saving this file. Please wait."
                } else {
                    "Save or discard your changes before leaving this file."
                }
                .into(),
            );
            cx.notify();
            true
        } else {
            false
        }
    }
    pub(super) fn request_text_reader_exit(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.text_reader.saving {
            // Honor Escape after the pending save; never discard a newer draft.
            self.text_reader.close_requested = true;
            cx.notify();
            return true;
        }
        if !self.text_reader.dirty {
            return false;
        }
        if let Some(editor) = &self.text_reader.editor {
            editor.update(cx, |editor, cx| {
                editor.focus(window, cx);
                editor.close_search(cx);
            });
        }
        self.text_reader.autosave_task = Task::ready(());
        let focus = [cx.focus_handle(), cx.focus_handle(), cx.focus_handle()];
        window.focus(&focus[0], cx);
        self.text_reader.exit_focus = Some(focus);
        cx.notify();
        true
    }
    fn finish_text_exit(&mut self, choice: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.text_reader.saving {
            return;
        }
        match choice {
            0 => self.save_text_reader(false, window, cx),
            1 => {
                self.text_reader.dirty = false;
                self.text_reader.exit_focus = None;
                self.close_reader(window, cx);
            }
            _ => {
                self.text_reader.exit_focus = None;
                self.schedule_text_autosave(window, cx);
                if let Some(editor) = &self.text_reader.editor {
                    editor.update(cx, |editor, cx| editor.focus(window, cx));
                }
                cx.notify();
            }
        }
    }
    pub(super) fn text_reader_focused(&self, window: &Window, cx: &App) -> bool {
        self.text_reader.editor.as_ref().is_some_and(|editor| {
            editor
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx)
        })
    }
    pub(super) fn escape_text_reader(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.text_reader.key.is_none() {
            return false;
        }
        if self.text_reader.exit_focus.is_some() {
            self.finish_text_exit(2, window, cx);
            return true;
        }
        if let Some(editor) = &self.text_reader.editor
            && editor.read(cx).search_session().is_active()
        {
            editor.update(cx, |editor, cx| {
                editor.focus(window, cx);
                editor.close_search(cx);
            });
            cx.notify();
        } else {
            self.close_reader(window, cx);
        }
        true
    }
    pub(super) fn text_reader_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.text_reader.key.is_none() {
            return false;
        }
        let key = event.keystroke.key.as_str();
        if let Some(focus) = self.text_reader.exit_focus.clone() {
            let current = focus
                .iter()
                .position(|focus| focus.is_focused(window))
                .unwrap_or(0);
            match key {
                "escape" => self.finish_text_exit(2, window, cx),
                "enter" | "space" => self.finish_text_exit(current, window, cx),
                "tab" | "right" | "left" => {
                    let backwards =
                        key == "left" || key == "tab" && event.keystroke.modifiers.shift;
                    window.focus(
                        &focus[if backwards {
                            (current + 2) % 3
                        } else {
                            (current + 1) % 3
                        }],
                        cx,
                    );
                    cx.notify();
                }
                "s" if event.keystroke.modifiers.control => self.finish_text_exit(0, window, cx),
                _ => {}
            }
            cx.stop_propagation();
            return true;
        }

        if event.keystroke.modifiers.control
            && key == "c"
            && self.preferences.hide_env_secrets
            && !self.text_reader.editing
            && !self.text_reader.env_reveal_all
            && let Some(env) = &self.text_reader.env_masked
            && env.has_any_secret()
        {
            let safe = env.render(false, &self.text_reader.env_revealed_lines);
            cx.write_to_clipboard(ClipboardItem::new_string(safe));
            cx.stop_propagation();
            return true;
        }
        // Let Ctrl+A and Ctrl+C pass through to the focused preview (Editor or
        // rendered Markdown TextView) without Quick Look swallowing them.
        if event.keystroke.modifiers.control
            && !event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.shift
            && matches!(key, "a" | "c")
        {
            return true;
        }
        if event.keystroke.modifiers.control && key == "s" {
            self.save_text_reader(false, window, cx);
            cx.stop_propagation();
            return true;
        }
        if event.keystroke.modifiers.control && key == "f" {
            self.text_reader.source = true;
            if let Some(editor) = &self.text_reader.editor {
                editor.update(cx, |editor, cx| {
                    editor.focus(window, cx);
                    editor.open_search(false, cx);
                });
            }
            cx.stop_propagation();
            cx.notify();
            return true;
        }
        if let Some(editor) = &self.text_reader.editor {
            if key == "escape" && editor.read(cx).search_session().is_active() {
                editor.update(cx, |editor, cx| {
                    editor.focus(window, cx);
                    editor.close_search(cx);
                });
                cx.stop_propagation();
                return true;
            }
            if editor.read(cx).search_session().is_active() {
                return true;
            }
            if key == "space"
                && self.reader_slot == ReaderTarget::QuickLook
                && !self.text_reader.editing
                && !event.keystroke.modifiers.control
                && !event.keystroke.modifiers.alt
                && !event.keystroke.modifiers.shift
            {
                self.close_reader(window, cx);
                cx.stop_propagation();
                return true;
            }
            // In Quick Look read mode, plain Up/Down navigate between files
            // even after the user clicked into the preview to select text.
            if self.reader_slot == ReaderTarget::QuickLook
                && !self.text_reader.editing
                && !event.keystroke.modifiers.shift
                && !event.keystroke.modifiers.control
                && !event.keystroke.modifiers.alt
                && matches!(key, "up" | "down")
            {
                return false;
            }
            if key == "escape" {
                self.close_reader(window, cx);
                cx.stop_propagation();
                return true;
            }
            if editor
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx)
                && key != "escape"
            {
                return true;
            }
        }
        if key == "space"
            && self.reader_slot == ReaderTarget::QuickLook
            && !self.text_reader.editing
            && !event.keystroke.modifiers.control
            && !event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.shift
        {
            self.close_reader(window, cx);
            cx.stop_propagation();
            return true;
        }
        false
    }
    fn text_editor_composing(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.text_reader.editor.as_ref().is_some_and(|editor| {
            editor.update(cx, |editor, cx| {
                gpui::EntityInputHandler::marked_text_range(editor, window, cx).is_some()
            })
        })
    }
    fn schedule_text_autosave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.reader_slot;
        self.text_reader.autosave_task = Task::ready(());
        if !self.preferences.auto_save_text
            || !self.text_reader.editing
            || !self.text_reader.dirty
            || self.text_reader.saving
            || self.text_reader.autosave_paused
            || self.text_reader.exit_focus.is_some()
        {
            return;
        }
        let revision = self.text_reader.revision;
        let draft = self.text_reader.draft_revision;
        self.text_reader.autosave_task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(1))
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.with_reader(target, |this| {
                    if this.text_reader.revision != revision
                        || this.text_reader.draft_revision != draft
                        || !this.preferences.auto_save_text
                        || !this.text_reader.dirty
                        || !this.text_reader.editing
                        || this.text_reader.autosave_paused
                        || this.text_reader.exit_focus.is_some()
                    {
                        return;
                    }
                    if this.text_reader.saving || this.text_editor_composing(window, cx) {
                        this.schedule_text_autosave(window, cx);
                        return;
                    }
                    this.save_text_reader_mode(false, true, window, cx);
                })
            });
        });
    }
    /// Save a Copy: the current text goes to a new file beside the original,
    /// which is left as it is. The draft stays open on the original.
    pub(super) fn save_text_copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(document), Some(editor)) = (
            self.text_reader.document.clone(),
            self.text_reader.editor.clone(),
        ) else {
            return;
        };
        let text = editor.read(cx).value().to_string();
        let target = self.reader_slot;
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { crate::text_document::save_copy(&document, &text) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(path) => {
                    let _ = this.backend.tx.send(Command::RefreshParents(
                        path.parent().map(|p| p.to_path_buf()).into_iter().collect(),
                    ));
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    this.show_toast(format!("Saved {name}"), false, window, cx);
                }
                Err(error) => {
                    this.with_reader(target, |this| this.text_reader.error = Some(error));
                    cx.notify();
                }
            });
        })
        .detach();
    }
    pub(super) fn save_text_reader(
        &mut self,
        undo: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.save_text_reader_mode(undo, false, window, cx);
    }
    fn save_text_reader_mode(
        &mut self,
        undo: bool,
        automatic: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.text_reader.saving {
            return;
        }
        let target = self.reader_slot;
        let Some(document) = self.text_reader.document.clone() else {
            return;
        };
        if undo && self.text_reader.dirty {
            self.text_reader.error =
                Some("Discard or save the current draft before undoing a saved change.".into());
            cx.notify();
            return;
        }
        let Some(editor) = self.text_reader.editor.clone() else {
            return;
        };
        let text = editor.read(cx).value().to_string();
        if !undo && !self.text_reader.dirty {
            return;
        }
        if self.text_editor_composing(window, cx) {
            self.schedule_text_autosave(window, cx);
            return;
        }
        self.text_reader.autosave_task = Task::ready(());
        self.text_reader.autosave_paused = false;
        let keep_backups = self.preferences.keep_text_backups;
        let close_after_save = self.text_reader.exit_focus.is_some();
        self.text_reader.saving = true;
        self.text_reader.error = None;
        self.text_reader.edit_lock = undo || close_after_save;
        editor.update(cx, |editor, cx| {
            editor.set_readonly(self.text_reader.edit_lock || !self.text_reader.editing, cx)
        });
        self.text_reader.task = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if undo {
                        crate::text_document::undo_with_options(&document, keep_backups)
                    } else {
                        crate::text_document::save_with_options(&document, &text, keep_backups)
                    }
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| this.with_reader(target, |this| {
                this.text_reader.saving = false;
                this.text_reader.edit_lock = false;
                let success=result.is_ok();
                match result {
                    Ok(document) => {
                        let path = document.path.clone();
                        if undo {
                            editor.update(cx, |editor, cx| {
                                editor.set_value(document.text.as_str().to_owned(), window, cx)
                            });
                        }
                        let current = editor.read(cx).value();
                        if let Some(rendered) = &this.text_reader.rendered {
                            rendered.update(cx, |rendered, cx| {
                                rendered.set_text(crate::markdown_extra::body(&current), cx)
                            });
                        }
                        this.text_reader.dirty = current.as_str() != document.text.as_str();
                        this.text_reader.document = Some(document);
                        this.text_reader.error = if undo {
                            Some("Saved change undone.".into())
                        } else {
                            None
                        };
                        let _ = this.backend.tx.send(Command::RefreshParents(
                            path.parent().map(|p| p.to_path_buf()).into_iter().collect(),
                        ));
                    }
                    Err(error) => {this.text_reader.autosave_paused=true;this.text_reader.error=Some(if automatic {format!("Auto-save paused. {error}")}else{error});},
                }
                crate::metrics::interaction("text_save",serde_json::json!({"automatic":automatic,"undo":undo,"success":success,"dirty_after":this.text_reader.dirty}));
                editor.update(cx, |editor, cx| {editor.set_readonly(!this.text_reader.editing, cx)});
                let requested_close = std::mem::take(&mut this.text_reader.close_requested);
                if success && (close_after_save || requested_close) && !this.text_reader.dirty {
                    this.text_reader.exit_focus = None;
                    this.close_reader(window, cx);
                } else if success && requested_close && this.text_reader.dirty {
                    this.request_text_reader_exit(window, cx);
                } else if this.text_reader.dirty && !this.text_reader.autosave_paused {
                    this.schedule_text_autosave(window, cx);
                }
                cx.notify();
            }));
        });
        cx.notify();
    }
    pub(super) fn compact_text_preview(
        &mut self,
        row: &Row,
        p: crate::appearance::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let details = match self
            .cache
            .lock()
            .unwrap()
            .get(&media::Key::new(row, 0, true))
        {
            Some(media::Value::Details(details)) => Some(details),
            _ => None,
        };
        let mut preview = div()
            .id("compact-text-preview")
            .w_full()
            .h(px(240.))
            .flex_shrink_0()
            .overflow_y_scroll()
            .text_size(px(13.))
            .line_height(px(22.));
        let unavailable = details.as_ref().map(|details| {
            details
                .error
                .clone()
                .unwrap_or_else(|| "No readable text preview is available.".into())
        });
        if let Some(text) = details.and_then(|details| details.text) {
            if markdown(&row.file.path) {
                use std::hash::{Hash, Hasher};
                let mut id = std::collections::hash_map::DefaultHasher::new();
                row.file.path.hash(&mut id);
                let folder = row
                    .file
                    .path
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .to_path_buf();
                let images = self.text_reader.image_sources.clone();
                let syntax = crate::appearance::SyntaxPalette::from_palette(p);
                let text = crate::markdown_extra::body(&text).to_string();
                // The preview scrolls inside its own box; it never spills
                // over the details below.
                return preview
                    .overflow_hidden()
                    .context_menu(|menu, window, _| {
                        super::daily::menu_style(menu, window).item(
                            PopupMenuItem::new("Copy").on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(gpui_base::input::Copy), cx);
                            }),
                        )
                    })
                    .child(
                        gpui_base::TextView::markdown(
                            ("inspector-markdown", id.finish() as usize),
                            text,
                        )
                        .selectable(true)
                        .scrollable(true)
                        .size_full()
                        .code_block_highlighter(move |block| {
                            let lang = block.lang().map(|l| fence_language(&l)).unwrap_or("text");
                            crate::code_highlight::highlight_text(lang, &block.code(), &syntax)
                        })
                        .image_source(move |uri| {
                            crate::markdown_images::cached_source(&folder, false, uri, &images)
                        })
                        .on_link_click(|url, _, _, cx| {
                            if url.starts_with("https://") || url.starts_with("http://") {
                                cx.open_url(url);
                            }
                        }),
                    )
                    .into_any_element();
            } else if self.preferences.hide_env_secrets
                && crate::env_secrets::is_env_file(&row.file.path)
            {
                let parsed = crate::env_secrets::MaskedEnv::parse(&text);
                let (reveal_all, revealed_lines) = self
                    .info_env_reveal
                    .as_ref()
                    .filter(|(p, _, _)| *p == row.file.path)
                    .map(|(_, all, set)| (*all, set.clone()))
                    .unwrap_or_default();
                let syntax = crate::appearance::SyntaxPalette::from_palette(p);
                let path_all = row.file.path.clone();
                let copy_text = parsed.render(reveal_all, &revealed_lines);
                let bar = div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .pb_2()
                    .flex_shrink_0()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .child(if reveal_all {
                                "Secrets revealed"
                            } else {
                                "Secrets hidden · click a value to reveal"
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                reader_control(
                                    "info-env-copy",
                                    "Copy",
                                    p,
                                )
                                .h(px(22.))
                                .text_size(px(11.))
                                .tooltip("Copy visible preview (hidden values stay masked)")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy_text.clone(),
                                    ));
                                    this.show_quiet_toast("Copied", window, cx);
                                })),
                            )
                            .child(
                                reader_control(
                                    "info-env-reveal",
                                    if reveal_all { "Hide" } else { "Reveal" },
                                    p,
                                )
                                .h(px(22.))
                                .text_size(px(11.))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let entry = this.info_env_reveal.get_or_insert_with(|| {
                                        (path_all.clone(), false, std::collections::HashSet::new())
                                    });
                                    if entry.0 != path_all {
                                        *entry = (
                                            path_all.clone(),
                                            true,
                                            std::collections::HashSet::new(),
                                        );
                                    } else {
                                        entry.1 = !entry.1;
                                        if !entry.1 {
                                            entry.2.clear();
                                        }
                                    }
                                    cx.notify();
                                })),
                            ),
                    );
                let wrap = self.preferences.wrap_info_panel;
                let mut rows = div()
                    .id("compact-env-preview")
                    .w_full()
                    .h(px(240.))
                    .flex_shrink_0()
                    .overflow_y_scroll()
                    .when(!wrap, |d| d.overflow_x_scroll())
                    .font_family(crate::tokens::FONT_MONO)
                    .text_size(px(12.))
                    .line_height(px(20.))
                    .flex()
                    .flex_col();
                for line in &parsed.lines {
                    rows = rows.child(self.env_preview_row(
                        line,
                        reveal_all,
                        &revealed_lines,
                        syntax,
                        p,
                        wrap,
                        None,
                        Some(row.file.path.clone()),
                        cx,
                    ));
                }
                let bar = bar.child(
                    reader_control("info-env-wrap", "Wrap", p)
                        .h(px(22.))
                        .text_size(px(11.))
                        .when(wrap, |b| b.bg(p.control_selected))
                        .tooltip("Wrap long lines")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.preferences.wrap_info_panel = !this.preferences.wrap_info_panel;
                            this.save_preferences(window, cx);
                        })),
                );
                return div()
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .child(bar)
                    .child(rows)
                    .into_any_element();
            } else {
                let lang = crate::code_highlight::language(&row.file.path);
                let pretty = crate::prettify::analyze(&row.file.path, &text);
                let (shown_text, pretty_toggle, pretty_note) = match pretty {
                    Some(crate::prettify::Outcome::Formatted {
                        text: formatted,
                        was_minified,
                    }) => {
                        let show_fmt = self
                            .info_pretty
                            .as_ref()
                            .filter(|(path, _)| *path == row.file.path)
                            .map_or(was_minified, |(_, on)| *on);
                        (
                            if show_fmt { formatted } else { text },
                            Some(show_fmt),
                            None,
                        )
                    }
                    Some(crate::prettify::Outcome::Raw { note }) => (text, None, note),
                    None => (text, None, None),
                };
                let formatted_flag = pretty_toggle.unwrap_or(false);
                let text_hash = {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    shown_text.hash(&mut h);
                    h.finish()
                };
                let wrap = self.preferences.wrap_info_panel;
                let reuse = self.info_editor.as_ref().is_some_and(|cached| {
                    cached.path == row.file.path
                        && cached.mtime == row.file.mtime
                        && cached.size == row.file.size
                        && cached.formatted == formatted_flag
                        && cached.text_hash == text_hash
                });
                if !reuse {
                    let value = shown_text.clone();
                    let editor = cx.new(|cx| {
                        let mut ed = EditorState::new(window, cx)
                            .language(lang)
                            .folding(false)
                            .soft_wrap(wrap)
                            .default_value(value);
                        ed.set_readonly(true, cx);
                        ed.set_highlighter_factory(
                            std::rc::Rc::new(|language| {
                                Some(Box::new(crate::code_highlight::Highlighter::new(language)))
                            }),
                            cx,
                        );
                        ed
                    });
                    self.info_editor = Some(InfoEditor {
                        path: row.file.path.clone(),
                        mtime: row.file.mtime,
                        size: row.file.size,
                        formatted: formatted_flag,
                        wrap,
                        text_hash,
                        editor,
                    });
                } else if let Some(cached) = &mut self.info_editor
                    && cached.wrap != wrap
                {
                    cached.wrap = wrap;
                    cached.editor.update(cx, |ed, cx| {
                        ed.set_soft_wrap(wrap, window, cx);
                    });
                }
                let editor = self.info_editor.as_ref().unwrap().editor.clone();
                let editor_box = div()
                    .id("compact-code-preview")
                    .w_full()
                    .h(px(240.))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .child(
                        Editor::new(&editor)
                            .aria_label("File preview")
                            .readonly(true)
                            .bordered(false)
                            .h(px(240.))
                            .font_family(crate::tokens::FONT_MONO)
                            .text_size(px(12.)),
                    );
                let path = row.file.path.clone();
                let mut bar = div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .pb_2()
                    .flex_shrink_0();
                let mut left = div().flex().items_center().gap_2().min_w_0();
                if let Some(show_fmt) = pretty_toggle {
                    let mut switch = div()
                        .id("info-pretty-mode")
                        .role(Role::RadioGroup)
                        .aria_label("Preview format")
                        .flex()
                        .items_center()
                        .p(px(2.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(p.text.opacity(0.08))
                        .bg(p.text.opacity(0.04));
                    for (id, label, formatted_mode) in [
                        ("info-pretty-formatted", "Formatted", true),
                        ("info-pretty-raw", "Raw", false),
                    ] {
                        let active = show_fmt == formatted_mode;
                        let path = path.clone();
                        switch = switch.child(
                            div()
                                .id(id)
                                .role(Role::RadioButton)
                                .aria_label(label)
                                .aria_selected(active)
                                .h(px(20.))
                                .px_2()
                                .flex()
                                .items_center()
                                .rounded(px(4.))
                                .cursor_pointer()
                                .text_size(px(11.))
                                .text_color(if active { p.text } else { p.muted })
                                .when(active, |d| d.bg(p.hover))
                                .child(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.info_pretty = Some((path.clone(), formatted_mode));
                                    cx.notify();
                                })),
                        );
                    }
                    left = left.child(switch);
                }
                if let Some(note) = pretty_note {
                    left = left.child(
                        div()
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .truncate()
                            .child(note),
                    );
                }
                bar = bar.child(left).child(
                    reader_control("info-wrap-toggle", "Wrap", p)
                        .h(px(22.))
                        .text_size(px(11.))
                        .when(wrap, |b| b.bg(p.control_selected))
                        .tooltip("Wrap long lines in information panel")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.preferences.wrap_info_panel = !this.preferences.wrap_info_panel;
                            this.save_preferences(window, cx);
                        })),
                );
                return div()
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .child(bar)
                    .child(editor_box)
                    .into_any_element();
            }
        } else {
            preview = preview
                .text_color(p.muted)
                .child(unavailable.unwrap_or_else(|| "Loading text preview...".into()));
        }
        preview.into_any_element()
    }
    pub(super) fn text_reader_view(
        &mut self,
        p: crate::appearance::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target = self.reader_slot;
        let Some(document) = self.text_reader.document.clone() else {
            return div()
                .p_4()
                .text_color(p.muted)
                .child(
                    self.text_reader
                        .error
                        .clone()
                        .unwrap_or_else(|| "Loading text...".into()),
                )
                .into_any_element();
        };
        let env_hiding = self.preferences.hide_env_secrets
            && !self.text_reader.editing
            && self
                .text_reader
                .env_masked
                .as_ref()
                .is_some_and(|e| e.has_any_secret());
        let wrap = match target {
            ReaderTarget::QuickLook => self.preferences.wrap_quick_look,
            ReaderTarget::Document(_) => self.preferences.wrap_document_pane,
        };
        if self.text_reader.editor.is_none() {
            let language = crate::code_highlight::language(&document.path);
            let initial_text = if env_hiding
                && let Some(env) = &self.text_reader.env_masked
            {
                env.render(
                    self.text_reader.env_reveal_all,
                    &self.text_reader.env_revealed_lines,
                )
            } else if !self.text_reader.editing
                && self.text_reader.show_formatted
                && let Some(crate::prettify::Outcome::Formatted { text, .. }) =
                    &self.text_reader.pretty
            {
                text.clone()
            } else {
                document.text.as_str().to_owned()
            };
            let editor = cx.new(|cx| {
                let mut editor = EditorState::new(window, cx)
                    .language(language)
                    // Fold arrows only where code has blocks to fold.
                    .folding(!matches!(language, "text" | "markdown"))
                    .soft_wrap(wrap)
                    .default_value(initial_text);
                editor.set_readonly(true, cx);
                editor.set_highlighter_factory(
                    std::rc::Rc::new(|language| {
                        Some(Box::new(crate::code_highlight::Highlighter::new(language)))
                    }),
                    cx,
                );
                editor
            });
            let rendered = cx.new(|cx| {
                TextViewState::markdown(crate::markdown_extra::body(&document.text), cx)
                    .selectable(true)
            });
            self.text_reader.subscription = Some(cx.subscribe_in(
                &editor,
                window,
                move |this, editor, event, window, cx| {
                    this.with_reader(target, |this| {
                        if matches!(event, InputEvent::Change) && !this.text_reader.syncing_display {
                            this.text_reader.draft_revision =
                                this.text_reader.draft_revision.wrapping_add(1);
                            let text = editor.read(cx).value();
                            this.text_reader.tasks = crate::markdown_tasks::tasks(&text);
                            this.text_reader.dirty =
                                this.text_reader.document.as_ref().is_some_and(|document| {
                                    document.text.as_str() != text.as_str()
                                });
                            if let Some(rendered) = &this.text_reader.rendered {
                                rendered.update(cx, |rendered, cx| {
                                    rendered.set_text(crate::markdown_extra::body(&text), cx)
                                });
                            }
                            this.schedule_text_autosave(window, cx);
                            cx.notify();
                        }
                    });
                },
            ));
            self.text_reader.editor = Some(editor);
            self.text_reader.rendered = Some(rendered);
            self.text_reader.applied_wrap = Some(wrap);
        } else if self.text_reader.applied_wrap != Some(wrap)
            && let Some(editor) = &self.text_reader.editor
        {
            self.text_reader.applied_wrap = Some(wrap);
            editor.update(cx, |ed, cx| ed.set_soft_wrap(wrap, window, cx));
        }
        let editor = self.text_reader.editor.as_ref().unwrap().clone();
        let mut controls = div()
            .id("text-reader-toolbar")
            .role(Role::Group)
            .aria_label("Text reader controls")
            .h(px(44.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .border_b_1()
            .border_color(p.line);
        if markdown(&document.path) {
            // Read / Source, with the same sliding pill as the settings'
            // segmented controls.
            let selected = self.text_reader.source as usize;
            const SEG: f32 = 30.;
            let dark = p.text.l > 0.5;
            let slot = |i: usize| px(2. + i as f32 * SEG);
            // Styled like the settings pill: raised, with an edge and shadow,
            // and nothing else in the track paints a fill.
            let indicator = div()
                .absolute()
                .top(px(2.))
                .h(px(24.))
                .w(px(SEG))
                .rounded(px(5.))
                .bg(if dark {
                    Hsla::from(rgba(0xffffff1f))
                } else {
                    Hsla::from(rgba(0xffffffff))
                })
                .border_1()
                .border_color(p.text.opacity(if dark { 0.14 } else { 0.1 }))
                .shadow_sm();
            let indicator: AnyElement = match self
                .segment_motion
                .get("text-view-mode")
                .copied()
                .filter(|(from, _)| self.motion_enabled && *from != selected)
            {
                Some((from, generation)) => indicator
                    .left(slot(selected))
                    .with_animation(
                        SharedString::from(format!("text-view-mode-slide-{generation}")),
                        crate::motion::reversible(),
                        move |d, t| {
                            d.left(px(2. + (from as f32 + (selected as f32 - from as f32) * t) * SEG))
                        },
                    )
                    .into_any_element(),
                None => indicator.left(slot(selected)).into_any_element(),
            };
            let mut modes = div()
                .id("text-view-mode")
                .role(Role::RadioGroup)
                .aria_label("Document view")
                .relative()
                .flex()
                .items_center()
                .p(px(2.))
                .rounded(px(7.))
                .border_1()
                .border_color(p.text.opacity(0.08))
                .bg(p.text.opacity(if dark { 0.04 } else { 0.05 }))
                .child(indicator);
            for (id, label, icon, source) in [
                ("text-read", "Read", "icons/eye.svg", false),
                ("text-source", "Source", "seek/source-code.svg", true),
            ] {
                let chosen = source as usize == selected;
                modes = modes.child(
                    div()
                        .id(id)
                        .role(Role::RadioButton)
                        .aria_label(label)
                        .aria_selected(chosen)
                        .relative()
                        .h(px(24.))
                        .w(px(SEG))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .text_color(if chosen { p.text } else { p.muted })
                        .when(!chosen, |d| d.hover(move |s| s.text_color(p.text)))
                        .child(Icon::empty().path(icon).size(px(15.)))
                        .tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(label).build(window, cx)
                        })
                        .on_click(Self::reader_listener::<ClickEvent>(
                            target,
                            cx,
                            move |this, _, window, cx| {
                                if this.text_reader.source == source {
                                    return;
                                }
                                this.slide_segment(
                                    "text-view-mode",
                                    this.text_reader.source as usize,
                                    source as usize,
                                );
                                this.text_reader.source = source;
                                if source {
                                    if let Some(editor) = &this.text_reader.editor {
                                        editor.update(cx, |editor, cx| editor.focus(window, cx));
                                    }
                                } else {
                                    this.preview_focus.focus(window, cx);
                                }
                                cx.notify();
                            },
                        )),
                );
            }
            controls = controls
                .child(modes)
                .child(div().w(px(1.)).h(px(16.)).bg(p.line));
        }
        if markdown(&document.path)
            && !self.text_reader.source
            && !self.text_reader.remote_images
            && crate::markdown_extra::has_remote_images(&document.text)
        {
            controls = controls.child(
                reader_control("text-load-images", "Load images", p)
                    .tooltip("This document shows pictures from the web. They load only when you ask.")
                    .on_click(Self::reader_listener::<ClickEvent>(target, cx, |this, _, _, cx| {
                        this.text_reader.remote_images = true;
                        cx.notify();
                    })),
            );
        }
        if markdown(&document.path) && self.text_reader.source && self.text_reader.editing {
            // The format bar: acts on the selection (or inserts the marks).
            use crate::markdown_extra::Format;
            let mut bar = div()
                .id("markdown-format-bar")
                .role(Role::Toolbar)
                .aria_label("Format")
                .flex()
                .items_center()
                .gap(px(2.))
                .p(px(2.))
                .rounded(px(7.))
                .bg(p.text.opacity(0.045));
            for (id, label, glyph, format) in [
                ("format-bold", "Bold", gpui_component_assets::IconName::Bold, Format::Bold),
                ("format-italic", "Italic", gpui_component_assets::IconName::Italic, Format::Italic),
                ("format-code", "Code", gpui_component_assets::IconName::Code, Format::Code),
                ("format-link", "Link", gpui_component_assets::IconName::Link, Format::Link),
                ("format-heading", "Heading", gpui_component_assets::IconName::Heading, Format::Heading),
                ("format-bullet", "Bulleted list", gpui_component_assets::IconName::List, Format::Bullet),
                ("format-quote", "Quote", gpui_component_assets::IconName::Quote, Format::Quote),
            ] {
                bar = bar.child(
                    Button::new(id)
                        .ghost()
                        .h(px(24.))
                        .w(px(26.))
                        .p_0()
                        .rounded(px(5.))
                        .accessibility_label(label)
                        .tooltip(label)
                        .child(
                            if format == Format::Bullet {
                                Icon::empty().path("icons/list-bullet.svg")
                            } else {
                                Icon::new(glyph)
                            }
                            .size(px(13.))
                            .text_color(p.muted),
                        )
                        .on_click(Self::reader_listener::<ClickEvent>(
                            target,
                            cx,
                            move |this, _, window, cx| {
                                if let Some(editor) = this.text_reader.editor.clone() {
                                    editor.update(cx, |editor, cx| {
                                        let selected = editor.selected_text().to_string();
                                        editor.replace(
                                            crate::markdown_extra::apply(format, &selected),
                                            window,
                                            cx,
                                        );
                                        editor.focus(window, cx);
                                    });
                                }
                            },
                        )),
                );
            }
            controls = controls
                .child(bar)
                .child(div().w(px(1.)).h(px(16.)).bg(p.line));
        }
        if env_hiding {
            let revealed = self.text_reader.env_reveal_all;
            controls = controls
                .child(
                    reader_control(
                        "text-env-reveal",
                        if revealed { "Hide values" } else { "Reveal" },
                        p,
                    )
                    .tooltip(if revealed {
                        "Hide secret values again"
                    } else {
                        "Temporarily reveal all secret values in this preview"
                    })
                    .on_click(Self::reader_listener::<ClickEvent>(
                        target,
                        cx,
                        |this, _, window, cx| {
                            this.text_reader.env_reveal_all = !this.text_reader.env_reveal_all;
                            if !this.text_reader.env_reveal_all {
                                this.text_reader.env_revealed_lines.clear();
                            }
                            this.sync_reader_display(window, cx);
                            this.focus_reader(window, cx);
                            cx.notify();
                        },
                    )),
                )
                .child(div().w(px(1.)).h(px(16.)).bg(p.line));
        }
        if !self.text_reader.editing
            && matches!(
                self.text_reader.pretty,
                Some(crate::prettify::Outcome::Formatted { .. })
            )
        {
            let show_fmt = self.text_reader.show_formatted;
            let dark = p.text.l > 0.5;
            let mut switch = div()
                .id("text-pretty-mode")
                .role(Role::RadioGroup)
                .aria_label("Preview format")
                .flex()
                .items_center()
                .p(px(2.))
                .rounded(px(7.))
                .border_1()
                .border_color(p.text.opacity(0.08))
                .bg(p.text.opacity(if dark { 0.04 } else { 0.05 }));
            for (id, label, formatted_mode) in [
                ("text-pretty-formatted", "Formatted", true),
                ("text-pretty-raw", "Raw", false),
            ] {
                let active = show_fmt == formatted_mode;
                switch = switch.child(
                    div()
                        .id(id)
                        .role(Role::RadioButton)
                        .aria_label(label)
                        .aria_selected(active)
                        .h(px(24.))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded(px(5.))
                        .cursor_pointer()
                        .text_size(px(12.))
                        .text_color(if active { p.text } else { p.muted })
                        .when(active, |d| {
                            d.bg(if dark {
                                Hsla::from(rgba(0xffffff1f))
                            } else {
                                Hsla::from(rgba(0xffffffff))
                            })
                        })
                        .when(!active, |d| d.hover(move |s| s.text_color(p.text)))
                        .child(label)
                        .on_click(Self::reader_listener::<ClickEvent>(
                            target,
                            cx,
                            move |this, _, window, cx| {
                                if this.text_reader.show_formatted == formatted_mode {
                                    return;
                                }
                                this.text_reader.show_formatted = formatted_mode;
                                this.sync_reader_display(window, cx);
                                this.focus_reader(window, cx);
                                cx.notify();
                            },
                        )),
                );
            }
            controls = controls
                .child(switch)
                .child(div().w(px(1.)).h(px(16.)).bg(p.line));
        }
        if self.text_reader.editing && crate::prettify::kind(&document.path).is_some() {
            controls = controls.child(
                reader_control("text-format-document", "Format document", p)
                    .tooltip("Format JSON or XML in this draft (undoable with Ctrl+Z)")
                    .disabled(self.text_reader.saving)
                    .on_click(Self::reader_listener::<ClickEvent>(
                        target,
                        cx,
                        |this, _, window, cx| this.format_editor_document(window, cx),
                    )),
            );
        }
        if self.text_reader.source || env_hiding {
            controls = controls.child(
                reader_control("text-wrap-toggle", "Wrap", p)
                    .when(wrap, |b| b.bg(p.control_selected))
                    .tooltip("Wrap long lines")
                    .on_click(Self::reader_listener::<ClickEvent>(
                        target,
                        cx,
                        move |this, _, window, cx| {
                            let next = match target {
                                ReaderTarget::QuickLook => {
                                    this.preferences.wrap_quick_look =
                                        !this.preferences.wrap_quick_look;
                                    this.preferences.wrap_quick_look
                                }
                                ReaderTarget::Document(_) => {
                                    this.preferences.wrap_document_pane =
                                        !this.preferences.wrap_document_pane;
                                    this.preferences.wrap_document_pane
                                }
                            };
                            if let Some(editor) = &this.text_reader.editor {
                                editor.update(cx, |ed, cx| ed.set_soft_wrap(next, window, cx));
                            }
                            this.save_preferences(window, cx);
                            this.focus_reader(window, cx);
                        },
                    )),
            );
        }
        controls = controls.child(
            reader_icon("text-find", "icons/search.svg", "Find (Ctrl+F)", p).on_click(
                Self::reader_listener::<ClickEvent>(target, cx, |this, _, window, cx| {
                    this.slide_segment("text-view-mode", this.text_reader.source as usize, 1);
                    this.text_reader.source = true;
                    if let Some(editor) = &this.text_reader.editor {
                        editor.update(cx, |editor, cx| {
                            editor.focus(window, cx);
                            editor.open_search(false, cx);
                        });
                    }
                    cx.notify();
                }),
            ),
        );
        if document.read_only.is_none() {
            if !self.text_reader.editing {
                controls = controls.child(
                    reader_icon("text-edit", "icons/pencil.svg", "Edit", p).on_click(
                        Self::reader_listener::<ClickEvent>(target, cx, |this, _, window, cx| {
                            let needs_env_notice = this.preferences.hide_env_secrets
                                && this
                                    .text_reader
                                    .env_masked
                                    .as_ref()
                                    .is_some_and(|e| e.has_any_secret())
                                && !this.text_reader.env_reveal_all;
                            if needs_env_notice {
                                this.text_reader.env_edit_prompt = true;
                                cx.notify();
                                return;
                            }
                            this.enter_text_edit(window, cx);
                        }),
                    ),
                );
            } else {
                // Split button: Save writes the file; the arrow offers Save a Copy.
                let copy_owner = cx.entity().downgrade();
                let saving = self.text_reader.saving;
                // One joined control: Save, and an arrow for Save a Copy. It
                // fills in the text colour when there is something to save.
                let dirty = self.text_reader.dirty;
                // Fill in, or fade back, whenever there starts or stops
                // being something to save.
                if dirty != self.text_reader.shown_dirty {
                    self.text_reader.save_motion += 1;
                }
                self.text_reader.shown_dirty = dirty;
                let (fill, ink, rule) = if dirty {
                    (p.text, crate::appearance::modal_surface(p), crate::appearance::modal_surface(p).opacity(0.25))
                } else {
                    (p.text.opacity(0.06), p.muted, p.line)
                };
                // Both halves paint only what this control chooses: the
                // component's own hover drew its ink over this fill.
                let variant = |cx: &App| {
                    gpui_component::button::ButtonCustomVariant::new(cx)
                        .color(rgba(0).into())
                        .foreground(ink)
                        .hover(ink.opacity(0.08))
                        .active(ink.opacity(0.14))
                };
                let save_motion = self.text_reader.save_motion;
                let motion_on = self.motion_enabled;
                controls = controls.child(
                    div()
                        .id("text-save-split")
                        .flex()
                        .items_center()
                        .h(px(28.))
                        .rounded(px(7.))
                        .overflow_hidden()
                        .bg(fill)
                        .child(
                            Button::new("text-save")
                                .custom(variant(cx))
                                .h(px(28.))
                                .pl(px(12.))
                                .pr(px(10.))
                                .rounded(px(0.))
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(ink)
                                .accessibility_label("Save")
                                .tooltip("Save (Ctrl+S)")
                                .disabled(!dirty || saving)
                                .child(super::interface::button_text("Save"))
                                .on_click(Self::reader_listener::<ClickEvent>(
                                    target,
                                    cx,
                                    |this, _, window, cx| this.save_text_reader(false, window, cx),
                                )),
                        )
                        .child(div().w(px(1.)).h(px(14.)).bg(rule))
                        .child(
                            Button::new("text-save-options")
                                .custom(variant(cx))
                                .h(px(28.))
                                .w(px(26.))
                                .p_0()
                                .rounded(px(0.))
                                .accessibility_label("Save options")
                                .tooltip("Save a Copy")
                                .disabled(saving)
                                .child(
                                    Icon::new(gpui_component_assets::IconName::ChevronDown)
                                        .size(px(12.))
                                        .text_color(ink),
                                )
                                .dropdown_menu(move |menu, window, _| {
                                    let owner = copy_owner.clone();
                                    super::daily::menu_style(menu, window).item(
                                        PopupMenuItem::new("Save a Copy").on_click(
                                            move |_, window, cx| {
                                                let _ = owner.update(cx, |this, cx| {
                                                    this.with_reader(target, |this| {
                                                        this.save_text_copy(window, cx)
                                                    });
                                                });
                                            },
                                        ),
                                    )
                                }),
                        )
                        // Filling in when there is something to save fades
                        // rather than snapping.
                        .with_animation(
                            SharedString::from(format!("text-save-fill-{save_motion}")),
                            crate::motion::enter(),
                            move |d, t| {
                                if !motion_on || save_motion == 0 {
                                    return d;
                                }
                                // 0 = nothing to save, 1 = filled.
                                let on = if dirty { t } else { 1. - t };
                                let mut fill = if dirty { p.text } else { p.text.opacity(0.06) };
                                fill.a = 0.06 + (p.text.a - 0.06) * on;
                                d.bg(fill).opacity(0.6 + 0.4 * t)
                            },
                        ),
                );
            }
        }
        let owner = cx.entity().downgrade();
        let dirty = self.text_reader.dirty;
        let saving = self.text_reader.saving;
        let undo = document.can_undo();
        let editing = self.text_reader.editing;
        controls = controls.child(
            reader_icon("text-actions", "icons/ellipsis.svg", "More", p).dropdown_menu(
                move |menu, window, _| {
                    let mut menu = super::daily::menu_style(menu, window);
                    for (label, action, disabled) in [
                        ("Undo saved change", 0, !undo || dirty || saving),
                        ("Discard changes", 1, !dirty || saving),
                        ("Finish editing", 2, !editing || dirty || saving),
                    ] {
                        let owner = owner.clone();
                        menu = menu.item(PopupMenuItem::new(label).disabled(disabled).on_click(
                            move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.with_reader(target, |this| match action {
                                        0 => this.save_text_reader(true, window, cx),
                                        1 => {
                                            if let (Some(editor), Some(document)) = (
                                                &this.text_reader.editor,
                                                &this.text_reader.document,
                                            ) {
                                                editor.update(cx, |editor, cx| {
                                                    editor.replace_all(
                                                        document.text.as_str().to_owned(),
                                                        window,
                                                        cx,
                                                    )
                                                });
                                            }
                                            this.text_reader.dirty = false;
                                            this.text_reader.error = None;
                                            this.text_reader.autosave_task = Task::ready(());
                                            cx.notify();
                                        }
                                        _ => {
                                            this.text_reader.editing = false;
                                            if let Some(doc) = &this.text_reader.document {
                                                this.text_reader.pretty =
                                                    crate::prettify::analyze(&doc.path, &doc.text);
                                                this.text_reader.env_masked =
                                                    crate::env_secrets::is_env_file(&doc.path)
                                                        .then(|| {
                                                            crate::env_secrets::MaskedEnv::parse(
                                                                &doc.text,
                                                            )
                                                        });
                                                this.text_reader.reset_env_reveal();
                                            }
                                            if let Some(editor) = &this.text_reader.editor {
                                                editor.update(cx, |editor, cx| {
                                                    editor.close_search(cx);
                                                    editor.set_readonly(true, cx);
                                                });
                                            }
                                            this.sync_reader_display(window, cx);
                                            this.preview_focus.focus(window, cx);
                                            cx.notify();
                                        }
                                    })
                                });
                            },
                        ));
                    }
                    menu
                },
            ),
        );
        let status = if self.text_reader.saving {
            "Saving..."
        } else if self.text_reader.autosave_paused {
            "Auto-save paused"
        } else if self.text_reader.dirty && self.preferences.auto_save_text {
            "Auto-save pending"
        } else if self.text_reader.dirty {
            "Unsaved changes"
        } else if document.can_undo() {
            if document.backup.is_some() {
                "Saved - backup kept"
            } else {
                "Saved"
            }
        } else {
            ""
        };
        controls = controls.child(div().flex_1()).child(
            div()
                .id("text-save-status")
                .role(Role::Status)
                .aria_label(status)
                .text_size(px(11.))
                .text_color(p.muted)
                .child(status),
        );
        let task_controls = if self.text_reader.editing
            && markdown(&document.path)
            && !self.text_reader.tasks.is_empty()
        {
            let mut tasks = div()
                .w_full()
                .max_w(px(900.))
                .pt_6()
                .flex()
                .flex_col()
                .gap_3()
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Edit tasks"))
                .child(div().text_size(px(12.)).text_color(p.muted).child(
                    if self.preferences.auto_save_text {
                        "Auto-save is on."
                    } else {
                        "Changes stay in your draft until Save."
                    },
                ));
            for task in self.text_reader.tasks.iter().take(100) {
                let offset = task.offset;
                let checked = task.checked;
                tasks = tasks.child(
                    gpui_component::checkbox::Checkbox::new(("markdown-task", offset))
                        .label(task.label.clone())
                        .checked(checked)
                        .disabled(self.text_reader.saving)
                        .on_click(Self::reader_listener::<bool>(
                            target,
                            cx,
                            move |this, _, window, cx| {
                                // The box takes focus when clicked; give it back to
                                // Quick Look so Space, arrows and Escape keep working.
                                this.focus_reader(window, cx);
                                if !this.text_reader.editing || this.text_reader.saving {
                                    return;
                                }
                                if let Some(editor) = &this.text_reader.editor {
                                    let text = editor.read(cx).value();
                                    if let Some(draft) =
                                        crate::markdown_tasks::toggle(&text, offset, checked)
                                    {
                                        editor.update(cx, |editor, cx| {
                                            editor.replace_all(draft, window, cx)
                                        });
                                    }
                                }
                            },
                        )),
                );
            }
            if self.text_reader.tasks.len() > 100 {
                tasks = tasks.child("More tasks are available in Source.");
            }
            Some(tasks)
        } else {
            None
        };
        let body = if env_hiding
            && !self.text_reader.env_reveal_all
            && let Some(env) = self.text_reader.env_masked.clone()
        {
            let syntax = crate::appearance::SyntaxPalette::from_palette(p);
            let revealed = self.text_reader.env_revealed_lines.clone();
            let mut list = div()
                .id("env-masked-reader")
                .size_full()
                .overflow_y_scroll()
                .when(!wrap, |d| d.overflow_x_scroll())
                .p_4()
                .font_family(crate::tokens::FONT_MONO)
                .text_size(px(14.))
                .line_height(px(22.))
                .flex()
                .flex_col();
            for line in &env.lines {
                list = list.child(self.env_preview_row(
                    line,
                    false,
                    &revealed,
                    syntax,
                    p,
                    wrap,
                    Some(target),
                    None,
                    cx,
                ));
            }
            list.into_any_element()
        } else if self.text_reader.source {
            div()
                .size_full()
                .child(
                    Editor::new(&editor)
                        .aria_label("File contents")
                        .readonly(
                            !self.text_reader.editing
                                || self.text_reader.saving
                                || self.text_reader.exit_focus.is_some(),
                        )
                        .bordered(false)
                        .h(relative(1.))
                        .font_family(crate::tokens::FONT_MONO)
                        .text_size(px(14.)),
                )
                .into_any_element()
        } else {
            let syntax = crate::appearance::SyntaxPalette::from_palette(p);
            div()
                .id("markdown-reader")
                .size_full()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .items_center()
                .p_6()
                .context_menu(|menu, window, _| {
                    super::daily::menu_style(menu, window).item(
                        PopupMenuItem::new("Copy").on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(gpui_base::input::Copy), cx);
                        }),
                    )
                })
                .child(
                    div()
                        .w_full()
                        .max_w(px(900.))
                        .line_height(px(27.))
                        .text_size(px(16.))
                        .children(self.front_matter_block(&document.text, p))
                        .child(
                            gpui_base::TextView::new(self.text_reader.rendered.as_ref().unwrap())
                                .selectable(true)
                                .code_block_highlighter(move |block| {
                                    let lang =
                                        block.lang().map(|l| fence_language(&l)).unwrap_or("text");
                                    crate::code_highlight::highlight_text(
                                        lang,
                                        &block.code(),
                                        &syntax,
                                    )
                                })
                                .markdown_extensions(self.text_reader.extensions.clone())
                                .code_block_actions({
                                    let owner = cx.entity().downgrade();
                                    let hover = p.text.opacity(0.08);
                                    let copied = self.text_reader.copied;
                                    move |block, _, _| {
                                        let code = block.code().to_string();
                                        let owner = owner.clone();
                                        let hash = {
                                            use std::hash::{Hash, Hasher};
                                            let mut h = std::collections::hash_map::DefaultHasher::new();
                                            code.hash(&mut h);
                                            h.finish()
                                        };
                                        let done = copied.filter(|(h, _)| *h == hash);
                                        // A plain press handler: the text view's selection
                                        // must not swallow the click.
                                        div()
                                            .id(("copy-code", {
                                                use std::hash::{Hash, Hasher};
                                                let mut h =
                                                    std::collections::hash_map::DefaultHasher::new();
                                                code.hash(&mut h);
                                                h.finish() as usize
                                            }))
                                            .role(Role::Button)
                                            .aria_label("Copy code")
                                            .h(px(24.))
                                            .px_2()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .rounded(px(5.))
                                            .cursor_pointer()
                                            .text_size(px(12.))
                                            .hover(move |d| d.bg(hover))
                                            .child(
                                                // "Copied" with a tick fades in where Copy was,
                                                // then Copy comes back.
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1()
                                                    .child(
                                                        Icon::empty()
                                                            .path(if done.is_some() { "icons/check.svg" } else { "icons/copy.svg" })
                                                            .size(px(12.)),
                                                    )
                                                    .child(if done.is_some() { "Copied" } else { "Copy" })
                                                    .with_animation(
                                                        SharedString::from(format!(
                                                            "copy-label-{hash}-{}",
                                                            copied.map_or(0, |(_, g)| g)
                                                        )),
                                                        crate::motion::enter(),
                                                        |d, t| d.opacity(t),
                                                    ),
                                            )
                                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                                cx.stop_propagation();
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    code.clone(),
                                                ));
                                                let _ = owner.update(cx, |this, cx| {
                                                    this.with_reader(target, |this| this.focus_reader(window, cx));
                                                    let generation = this.text_reader.copied.map_or(1, |(_, g)| g + 1);
                                                    this.text_reader.copied = Some((hash, generation));
                                                    cx.notify();
                                                    cx.spawn(async move |this, cx| {
                                                        cx.background_executor()
                                                            .timer(std::time::Duration::from_millis(1500))
                                                            .await;
                                                        let _ = this.update(cx, |this, cx| {
                                                            if this.text_reader.copied.is_some_and(|(_, g)| g == generation) {
                                                                // A new generation, so Copy fades back in too.
                                                                this.text_reader.copied = Some((0, generation + 1));
                                                                cx.notify();
                                                            }
                                                        });
                                                    })
                                                    .detach();
                                                });
                                            })
                                    }
                                })
                                .on_task_toggle({
                                    let owner = cx.entity().downgrade();
                                    move |start, end, checked, window, cx| {
                                        let _ = owner.update(cx, |this, cx| {
                                            this.with_reader(target, |this| {
                                                this.toggle_rendered_task(start, end, checked, window, cx)
                                            })
                                        });
                                    }
                                })
                                .image_source({
                                    let folder = document
                                        .path
                                        .parent()
                                        .unwrap_or(std::path::Path::new("."))
                                        .to_path_buf();
                                    let sources = self.text_reader.image_sources.clone();
                                    let remote = self.text_reader.remote_images;
                                    move |uri| {
                                        crate::markdown_images::cached_source(
                                            &folder, remote, uri, &sources,
                                        )
                                    }
                                })
                                .on_link_click(|url, _, _, cx| {
                                    if url.starts_with("https://") || url.starts_with("http://") {
                                        cx.open_url(url);
                                    }
                                }),
                        ),
                )
                .children(task_controls)
                .into_any_element()
        };
        let mut view = div().size_full().flex().flex_col().child(controls);
        if let Some(focus) = self.text_reader.exit_focus.clone() {
            let mut prompt = div()
                .id("text-unsaved-choice")
                .role(Role::Group)
                .aria_label("Unsaved changes")
                // The interface font, not the document's: its metrics set
                // where button labels sit.
                .font_family(crate::tokens::FONT_UI)
                .text_size(px(13.))
                .line_height(px(20.))
                .flex()
                .flex_wrap()
                .items_center()
                .gap_3()
                .px_4()
                .py_3()
                .border_b_1()
                .border_color(p.line)
                .child(div().flex_1().child(if target == ReaderTarget::QuickLook {
                    "Save changes before closing Quick Look?"
                } else {
                    "Save changes before leaving this document?"
                }));
            for (choice, id, label) in [
                (0, "text-save-close", "Save and close"),
                (1, "text-discard-close", "Discard and close"),
                (2, "text-keep-editing", "Keep editing"),
            ] {
                prompt = prompt.child(
                    super::settings_view::control(id, label, p, cx)
                        .h(crate::tokens::BUTTON_HEIGHT)
                        .track_focus(&focus[choice])
                        .disabled(self.text_reader.saving)
                        .on_click(Self::reader_listener::<ClickEvent>(
                            target,
                            cx,
                            move |this, _, window, cx| this.finish_text_exit(choice, window, cx),
                        )),
                );
            }
            view = view.child(prompt);
        }
        if self.text_reader.env_edit_prompt {
            let notice = div()
                .id("env-edit-notice")
                .role(Role::Group)
                .aria_label("Reveal secret values to edit")
                .font_family(crate::tokens::FONT_UI)
                .text_size(px(13.))
                .line_height(px(20.))
                .flex()
                .flex_wrap()
                .items_center()
                .gap_3()
                .px_4()
                .py_3()
                .border_b_1()
                .border_color(p.line)
                .bg(p.tints.orange.opacity(0.08))
                .child(
                    div()
                        .flex_1()
                        .child("Editing this .env file will reveal its secret values on screen."),
                )
                .child(
                    super::settings_view::control("env-edit-confirm", "Reveal and edit", p, cx)
                        .h(crate::tokens::BUTTON_HEIGHT)
                        .on_click(Self::reader_listener::<ClickEvent>(
                            target,
                            cx,
                            |this, _, window, cx| {
                                this.text_reader.env_edit_prompt = false;
                                this.text_reader.env_reveal_all = true;
                                this.enter_text_edit(window, cx);
                            },
                        )),
                )
                .child(
                    super::settings_view::control("env-edit-cancel", "Cancel", p, cx)
                        .h(crate::tokens::BUTTON_HEIGHT)
                        .on_click(Self::reader_listener::<ClickEvent>(
                            target,
                            cx,
                            |this, _, _, cx| {
                                this.text_reader.env_edit_prompt = false;
                                cx.notify();
                            },
                        )),
                );
            view = view.child(notice);
        }
        if let Some(reason) = document.read_only {
            view = view.child(div().px_4().py_2().text_color(p.muted).child(reason));
        }
        if !self.text_reader.editing
            && let Some(crate::prettify::Outcome::Raw { note: Some(note) }) =
                &self.text_reader.pretty
        {
            view = view.child(
                div()
                    .px_4()
                    .py_1()
                    .text_size(px(12.))
                    .text_color(p.muted)
                    .child(*note),
            );
        }
        if let Some(error) = &self.text_reader.error {
            view = view.child(div().px_4().py_2().text_size(px(12.)).child(error.clone()));
        }
        view.child(div().flex_1().min_h_0().child(body))
            .child(
                div()
                    .h(px(26.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .border_t_1()
                    .border_color(p.line)
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(div().flex_1())
                    .font_family(crate::tokens::FONT_MONO)
                    .child(format!(
                        "{} | {}",
                        document
                            .encoding
                            .map_or("Read only encoding", |e| e.label()),
                        document.eol.label()
                    )),
            )
            .into_any_element()
    }
}

impl Seek {
    /// Front matter as a quiet block of keys and values above the document.
    fn front_matter_block(
        &self,
        text: &str,
        p: crate::appearance::Palette,
    ) -> Option<AnyElement> {
        use crate::tokens::*;
        let (fields, _) = crate::markdown_extra::front_matter(text)?;
        if fields.is_empty() {
            return None;
        }
        let mut block = div()
            .id("markdown-front-matter")
            .role(Role::Group)
            .aria_label("Front matter")
            .mb_6()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_1()
            .rounded(RADIUS_CARD)
            .border_1()
            .border_color(p.line)
            .bg(p.text.opacity(0.03))
            .text_size(TEXT_SMALL)
            .line_height(px(20.));
        for (key, value) in fields {
            block = block.child(
                div()
                    .flex()
                    .gap_4()
                    .child(
                        div()
                            .w(px(120.))
                            .flex_shrink_0()
                            .truncate()
                            .font_family(FONT_MONO)
                            .text_color(p.muted)
                            .child(key),
                    )
                    .child(div().flex_1().min_w_0().text_color(p.text).child(value)),
            );
        }
        Some(block.into_any_element())
    }
}

impl Seek {
    /// A checkbox clicked in the formatted view: toggle that task in the
    /// draft (entering editing), so Save or auto-save writes it, never a
    /// silent write. Read-only documents are left alone.
    fn toggle_rendered_task(
        &mut self,
        start: usize,
        end: usize,
        checked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_reader(window, cx);
        if self.text_reader.saving
            || self
                .text_reader
                .document
                .as_ref()
                .is_none_or(|document| document.read_only.is_some())
        {
            return;
        }
        let Some(editor) = self.text_reader.editor.clone() else {
            return;
        };
        let text = editor.read(cx).value().to_string();
        // The view renders the body only; its offsets start after any front matter.
        let base = text.len() - crate::markdown_extra::body(&text).len();
        let Some(task) = crate::markdown_tasks::tasks(&text)
            .into_iter()
            .find(|task| (base + start..base + end).contains(&task.offset))
        else {
            return;
        };
        let Some(draft) = crate::markdown_tasks::toggle(&text, task.offset, checked) else {
            return;
        };
        if !self.text_reader.editing {
            self.text_reader.editing = true;
            editor.update(cx, |editor, cx| editor.set_readonly(false, cx));
        }
        editor.update(cx, |editor, cx| editor.replace_all(draft, window, cx));
        cx.notify();
    }

    /// Synchronises the read-only editor's visible text with Formatted / Raw
    /// preview mode (or raw text when entering Edit), never marking dirty or
    /// modifying disk.
    pub(super) fn sync_reader_display(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(editor), Some(document)) = (
            self.text_reader.editor.clone(),
            self.text_reader.document.clone(),
        ) else {
            return;
        };
        let wanted = if self.preferences.hide_env_secrets
            && !self.text_reader.editing
            && let Some(env) = &self.text_reader.env_masked
            && env.has_any_secret()
        {
            env.render(
                self.text_reader.env_reveal_all,
                &self.text_reader.env_revealed_lines,
            )
        } else if !self.text_reader.editing
            && self.text_reader.show_formatted
            && let Some(crate::prettify::Outcome::Formatted { text, .. }) =
                &self.text_reader.pretty
        {
            text.clone()
        } else {
            document.text.as_str().to_owned()
        };
        self.text_reader.syncing_display = true;
        editor.update(cx, |editor, cx| {
            editor.set_value(wanted, window, cx);
        });
        self.text_reader.syncing_display = false;
    }

    fn enter_text_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.text_reader.editing = true;
        self.slide_segment("text-view-mode", self.text_reader.source as usize, 1);
        self.text_reader.source = true;
        // Always load the real raw document text when entering Edit mode.
        self.sync_reader_display(window, cx);
        if let Some(editor) = &self.text_reader.editor {
            editor.update(cx, |editor, cx| {
                editor.set_readonly(false, cx);
                editor.focus(window, cx);
            });
        }
        cx.notify();
    }

    #[allow(clippy::too_many_arguments)]
    fn env_preview_row(
        &self,
        line: &crate::env_secrets::EnvLine,
        reveal_all: bool,
        revealed_lines: &std::collections::HashSet<usize>,
        syntax: crate::appearance::SyntaxPalette,
        p: crate::appearance::Palette,
        wrap: bool,
        reader_target: Option<ReaderTarget>,
        info_path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let idx = line.line_index();
        let line_no = div()
            .w(px(34.))
            .flex_shrink_0()
            .pr_2()
            .text_right()
            .text_color(p.muted.opacity(0.6))
            .child(format!("{}", idx + 1));
        match line {
            crate::env_secrets::EnvLine::Plain { text, .. } => {
                let is_comment = text.trim_start().starts_with('#');
                div()
                    .flex()
                    .items_baseline()
                    .w_full()
                    .child(line_no)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .when(!wrap, |d| d.whitespace_nowrap())
                            .text_color(if is_comment { syntax.comment } else { p.text })
                            .when(is_comment, |d| d.italic())
                            .child(if text.is_empty() {
                                " ".to_owned()
                            } else {
                                text.clone()
                            }),
                    )
                    .into_any_element()
            }
            crate::env_secrets::EnvLine::Entry {
                prefix,
                key,
                eq,
                open_quote,
                masked_value,
                real_value,
                close_quote,
                suffix,
                ..
            } => {
                let is_revealed = reveal_all || revealed_lines.contains(&idx);
                let has_secret = !real_value.is_empty();
                let mut content = div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .when(wrap, |d| d.flex_wrap())
                    .when(!wrap, |d| d.whitespace_nowrap())
                    .items_baseline();
                if !prefix.is_empty() {
                    content = content.child(
                        div()
                            .text_color(syntax.keyword)
                            .child(prefix.clone()),
                    );
                }
                content = content
                    .child(div().text_color(syntax.function).child(key.clone()))
                    .child(div().text_color(syntax.punctuation).child(eq.clone()));
                if !open_quote.is_empty() {
                    content = content.child(
                        div()
                            .text_color(syntax.string)
                            .child(open_quote.clone()),
                    );
                }
                if has_secret {
                    let display_val = if is_revealed {
                        real_value.clone()
                    } else {
                        masked_value.clone()
                    };
                    let mut pill = div()
                        .id((
                            if reader_target.is_some() {
                                "env-val-reader"
                            } else {
                                "env-val-info"
                            },
                            idx,
                        ))
                        .role(Role::Button)
                        .aria_label(if is_revealed {
                            format!("Hide value for {key}")
                        } else {
                            format!("Reveal value for {key}")
                        })
                        .cursor_pointer()
                        .rounded(px(4.))
                        .px(px(4.))
                        .when(!is_revealed, |d| {
                            d.bg(p.text.opacity(0.08))
                                .text_color(p.muted)
                                .hover(move |s| s.bg(p.text.opacity(0.14)))
                        })
                        .when(is_revealed, |d| {
                            d.text_color(syntax.string)
                                .hover(move |s| s.bg(p.hover))
                        })
                        .tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(if is_revealed {
                                "Click to hide"
                            } else {
                                "Click to reveal value"
                            })
                            .build(window, cx)
                        })
                        .child(display_val);
                    if let Some(target) = reader_target {
                        pill = pill.on_click(Self::reader_listener::<ClickEvent>(
                            target,
                            cx,
                            move |this, _, window, cx| {
                                if !this.text_reader.env_revealed_lines.insert(idx) {
                                    this.text_reader.env_revealed_lines.remove(&idx);
                                }
                                this.sync_reader_display(window, cx);
                                cx.notify();
                            },
                        ));
                    } else if let Some(path) = info_path {
                        pill = pill.on_click(cx.listener(move |this, _, _, cx| {
                            let entry = this.info_env_reveal.get_or_insert_with(|| {
                                (path.clone(), false, std::collections::HashSet::new())
                            });
                            if entry.0 != path {
                                *entry = (path.clone(), false, std::collections::HashSet::new());
                            }
                            if !entry.2.insert(idx) {
                                entry.2.remove(&idx);
                            }
                            cx.notify();
                        }));
                    }
                    content = content.child(pill);
                }
                if !close_quote.is_empty() {
                    content = content.child(
                        div()
                            .text_color(syntax.string)
                            .child(close_quote.clone()),
                    );
                }
                if !suffix.is_empty() {
                    content = content.child(
                        div()
                            .text_color(syntax.comment)
                            .italic()
                            .child(suffix.clone()),
                    );
                }
                div()
                    .flex()
                    .items_baseline()
                    .w_full()
                    .child(line_no)
                    .child(content)
                    .into_any_element()
            }
        }
    }

    /// Explicit "Format document" command inside the editor. Never runs
    /// automatically; applies through `replace_all` so Ctrl+Z restores the
    /// exact previous text, and leaves draft/file untouched on error.
    pub(super) fn format_editor_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.text_reader.editing || self.text_reader.saving {
            return;
        }
        let (Some(editor), Some(document)) = (
            self.text_reader.editor.clone(),
            self.text_reader.document.clone(),
        ) else {
            return;
        };
        let current = editor.read(cx).value().to_string();
        match crate::prettify::format_for_editor(&document.path, &current) {
            Ok(formatted) => {
                self.text_reader.error = None;
                if formatted != current {
                    editor.update(cx, |editor, cx| {
                        editor.replace_all(formatted, window, cx);
                        editor.focus(window, cx);
                    });
                }
            }
            Err(error) => {
                self.text_reader.error = Some(error.into());
            }
        }
        cx.notify();
    }
}
