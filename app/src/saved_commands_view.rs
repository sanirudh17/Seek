use super::*;
use crate::saved_commands::{SavedCommand, Selection};
use gpui_component::input::{Textarea, TextareaState};

pub(super) struct Form {
    pub name: Entity<InputState>,
    pub program: Entity<InputState>,
    pub args: Entity<TextareaState>,
    pub cwd: Entity<InputState>,
    id: Option<u64>,
    selection: Selection,
    error: Option<String>,
    /// The editor shows only while adding or editing a command.
    open: bool,
}
pub(super) struct RunState {
    id: u64,
    name: String,
    pid: Option<u32>,
    result: Option<Result<crate::saved_commands::RunOutput, String>>,
}
enum RunEvent {
    Started(u32),
    Finished(Result<crate::saved_commands::RunOutput, String>),
}
impl Form {
    pub fn new(window: &mut Window, cx: &mut Context<Seek>) -> Self {
        Self {
            name: cx.new(|cx| InputState::new(window, cx).placeholder("Open in VS Code")),
            program: cx.new(|cx| InputState::new(window, cx).placeholder("Choose a program")),
            args: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("{files}")
                    .auto_grow(2, 8)
            }),
            cwd: cx.new(|cx| InputState::new(window, cx).placeholder("Current folder")),
            id: None,
            selection: Selection::One,
            error: None,
            open: false,
        }
    }
}
impl Seek {
    pub(super) fn command_selection(&self, target: Option<&PathBuf>, cx: &App) -> Vec<PathBuf> {
        let table = self.table.read(cx);
        let data = table.delegate();
        if target.is_none_or(|path| data.selected_many.contains(path)) {
            data.rows
                .iter()
                .filter(|row| data.selected_many.contains(&row.file.path))
                .map(|row| row.file.path.clone())
                .collect()
        } else {
            target.cloned().into_iter().collect()
        }
    }
    pub(super) fn invoke_saved_command(
        &mut self,
        command: SavedCommand,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let invocation = match command.expand(&paths, self.scope.as_deref()) {
            Ok(invocation) => invocation,
            Err(error) => {
                self.message = Some(error);
                cx.notify();
                return;
            }
        };
        self.command_run_revision = self.command_run_revision.wrapping_add(1);
        let id = self.command_run_revision;
        self.command_run = Some(RunState {
            id,
            name: command.name,
            pid: None,
            result: None,
        });
        self.command_run_focus.focus(window, cx);
        cx.notify();
        let (send, receive) = async_channel::unbounded();
        cx.background_executor()
            .spawn(async move {
                let result = crate::saved_commands::run(invocation, |pid| {
                    let _ = send.try_send(RunEvent::Started(pid));
                });
                let _ = send.try_send(RunEvent::Finished(result));
            })
            .detach();
        cx.spawn(async move |this, cx| {
            while let Ok(event) = receive.recv().await {
                let finished = matches!(&event, RunEvent::Finished(_));
                let _ = this.update(cx, |this, cx| {
                    if let Some(run) = &mut this.command_run
                        && run.id == id
                    {
                        match event {
                            RunEvent::Started(pid) => run.pid = Some(pid),
                            RunEvent::Finished(result) => run.result = Some(result),
                        }
                        cx.notify();
                    }
                });
                if finished {
                    break;
                }
            }
        })
        .detach();
    }
    pub(super) fn close_command_result(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings {
            self.settings_focus.focus(window, cx);
        } else {
            self.table.read(cx).focus_handle(cx).focus(window, cx);
        }
        self.command_run = None;
        cx.notify();
    }
    pub(super) fn command_result_view(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(run) = &self.command_run else {
            return div().into_any_element();
        };
        let status = match &run.result {
            None => run
                .pid
                .map(|pid| format!("Running · process {pid}"))
                .unwrap_or_else(|| "Starting program...".into()),
            Some(Err(error)) => error.clone(),
            Some(Ok(result)) => format!(
                "{} · exit {}",
                if result.success {
                    "Finished successfully"
                } else {
                    "Program finished with an error"
                },
                result
                    .code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".into())
            ),
        };
        let mut body = div()
            .id("saved-command-result-body")
            .max_h(px(300.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3();
        let mut copy = String::new();
        if let Some(Ok(result)) = &run.result {
            for (label, text) in [("Output", &result.stdout), ("Messages", &result.stderr)] {
                if !text.is_empty() {
                    copy.push_str(text);
                    body = body
                        .child(div().text_size(px(11.)).text_color(p.muted).child(label))
                        .child(
                            div()
                                .font_family(crate::tokens::FONT_MONO)
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .child(text.clone()),
                        );
                }
            }
            if copy.is_empty() {
                body = body.child(
                    div()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child("The program did not write any output."),
                );
            }
            if result.truncated {
                body = body.child(
                    div()
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child("Output shortened to 32 KiB per stream."),
                );
            }
        }
        div()
            .id("saved-command-result-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .bg(rgba(0x00000055))
            .child(
                div()
                    .id("saved-command-result")
                    .role(Role::Dialog)
                    .aria_label("Saved command result")
                    .track_focus(&self.command_run_focus)
                    .w(px(640.))
                    .max_w(relative(0.94))
                    .max_h(relative(0.85))
                    .p_4()
                    .rounded(px(12.))
                    .bg(crate::appearance::modal_surface(p))
                    .border_1()
                    .border_color(crate::appearance::modal_border(p))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(run.name.clone()),
                            )
                            .child(
                                super::settings_view::control(
                                    "close-command-result",
                                    "Close",
                                    p,
                                    cx,
                                )
                                .h(px(28.))
                                .with_size(px(28.))
                                .px_2()
                                .on_click(cx.listener(
                                    |this, _, window, cx| this.close_command_result(window, cx),
                                )),
                            ),
                    )
                    .child(
                        div()
                            .id("saved-command-status")
                            .role(Role::Status)
                            .aria_label(status.clone())
                            .text_size(px(12.))
                            .text_color(p.muted)
                            .child(status),
                    )
                    .child(body)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().flex_1().text_size(px(11.)).text_color(p.muted).child(
                                if run.result.is_none() {
                                    "Closing this view leaves the program running."
                                } else {
                                    "Esc: close"
                                },
                            ))
                            .when(!copy.is_empty(), |footer| {
                                footer.child(
                                    super::settings_view::control(
                                        "copy-command-output",
                                        "Copy output",
                                        p,
                                        cx,
                                    )
                                    .h(px(28.))
                                    .with_size(px(28.))
                                    .px_2()
                                    .on_click(
                                        move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copy.clone(),
                                            ))
                                        },
                                    ),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }
    fn edit_saved_command(
        &mut self,
        command: Option<SavedCommand>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let form = &mut self.command_form;
        form.open = true;
        form.id = command.as_ref().map(|command| command.id);
        form.selection = command
            .as_ref()
            .map_or(Selection::One, |command| command.selection);
        form.error = None;
        form.name.update(cx, |input, cx| {
            input.set_value(command.as_ref().map_or("", |c| c.name.as_str()), window, cx)
        });
        form.program.update(cx, |input, cx| {
            input.set_value(
                command
                    .as_ref()
                    .map(|c| c.program.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                window,
                cx,
            )
        });
        form.args.update(cx, |input, cx| {
            input.set_value(
                command
                    .as_ref()
                    .map(|c| c.args.join("\n"))
                    .unwrap_or_default(),
                window,
                cx,
            )
        });
        form.cwd.update(cx, |input, cx| {
            input.set_value(
                command
                    .as_ref()
                    .and_then(|c| c.cwd.as_ref())
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                window,
                cx,
            )
        });
        form.name.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }
    fn save_command_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form = &self.command_form;
        let id = form.id.or_else(|| {
            self.preferences
                .saved_commands
                .iter()
                .map(|command| command.id)
                .max()
                .unwrap_or(0)
                .checked_add(1)
        });
        let Some(id) = id else {
            self.command_form.error = Some("No more command identifiers are available.".into());
            cx.notify();
            return;
        };
        let cwd = form.cwd.read(cx).value().trim().to_owned();
        let command = SavedCommand {
            id,
            name: form.name.read(cx).value().trim().into(),
            program: PathBuf::from(form.program.read(cx).value().trim()),
            args: form
                .args
                .read(cx)
                .value()
                .lines()
                .map(|line| line.trim_end_matches('\r').to_owned())
                .collect(),
            cwd: (!cwd.is_empty()).then(|| PathBuf::from(cwd)),
            selection: form.selection,
        };
        if let Err(error) = command.validate() {
            self.command_form.error = Some(error);
            cx.notify();
            return;
        }
        if let Some(existing) = self
            .preferences
            .saved_commands
            .iter_mut()
            .find(|row| row.id == id)
        {
            *existing = command;
        } else if self.preferences.saved_commands.len() < 32 {
            self.preferences.saved_commands.push(command);
        } else {
            self.command_form.error =
                Some("Remove a saved command before adding more than 32.".into());
            cx.notify();
            return;
        }
        self.save_preferences(window, cx);
        self.edit_saved_command(None, window, cx);
        self.command_form.open = false;
        self.message =
            Some("Saved. Run it from Jump (Ctrl+K) or a file's right-click menu.".into());
        self.settings_focus.focus(window, cx);
        cx.notify();
    }
    fn close_command_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.command_form.open = false;
        self.command_form.id = None;
        self.command_form.error = None;
        self.settings_focus.focus(window, cx);
        cx.notify();
    }
    fn browse_for(&mut self, folder: bool, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: !folder,
            directories: folder,
            multiple: false,
            prompt: Some(if folder { "Start in" } else { "Choose program" }.into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |this, window, cx| {
                    let value = path.to_string_lossy().into_owned();
                    let field = if folder {
                        this.command_form.cwd.clone()
                    } else {
                        this.command_form.program.clone()
                    };
                    field.update(cx, |input, cx| input.set_value(value, window, cx));
                    // A new command takes the program's name until one is typed.
                    if !folder
                        && this.command_form.name.read(cx).value().trim().is_empty()
                        && let Some(stem) = path.file_stem()
                    {
                        let stem = stem.to_string_lossy().into_owned();
                        this.command_form
                            .name
                            .update(cx, |input, cx| input.set_value(stem, window, cx));
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }
    pub(super) fn saved_commands_settings(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::tokens::*;
        let tint = p.muted;
        let card = || {
            div()
                .w_full()
                .flex()
                .flex_col()
                .rounded(RADIUS_CARD)
                .border_1()
                .border_color(p.text.opacity(0.1))
                .bg(p.settings_card)
        };
        // The command's mark: a soft tile in the page's colour.
        let mark = |size: f32| {
            div()
                .size(px(size))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(size * 0.28))
                .bg(p.text.opacity(0.06))
                .child(
                    Icon::empty()
                        .path("icons/terminal.svg")
                        .size(px(size * 0.55))
                        .text_color(tint),
                )
        };
        let icon_button = |id: ElementId, icon: &'static str, label: &'static str| {
            Button::new(id)
                .ghost()
                .h(BUTTON_HEIGHT)
                .w(BUTTON_HEIGHT)
                .p_0()
                .rounded(RADIUS_CONTROL)
                .accessibility_label(label)
                .tooltip(label)
                .child(Icon::empty().path(icon).size(px(14.)).text_color(p.muted))
        };
        let new_button = |id: &'static str, cx: &mut Context<Self>| {
            Button::new(id)
                .h(BUTTON_HEIGHT)
                .px_3()
                .rounded(RADIUS_CONTROL)
                .bg(p.text)
                .text_color(crate::appearance::modal_surface(p))
                .font_weight(FontWeight::MEDIUM)
                .text_size(TEXT_BODY)
                .accessibility_label("New command")
                .child(super::interface::button_text("New command"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.edit_saved_command(None, window, cx)
                }))
        };
        let form = &self.command_form;
        let commands = &self.preferences.saved_commands;
        let mut view = super::settings_view::section("Saved commands");

        if commands.is_empty() && !form.open {
            // First run: what a saved command is, and the one way to start.
            view = view.child(
                card()
                    .items_center()
                    .px_8()
                    .py_10()
                    .gap_3()
                    .child(mark(40.))
                    .child(
                        div()
                            .pt_1()
                            .text_size(TEXT_TITLE)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("No saved commands"),
                    )
                    .child(
                        div()
                            .max_w(px(360.))
                            .text_center()
                            .text_size(TEXT_BODY)
                            .line_height(px(20.))
                            .text_color(p.muted)
                            .child(
                                "Open the selected files in any program, from Jump or the right-click menu.",
                            ),
                    )
                    .child(div().pt_2().child(new_button("new-command-empty", cx))),
            );
        } else if !commands.is_empty() {
            let mut list = card();
            let count = commands.len();
            for (index, command) in commands.iter().enumerate() {
                let edit = command.clone();
                let id = command.id;
                let editing = form.open && form.id == Some(id);
                let program = command
                    .program
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| command.program.to_string_lossy().into_owned());
                let needs = match command.selection {
                    Selection::None => "No selection",
                    Selection::One => "One item",
                    Selection::Many => "Several items",
                };
                list = list.child(
                    div()
                        .id(("saved-command-row", id as usize))
                        .min_h(px(56.))
                        .px_4()
                        .py_2()
                        .flex()
                        .items_center()
                        .gap_3()
                        .when(index + 1 < count, |row| {
                            row.border_b_1().border_color(p.line)
                        })
                        .when(editing, |row| row.bg(p.text.opacity(0.04)))
                        .child(mark(28.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(TEXT_BODY)
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(command.name.clone()),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(TEXT_SMALL)
                                        .text_color(p.muted)
                                        .child(program),
                                ),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .px_2()
                                .text_size(TEXT_SMALL)
                                .text_color(p.muted)
                                .child(needs),
                        )
                        .child(
                            icon_button(
                                ("edit-command", id as usize).into(),
                                "icons/pencil.svg",
                                "Edit",
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.edit_saved_command(Some(edit.clone()), window, cx)
                                },
                            )),
                        )
                        .child(
                            icon_button(
                                ("remove-command", id as usize).into(),
                                "icons/trash.svg",
                                "Remove",
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.preferences
                                        .saved_commands
                                        .retain(|command| command.id != id);
                                    if this.command_form.id == Some(id) {
                                        this.close_command_form(window, cx);
                                    }
                                    this.save_preferences(window, cx);
                                },
                            )),
                        ),
                );
            }
            view = view.child(list);
            if !form.open {
                view = view.child(div().pt_4().child(new_button("new-command", cx)));
            }
        }
        if !form.open {
            return view.into_any_element();
        }

        // The editor: one card, a label column on the left, the field on the right.
        let row = |label: &'static str, content: AnyElement, last: bool| {
            div()
                .w_full()
                .px_5()
                .py_3()
                .flex()
                .items_start()
                .gap_4()
                .when(!last, |d| d.border_b_1().border_color(p.line))
                .child(
                    div()
                        .w(px(96.))
                        .flex_shrink_0()
                        .h(FIELD_HEIGHT)
                        .flex()
                        .items_center()
                        .text_size(TEXT_BODY)
                        .text_color(p.muted)
                        .child(label),
                )
                .child(div().flex_1().min_w_0().child(content))
        };
        let browse = |id: &'static str, folder: bool, cx: &mut Context<Self>| {
            super::settings_view::control(id, "Browse", p, cx)
                .h(FIELD_HEIGHT)
                .px_3()
                .flex_shrink_0()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.browse_for(folder, window, cx)
                }))
        };
        let with_browse = |input: AnyElement, button: Button| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().flex_1().min_w_0().child(input))
                .child(button)
                .into_any_element()
        };
        // What each variable becomes, in plain words. Clicking one adds it
        // to the arguments on a line of its own.
        let mut variables = div().pt_3().flex().flex_col().gap_1();
        for (index, (token, meaning)) in [
            ("{file}", "The selected item"),
            ("{files}", "Every selected item"),
            ("{name}", "The item's name"),
            ("{folder}", "The current folder"),
        ]
        .into_iter()
        .enumerate()
        {
            variables = variables.child(
                div()
                    .id(("command-variable", index))
                    .role(Role::Button)
                    .aria_label(format!("Add {token}: {meaning}"))
                    .h(px(26.))
                    .px_2()
                    .mx(px(-8.))
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded(RADIUS_ROW)
                    .cursor_pointer()
                    .hover(move |d| d.bg(p.hover))
                    .child(
                        div()
                            .w(px(64.))
                            .flex_shrink_0()
                            .font_family(FONT_MONO)
                            .text_size(TEXT_SMALL)
                            .text_color(p.text)
                            .child(token),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child(meaning),
                    )
                    .child(
                        Icon::empty()
                            .path("icons/add-01.svg")
                            .size(px(12.))
                            .text_color(p.muted.opacity(0.7)),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let args = this.command_form.args.clone();
                        let current = args.read(cx).value().to_string();
                        let next = if current.is_empty() || current.ends_with('\n') {
                            format!("{current}{token}")
                        } else {
                            format!("{current}\n{token}")
                        };
                        args.update(cx, |input, cx| {
                            input.set_value(next, window, cx);
                            input.focus(window, cx);
                        });
                    })),
            );
        }
        let selection_index = match form.selection {
            Selection::None => 0,
            Selection::One => 1,
            Selection::Many => 2,
        };
        let selection = super::settings_view::segmented(
            "command-selection",
            &["None", "One item", "Several"],
            selection_index,
            self.segment_motion.get("command-selection").copied(),
            self.motion_enabled,
            p,
            cx,
            move |this, index, _, cx| {
                this.slide_segment("command-selection", selection_index, index);
                this.command_form.selection =
                    [Selection::None, Selection::One, Selection::Many][index];
                cx.notify();
            },
        );
        let primary = Button::new("save-command")
            .h(BUTTON_HEIGHT)
            .px_3()
            .rounded(RADIUS_CONTROL)
            .bg(p.text)
            .text_color(crate::appearance::modal_surface(p))
            .font_weight(FontWeight::MEDIUM)
            .text_size(TEXT_BODY)
            .accessibility_label("Save command")
            .child(super::interface::button_text(if form.id.is_some() {
                "Save changes"
            } else {
                "Add command"
            }))
            .on_click(cx.listener(Self::save_command_click));
        let cancel = Button::new("cancel-command")
            .ghost()
            .h(BUTTON_HEIGHT)
            .px_3()
            .rounded(RADIUS_CONTROL)
            .text_size(TEXT_BODY)
            .accessibility_label("Cancel")
            .child(super::interface::button_text("Cancel"))
            .on_click(cx.listener(|this, _, window, cx| this.close_command_form(window, cx)));
        let program_browse = browse("browse-command-program", false, cx);
        let folder_browse = browse("browse-command-folder", true, cx);
        let editor = card()
            .mt_4()
            .child(
                div()
                    .px_5()
                    .pt_4()
                    .pb_3()
                    .border_b_1()
                    .border_color(p.line)
                    .text_size(TEXT_BODY)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if form.id.is_some() {
                        "Edit command"
                    } else {
                        "New command"
                    }),
            )
            .child(row(
                "Name",
                Input::new(&form.name)
                    .aria_label("Command name")
                    .h(FIELD_HEIGHT)
                    .into_any_element(),
                false,
            ))
            .child(row(
                "Program",
                with_browse(
                    Input::new(&form.program)
                        .aria_label("Program path")
                        .h(FIELD_HEIGHT)
                        .into_any_element(),
                    program_browse,
                ),
                false,
            ))
            .child(row(
                "Arguments",
                div()
                    .flex()
                    .flex_col()
                    .child(
                        Textarea::new(&form.args)
                            .aria_label("Arguments, one per line")
                            .h(px(64.)),
                    )
                    .child(variables)
                    .into_any_element(),
                false,
            ))
            .child(row(
                "Start in",
                with_browse(
                    Input::new(&form.cwd)
                        .aria_label("Start in folder")
                        .h(FIELD_HEIGHT)
                        .into_any_element(),
                    folder_browse,
                ),
                false,
            ))
            .child(row("Works on", selection, true))
            .child(
                div()
                    .px_5()
                    .py_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_t_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .id("saved-command-error")
                            .role(Role::Status)
                            .flex_1()
                            .min_w_0()
                            .text_size(TEXT_SMALL)
                            .text_color(p.tints.pink)
                            .when_some(form.error.clone(), |d, error| d.child(error)),
                    )
                    .child(cancel)
                    .child(primary),
            );
        view.child(editor).into_any_element()
    }
    fn save_command_click(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.save_command_form(window, cx);
    }
}
