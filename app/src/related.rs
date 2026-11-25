//! Related folders under tabs (3.6): a thin row of folder chips linked to a
//! tab. Off by default; shown from the tab's context menu. Chips are added
//! from the "+" menu, by dropping a folder on the row, or by dropping another
//! tab (which becomes a chip). Clicking a chip opens it in the same tab with
//! normal back/forward history. Folders only; chips persist per tab.
use super::drag_split::PlaceDrag;
use super::panes::TabDrag;
use super::*;
use gpui_component::menu::{ContextMenuExt, DropdownMenu, PopupMenuItem};
use gpui_component_assets::IconName as Glyph;

pub(super) const MAX_RELATED: usize = 16;
pub(super) const MAX_LABEL: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Related {
    pub(super) path: PathBuf,
    pub(super) label: Option<String>,
}
impl Related {
    pub(super) fn label(&self) -> String {
        self.label
            .clone()
            .unwrap_or_else(|| super::tabs::folder_name(&self.path))
    }
}

/// Add `path` unless already present (case-insensitively); bounded.
pub(super) fn add(chips: &mut Vec<Related>, path: PathBuf) -> Result<(), String> {
    let key = |p: &std::path::Path| p.to_string_lossy().trim_end_matches('\\').to_lowercase();
    if chips.iter().any(|chip| key(&chip.path) == key(&path)) {
        return Ok(());
    }
    if chips.len() >= MAX_RELATED {
        return Err(format!("A tab keeps up to {MAX_RELATED} related folders."));
    }
    chips.push(Related { path, label: None });
    Ok(())
}

pub(super) struct ChipEdit {
    index: usize,
    input: Entity<InputState>,
    _subscription: Subscription,
}

impl Seek {
    pub(super) fn toggle_related(&mut self, cx: &mut Context<Self>) {
        self.related_visible = !self.related_visible;
        self.remember_session(cx);
        cx.notify();
    }

    fn add_related(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !std::fs::metadata(&path).is_ok_and(|m| m.is_dir()) {
            self.message = Some("Related items are folders.".into());
            cx.notify();
            return;
        }
        if let Err(error) = add(&mut self.related, path) {
            self.message = Some(error);
        }
        self.related_visible = true;
        self.remember_session(cx);
        cx.notify();
    }

