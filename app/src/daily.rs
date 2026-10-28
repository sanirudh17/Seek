use super::*;
use crate::file_ops::Operation;
use gpui_component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};

#[derive(Clone, Copy)]
pub(super) enum Action {
    Open,
    OpenWith,
    Reveal,
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
    Properties,
}
pub(super) struct Edit {
    pub(super) input: Entity<InputState>,
    path: Option<PathBuf>,
    folder: PathBuf,
    pub(super) cancel_focus: FocusHandle,
    pub(super) save_focus: FocusHandle,
}
#[derive(Clone)]
pub(super) struct FilesDrag {
    pub paths: Arc<Vec<PathBuf>>,
    pub label: String,
}
impl Render for FilesDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_component::ActiveTheme;
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .child(self.label.clone())
    }
}
pub(super) fn menu_style(menu: PopupMenu, window: &Window) -> PopupMenu {
    menu.scrollable(true)
        .scrollbar_visible(false)
        .item_height(px(28.))
        .max_h((window.viewport_size().height - px(24.)).max(px(120.)))
}
pub(super) fn actions_menu(
    mut menu: PopupMenu,
    owner: WeakEntity<Seek>,
    target: Option<PathBuf>,
    target_is_folder: bool,
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
    menu = menu.item(
        PopupMenuItem::new(if target_is_folder {
            "Open folder in new tab"
        } else {
            "Open search in new tab"
        })
        .on_click(move |_, window, cx| {
            let _ = tab_owner.update(cx, |this, cx| {
                this.new_tab(
                    if target_is_folder {
                        tab_target.clone()
                    } else {
                        None
                    },
                    window,
                    cx,
                )
            });
        }),
    );
    for (label, action, enabled) in [
        ("Open", Action::Open, selected),
        ("Open with…", Action::OpenWith, selected),
        ("Reveal in Explorer", Action::Reveal, selected),
        ("Copy", Action::Copy, selected),
        ("Cut", Action::Cut, selected),
        ("Paste", Action::Paste, destination),
        ("Copy path", Action::Path, selected),
        ("Copy name", Action::Name, selected),
        ("Copy path in quotes", Action::QuotedPath, selected),
        ("Rename", Action::Rename, selected),
        ("Move to Recycle Bin", Action::Recycle, selected),
        ("Delete permanently...", Action::DeletePermanently, selected),
        ("New folder", Action::NewFolder, destination),
        ("Properties", Action::Properties, selected),
    ] {
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
    pub(super) fn file_action(
        &mut self,
        action: Action,
        target: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.permanent_delete.is_some() {
            return;
        }
        self.suggestions = None;
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
            )
        {
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
        if matches!(action, Action::Rename | Action::NewFolder) {
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
                let Some(folder) = folder else {
                    return;
                };
                (None, folder, "New folder".into())
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
            self.edit = Some(Edit {
                input,
                path,
                folder,
                cancel_focus: cx.focus_handle(),
                save_focus: cx.focus_handle(),
            });
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
                    Action::OpenWith | Action::Properties => Operation::Dialog {
                        path,
                        properties: matches!(action, Action::Properties),
                    },
                    Action::Path | Action::Name | Action::QuotedPath => {
                        let value = match action {
                            Action::Name => path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                            Action::QuotedPath => format!("\"{}\"", path.display()),
                            _ => path.to_string_lossy().into_owned(),
                        };
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
        self.message = self
            .files
            .send(
                op,
                crate::instance::hwnd(window).unwrap_or(std::ptr::null_mut()) as usize,
            )
            .err();
        cx.notify();
    }
    pub(super) fn finish_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.edit.as_ref() else {
            return;
        };
        let name = edit.input.read(cx).value().to_string();
        if !crate::file_ops::valid_name(&name) {
            self.message =
                Some("Enter a valid Windows name without slashes or reserved characters.".into());
            cx.notify();
            return;
        }
        let edit = self.edit.take().unwrap();
        let op = match edit.path {
            Some(path) => Operation::Rename { path, name },
            None => Operation::NewFolder {
                folder: edit.folder,
                name,
            },
        };
        self.run_file_op(op, window, cx);
        self.table.read(cx).focus_handle(cx).focus(window, cx);
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
                actions_menu(menu, view.clone(), None, false, cx)
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
        if self.suggestions.is_none() && self.edit.is_none() && self.permanent_delete.is_none() {
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
                            .w(px(380.))
                            .p_4()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .rounded(px(12.))
                            .bg(p.sheet)
                            .text_color(p.text)
                            .child(if edit.path.is_some() {
                                "Rename"
                            } else {
                                "New folder"
                            })
                            .child(Input::new(&edit.input))
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap_2()
                                    .child(
                                        Button::new("cancel-file-edit")
                                            .track_focus(&edit.cancel_focus)
                                            .accessibility_label("Cancel")
                                            .child(interface::button_text("Cancel"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.edit = None;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("save-file-edit")
                                            .track_focus(&edit.save_focus)
                                            .accessibility_label("Save")
                                            .child(interface::button_text("Save"))
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
        deferred(layer)
            .with_priority(gpui_base::POPUP_PRIORITY + if self.edit.is_some() { 10 } else { 0 })
            .into_any_element()
    }
    pub(super) fn drop_files(
        &mut self,
        drag: &FilesDrag,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
