use super::*;
use crate::file_ops::Operation;
use gpui_component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};

#[derive(Clone, Copy)]
pub(super) enum Action {
    CancelOperations,
    SaveSearch,
    Shortcuts,
    History,
    ReportBug,
    Extract,
    Compress,
    Download,
    UndoRename,
    RedoRename,
    PutBack,
    Open,
    QuickLook,
    DocumentPane,
    OpenWith,
    Reveal,
    Share,
    Copy,
    Cut,
    Paste,
    Path,
    Name,
    QuotedPath,
    Rename,
    Recycle,
    DeletePermanently,
    NewFolder,
    GroupSelection,
    BatchRename,
    Properties,
    Organize,
    NewFile,
    SuggestNames,
}
pub(super) struct Edit {
    pub(super) batch: Option<super::batch_rename_view::State>,
    pub(super) input: Entity<InputState>,
    path: Option<PathBuf>,
    grouping: Vec<PathBuf>,
    /// Creating an empty file rather than a folder.
    file: bool,
    pub(super) pending: bool,
    pub(super) error: Option<String>,
    _subscription: Subscription,
    folder: PathBuf,
    pub(super) root_focus: FocusHandle,
    pub(super) cancel_focus: FocusHandle,
    pub(super) save_focus: FocusHandle,
}
#[derive(Clone)]
pub(super) struct FilesDrag {
    pub paths: Arc<Vec<PathBuf>>,
    pub label: String,
}
pub(super) fn drop_allowed(paths: &[PathBuf], folder: &std::path::Path) -> bool {
    !paths.is_empty()
        && paths.iter().all(|path| {
            let source = seek_engine::paths::internal(path);
            let target = seek_engine::paths::internal(folder);
            if seek_engine::paths::cmp_ci(&source, &target).is_eq() {
                return false;
            }
            if path.parent().is_some_and(|parent| {
                seek_engine::paths::cmp_ci(&seek_engine::paths::internal(parent), &target).is_eq()
            }) {
                return false;
            }
            !crate::file_changes::under_any(folder, std::slice::from_ref(path))
        })
}
impl Render for FilesDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_component::ActiveTheme;
        div()
            .h(px(28.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(6.))
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .border_1()
            .border_color(cx.theme().border)
            .shadow_md()
            .text_size(px(13.))
            .child(
                Icon::new(if self.paths.len() > 1 {
                    gpui_component::IconName::Copy
                } else {
                    gpui_component::IconName::File
                })
                .size(px(13.)),
            )
            .child(div().max_w(px(220.)).truncate().child(self.label.clone()))
    }
}
pub(super) fn menu_style(menu: PopupMenu, window: &Window) -> PopupMenu {
    menu.scrollable(true)
        .scrollbar_visible(false)
        .item_height(crate::tokens::MENU_ROW)
        .max_h((window.viewport_size().height - px(24.)).max(px(120.)))
}
pub(super) fn actions_menu(
    menu: PopupMenu,
    owner: WeakEntity<Seek>,
    target: Option<PathBuf>,
    target_is_folder: bool,
    cx: &App,
) -> PopupMenu {
    actions_menu_selection(menu, owner, target, target_is_folder, None, cx)
}
pub(super) fn actions_menu_selection(
    mut menu: PopupMenu,
    owner: WeakEntity<Seek>,
    target: Option<PathBuf>,
    target_is_folder: bool,
    selection: Option<Vec<PathBuf>>,
    cx: &App,
) -> PopupMenu {
    let (selected, destination) = owner
        .upgrade()
        .map(|view| {
            let this = view.read(cx);
            // A row menu is built while TableState is borrowed by its delegate.
            // Do not read that entity again until a menu action is invoked.
            if target.is_some() {
                (true, target_is_folder || this.scope.is_some())
            } else {
                (
                    this.selected(cx).is_some(),
                    this.destination(None, cx).is_some(),
                )
            }
        })
        .unwrap_or_default();
    let tab_owner = owner.clone();
    let tab_target = target.clone();
    if target_is_folder {
        menu = menu.item(PopupMenuItem::new("Open folder in new tab").on_click(
            move |_, window, cx| {
                let _ = tab_owner.update(cx, |this, cx| {
                    this.new_tab(tab_target.clone(), window, cx);
                });
            },
        ));
        let pane_owner = owner.clone();
        let pane_target = target.clone();
        menu = menu.item(PopupMenuItem::new("Open folder in new pane").on_click(
            move |_, window, cx| {
                let _ = pane_owner.update(cx, |this, cx| {
                    this.split_pane(
                        crate::pane_layout::Axis::Horizontal,
                        pane_target.clone(),
                        window,
                        cx,
                    );
                });
            },
        ));
    }
    // Row delegates supply a snapshot, so this does not borrow TableState
    // while the table's context-menu callback is already using it.
    let multiple = selection.as_ref().is_some_and(|paths| paths.len() > 1);
    // Less common actions and saved programs remain searchable in Jump.
    for (label, action, enabled) in [
        ("Open", Action::Open, selected),
        ("Quick Look", Action::QuickLook, selected),
        (
            "Open in document pane",
            Action::DocumentPane,
            selected && !target_is_folder,
        ),
        ("Open with...", Action::OpenWith, selected),
        ("Reveal in Explorer", Action::Reveal, selected),
        ("Copy", Action::Copy, selected),
        ("Cut", Action::Cut, selected),
        ("Paste", Action::Paste, destination),
        ("Copy path", Action::Path, selected),
        (
            if multiple {
                "Batch rename..."
            } else {
                "Rename"
            },
            if multiple {
                Action::BatchRename
            } else {
                Action::Rename
            },
            selected,
        ),
        (
            "Suggest names...",
            Action::SuggestNames,
            selected && !target_is_folder,
        ),
        (
            "Organize folder...",
            Action::Organize,
            selected && target_is_folder && !multiple,
        ),
        (
            "Extract here",
            Action::Extract,
            selected
                && target
                    .as_ref()
                    .is_some_and(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"))),
        ),
        ("Compress to ZIP", Action::Compress, selected),
        (
            "Download from the cloud",
            Action::Download,
            selected && target.as_ref().is_some_and(|p| crate::cloud::is_placeholder(p)),
        ),
        ("Move to Recycle Bin", Action::Recycle, selected),
        ("New folder", Action::NewFolder, destination),
        ("Properties", Action::Properties, selected),
    ] {
        if matches!(
            action,
            Action::Copy | Action::Rename | Action::BatchRename | Action::NewFolder
        ) {
            menu = menu.separator();
        }
        let owner = owner.clone();
        let target = target.clone();
        menu = menu.item(PopupMenuItem::new(label).disabled(!enabled).on_click(
            move |_, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.file_action(action, target.clone(), window, cx)
                });
            },
        ));
    }
    let commands = owner.clone();
    menu = menu
        .separator()
        .item(
            PopupMenuItem::new("More actions in Jump...").on_click(move |_, window, cx| {
                let commands = commands.clone();
                window.defer(cx, move |window, cx| {
                    let _ = commands.update(cx, |this, cx| this.open_command_palette(window, cx));
                });
            }),
        );
    menu
}
/// What the empty-space menu shows, read before it is built: list menus open
/// while the table is borrowed, so the menu must not read the table itself.
#[derive(Clone)]
pub(super) struct Background {
    pub folder: Option<PathBuf>,
    pub view: View,
    pub sort: Option<(usize, gpui_component::table::ColumnSort)>,
    pub hidden: bool,
}
/// A menu row with its shortcut aligned on the right.
fn keyed(label: &'static str, keys: &'static str) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        use gpui_component::ActiveTheme;
        div()
            .flex()
            .flex_1()
            .min_w(px(168.))
            .items_center()
            .gap_4()
            .child(div().flex_1().child(label))
            .when(!keys.is_empty(), |row| {
                row.child(
                    div()
                        .font_family(crate::tokens::FONT_MONO)
                        .text_size(crate::tokens::TEXT_CAPTION)
                        .text_color(cx.theme().muted_foreground)
                        .child(keys),
                )
            })
    })
}
/// Right-click on empty space in a folder: create, organize, paste, and the
/// folder's view and sort. The current view and sort are checked.
pub(super) fn background_menu(
    mut menu: PopupMenu,
    owner: WeakEntity<Seek>,
    state: Background,
) -> PopupMenu {
    use gpui_component::table::ColumnSort;
    let has_folder = state.folder.is_some();
    let act = |menu: PopupMenu, item: PopupMenuItem, action: Action, enabled: bool| {
        let owner = owner.clone();
        let folder = state.folder.clone();
        menu.item(item.disabled(!enabled).on_click(move |_, window, cx| {
            let _ = owner.update(cx, |this, cx| {
                this.file_action(action, folder.clone(), window, cx)
            });
        }))
    };
    menu = act(
        menu,
        keyed("New Folder", "Ctrl+Shift+N"),
        Action::NewFolder,
        has_folder,
    );
    menu = act(menu, keyed("New File", ""), Action::NewFile, has_folder);
    menu = act(
        menu,
        keyed("Organize Folder", ""),
        Action::Organize,
        has_folder,
    );
    menu = act(
        menu,
        keyed("Properties", ""),
        Action::Properties,
        has_folder,
    );
    menu = menu.separator();
    menu = act(menu, keyed("Paste", "Ctrl+V"), Action::Paste, has_folder);
    menu = menu.separator();
    for (label, keys, view) in [
        ("As List", "Ctrl+1", View::List),
        ("As Grid", "Ctrl+2", View::Icons),
        ("As Columns", "Ctrl+4", View::Columns),
    ] {
        let owner = owner.clone();
        menu = menu.item(keyed(label, keys).checked(state.view == view).on_click(
            move |_, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.set_view(view, cx);
                    this.save_preferences(window, cx);
                });
            },
        ));
    }
    let options_owner = owner.clone();
    menu = menu.item(keyed("Icon View Options…", "Ctrl+J").on_click(move |_, window, cx| {
        let _ = options_owner.update(cx, |this, cx| this.toggle_icon_options(window, cx));
    }));
    menu = menu.separator();
    let hidden_owner = owner.clone();
    menu = menu.item(
        keyed("Show Hidden Files", "")
            .checked(state.hidden)
            .on_click(move |_, window, cx| {
                let _ = hidden_owner.update(cx, |this, cx| this.toggle_hidden(window, cx));
            }),
    );
    menu = menu.separator();
    for (label, column, direction) in [
        ("Sort by Name", 0, ColumnSort::Ascending),
        ("Sort by Date Modified", 1, ColumnSort::Descending),
        ("Sort by Size", 2, ColumnSort::Descending),
        ("Sort by Kind", 3, ColumnSort::Ascending),
    ] {
        let owner = owner.clone();
        menu = menu.item(
            keyed(label, "")
                .checked(state.sort.is_some_and(|(c, _)| c == column))
                .on_click(move |_, window, cx| {
                    let _ =
                        owner.update(cx, |this, cx| this.sort_by(column, direction, window, cx));
                }),
        );
    }
    menu
}
pub(super) fn kind_menu(mut menu: PopupMenu, owner: WeakEntity<Seek>) -> PopupMenu {
    menu = menu
        .min_w(px(144.))
        .max_w(px(180.))
        .scrollbar_visible(false)
        .item_height(px(28.));
    for kind in crate::kinds::Kind::ALL {
        let owner = owner.clone();
        menu = menu.item(
            PopupMenuItem::new(kind.label()).on_click(move |_, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.choose_kind(kind.filter(), false, window, cx)
                });
            }),
        );
    }
    menu
}
impl Seek {
    pub(super) fn show_content_failures(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::WindowExt;
        let failures = self.content_failures.clone();
        let p = crate::appearance::palette(&self.preferences, cx);
        let mut background = p.sheet;
        background.a = 1.;
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Unreadable files")
                .w(px(560.))
                .bg(background)
                .text_color(p.text)
                .child(
                    div()
                        .id("unreadable-file-list")
                        .flex()
                        .flex_col()
                        .gap_3()
                        .max_h(px(320.))
                        .overflow_y_scroll()
                        .children(failures.iter().map(|failure| {
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(failure.path.to_string_lossy().to_string())
                                .child(div().text_size(px(12.)).text_color(p.muted).child(format!(
                                    "After {} attempt(s): {}",
                                    failure.attempts, failure.error
                                )))
                        })),
                )
        });
    }
    fn destination(&self, target: Option<&PathBuf>, cx: &App) -> Option<PathBuf> {
        if let Some(path) = target
            && self
                .table
                .read(cx)
                .delegate()
                .rows
                .iter()
                .any(|r| r.file.path == *path && r.file.kind & 3 == seek_engine::walk::KIND_DIR)
        {
            return Some(path.clone());
        }
        if let Some(scope) = &self.scope {
            return Some(scope.clone());
        }
        let path = target.cloned().or_else(|| {
            self.selected(cx)
                .filter(|r| r.file.kind & 3 == seek_engine::walk::KIND_DIR)
                .map(|r| r.file.path)
        });
        path.filter(|p| {
            self.table
                .read(cx)
                .delegate()
                .rows
                .iter()
                .any(|r| r.file.path == *p && r.file.kind & 3 == seek_engine::walk::KIND_DIR)
        })
    }
    /// Extract a ZIP beside itself, or compress the selection into a new
    /// ZIP, off the interface thread. Neither replaces anything existing.
    fn archive_job(
        &mut self,
        compress: bool,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        if paths.iter().any(|p| crate::archive::inside(p)) {
            self.message = Some(crate::file_ops::READ_ONLY_ARCHIVE.into());
            cx.notify();
            return;
        }
        self.message = Some(if compress { "Compressing…" } else { "Extracting…" }.into());
        cx.notify();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let job = cx.background_executor().spawn(async move {
            if compress {
                crate::archive::compress(&paths, &cancel).map(|(zip, count)| {
                    (
                        zip.clone(),
                        format!(
                            "Compressed {count} item{} into {}",
                            if count == 1 { "" } else { "s" },
                            zip.file_name().unwrap_or_default().to_string_lossy()
                        ),
                        None,
                    )
                })
            } else {
                crate::archive::extract(&paths[0], &cancel).map(|done| {
                    let skipped = (done.skipped > 0).then(|| {
                        format!(
                            "{} item{} skipped: unsafe names or links",
                            done.skipped,
                            if done.skipped == 1 { "" } else { "s" }
                        )
                    });
                    (
                        done.folder.clone(),
                        format!(
                            "Extracted {} file{} to {}",
                            done.files,
                            if done.files == 1 { "" } else { "s" },
                            done.folder.file_name().unwrap_or_default().to_string_lossy()
                        ),
                        skipped,
                    )
                })
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok((made, text, note)) => {
                        this.message = note;
                        if let Some(parent) = made.parent() {
                            let _ = this
                                .backend
                                .tx
                                .send(Command::RefreshParents(vec![parent.to_path_buf()]));
                        }
                        this.show_toast(text, false, window, cx);
                    }
                    Err(error) => this.message = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn file_action(
        &mut self,
        action: Action,
        target: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(action, Action::CancelOperations) {
            self.cancel_file_operations(cx);
            return;
        }
        if self.history_open
            || self.edit.is_some()
            || self.command_palette.is_some()
            || self.settings
            || self.permanent_delete.is_some()
            || self.organize.is_some()
        {
            return;
        }
        if matches!(action, Action::Organize) {
            let folder = target.filter(|path| path.is_dir());
            self.open_organize(folder, window, cx);
            return;
        }
        if matches!(action, Action::SaveSearch) {
            self.save_current_search(cx);
            return;
        }
        if matches!(action, Action::Download) {
            let paths: Vec<PathBuf> = self
                .command_selection(target.as_ref(), cx)
                .into_iter()
                .filter(|p| crate::cloud::is_placeholder(p))
                .collect();
            if paths.is_empty() {
                return;
            }
            self.message = Some("Downloading…".into());
            let job = cx.background_executor().spawn(async move {
                let mut failed = None;
                let mut done = 0;
                for path in &paths {
                    match crate::cloud::download(path) {
                        Ok(()) => done += 1,
                        Err(error) => failed = Some(error),
                    }
                }
                (done, failed, paths)
            });
            cx.spawn_in(window, async move |this, cx| {
                let (done, failed, paths) = job.await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.message = failed;
                    let parents: Vec<PathBuf> =
                        paths.iter().filter_map(|p| p.parent().map(PathBuf::from)).collect();
                    let _ = this.backend.tx.send(Command::RefreshParents(parents));
                    if done > 0 {
                        this.show_toast(
                            format!("Downloaded {done} file{}", if done == 1 { "" } else { "s" }),
                            false,
                            window,
                            cx,
                        );
                    }
                    cx.notify();
                });
            })
            .detach();
            return;
        }
        if matches!(action, Action::Extract | Action::Compress) {
            let paths = self.command_selection(target.as_ref(), cx);
            self.archive_job(matches!(action, Action::Compress), paths, window, cx);
            return;
        }
        if matches!(action, Action::ReportBug) {
            self.report_bug(cx);
            return;
        }
        if matches!(action, Action::Shortcuts) {
            self.open_shortcuts(window, cx);
            return;
        }
        self.suggestions = None;
        if matches!(action, Action::PutBack) {
            if let Some(job) = self
                .operation_history
                .as_ref()
                .ok()
                .and_then(|history| history.latest_put_back())
            {
                self.put_back_job(job, window, cx);
            }
            return;
        }
        if matches!(action, Action::History) {
            self.open_operation_history(window, cx);
            return;
        }
        if matches!(action, Action::UndoRename | Action::RedoRename) {
            let redo = matches!(action, Action::RedoRename);
            self.reverse_file_operation(redo, window, cx);
            return;
        }
        if matches!(action, Action::QuickLook) {
            self.open_quick_look(target, window, cx);
            return;
        }
        if matches!(action, Action::DocumentPane) {
            let path = target.or_else(|| self.selected(cx).map(|row| row.file.path));
            if let Some(path) = path {
                self.open_document_pane(path, window, cx);
            }
            return;
        }
        let path = target
            .clone()
            .or_else(|| self.selected(cx).map(|r| r.file.path));
        let selected_paths = {
            let data = self.table.read(cx).delegate();
            if !data.selected_many.is_empty()
                && target
                    .as_ref()
                    .is_none_or(|p| data.selected_many.contains(p))
            {
                data.rows
                    .iter()
                    .filter(|r| data.selected_many.contains(&r.file.path))
                    .map(|r| r.file.path.clone())
                    .collect::<Vec<_>>()
            } else {
                // A keyboard caret is not a selection (Ctrl+arrow, or the last
                // Ctrl+click deselecting an item). Context-menu targets remain explicit.
                target.clone().into_iter().collect()
            }
        };
        if selected_paths.is_empty()
            && matches!(
                action,
                Action::DeletePermanently
                    | Action::Recycle
                    | Action::Copy
                    | Action::Cut
                    | Action::Rename
                    | Action::GroupSelection
                    | Action::BatchRename
                    | Action::SuggestNames
            )
        {
            return;
        }
        if matches!(action, Action::Share) {
            let paths = if selected_paths.is_empty() {
                path.clone().into_iter().collect()
            } else {
                selected_paths
            };
            if paths.is_empty() {
                return;
            }
            let Some(hwnd) = crate::instance::hwnd(window) else {
                return;
            };
            // Revoke any previous per-window handler before installing a new selection.
            self.share = None;
            match crate::native_share::Session::show(paths, windows::Win32::Foundation::HWND(hwnd))
            {
                Ok(session) => self.share = Some(session),
                Err(error) => self.message = Some(format!("Cannot open Windows Share: {error}")),
            }
            cx.notify();
            return;
        }
        let folder = self.destination(target.as_ref(), cx);
        if matches!(action, Action::DeletePermanently) {
            use gpui_component::WindowExt;
            if selected_paths.is_empty() || window.has_active_dialog(cx) {
                return;
            }
            let cancel = cx.focus_handle();
            let delete = cx.focus_handle();
            self.permanent_delete = Some(super::permanent_delete::Confirmation {
                paths: selected_paths,
                empty_recycle_bin: None,
                root: cx.focus_handle(),
                cancel: cancel.clone(),
                delete: delete.clone(),
                restore: window.focused(cx),
            });
            window.focus(&delete, cx);
            cx.notify();
            let owner = cx.entity().downgrade();
            window.defer(cx, move |window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if let Some(confirm) = &this.permanent_delete {
                        window.focus(&confirm.delete, cx);
                    }
                });
            });
            return;
        }
        if matches!(
            action,
            Action::Rename
                | Action::NewFolder
                | Action::NewFile
                | Action::GroupSelection
                | Action::BatchRename
                | Action::SuggestNames
        ) {
            if self.files_pending != 0 {
                self.message = Some("Wait for the current file operation to finish.".into());
                cx.notify();
                return;
            }
            let (path, folder, name) = if matches!(action, Action::Rename) {
                let Some(path) = path else {
                    return;
                };
                let folder = path.parent().unwrap_or(&path).to_path_buf();
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                (Some(path), folder, name)
            } else {
                let folder = if matches!(
                    action,
                    Action::GroupSelection | Action::BatchRename | Action::SuggestNames
                ) {
                    let Some(folder) = selected_paths
                        .first()
                        .and_then(|path| path.parent())
                        .map(std::path::Path::to_path_buf)
                    else {
                        return;
                    };
                    if selected_paths
                        .iter()
                        .any(|path| path.parent() != Some(folder.as_path()))
                    {
                        self.message =
                            Some("Select items from the same folder before grouping them.".into());
                        cx.notify();
                        return;
                    }
                    folder
                } else {
                    let Some(folder) = folder else {
                        return;
                    };
                    folder
                };
                (
                    None,
                    folder,
                    if matches!(action, Action::BatchRename | Action::SuggestNames) {
                        String::new()
                    } else if matches!(action, Action::NewFile) {
                        "New text document.txt".into()
                    } else {
                        "New folder".into()
                    },
                )
            };
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .context_menu(false)
                    .placeholder("File or folder name")
                    .default_value(name)
            });
            input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
            let subscription = cx.subscribe(&input, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    if let Some(edit) = &mut this.edit {
                        edit.error = None;
                    }
                    this.refresh_batch_preview(cx);
                    cx.notify();
                }
            });
            self.edit = Some(Edit {
                batch: None,
                file: matches!(action, Action::NewFile),
                grouping: if matches!(action, Action::GroupSelection) {
                    selected_paths.clone()
                } else {
                    vec![]
                },
                pending: false,
                error: None,
                _subscription: subscription,
                input,
                path,
                folder,
                root_focus: cx.focus_handle(),
                cancel_focus: cx.focus_handle(),
                save_focus: cx.focus_handle(),
            });
            if matches!(action, Action::BatchRename) {
                self.install_batch_rename(selected_paths, window, cx);
            } else if matches!(action, Action::SuggestNames) {
                self.install_batch_rename(selected_paths, window, cx);
                self.batch_suggest(window, cx);
            }
            cx.notify();
            return;
        }
        let op = match action {
            Action::Paste => {
                let Some(folder) = folder else {
                    self.message = Some("Open a folder before pasting files.".into());
                    cx.notify();
                    return;
                };
                Operation::Paste { folder }
            }
            action => {
                let Some(path) = path else {
                    return;
                };
                match action {
                    Action::Open | Action::Reveal => {
                        if matches!(action, Action::Open)
                            && self.table.read(cx).delegate().rows.iter().any(|r| {
                                r.file.path == path
                                    && r.file.kind & 3 == seek_engine::walk::KIND_DIR
                            })
                        {
                            self.scope_to(Some(path), window, cx);
                        } else {
                            let _ = self
                                .backend
                                .tx
                                .send(Command::Open(path, matches!(action, Action::Reveal)));
                        }
                        return;
                    }
                    Action::Copy | Action::Cut => Operation::Clipboard {
                        paths: selected_paths,
                        cut: matches!(action, Action::Cut),
                    },
                    Action::Recycle => Operation::Recycle {
                        paths: selected_paths,
                    },
                    Action::QuickLook => return,
                    Action::OpenWith | Action::Properties => Operation::Dialog {
                        path,
                        properties: matches!(action, Action::Properties),
                    },
                    Action::Path | Action::Name | Action::QuotedPath => {
                        let paths = if selected_paths.is_empty() {
                            vec![path]
                        } else {
                            selected_paths
                        };
                        let value = paths
                            .into_iter()
                            .map(|path| match action {
                                Action::Name => path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                                Action::QuotedPath => format!("\"{}\"", path.display()),
                                _ => path.to_string_lossy().into_owned(),
                            })
                            .collect::<Vec<_>>()
                            .join("\r\n");
                        cx.write_to_clipboard(ClipboardItem::new_string(value));
                        return;
                    }
                    _ => return,
                }
            }
        };
        self.run_file_op(op, window, cx);
    }
    pub(super) fn run_file_op(&mut self, op: Operation, window: &Window, cx: &mut Context<Self>) {
        let owner = self.next_op_owner.take();
        match self.files.send(
            op,
            crate::instance::hwnd(window).unwrap_or(std::ptr::null_mut()) as usize,
        ) {
            Ok(()) => {
                self.files_pending = self.files_pending.saturating_add(1);
                self.message = None;
                if let Some(job) = &owner {
                    crate::mcp::owner::started(job, None);
                }
                self.op_owners.push_back(owner);
            }
            Err(error) => {
                if let Some(job) = &owner {
                    crate::mcp::owner::started(job, Some(error.clone()));
                }
                self.message = Some(error);
                self.history_loading = false;
            }
        }
        cx.notify();
    }

    /// Runs a waiting agent undo once the file worker is idle, reloading the
    /// undo history once first if it has nothing to reverse (a just-started
    /// window hasn't read it yet).
    pub(super) fn try_agent_undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(job) = self.pending_agent_undo.clone() else {
            return;
        };
        if self.files_pending != 0 {
            return; // Tried again when the worker finishes.
        }
        if self.can_reverse_rename(false) {
            self.pending_agent_undo = None;
            self.agent_undo_refreshed = false;
            self.next_op_owner = Some(job.clone());
            self.reverse_file_operation(false, window, cx);
            if self.next_op_owner.take().is_some() {
                crate::mcp::owner::started(&job, Some("Nothing to undo.".into()));
            }
        } else if !self.agent_undo_refreshed {
            self.agent_undo_refreshed = true;
            self.run_file_op(Operation::History, window, cx);
        } else {
            self.pending_agent_undo = None;
            self.agent_undo_refreshed = false;
            crate::mcp::owner::started(&job, Some("Nothing to undo.".into()));
        }
    }

    /// A change an agent asked for, run as the person's would be.
    pub(super) fn run_agent_request(
        &mut self,
        request: crate::mcp::owner::Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::mcp::owner::Request;
        let job = match request {
            Request::File { op, job } => {
                self.next_op_owner = Some(job.clone());
                self.run_file_op(op, window, cx);
                job
            }
            Request::Undo { job } => {
                if let Some(earlier) = self.pending_agent_undo.replace(job) {
                    crate::mcp::owner::started(&earlier, Some("Replaced by a newer undo.".into()));
                }
                self.agent_undo_refreshed = false;
                self.try_agent_undo(window, cx);
                return;
            }
        };
        // Nothing reached the worker (nothing to undo, or refused early).
        if self.next_op_owner.take().is_some() {
            crate::mcp::owner::started(
                &job,
                Some(self.message.clone().unwrap_or_else(|| "Nothing to undo.".into())),
            );
        }
    }
    pub(super) fn cancel_file_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.edit.as_ref().is_some_and(|edit| edit.pending) {
            self.cancel_file_operations(cx);
            return;
        }
        self.table.read(cx).focus_handle(cx).focus(window, cx);
        self.edit = None;
        cx.notify();
    }
    pub(super) fn cancel_file_operations(&mut self, cx: &mut Context<Self>) {
        if self.files_pending == 0 {
            return;
        }
        self.files.cancel();
        self.message =
            Some("Stopping file operations. Completed items are preserved in history.".into());
        if let Some(edit) = &mut self.edit
            && edit.pending
        {
            edit.error = Some("Stopping the operation...".into());
        }
        cx.notify();
    }
    pub(super) fn finish_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.edit.as_ref().is_some_and(|edit| edit.batch.is_some()) {
            self.finish_batch_rename(window, cx);
            return;
        }
        let Some(edit) = &mut self.edit else {
            return;
        };
        if edit.pending {
            return;
        }
        let name = edit.input.read(cx).value().to_string();
        if !crate::file_ops::valid_name(&name) {
            edit.error =
                Some("Enter a valid Windows name without slashes or reserved characters.".into());
            cx.notify();
            return;
        }
        if edit
            .path
            .as_ref()
            .is_some_and(|path| path.file_name() == Some(std::ffi::OsStr::new(&name)))
        {
            self.cancel_file_edit(window, cx);
            return;
        }
        let op = match &edit.path {
            Some(path) => Operation::Rename {
                path: path.clone(),
                name,
            },
            None if edit.file => Operation::NewFile {
                folder: edit.folder.clone(),
                name,
            },
            None if !edit.grouping.is_empty() => Operation::GroupSelection {
                paths: edit.grouping.clone(),
                folder: edit.folder.clone(),
                name,
            },
            None => Operation::NewFolder {
                folder: edit.folder.clone(),
                name,
            },
        };
        edit.pending = true;
        edit.error = None;
        self.run_file_op(op, window, cx);
        if let Some(error) = self.message.clone()
            && let Some(edit) = &mut self.edit
        {
            edit.pending = false;
            edit.error = Some(error);
        }
        cx.notify();
    }
    pub(super) fn choose_kind(
        &mut self,
        filter: &str,
        replace_name: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = if replace_name {
            String::new()
        } else {
            self.input.read(cx).value().to_string()
        };
        let text = crate::kinds::replace_kind(&text, filter);
        self.input.update(cx, |input, cx| {
            input.set_value(text, window, cx);
            input.focus(window, cx);
        });
        self.suggestions = None;
        self.largest = None;
        self.content_mode = false;
        self.decorate_filters(window, cx);
        self.submit(cx);
        cx.notify();
    }
    pub(super) fn refresh_suggestions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        let word = text.trim();
        if crate::location::address(word).is_some() {
            let owner = cx.entity().downgrade();
            let menu = PopupMenu::build(window, cx, move |menu, window, _| {
                menu_style(menu, window).item(
                    PopupMenuItem::new(format!("Open folder {word}")).on_click(
                        move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                this.open_address(window, cx);
                            });
                        },
                    ),
                )
            });
            self.suggestion_width = px(420.);
            self.attach_suggestions(menu, Point::default(), window, cx);
            self.suggestions_at_search = true;
            return;
        }
        if self.content_mode
            || word.is_empty()
            || word.contains(':')
            || !self.input.read(cx).focus_handle(cx).is_focused(window)
        {
            self.suggestions = None;
            return;
        }
        let owner = cx.entity().downgrade();
        let menu = PopupMenu::build(window, cx, move |mut menu, window, _| {
            menu = menu_style(menu, window);
            let literal = word.to_string();
            let owner_name = owner.clone();
            menu = menu.item(
                PopupMenuItem::new(format!("Name contains “{word}”")).on_click(
                    move |_, window, cx| {
                        let _ = owner_name.update(cx, |this, cx| {
                            this.input.update(cx, |input, cx| {
                                input.set_value(
                                    crate::kinds::name_contains_query(&literal),
                                    window,
                                    cx,
                                )
                            });
                            this.suggestions = None;
                            this.submit(cx);
                        });
                    },
                ),
            );
            let contents_owner = owner.clone();
            menu = menu.item(
                PopupMenuItem::new(format!("Content contains \u{201c}{word}\u{201d}")).on_click(
                    move |_, window, cx| {
                        let _ = contents_owner
                            .update(cx, |this, cx| this.set_content_mode(true, window, cx));
                    },
                ),
            );
            for (label, filter) in crate::kinds::suggestions(word) {
                let owner = owner.clone();
                menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    let _ =
                        owner.update(cx, |this, cx| this.choose_kind(&filter, true, window, cx));
                }));
            }
            menu
        });
        self.suggestion_width = px(420.);
        self.attach_suggestions(menu, Point::default(), window, cx);
        self.suggestions_at_search = true;
    }
    pub(super) fn set_content_mode(
        &mut self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if enabled && self.open_address(window, cx) {
            return;
        }
        self.content_mode = enabled;
        self.largest = None;
        self.suggestions = None;
        self.normal_results = None;
        self.table.update(cx, |table, _| {
            table.delegate_mut().sort = None;
            table.delegate_mut().expanded.clear();
            table.delegate_mut().children.clear();
        });
        if enabled {
            self.set_view(View::List, cx);
        }
        self.input.update(cx, |input, cx| input.focus(window, cx));
        self.submit(cx);
        cx.notify();
    }
    fn attach_suggestions(
        &mut self,
        menu: Entity<PopupMenu>,
        origin: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.suggestion_dismiss =
            Some(
                cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, window, cx| {
                    this.suggestions = None;
                    this.input.update(cx, |input, cx| input.focus(window, cx));
                    cx.notify();
                }),
            );
        self.suggestions_at_search = false;
        self.suggestion_origin = origin;
        self.suggestions = Some(menu);
        cx.notify();
    }
    pub(super) fn kind_popup(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let menu = PopupMenu::build(window, cx, move |menu, _, _| kind_menu(menu, owner));
        menu.focus_handle(cx).focus(window, cx);
        self.suggestion_width = px(160.);
        self.attach_suggestions(
            menu,
            point(bounds.origin.x, bounds.bottom() + px(4.)),
            window,
            cx,
        );
    }
    pub(super) fn leave_largest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.largest.take().is_none() {
            return;
        }
        if let Some((query, sort, view)) = self.normal_results.take() {
            self.view = view;
            self.table
                .update(cx, |table, _| table.delegate_mut().sort = sort);
            self.input
                .update(cx, |input, cx| input.set_value(query, window, cx));
            self.decorate_filters(window, cx);
        }
        self.submit(cx);
        cx.notify();
    }
    pub(super) fn daily_controls(
        &self,
        p: crate::appearance::Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let view = cx.entity().downgrade();
        let active = self.largest;
        let content = self.content_mode;
        Button::new("file-actions")
            .ghost()
            .w(px(30.))
            .h(px(30.))
            .p_0()
            .rounded(px(5.))
            .child(Icon::new(gpui_component::IconName::Ellipsis).size(px(14.)))
            .custom(
                gpui_component::button::ButtonCustomVariant::new(cx)
                    .foreground(p.muted)
                    .hover(p.hover)
                    .active(p.accent.opacity(0.14)),
            )
            .tooltip("File actions and result modes")
            .accessibility_label("File actions")
            .dropdown_menu(move |mut menu, window, cx| {
                menu = menu_style(menu, window);
                let normal = view.clone();
                menu = menu.item(
                    PopupMenuItem::new("All items")
                        .checked(active.is_none() && !content)
                        .on_click(move |_, window, cx| {
                            let _ = normal.update(cx, |this, cx| {
                                if this.content_mode {
                                    this.set_content_mode(false, window, cx);
                                } else {
                                    this.leave_largest(window, cx);
                                }
                            });
                        }),
                );
                for (folders, label) in [(false, "Largest files"), (true, "Largest folders")] {
                    let view = view.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .checked(active == Some(folders))
                            .on_click(move |_, window, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    if this.largest.is_none() {
                                        this.normal_results = Some((
                                            this.input.read(cx).value().to_string(),
                                            this.table.read(cx).delegate().sort,
                                            this.view,
                                        ));
                                    }
                                    this.content_mode = false;
                                    this.set_view(View::List, cx);
                                    this.largest = Some(folders);
                                    this.input
                                        .update(cx, |input, cx| input.set_value("", window, cx));
                                    this.table.update(cx, |table, _| {
                                        table.delegate_mut().sort =
                                            Some((2, ColumnSort::Descending))
                                    });
                                    this.submit(cx);
                                });
                            }),
                    );
                }
                let contents = view.clone();
                menu = menu.item(
                    PopupMenuItem::new("Search file contents")
                        .checked(content)
                        .on_click(move |_, window, cx| {
                            let _ = contents.update(cx, |this, cx| {
                                this.set_content_mode(!this.content_mode, window, cx)
                            });
                        }),
                );
                let types = view.clone();
                menu = menu.submenu("Type", window, cx, move |menu, _, _| {
                    kind_menu(menu, types.clone())
                });
                menu = menu.separator();
                if let Some(entity) = view.upgrade() {
                    let this = entity.read(cx);
                    for (label, action, visible) in [
                        (
                            "Organize this folder…",
                            Action::Organize,
                            this.scope.is_some(),
                        ),
                        (
                            "Save current search",
                            Action::SaveSearch,
                            !this.input.read(cx).value().trim().is_empty(),
                        ),
                        (
                            "Undo last operation",
                            Action::UndoRename,
                            this.can_reverse_rename(false),
                        ),
                        (
                            "Redo last operation",
                            Action::RedoRename,
                            this.can_reverse_rename(true),
                        ),
                        (
                            "Stop file operations",
                            Action::CancelOperations,
                            this.files_pending != 0,
                        ),
                    ] {
                        if !visible {
                            continue;
                        }
                        let owner = view.clone();
                        menu =
                            menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.file_action(action, None, window, cx)
                                });
                            }));
                    }
                }
                for (label, action) in [
                    ("Operation history", Action::History),
                    ("Keyboard shortcuts", Action::Shortcuts),
                    ("Report a bug…", Action::ReportBug),
                ] {
                    let owner = view.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        let _ =
                            owner.update(cx, |this, cx| this.file_action(action, None, window, cx));
                    }));
                }
                let owner = view.clone();
                menu.item(
                    PopupMenuItem::new("All commands...").on_click(move |_, window, cx| {
                        let owner = owner.clone();
                        window.defer(cx, move |window, cx| {
                            let _ =
                                owner.update(cx, |this, cx| this.open_command_palette(window, cx));
                        });
                    }),
                )
            })
            .into_any_element()
    }
    pub(super) fn search_suggestions(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        if self.suggestions_at_search
            && let Some(menu) = &self.suggestions
            && (self.input.read(cx).focus_handle(cx).is_focused(window)
                || menu.focus_handle(cx).contains_focused(window, cx))
        {
            // The parent is the actual search field. Flex centering, sidebar and
            // window resizing therefore move this popup in the same layout pass.
            return deferred(
                div()
                    .absolute()
                    .top(px(36.))
                    .left_0()
                    .w_full()
                    .max_w(px(420.))
                    .child(menu.clone()),
            )
            .with_priority(gpui_base::POPUP_PRIORITY)
            .into_any_element();
        }
        div().into_any_element()
    }
    pub(super) fn daily_overlays(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        // Size the overlay to the window, not the zero-height trailing flex
        // child. Otherwise the edit sheet can land below a restored window.
        if self.suggestions.is_none()
            && self.edit.is_none()
            && self.permanent_delete.is_none()
            && self.organize.is_none()
            && self.toast.is_none()
        {
            return div().into_any_element();
        }
        let mut layer = div().absolute().inset_0();
        if let Some(menu) = &self.suggestions
            && !self.suggestions_at_search
            && (self.input.read(cx).focus_handle(cx).is_focused(window)
                || menu.focus_handle(cx).contains_focused(window, cx))
        {
            layer = layer.child(
                div()
                    .absolute()
                    .top(self.suggestion_origin.y)
                    .left(self.suggestion_origin.x)
                    .w(self.suggestion_width)
                    .child(menu.clone()),
            );
        }
        if let Some(edit) = &self.edit {
            let p = crate::appearance::palette(&self.preferences, cx);
            layer = layer.child(
                div()
                    .absolute()
                    .inset_0()
                    .occlude()
                    .bg(rgba(0x00000055))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("file-name-sheet")
                            .track_focus(&edit.root_focus)
                            .role(Role::Dialog)
                            .aria_label(if edit.batch.is_some() { "Batch rename" } else { "File name" })
                            .w(px(if edit.batch.as_ref().is_some_and(|b| b.advanced.is_some()) {
                                960.
                            } else if edit.batch.is_some() {
                                680.
                            } else {
                                380.
                            }))
                            .max_w(relative(0.95))
                            .max_h(relative(0.9))
                            .overflow_y_scroll()
                            .p_4()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .rounded(px(12.))
                            .bg(crate::appearance::modal_surface(p))
                            .border_1()
                            .border_color(crate::appearance::modal_border(p))
                            .text_color(p.text)
                            .child(if edit.batch.as_ref().is_some_and(|b| b.suggesting()) {
                                "Suggest names"
                            } else if edit.batch.is_some() {
                                "Batch rename"
                            } else if edit.path.is_some() {
                                "Rename"
                            } else if !edit.grouping.is_empty() {
                                "New folder with selection"
                            } else if edit.file {
                                "New file"
                            } else {
                                "New folder"
                            })
                            .child(
                                if edit.batch.is_some() { self.batch_name_fields(cx) }
                                else { Input::new(&edit.input).readonly(edit.pending).min_h(px(36.)).into_any_element() },
                            )
                            .when(!edit.grouping.is_empty(), |dialog| {
                                dialog.child(div().text_size(px(12.)).text_color(p.muted)
                                    .child(format!("Move {} selected items into this folder. Undo returns the items and keeps the folder.", edit.grouping.len())))
                            })
                            .when(edit.input.read(cx).value().starts_with(' '), |dialog| {
                                dialog.child(div().text_size(px(12.)).text_color(p.muted).child(
                                    "This name begins with a space, so it is a different filename.",
                                ))
                            })
                            .when_some(edit.error.clone(), |dialog, error| {
                                dialog.child(
                                    div()
                                        .id("file-edit-error")
                                        .role(Role::Status)
                                        .aria_label(error.clone())
                                        .text_size(px(13.))
                                        .child(error),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap_2()
                                    .child(
                                        Button::new("cancel-file-edit")
                                            .track_focus(&edit.cancel_focus)
                                            .accessibility_label(if edit.pending {
                                                "Stop operation"
                                            } else {
                                                "Cancel"
                                            })
                                            .child(interface::button_text(if edit.pending {
                                                "Stop"
                                            } else {
                                                "Cancel"
                                            }))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.cancel_file_edit(window, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new("save-file-edit")
                                            .track_focus(&edit.save_focus)
                                            .accessibility_label(if edit.batch.is_some() { "Apply renames" } else { "Save" })
                                            .disabled(edit.pending || !self.batch_apply_ready())
                                            .child(interface::button_text(if edit.pending {
                                                "Saving..."
                                            } else if edit.batch.is_some() { "Apply renames" }
                                            else { "Save" }))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.finish_edit(window, cx)
                                            })),
                                    ),
                            ),
                    ),
            );
        }
        if self.permanent_delete.is_some() {
            layer = layer.child(self.permanent_delete_view(window, cx));
        }
        if self.organize.is_some() {
            layer = layer.child(self.organize_view(window, cx));
        }
        if let Some(toast) = self.toast_view(cx) {
            layer = layer.child(toast);
        }
        deferred(layer)
            .with_priority(gpui_base::POPUP_PRIORITY + if self.edit.is_some() { 10 } else { 0 })
            .into_any_element()
    }
    pub(super) fn hover_drop_folder(
        &mut self,
        path: PathBuf,
        event: &DragMoveEvent<FilesDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.bounds.contains(&event.event.position)
            || !drop_allowed(&event.drag(cx).paths, &path)
        {
            if self.drag_hover.as_ref() == Some(&path) {
                self.drag_hover = None;
                self.drag_hover_task = Task::ready(());
            }
            return;
        }
        if self.drag_hover.as_ref() == Some(&path) {
            return;
        }
        self.drag_hover = Some(path.clone());
        let bounds = event.bounds;
        let delay = self.preferences.drag_hover_ms as u64;
        self.drag_hover_task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(delay))
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.drag_hover.as_ref() == Some(&path)
                    && cx.has_active_drag()
                    && bounds.contains(&window.mouse_position())
                    && window.is_window_active()
                    && !this.table.read(cx).delegate().expanded.contains(&path)
                {
                    this.expand_folder(path, true, cx);
                }
            });
        });
    }
    pub(super) fn drop_files(
        &mut self,
        drag: &FilesDrag,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag_hover = None;
        self.drag_hover_task = Task::ready(());
        if self.edit.is_some() || self.history_open || self.settings {
            return;
        }
        if !drop_allowed(&drag.paths, &folder) {
            self.message = Some("Choose a different folder outside the selected items.".into());
            cx.notify();
            return;
        }
        self.run_file_op(
            Operation::Move {
                paths: drag.paths.as_ref().clone(),
                folder,
            },
            window,
            cx,
        );
    }
}

#[cfg(test)]
mod drop_tests {
    use super::drop_allowed;
    use std::path::{Path, PathBuf};
    #[test]
    fn folder_drop_rejects_aliases_descendants_and_no_ops() {
        let paths = vec![PathBuf::from(r"C:\Owned\Folder")];
        assert!(!drop_allowed(&paths, Path::new(r"\\?\c:\owned\folder")));
        assert!(!drop_allowed(&paths, Path::new(r"C:\Owned\Folder\child")));
        assert!(!drop_allowed(&paths, Path::new(r"C:\Owned")));
        assert!(drop_allowed(&paths, Path::new(r"C:\Owned\Folder-two")));
        assert!(!drop_allowed(&[], Path::new(r"C:\Owned")));
    }
}
