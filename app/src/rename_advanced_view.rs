//! Advanced rename (8.2), the fourth mode of the batch rename sheet: a stack
//! of steps reordered by dragging, a live Before and After list, options,
//! variables and saved presets (after Tilde's advanced rename).
use super::*;
use crate::rename_steps::{Options, Preset, Step, VARIABLES};
use gpui_component::{
    checkbox::Checkbox,
    menu::{DropdownMenu, PopupMenuItem},
};

pub(super) struct StepRow {
    pub(super) step: Step,
    /// The action's text fields, in the order of `field_labels`.
    fields: Vec<Entity<InputState>>,
    id: u64,
    _subscriptions: Vec<Subscription>,
}

pub(super) struct Advanced {
    pub(super) rows: Vec<StepRow>,
    pub(super) options: Options,
    index_start: Entity<InputState>,
    index_digits: Entity<InputState>,
    preset_name: Entity<InputState>,
    saving: bool,
    next_id: u64,
    /// Where a picked variable goes.
    last_field: Option<Entity<InputState>>,
    _subscriptions: Vec<Subscription>,
}

/// Saved presets are bounded like the other lists in settings.
const PRESET_LIMIT: usize = 32;
const STEP_LIMIT: usize = 16;

#[derive(Clone)]
pub(super) struct StepDrag {
    index: usize,
    label: SharedString,
}
impl Render for StepDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_component::ActiveTheme;
        div()
            .h(px(28.))
            .px_3()
            .flex()
            .items_center()
            .rounded(crate::tokens::RADIUS_CONTROL)
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .text_size(crate::tokens::TEXT_SMALL)
            .shadow_md()
            .child(self.label.clone())
    }
}

fn field_labels(step: &Step) -> &'static [&'static str] {
    match step {
        Step::Replace { .. } => &["Find", "Replace with"],
        Step::Append { .. } => &["Text"],
        Step::Overwrite { .. } => &["Text", "From character"],
        Step::Remove { .. } => &["From character", "How many"],
        Step::RemoveLast { .. } => &["How many"],
        Step::NewName { .. } => &["Name"],
        Step::RemoveText { .. } => &["Text"],
        _ => &[],
    }
}

/// Short examples for the empty fields (the labels say what they are).
fn field_examples(step: &Step) -> &'static [&'static str] {
    match step {
        Step::Replace { .. } => &["IMG_", "Photo "],
        Step::Append { .. } => &[" copy"],
        Step::Overwrite { .. } => &["2026", "1"],
        Step::Remove { .. } => &["1", "1"],
        Step::RemoveLast { .. } => &["1"],
        Step::NewName { .. } => &["Photo $INDEX"],
        Step::RemoveText { .. } => &[" (1)"],
        _ => &[],
    }
}

/// The fields' starting text for a step. Positions show 1-based.
fn field_values(step: &Step) -> Vec<String> {
    match step {
        Step::Replace { find, with, .. } => vec![find.clone(), with.clone()],
        Step::Append { text, .. } | Step::RemoveText { text } => vec![text.clone()],
        Step::Overwrite { text, at } => vec![text.clone(), (at + 1).to_string()],
        Step::Remove { at, count } => vec![(at + 1).to_string(), count.to_string()],
        Step::RemoveLast { count } => vec![count.to_string()],
        Step::NewName { template } => vec![template.clone()],
        _ => vec![],
    }
}

/// Reads the fields back into the step, keeping its switches.
fn read_fields(step: &Step, values: &[String]) -> Step {
    let text = |i: usize| values.get(i).cloned().unwrap_or_default();
    let number = |i: usize, default: usize| {
        values
            .get(i)
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(default)
    };
    let position = |i: usize| number(i, 1).max(1) - 1;
    match step {
        Step::Replace { regex, .. } => Step::Replace {
            find: text(0),
            with: text(1),
            regex: *regex,
        },
        Step::Append { at_start, .. } => Step::Append {
            text: text(0),
            at_start: *at_start,
        },
        Step::Overwrite { .. } => Step::Overwrite {
            text: text(0),
            at: position(1),
        },
        Step::Remove { .. } => Step::Remove {
            at: position(0),
            count: number(1, 1),
        },
        Step::RemoveLast { .. } => Step::RemoveLast {
            count: number(0, 1),
        },
        Step::NewName { .. } => Step::NewName { template: text(0) },
        Step::RemoveText { .. } => Step::RemoveText { text: text(0) },
        other => other.clone(),
    }
}

