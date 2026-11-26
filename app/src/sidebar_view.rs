//! Sidebar Favorites and user groups (3.4). Rows keep the existing favorite
//! behaviour (click opens, files dropped on a folder move into it, drag onto a
//! pane splits). Groups add a chevron header that collapses on the motion
//! spring, inline naming, and drag-to-arrange that highlights the target row.
use super::drag_split::PlaceDrag;
use super::*;
use crate::sidebar_groups::{Item, Layout, Place};
use gpui_component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_component_assets::IconName as Glyph;

const ROW: f32 = 30.;

#[derive(Clone)]
pub(super) struct GroupDrag {
    id: u64,
    name: String,
}
impl Render for GroupDrag {
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
            .child(Icon::new(Glyph::Folder).size(px(13.)))
            .child(self.name.clone())
    }
}

pub(super) struct GroupEdit {
    pub(super) id: u64,
    pub(super) input: Entity<InputState>,
    _subscription: Subscription,
}

#[derive(Default)]
pub(super) struct State {
    /// Insertion slot for an item being dragged within the sidebar.
    pub(super) drop: Option<(Place, usize)>,
    /// Insertion slot for a group being dragged.
    pub(super) group_drop: Option<usize>,
    /// The row or group under the pointer while dragging; it is highlighted
    /// whole instead of drawing an insertion line.
    pub(super) drop_row: Option<(Place, usize)>,
    pub(super) group_hover: Option<usize>,
    /// The sidebar folder that files dragged from the list would move into:
    /// the middle of a folder row. Its edges insert the folder instead.
    pub(super) file_into: Option<PathBuf>,
    pub(super) edit: Option<GroupEdit>,
    /// Group whose items are animating open (true) or closed (false).
    motion: Option<(u64, bool, Instant, u64)>,
    generation: u64,
}

impl Seek {
    pub(super) fn sidebar_layout(&self) -> Layout {
        self.preferences.sidebar.resolve(&self.favorites)
    }