    fn pick_related(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Add related folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                let _ = this.update(cx, |this, cx| {
                    for path in paths {
                        this.add_related(path, cx);
                    }
                });
            }
        })
        .detach();
    }

    /// Dropping another tab on the row turns it into a chip.
    fn drop_tab_as_related(&mut self, drag: &TabDrag, window: &mut Window, cx: &mut Context<Self>) {
        let folder = if drag.pane == self.pane_identity {
            if self
                .tabs
                .get(self.active_tab)
                .is_some_and(|tab| tab.id == drag.tab)
            {
                self.message = Some("Drag a different tab to link it here.".into());
                cx.notify();
                return;
            }
            self.tabs
                .iter()
                .find(|tab| tab.id == drag.tab)
                .and_then(|tab| tab.scope.clone())
        } else {
            self.panes
                .iter()
                .find(|slot| slot.id == drag.pane)
                .and_then(|slot| slot.tabs.iter().find(|tab| tab.id == drag.tab))
                .and_then(|tab| tab.scope.clone())
        };
        let Some(folder) = folder else {
            self.message = Some("Only folder tabs can become related folders.".into());
            cx.notify();
            return;
        };
        self.add_related(folder, cx);
        // The dragged tab is replaced by the chip.
        if drag.pane == self.pane_identity {
            if let Some(index) = self.tabs.iter().position(|tab| tab.id == drag.tab) {
                self.close_tab(index, window, cx);
            }
        } else if let Some(index) = self.panes.iter().position(|slot| slot.id == drag.pane) {
            self.take_from_slot(index, drag.tab, cx);
        }
        self.remember_session(cx);
    }

    fn rename_chip(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chip) = self.related.get(index) else {
            return;
        };
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .context_menu(false)
                .default_value(chip.label())
        });
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        let subscription =
            cx.subscribe_in(&input, window, move |this, input, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur)
                    && this
                        .related_edit
                        .as_ref()
                        .is_some_and(|edit| edit.index == index)
                {
                    let label: String = input
                        .read(cx)
                        .value()
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(MAX_LABEL)
                        .collect();
                    this.related_edit = None;
                    if let Some(chip) = this.related.get_mut(index) {
                        let label = label.trim().to_owned();
                        chip.label = (!label.is_empty()).then_some(label);
                    }
                    this.remember_session(cx);
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.table.read(cx).focus_handle(cx).focus(window, cx);
                    }
                    cx.notify();
                }
            });
        self.related_edit = Some(ChipEdit {
            index,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(super) fn cancel_chip_edit(&mut self, cx: &mut Context<Self>) -> bool {
        if self.related_edit.take().is_some() {
            cx.notify();
            return true;
        }
        false
    }

    /// The related-folders row for the focused pane's tab, if shown.
    pub(super) fn related_row(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.related_visible {
            return None;
        }
        let mut row = div()
            .id("related-folders")
            .role(Role::Group)
            .aria_label("Related folders")
            .h(px(36.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .border_b_1()
            .border_color(p.line)
            .bg(p.chrome)
            .overflow_x_scroll()
            .drag_over::<super::daily::FilesDrag>(move |style, _, _, _| {
                style.bg(p.accent.opacity(0.08))
            })
            .drag_over::<PlaceDrag>(move |style, _, _, _| style.bg(p.accent.opacity(0.08)))
            .drag_over::<TabDrag>(move |style, _, _, _| style.bg(p.accent.opacity(0.08)))
            .on_drop(cx.listener(|this, drag: &super::daily::FilesDrag, _, cx| {
                for path in drag.paths.iter() {
                    if std::fs::metadata(path).is_ok_and(|m| m.is_dir()) {
                        this.add_related(path.clone(), cx);
                    }
                }
            }))
            .on_drop(cx.listener(|this, drag: &PlaceDrag, _, cx| {
                if let Some(path) = drag.path.clone() {
                    this.add_related(path, cx);
                }
            }))
            .on_drop(cx.listener(|this, drag: &TabDrag, window, cx| {
                this.tab_drop = None;
                this.drop_tab_as_related(drag, window, cx)
            }));
        if self.related.is_empty() {
            row = row.child(
                div()
                    .px_1()
                    .text_size(px(12.))
                    .text_color(p.muted)
                    .child("Drop folders or tabs here to link them to this tab"),
            );
        }
        for (index, chip) in self.related.iter().enumerate() {
            let selected = self.scope.as_ref() == Some(&chip.path);
            let label = chip.label();
            let open = chip.path.clone();
            let owner = cx.entity().downgrade();
            let content: AnyElement = match self
                .related_edit
                .as_ref()
                .filter(|edit| edit.index == index)
            {
                Some(edit) => div()
                    .w(px(160.))
                    .child(Input::new(&edit.input).small())
                    .into_any_element(),
                None => div()
                    .max_w(px(180.))
                    .truncate()
                    .child(label.clone())
                    .into_any_element(),
            };
            row = row.child(
                div()
                    .id(("related-chip", index))
                    .role(Role::Button)
                    .aria_label(format!("Open related folder {label}"))
                    // Same plate language as the tabs above: 6 px radius,
                    // 4% rest, 7% hover, 10% for the open folder.
                    .h(px(26.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(8.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(if selected { p.text } else { p.muted })
                    .bg(p.text.opacity(if selected { 0.10 } else { 0.04 }))
                    .hover(move |d| {
                        d.bg(p.text.opacity(if selected { 0.10 } else { 0.07 }))
                            .text_color(p.text)
                    })
                    .tooltip({
                        let path =
                            seek_engine::paths::native(&seek_engine::paths::internal(&chip.path))
                                .to_string_lossy()
                                .into_owned();
                        move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(path.clone()).build(window, cx)
                        }
                    })
                    .child(Icon::new(Glyph::Folder).size(px(13.)))
                    .child(content)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.related_edit.is_none() {
                            this.scope_to(Some(open.clone()), window, cx);
                        }
                    }))
                    .context_menu(move |menu, window, _| {
                        let (rename, remove) = (owner.clone(), owner.clone());
                        super::daily::menu_style(menu, window)
                            .item(PopupMenuItem::new("Rename label").on_click(
                                move |_, window, cx| {
                                    let _ = rename
                                        .update(cx, |this, cx| this.rename_chip(index, window, cx));
                                },
                            ))
                            .item(PopupMenuItem::new("Remove").on_click(move |_, _, cx| {
                                let _ = remove.update(cx, |this, cx| {
                                    if index < this.related.len() {
                                        this.related.remove(index);
                                        this.related_edit = None;
                                        this.remember_session(cx);
                                        cx.notify();
                                    }
                                });
                            }))
                    }),
            );
        }
        let owner = cx.entity().downgrade();
        let current = self.scope.clone();
        row = row.child(
            Button::new("related-add")
                .ghost()
                .small()
                .accessibility_label("Related folder actions")
                .flex_shrink_0()
                .rounded(px(6.))
                .icon(gpui_component::IconName::Plus)
                .dropdown_menu(move |menu, window, _| {
                    let (pick, here, hide) = (owner.clone(), owner.clone(), owner.clone());
                    let mut menu = super::daily::menu_style(menu, window).item(
                        PopupMenuItem::new("Add related folder…").on_click(move |_, window, cx| {
                            let _ = pick.update(cx, |this, cx| this.pick_related(window, cx));
                        }),
                    );
                    if let Some(folder) = current.clone() {
                        menu = menu.item(PopupMenuItem::new("Add current folder").on_click(
                            move |_, _, cx| {
                                let folder = folder.clone();
                                let _ = here.update(cx, |this, cx| this.add_related(folder, cx));
                            },
                        ));
                    }
                    menu.separator()
                        .item(PopupMenuItem::new("Hide related folders").on_click(
                            move |_, _, cx| {
                                let _ = hide.update(cx, |this, cx| this.toggle_related(cx));
                            },
                        ))
                }),
        );
        Some(row.into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_RELATED, Related, add};
    use std::path::PathBuf;
    #[test]
    fn chips_are_unique_bounded_and_labelled() {
        let mut chips = vec![];
        add(&mut chips, PathBuf::from(r"C:\Work\Docs")).unwrap();
        add(&mut chips, PathBuf::from(r"c:\work\docs\")).unwrap();
        assert_eq!(chips.len(), 1);
        assert_eq!(chips[0].label(), "Docs");
        let renamed = Related {
            path: chips[0].path.clone(),
            label: Some("Specs".into()),
        };
        assert_eq!(renamed.label(), "Specs");
        for i in 0..MAX_RELATED * 2 {
            let _ = add(&mut chips, PathBuf::from(format!(r"D:\f{i}")));
        }
        assert_eq!(chips.len(), MAX_RELATED);
        assert!(add(&mut chips, PathBuf::from(r"E:\x")).is_err());
    }
}