impl Seek {
    fn advanced_input(
        &mut self,
        value: String,
        placeholder: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<InputState>, Vec<Subscription>) {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .context_menu(false)
                .placeholder(placeholder)
                .default_value(value)
        });
        let subscriptions = vec![cx.subscribe(&input, |this, input, event, cx| match event {
            InputEvent::Change => this.advanced_changed(cx),
            InputEvent::Focus => {
                if let Some(advanced) = this.advanced_mut() {
                    advanced.last_field = Some(input);
                }
            }
            _ => {}
        })];
        (input, subscriptions)
    }

    fn step_row(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) -> StepRow {
        let mut fields = vec![];
        let mut subscriptions = vec![];
        let placeholders = field_examples(&step);
        for (value, placeholder) in field_values(&step).into_iter().zip(placeholders) {
            let (input, subs) = self.advanced_input(value, placeholder, window, cx);
            fields.push(input);
            subscriptions.extend(subs);
        }
        let id = self.advanced_mut().map_or(0, |a| {
            a.next_id += 1;
            a.next_id
        });
        StepRow {
            step,
            fields,
            id,
            _subscriptions: subscriptions,
        }
    }

    pub(super) fn advanced_mut(&mut self) -> Option<&mut Advanced> {
        self.edit
            .as_mut()
            .and_then(|edit| edit.batch.as_mut())
            .and_then(|batch| batch.advanced.as_mut())
    }

    /// Switch the batch sheet to Advanced, starting with one Replace step.
    pub(super) fn batch_advanced(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .edit
            .as_ref()
            .and_then(|edit| edit.batch.as_ref())
            .is_none_or(|batch| batch.advanced.is_some())
        {
            return;
        }
        let options = Options::default();
        let (index_start, mut subscriptions) =
            self.advanced_input(options.index_start.to_string(), "1", window, cx);
        let (index_digits, subs) =
            self.advanced_input(options.index_digits.to_string(), "1", window, cx);
        subscriptions.extend(subs);
        let preset_name = cx.new(|cx| {
            InputState::new(window, cx)
                .context_menu(false)
                .placeholder("Preset name")
        });
        if let Some(batch) = self.edit.as_mut().and_then(|edit| edit.batch.as_mut()) {
            batch.suggested = None;
            batch.numbered = false;
            batch.advanced = Some(Advanced {
                rows: vec![],
                options,
                index_start,
                index_digits,
                preset_name,
                saving: false,
                next_id: 0,
                last_field: None,
                _subscriptions: subscriptions,
            });
        }
        let first = self.step_row(Step::of_kind(0), window, cx);
        if let Some(advanced) = self.advanced_mut() {
            advanced.last_field = first.fields.first().cloned();
            advanced.rows.push(first);
        }
        self.advanced_changed(cx);
    }

    /// Re-read every field into its step, then refresh the preview.
    fn advanced_changed(&mut self, cx: &mut Context<Self>) {
        let Some(advanced) = self.advanced_mut() else {
            return;
        };
        let mut values: Vec<Vec<String>> = vec![];
        for row in &advanced.rows {
            values.push(
                row.fields
                    .iter()
                    .map(|field| field.read(cx).value().to_string())
                    .collect(),
            );
        }
        let start = advanced.index_start.read(cx).value().trim().parse::<u64>();
        let digits = advanced.index_digits.read(cx).value().trim().parse::<u8>();
        let advanced = self.advanced_mut().unwrap();
        for (row, values) in advanced.rows.iter_mut().zip(values) {
            row.step = read_fields(&row.step, &values);
        }
        advanced.options.index_start = start.unwrap_or(1);
        advanced.options.index_digits = digits.unwrap_or(1).clamp(1, 9);
        self.refresh_batch_preview(cx);
    }

    fn advanced_edit(
        &mut self,
        change: impl FnOnce(&mut Advanced),
        cx: &mut Context<Self>,
    ) {
        if let Some(advanced) = self.advanced_mut() {
            change(advanced);
        }
        self.advanced_changed(cx);
    }

    fn add_step(&mut self, kind: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.advanced_mut().is_none_or(|a| a.rows.len() >= STEP_LIMIT) {
            return;
        }
        let row = self.step_row(Step::of_kind(kind), window, cx);
        let focus = row.fields.first().cloned();
        self.advanced_edit(|a| a.rows.push(row), cx);
        if let Some(focus) = focus {
            focus.update(cx, |input, cx| input.focus(window, cx));
        }
    }

    fn change_step(&mut self, index: usize, kind: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .advanced_mut()
            .is_none_or(|a| a.rows.get(index).is_none_or(|row| row.step.kind() == kind))
        {
            return;
        }
        let row = self.step_row(Step::of_kind(kind), window, cx);
        self.advanced_edit(|a| a.rows[index] = row, cx);
    }

    fn insert_variable(&mut self, token: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.advanced_mut().and_then(|a| a.last_field.clone()) else {
            return;
        };
        field.update(cx, |input, cx| {
            input.insert(token, window, cx);
            input.focus(window, cx);
        });
    }

    fn load_preset(&mut self, preset: Preset, window: &mut Window, cx: &mut Context<Self>) {
        let mut rows = vec![];
        for step in preset.steps.iter().take(STEP_LIMIT) {
            rows.push(self.step_row(step.clone(), window, cx));
        }
        let options = preset.options.clone();
        if let Some(advanced) = self.advanced_mut() {
            advanced.rows = rows;
            advanced.options = options.clone();
            advanced.last_field = advanced.rows.iter().find_map(|r| r.fields.first().cloned());
            let (start, digits) = (advanced.index_start.clone(), advanced.index_digits.clone());
            start.update(cx, |input, cx| {
                input.set_value(options.index_start.to_string(), window, cx)
            });
            digits.update(cx, |input, cx| {
                input.set_value(options.index_digits.to_string(), window, cx)
            });
        }
        self.advanced_changed(cx);
    }

    fn save_preset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(advanced) = self.advanced_mut() else {
            return;
        };
        let name = advanced.preset_name.read(cx).value().trim().to_string();
        if name.is_empty() || name.chars().count() > 60 {
            if let Some(edit) = &mut self.edit {
                edit.error = Some("Name the preset (up to 60 characters).".into());
            }
            cx.notify();
            return;
        }
        let preset = Preset {
            name: name.clone(),
            steps: advanced.rows.iter().map(|row| row.step.clone()).collect(),
            options: advanced.options.clone(),
        };
        advanced.saving = false;
        let presets = &mut self.preferences.rename_presets;
        if let Some(existing) = presets.iter_mut().find(|p| p.name.eq_ignore_ascii_case(&name)) {
            *existing = preset;
        } else if presets.len() < PRESET_LIMIT {
            presets.push(preset);
        } else {
            if let Some(edit) = &mut self.edit {
                edit.error = Some(format!(
                    "Remove a preset before saving more than {PRESET_LIMIT}."
                ));
            }
            cx.notify();
            return;
        }
        if let Some(edit) = &mut self.edit {
            edit.error = None;
        }
        self.save_preferences(window, cx);
        cx.notify();
    }

    pub(super) fn advanced_fields(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::tokens::*;
        let Some(edit) = &self.edit else {
            return div().into_any_element();
        };
        let Some(batch) = &edit.batch else {
            return div().into_any_element();
        };
        let Some(advanced) = &batch.advanced else {
            return div().into_any_element();
        };
        let danger = Hsla::from(rgb(if p.text.l > 0.5 { 0xf87171 } else { 0xc81e1e }));
        let small_label = |text: &'static str| {
            div()
                .text_size(TEXT_SMALL)
                .text_color(p.muted)
                .child(text)
        };
        // The steps, top to bottom.
        let mut steps = div().flex().flex_col().gap_2();
        for (index, row) in advanced.rows.iter().enumerate() {
            let kind = row.step.kind();
            let owner = cx.entity().downgrade();
            let picker = super::settings_view::choice_control(
                ("step-action", row.id as usize),
                Step::KINDS[kind],
                p,
                cx,
            )
            .h(px(28.))
            .w(px(168.))
            .dropdown_menu(move |menu, window, _| {
                let mut menu = super::daily::menu_style(menu, window);
                for (choice, label) in Step::KINDS.iter().enumerate() {
                    let owner = owner.clone();
                    menu = menu.item(PopupMenuItem::new(*label).checked(choice == kind).on_click(
                        move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                this.change_step(index, choice, window, cx)
                            });
                        },
                    ));
                }
                menu
            });
            let mut fields = div().flex().flex_col().gap_2();
            let labels = field_labels(&row.step);
            let pair = labels.len() == 2 && !matches!(row.step, Step::Replace { .. });
            if pair {
                // Two short fields side by side (positions and counts).
                let mut line = div().flex().gap_2();
                for (field, label) in row.fields.iter().zip(labels) {
                    line = line.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(small_label(label))
                            .child(Input::new(field).h(FIELD_HEIGHT)),
                    );
                }
                fields = fields.child(line);
            } else {
                for (field, label) in row.fields.iter().zip(labels) {
                    fields = fields.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(small_label(label))
                            .child(Input::new(field).h(FIELD_HEIGHT)),
                    );
                }
            }
            match row.step {
                Step::Replace { regex, .. } => {
                    fields = fields.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Checkbox::new(("step-regex", row.id as usize))
                                    .checked(regex)
                                    .accessibility_label("Regular expression")
                                    .on_change(cx.listener(move |this, on: &bool, _, cx| {
                                        let on = *on;
                                        this.advanced_edit(
                                            |a| {
                                                if let Some(Step::Replace { regex, .. }) =
                                                    a.rows.get_mut(index).map(|r| &mut r.step)
                                                {
                                                    *regex = on;
                                                }
                                            },
                                            cx,
                                        )
                                    })),
                            )
                            .child(small_label("Regular expression")),
                    )
                }
                Step::Append { at_start, .. } => {
                    fields = fields.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Checkbox::new(("step-start", row.id as usize))
                                    .checked(at_start)
                                    .accessibility_label("At the start")
                                    .on_change(cx.listener(move |this, on: &bool, _, cx| {
                                        let on = *on;
                                        this.advanced_edit(
                                            |a| {
                                                if let Some(Step::Append { at_start, .. }) =
                                                    a.rows.get_mut(index).map(|r| &mut r.step)
                                                {
                                                    *at_start = on;
                                                }
                                            },
                                            cx,
                                        )
                                    })),
                            )
                            .child(small_label("At the start")),
                    )
                }
                _ => {}
            }
            let drag = StepDrag {
                index,
                label: SharedString::from(Step::KINDS[kind]),
            };
            steps = steps.child(
                div()
                    .id(("rename-step", row.id as usize))
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .rounded(RADIUS_CARD)
                    .border_1()
                    .border_color(p.line)
                    .bg(p.text.opacity(0.025))
                    .on_drag(drag, |drag, offset, _, cx| {
                        super::drag_preview::near_cursor(drag.clone(), offset, cx)
                    })
                    .drag_over::<StepDrag>(move |style, _, _, _| {
                        style.border_color(p.accent).bg(p.accent.opacity(0.08))
                    })
                    .on_drop(cx.listener(move |this, drag: &StepDrag, _, cx| {
                        let from = drag.index;
                        this.advanced_edit(
                            |a| {
                                if from < a.rows.len() && index < a.rows.len() && from != index {
                                    let row = a.rows.remove(from);
                                    a.rows.insert(index, row);
                                }
                            },
                            cx,
                        );
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .cursor_grab()
                                    .text_color(p.muted)
                                    .child(Icon::new(gpui_component_assets::IconName::GripVertical).size(px(14.))),
                            )
                            .child(
                                div()
                                    .w(px(18.))
                                    .text_size(TEXT_SMALL)
                                    .font_family(FONT_MONO)
                                    .text_color(p.muted)
                                    .child(format!("{}", index + 1)),
                            )
                            .child(picker)
                            .child(div().flex_1())
                            .child(
                                Button::new(("remove-step", row.id as usize))
                                    .ghost()
                                    .h(px(24.))
                                    .w(px(24.))
                                    .p_0()
                                    .rounded(RADIUS_CONTROL)
                                    .accessibility_label("Remove step")
                                    .tooltip("Remove step")
                                    .disabled(advanced.rows.len() == 1)
                                    .child(
                                        Icon::empty()
                                            .path("icons/close.svg")
                                            .size(px(12.))
                                            .text_color(p.muted),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.advanced_edit(
                                            |a| {
                                                if a.rows.len() > 1 {
                                                    a.rows.remove(index);
                                                }
                                            },
                                            cx,
                                        )
                                    })),
                            ),
                    )
                    .when(!row.fields.is_empty() || matches!(row.step, Step::Replace { .. } | Step::Append { .. }), |d| {
                        d.child(fields)
                    }),
            );
        }
        let add_owner = cx.entity().downgrade();
        let add = super::settings_view::control("add-step", "", p, cx)
            .h(px(28.))
            .disabled(advanced.rows.len() >= STEP_LIMIT)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(TEXT_BODY)
                    .child(Icon::empty().path("icons/plus.svg").size(px(13.)))
                    .child("Add step"),
            )
            .dropdown_menu(move |menu, window, _| {
                let mut menu = super::daily::menu_style(menu, window);
                for (kind, label) in Step::KINDS.iter().enumerate() {
                    let owner = add_owner.clone();
                    menu = menu.item(PopupMenuItem::new(*label).on_click(move |_, window, cx| {
                        let _ = owner.update(cx, |this, cx| this.add_step(kind, window, cx));
                    }));
                }
                menu
            });
        // Variables: the picker adds to the field last typed in.
        let var_owner = cx.entity().downgrade();
        let variables = super::settings_view::control("rename-variables", "", p, cx)
            .h(px(28.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(TEXT_BODY)
                    .child(div().font_family(FONT_MONO).child("$"))
                    .child("Variables"),
            )
            .dropdown_menu(move |menu, window, _| {
                let mut menu = super::daily::menu_style(menu, window).min_w(px(300.));
                for (token, meaning) in VARIABLES {
                    let owner = var_owner.clone();
                    menu = menu.item(
                        PopupMenuItem::new(format!("{token}   {meaning}")).on_click(
                            move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.insert_variable(token, window, cx)
                                });
                            },
                        ),
                    );
                }
                menu
            });
        let option = |id: &'static str, label: &'static str, on: bool, flip: fn(&mut Options)| {
            div()
                .h(px(28.))
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Checkbox::new(id)
                        .checked(on)
                        .accessibility_label(label)
                        .on_change(cx.listener(move |this, _: &bool, _, cx| {
                            this.advanced_edit(|a| flip(&mut a.options), cx)
                        })),
                )
                .child(div().text_size(TEXT_BODY).child(label))
        };
        let o = &advanced.options;
        let options = div()
            .flex()
            .flex_col()
            .child(option("rename-case", "Case sensitive", o.case_sensitive, |o| {
                o.case_sensitive = !o.case_sensitive
            }))
            .child(option(
                "rename-extension",
                "Include extension",
                o.include_extension,
                |o| o.include_extension = !o.include_extension,
            ))
            .child(option(
                "rename-increment",
                "Auto increment on conflict",
                o.auto_increment,
                |o| o.auto_increment = !o.auto_increment,
            ))
            .child(option(
                "rename-tail",
                "Count $INDEX from the last item",
                o.index_from_tail,
                |o| o.index_from_tail = !o.index_from_tail,
            ))
            .child(
                div()
                    .pt_1()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(small_label("$INDEX starts at"))
                            .child(Input::new(&advanced.index_start).h(FIELD_HEIGHT)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(small_label("Digits"))
                            .child(Input::new(&advanced.index_digits).h(FIELD_HEIGHT)),
                    ),
            );
        // Presets.
        let presets = self.preferences.rename_presets.clone();
        let load_owner = cx.entity().downgrade();
        let load = super::settings_view::choice_control("load-preset", "Load preset", p, cx)
            .h(px(28.))
            .w(px(140.))
            .disabled(presets.is_empty())
            .dropdown_menu(move |menu, window, _| {
                let mut menu = super::daily::menu_style(menu, window);
                for preset in &presets {
                    let owner = load_owner.clone();
                    let chosen = preset.clone();
                    menu = menu.item(PopupMenuItem::new(preset.name.clone()).on_click(
                        move |_, window, cx| {
                            let chosen = chosen.clone();
                            let _ = owner.update(cx, |this, cx| this.load_preset(chosen, window, cx));
                        },
                    ));
                }
                menu
            });
        let save_row = if advanced.saving {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().flex_1().child(Input::new(&advanced.preset_name).h(px(28.))))
                .child(
                    super::settings_view::control("confirm-preset", "Save", p, cx)
                        .h(px(28.))
                        .on_click(cx.listener(|this, _, window, cx| this.save_preset(window, cx))),
                )
                .into_any_element()
        } else {
            super::settings_view::control("save-preset", "Save preset", p, cx)
                .h(px(28.))
                .on_click(cx.listener(|this, _, window, cx| {
                    let name = this.advanced_mut().map(|a| a.preset_name.clone());
                    if let Some(a) = this.advanced_mut() {
                        a.saving = true;
                    }
                    if let Some(name) = name {
                        name.update(cx, |input, cx| input.focus(window, cx));
                    }
                    cx.notify();
                }))
                .into_any_element()
        };
        let left = div()
            .w(px(372.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .id("rename-steps")
                    .max_h(px(250.))
                    .overflow_y_scroll()
                    .child(steps),
            )
            .child(div().flex().gap_2().child(add).child(variables))
            .child(div().h(px(1.)).bg(p.line))
            .child(options)
            .child(div().h(px(1.)).bg(p.line))
            .child(div().flex().items_center().gap_2().child(load).child(save_row));

        // Before and after, with conflicts in red before anything is applied.
        let mut list = div()
            .id("advanced-rename-preview")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        for (index, row) in batch.rows.iter().enumerate() {
            let before = row
                .source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let error = row.error.clone();
            list = list.child(
                div()
                    .id(("advanced-rename-row", index))
                    .py(px(6.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .truncate()
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child(before),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(TEXT_BODY)
                            .text_color(if error.is_some() {
                                danger
                            } else if row.changed {
                                p.text
                            } else {
                                p.muted
                            })
                            .child(row.name.clone()),
                    )
                    .when_some(error, |d, error| {
                        d.child(div().text_size(TEXT_SMALL).text_color(danger).child(error))
                    }),
            );
        }
        let applicable = batch
            .rows
            .iter()
            .filter(|row| row.changed && row.error.is_none())
            .count();
        let conflicts = batch.rows.iter().filter(|row| row.error.is_some()).count();
        let right = div()
            .flex_1()
            .min_w_0()
            .h(px(452.))
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded(RADIUS_CARD)
            .border_1()
            .border_color(p.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(TEXT_SMALL)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.muted)
                            .child("Before and after"),
                    )
                    .child(
                        div()
                            .text_size(TEXT_SMALL)
                            .text_color(if conflicts > 0 { danger } else { p.muted })
                            .child(if batch.loading {
                                "Checking…".to_string()
                            } else {
                                let changes = if applicable == 1 { "1 change".to_string() } else { format!("{applicable} changes") };
                                match conflicts {
                                    0 => changes,
                                    1 => format!("{changes} · 1 conflict"),
                                    n => format!("{changes} · {n} conflicts"),
                                }
                            }),
                    ),
            )
            .child(list);
        div()
            .flex()
            .gap_4()
            .items_start()
            .child(left)
            .child(right)
            .into_any_element()
    }
}