    /// Apply a change to the sidebar layout and save it. Nothing here moves files.
    fn change_sidebar(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Layout) -> Result<(), String>,
    ) {
        let mut layout = self.sidebar_layout();
        match change(&mut layout) {
            Ok(()) => {
                self.preferences.sidebar = layout;
                self.store.save(&self.preferences);
            }
            Err(error) => self.message = Some(error),
        }
        cx.notify();
    }

    /// Create a group and start naming it in place.
    pub(super) fn new_sidebar_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut created = None;
        self.change_sidebar(cx, |layout| {
            created = Some(layout.create_group("New group")?);
            Ok(())
        });
        if let Some(id) = created {
            self.sidebar = true;
            self.edit_group_name(id, window, cx);
        }
    }

    fn edit_group_name(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .sidebar_layout()
            .groups
            .iter()
            .find(|g| g.id == id)
            .map(|g| g.name.clone())
            .unwrap_or_default();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .context_menu(false)
                .placeholder("Group name")
                .default_value(name)
        });
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |this, input, event, window, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur
                    if this.sidebar_state.edit.as_ref().is_some_and(|e| e.id == id) =>
                {
                    let name = input.read(cx).value().to_string();
                    this.sidebar_state.edit = None;
                    this.change_sidebar(cx, |layout| layout.rename_group(id, &name));
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.table.read(cx).focus_handle(cx).focus(window, cx);
                    }
                }
                _ => {}
            },
        );
        self.sidebar_state.edit = Some(GroupEdit {
            id,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Escape while naming a group keeps the previous name.
    pub(super) fn cancel_group_edit(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.sidebar_state.edit.take().is_some() {
            self.table.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
            return true;
        }
        false
    }

    fn toggle_group(&mut self, id: u64, cx: &mut Context<Self>) {
        let mut collapsed = None;
        self.change_sidebar(cx, |layout| {
            collapsed = layout.toggle(id);
            Ok(())
        });
        if let Some(collapsed) = collapsed
            && self.motion_enabled
        {
            self.sidebar_state.generation = self.sidebar_state.generation.wrapping_add(1);
            self.sidebar_state.motion = Some((
                id,
                !collapsed,
                Instant::now(),
                self.sidebar_state.generation,
            ));
        }
    }

    fn open_sidebar_item(&mut self, item: &Item, window: &mut Window, cx: &mut Context<Self>) {
        match item {
            Item::Folder { path, .. } => self.scope_to(Some(path.clone()), window, cx),
            // A saved search runs its query each time it is opened.
            Item::Search { search } => self.recall_search(search.clone(), window, cx),
        }
    }

    /// Alt+1-9: open the n-th sidebar item in on-screen order.
    pub(super) fn open_numbered(
        &mut self,
        number: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let layout = self.sidebar_layout();
        let Some(item) = layout
            .numbered()
            .get(number.wrapping_sub(1))
            .cloned()
            .cloned()
        else {
            return false;
        };
        self.open_sidebar_item(&item, window, cx);
        true
    }

    /// Pin a folder to sidebar Favorites from a context menu, with a quiet toast.
    pub(super) fn pin_folder_to_sidebar(
        &mut self,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = folder
            .file_name()
            .unwrap_or(folder.as_os_str())
            .to_string_lossy()
            .into_owned();
        let mut added = false;
        self.change_sidebar(cx, |layout| {
            layout.place_item(Item::folder(folder, None), Place::Favorites, None)?;
            added = true;
            Ok(())
        });
        if added {
            self.sidebar = true;
            self.show_quiet_toast(format!("Added {name} to sidebar"), window, cx);
        }
    }

    /// Folders dragged from the file list become sidebar shortcuts here.
    fn add_dragged_folders(
        &mut self,
        paths: &[PathBuf],
        place: Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let slot = self
            .sidebar_state
            .drop
            .take()
            .filter(|(target, _)| *target == place)
            .map(|(_, slot)| slot);
        self.sidebar_state.file_into = None;
        self.sidebar_state.drop_row = None;
        let folders: Vec<PathBuf> = paths
            .iter()
            .filter(|path| std::fs::metadata(path).is_ok_and(|m| m.is_dir()))
            .cloned()
            .collect();
        if folders.is_empty() {
            self.message = Some("Only folders can be added to the sidebar.".into());
            cx.notify();
            return;
        }
        let count = folders.len();
        let first_name = folders[0]
            .file_name()
            .unwrap_or(folders[0].as_os_str())
            .to_string_lossy()
            .into_owned();
        let mut ok = false;
        self.change_sidebar(cx, |layout| {
            for (offset, folder) in folders.into_iter().enumerate() {
                layout.place_item(Item::folder(folder, None), place, slot.map(|s| s + offset))?;
            }
            ok = true;
            Ok(())
        });
        if ok {
            let text = if count == 1 {
                format!("Added {first_name} to sidebar")
            } else {
                format!("Added {count} folders to sidebar")
            };
            self.show_quiet_toast(text, window, cx);
        }
    }

    fn drop_on_place(&mut self, drag: &PlaceDrag, place: Place, cx: &mut Context<Self>) {
        self.sidebar_state.drop_row = None;
        let slot = self
            .sidebar_state
            .drop
            .take()
            .filter(|(target, _)| *target == place)
            .map(|(_, slot)| slot);
        let Some(item) = drag.item.clone().or_else(|| {
            drag.path
                .clone()
                .map(|path| Item::folder(path, Some(drag.label.clone())))
        }) else {
            return;
        };
        self.change_sidebar(cx, |layout| layout.place_item(item, place, slot));
    }

    fn track_item_drop(
        &mut self,
        place: Place,
        index: usize,
        event: &DragMoveEvent<PlaceDrag>,
        cx: &mut Context<Self>,
    ) {
        let bounds = event.bounds;
        if !bounds.contains(&event.event.position) {
            if self.sidebar_state.drop_row == Some((place, index)) {
                self.sidebar_state.drop_row = None;
                cx.notify();
            }
            return;
        }
        let after = event.event.position.y > bounds.center().y;
        let slot = (place, index + usize::from(after));
        if self.sidebar_state.drop != Some(slot)
            || self.sidebar_state.drop_row != Some((place, index))
        {
            self.sidebar_state.drop = Some(slot);
            self.sidebar_state.drop_row = Some((place, index));
            cx.notify();
        }
    }

    pub(super) fn clear_sidebar_drop(&mut self) -> bool {
        self.sidebar_state.drop_row = None;
        self.sidebar_state.group_hover = None;
        self.sidebar_state.drop.take().is_some()
            | self.sidebar_state.group_drop.take().is_some()
            | self.sidebar_state.file_into.take().is_some()
    }

    /// Files dragged over a sidebar row: the middle half of a folder row means
    /// "move into this folder"; the edges insert the dragged folders here.
    fn track_files_over_item(
        &mut self,
        place: Place,
        index: usize,
        folder: Option<PathBuf>,
        event: &DragMoveEvent<super::daily::FilesDrag>,
        cx: &mut Context<Self>,
    ) {
        let bounds = event.bounds;
        let y = event.event.position.y;
        if !bounds.contains(&event.event.position) {
            if (folder.is_some() && self.sidebar_state.file_into == folder)
                || self.sidebar_state.drop_row == Some((place, index))
            {
                if self.sidebar_state.file_into == folder {
                    self.sidebar_state.file_into = None;
                }
                if self.sidebar_state.drop_row == Some((place, index)) {
                    self.sidebar_state.drop_row = None;
                }
                cx.notify();
            }
            return;
        }
        let quarter = bounds.size.height / 4.;
        let middle = y > bounds.top() + quarter && y < bounds.bottom() - quarter;
        let into =
            folder.filter(|path| middle && super::daily::drop_allowed(&event.drag(cx).paths, path));
        let slot = into
            .is_none()
            .then(|| (place, index + usize::from(y > bounds.center().y)));
        let row = slot.map(|_| (place, index));
        if self.sidebar_state.file_into != into
            || self.sidebar_state.drop != slot
            || self.sidebar_state.drop_row != row
        {
            self.sidebar_state.file_into = into;
            self.sidebar_state.drop = slot;
            self.sidebar_state.drop_row = row;
            cx.notify();
        }
    }

    fn sidebar_item(
        &self,
        item: &Item,
        place: Place,
        index: usize,
        indent: bool,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = item.label();
        // While Storage or Worktrees fills the window, only that tool is
        // highlighted, as "All locations" already does.
        let selected = self.storage_page.is_none()
            && self.worktrees_page.is_none()
            && item
                .path()
                .is_some_and(|path| self.scope.as_deref() == Some(path));
        let known = item
            .path()
            .is_some_and(|path| self.favorites.iter().any(|(_, known)| known == path));
        let icon: AnyElement = match item {
            Item::Folder { .. } => {
                // One quiet ink for every place, as in Monodrive: the glyph
                // tells them apart, colour is kept for content.
                let glyph = match label.as_str() {
                    "Downloads" => Glyph::Download,
                    "Desktop" => Glyph::Monitor,
                    "Documents" => Glyph::FileText,
                    "Pictures" => Glyph::Image,
                    "Videos" => Glyph::Film,
                    "Music" => Glyph::Music,
                    "Home" => Glyph::House,
                    _ if label.contains("OneDrive") => Glyph::Cloud,
                    _ => Glyph::Folder,
                };
                Icon::new(glyph)
                    .size(px(16.))
                    .text_color(sidebar_icon(p, selected))
                    .into_any_element()
            }
            Item::Search { .. } => Icon::new(Glyph::Search)
                .size(px(16.))
                .text_color(sidebar_icon(p, selected))
                .into_any_element(),
        };
        let open = item.clone();
        let drag = PlaceDrag {
            path: item.path().map(PathBuf::from),
            label: label.clone(),
            item: Some(item.clone()),
        };
        let owner = cx.entity().downgrade();
        let menu_path = item.path().map(PathBuf::from);
        let file_target = item.path().map(PathBuf::from);
        let moving_into = (file_target.is_some() && self.sidebar_state.file_into == file_target)
            || self.sidebar_state.drop_row == Some((place, index));
        let over_target = file_target.clone();
        let drop_target = file_target.clone();
        let row = div()
            .id(SharedString::from(format!(
                "sidebar-item-{place:?}-{index}"
            )))
            .role(Role::Button)
            .aria_label(match item {
                Item::Folder { .. } => label.clone(),
                Item::Search { .. } => format!("Saved search: {label}"),
            })
            .flex()
            .items_center()
            .gap_2()
            .h(px(ROW))
            .px_2()
            .when(indent, |d| d.pl(px(22.)))
            .rounded(crate::tokens::RADIUS_ROW)
            .cursor_pointer()
            .bg(if moving_into {
                p.accent.opacity(0.22)
            } else if selected {
                p.hover
            } else {
                rgba(0).into()
            })
            .text_color(p.text)
            .hover(move |s| s.bg(p.hover))
            .child(icon)
            .child(div().flex_1().min_w_0().truncate().child(label))
            .on_click(
                cx.listener(move |this, _, window, cx| this.open_sidebar_item(&open, window, cx)),
            )
            .on_drag(drag, |drag, offset, _, cx| {
                super::drag_preview::near_cursor(drag.clone(), offset, cx)
            })
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<PlaceDrag>, _, cx| {
                    this.track_item_drop(place, index, event, cx)
                }),
            )
            .on_drop(
                cx.listener(move |this, drag: &PlaceDrag, _, cx| {
                    this.drop_on_place(drag, place, cx)
                }),
            )
            .on_drag_move(cx.listener(
                move |this, event: &DragMoveEvent<super::daily::FilesDrag>, _, cx| {
                    this.track_files_over_item(place, index, over_target.clone(), event, cx)
                },
            ))
            .on_drop(
                cx.listener(move |this, drag: &super::daily::FilesDrag, window, cx| {
                    match drop_target.clone() {
                        Some(path) if this.sidebar_state.file_into.as_ref() == Some(&path) => {
                            this.sidebar_state.file_into = None;
                            this.drop_files(drag, path, window, cx)
                        }
                        _ => this.add_dragged_folders(&drag.paths, place, window, cx),
                    }
                }),
            )
            .context_menu(move |menu, window, cx| {
                let menu = match &menu_path {
                    Some(path) => super::daily::actions_menu(
                        super::daily::menu_style(menu, window),
                        owner.clone(),
                        Some(path.clone()),
                        true,
                        cx,
                    )
                    .separator(),
                    None => super::daily::menu_style(menu, window),
                };
                let new_owner = owner.clone();
                let menu = menu.item(PopupMenuItem::new("New group").on_click(
                    move |_, window, cx| {
                        let _ = new_owner.update(cx, |this, cx| this.new_sidebar_group(window, cx));
                    },
                ));
                if known {
                    menu
                } else {
                    let remove_owner = owner.clone();
                    menu.item(PopupMenuItem::new("Remove from sidebar").on_click(
                        move |_, _, cx| {
                            let _ = remove_owner.update(cx, |this, cx| {
                                this.change_sidebar(cx, |layout| {
                                    layout
                                        .remove_item(place, index)
                                        .map(|_| ())
                                        .ok_or("That item moved.".into())
                                })
                            });
                        },
                    ))
                }
            });
        row.into_any_element()
    }

    /// The Favorites heading and items, then each group.
    pub(super) fn favorites_section(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let layout = self.sidebar_layout();
        let mut out = vec![];
        let owner = cx.entity().downgrade();
        out.push(
            super::interface::heading("Favorites", p)
                .id("favorites-heading")
                .drag_over::<PlaceDrag>(move |style, _, _, _| style.text_color(p.text))
                .on_drop(cx.listener(|this, drag: &PlaceDrag, _, cx| {
                    this.sidebar_state.drop = None;
                    this.drop_on_place(drag, Place::Favorites, cx)
                }))
                .on_drop(cx.listener(|this, drag: &super::daily::FilesDrag, window, cx| {
                    this.add_dragged_folders(&drag.paths, Place::Favorites, window, cx)
                }))
                .context_menu(move |menu, window, _| {
                    let owner = owner.clone();
                    super::daily::menu_style(menu, window).item(
                        PopupMenuItem::new("New group").on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| this.new_sidebar_group(window, cx));
                        }),
                    )
                })
                .into_any_element(),
        );
        out.push(self.all_locations_row(p, cx));
        for (index, item) in layout.favorites.iter().enumerate() {
            out.push(self.sidebar_item(item, Place::Favorites, index, false, p, cx));
        }
        for (position, group) in layout.groups.iter().enumerate() {
            let header = self.group_header(group, position, p, cx);
            let place = Place::Group(group.id);
            let mut items = div().flex().flex_col();
            for (index, item) in group.items.iter().enumerate() {
                items = items.child(self.sidebar_item(item, place, index, true, p, cx));
            }
            // An empty group opens a drop area only while something is dragged.
            if group.items.is_empty() && !group.collapsed && cx.has_active_drag() {
                items = items.child(div().h(px(ROW)));
            }
            let height = (group.items.len().max(1) as f32) * ROW;
            let motion = self
                .sidebar_state
                .motion
                .filter(|(id, _, at, _)| *id == group.id && at.elapsed() < crate::motion::SPRING);
            let body = match motion {
                Some((_, expanding, _, generation)) => Some(
                    items
                        .overflow_hidden()
                        .with_animation(
                            ("group-motion", generation as usize),
                            crate::motion::reversible(),
                            move |items, t| {
                                let shown = if expanding { t } else { 1. - t };
                                items.h(px(height * shown)).opacity(shown)
                            },
                        )
                        .into_any_element(),
                ),
                None if group.collapsed => None,
                None => Some(items.into_any_element()),
            };
            // The whole group, heading, items and gaps, accepts dropped folders
            // and sidebar items; a row's own drop (move into, or insert at its
            // edge) is handled first.
            out.push(
                div()
                    .id(("group-area", group.id as usize))
                    .flex()
                    .flex_col()
                    .rounded(crate::tokens::RADIUS_ROW)
                    .when(self.sidebar_state.group_hover == Some(position), |d| {
                        d.bg(p.accent.opacity(0.12))
                    })
                    .drag_over::<PlaceDrag>(move |style, _, _, _| style.bg(p.accent.opacity(0.08)))
                    .drag_over::<super::daily::FilesDrag>(move |style, _, _, _| {
                        style.bg(p.accent.opacity(0.08))
                    })
                    .on_drop(cx.listener(move |this, drag: &PlaceDrag, _, cx| {
                        this.drop_on_place(drag, place, cx)
                    }))
                    .on_drop(
                        cx.listener(move |this, drag: &super::daily::FilesDrag, window, cx| {
                            this.add_dragged_folders(&drag.paths, place, window, cx)
                        }),
                    )
                    .child(header)
                    .children(body)
                    .into_any_element(),
            );
        }
        out
    }

    pub(super) fn storage_row(&self, p: crate::appearance::Palette, cx: &mut Context<Self>) -> AnyElement {
        let open = self.storage_page.is_some();
        div()
            .id("storage")
            .role(Role::Button)
            .aria_label("Storage")
            .flex()
            .items_center()
            .gap_2()
            .h(px(ROW))
            .px_2()
            .rounded(crate::tokens::RADIUS_ROW)
            .cursor_pointer()
            .bg(if open { p.hover } else { rgba(0).into() })
            .text_color(p.text)
            .hover(move |s| s.bg(p.hover))
            .child(
                Icon::empty()
                    .path("seek/pie-chart.svg")
                    .size(px(16.))
                    .text_color(sidebar_icon(p, open)),
            )
            .child("Storage")
            .on_click(cx.listener(|this, _, _, cx| {
                if this.storage_page.is_none() {
                    this.open_storage(this.scope.clone().and_then(|s| {
                        this.roots.iter().find(|r| s.starts_with(r)).cloned()
                    }), cx);
                }
            }))
            .into_any_element()
    }

    pub(super) fn worktrees_row(&self, p: crate::appearance::Palette, cx: &mut Context<Self>) -> AnyElement {
        let open = self.worktrees_page.is_some();
        div()
            .id("worktrees")
            .role(Role::Button)
            .aria_label("Worktrees")
            .flex()
            .items_center()
            .gap_2()
            .h(px(ROW))
            .px_2()
            .rounded(crate::tokens::RADIUS_ROW)
            .cursor_pointer()
            .bg(if open { p.hover } else { rgba(0).into() })
            .text_color(p.text)
            .hover(move |s| s.bg(p.hover))
            .child(
                Icon::empty()
                    .path("seek/git-branch.svg")
                    .size(px(16.))
                    .text_color(sidebar_icon(p, open)),
            )
            .child("Worktrees")
            .on_click(cx.listener(|this, _, _, cx| {
                if this.worktrees_page.is_none() {
                    this.open_worktrees(cx);
                }
            }))
            .into_any_element()
    }

    fn all_locations_row(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let all = self.scope.is_none() && self.storage_page.is_none() && self.worktrees_page.is_none();
        div()
            .id("all")
            .role(Role::Button)
            .aria_label("All locations")
            .flex()
            .items_center()
            .gap_2()
            .h(px(ROW))
            .px_2()
            .rounded(crate::tokens::RADIUS_ROW)
            .cursor_pointer()
            .bg(if all { p.hover } else { rgba(0).into() })
            .text_color(p.text)
            .hover(move |s| s.bg(p.hover))
            .child(
                Icon::new(Glyph::Search)
                    .size(px(16.))
                    .text_color(sidebar_icon(p, all)),
            )
            .child("All locations")
            .on_click(cx.listener(|this, _, window, cx| this.scope_to(None, window, cx)))
            .into_any_element()
    }

    fn group_header(
        &self,
        group: &crate::sidebar_groups::Group,
        position: usize,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = group.id;
        let collapsed = group.collapsed;
        let place = Place::Group(id);
        let owner = cx.entity().downgrade();
        // Naming gets its own full-width field with a short hint, rather than
        // squeezing an input into the 30 px header row.
        if let Some(edit) = self.sidebar_state.edit.as_ref().filter(|e| e.id == id) {
            return div()
                .id(("sidebar-group-edit", id as usize))
                .role(Role::Group)
                .aria_label("Name this group")
                .mt_2()
                .mb_1()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(Input::new(&edit.input).w_full())
                .into_any_element();
        }
        let name = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .child(group.name.clone());
        div()
            .id(("sidebar-group", id as usize))
            .role(Role::Button)
            .aria_label(format!(
                "{} group, {}",
                group.name,
                if collapsed { "collapsed" } else { "expanded" }
            ))
            .mt_2()
            .flex()
            .items_center()
            .gap_1()
            .h(px(ROW))
            .px_2()
            .rounded(crate::tokens::RADIUS_ROW)
            .cursor_pointer()
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(p.muted)
            .hover(move |s| s.bg(p.hover))
            .child(
                Icon::new(if collapsed {
                    Glyph::ChevronRight
                } else {
                    Glyph::ChevronDown
                })
                .size(px(12.))
                .flex_shrink_0(),
            )
            .child(name)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_group(id, cx);
            }))
            .on_drag(
                GroupDrag {
                    id,
                    name: group.name.clone(),
                },
                |drag, offset, _, cx| super::drag_preview::near_cursor(drag.clone(), offset, cx),
            )
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<GroupDrag>, _, cx| {
                    let bounds = event.bounds;
                    if bounds.contains(&event.event.position) {
                        let slot =
                            position + usize::from(event.event.position.y > bounds.center().y);
                        if this.sidebar_state.group_drop != Some(slot)
                            || this.sidebar_state.group_hover != Some(position)
                        {
                            this.sidebar_state.group_drop = Some(slot);
                            this.sidebar_state.group_hover = Some(position);
                            cx.notify();
                        }
                    }
                }),
            )
            .on_drop(cx.listener(move |this, drag: &GroupDrag, _, cx| {
                this.sidebar_state.group_hover = None;
                let slot = this.sidebar_state.group_drop.take().unwrap_or(position);
                let moved = drag.id;
                this.change_sidebar(cx, |layout| layout.move_group(moved, slot));
            }))
            .drag_over::<PlaceDrag>(move |style, _, _, _| style.bg(p.accent.opacity(0.12)))
            .on_drop(cx.listener(move |this, drag: &PlaceDrag, _, cx| {
                this.sidebar_state.drop = None;
                this.drop_on_place(drag, place, cx)
            }))
            .drag_over::<super::daily::FilesDrag>(move |style, _, _, _| {
                style.bg(p.accent.opacity(0.12))
            })
            .on_drop(
                cx.listener(move |this, drag: &super::daily::FilesDrag, window, cx| {
                    this.add_dragged_folders(&drag.paths, place, window, cx)
                }),
            )
            .context_menu(move |menu, window, cx| {
                let current = owner
                    .upgrade()
                    .map(|view| {
                        let this = view.read(cx);
                        let text = this.input.read(cx).value().to_string();
                        (!text.trim().is_empty()).then(|| crate::settings::Search {
                            query: text,
                            scope: this.scope.clone(),
                            content: this.content_mode,
                        })
                    })
                    .unwrap_or_default();
                let (rename, delete, add, new) =
                    (owner.clone(), owner.clone(), owner.clone(), owner.clone());
                let mut menu = super::daily::menu_style(menu, window)
                    .item(
                        PopupMenuItem::new("Rename group").on_click(move |_, window, cx| {
                            let _ =
                                rename.update(cx, |this, cx| this.edit_group_name(id, window, cx));
                        }),
                    )
                    .item(
                        PopupMenuItem::new("Delete group").on_click(move |_, _, cx| {
                            let _ = delete.update(cx, |this, cx| {
                                this.change_sidebar(cx, |layout| layout.delete_group(id))
                            });
                        }),
                    );
                if let Some(search) = current {
                    menu = menu.item(PopupMenuItem::new("Add current search").on_click(
                        move |_, _, cx| {
                            let search = search.clone();
                            let _ = add.update(cx, |this, cx| {
                                this.change_sidebar(cx, |layout| {
                                    layout.place_item(Item::Search { search }, place, None)
                                })
                            });
                        },
                    ));
                }
                menu.separator()
                    .item(
                        PopupMenuItem::new("New group").on_click(move |_, window, cx| {
                            let _ = new.update(cx, |this, cx| this.new_sidebar_group(window, cx));
                        }),
                    )
            })
            .into_any_element()
    }
}

/// Sidebar icons carry the palette's accent: a touch of colour beside quiet
/// labels, a little stronger on the place that is open.
fn sidebar_icon(p: crate::appearance::Palette, selected: bool) -> Hsla {
    p.accent.opacity(if selected { 1. } else { 0.8 })
}
