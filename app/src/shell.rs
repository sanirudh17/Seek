use crate::backend::{Backend, Command, Request, Row, Update};
use crate::{
    icons::Atlas,
    media,
    settings::{Appearance, Settings, Store, View},
};
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::{
    Disableable, Icon, Root, Sizable,
    button::{Button, ButtonVariants},
    input::{InlineToken, Input, InputContent, InputEvent, InputState},
    table::{Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState},
};
use seek_engine::Status;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use std::{path::PathBuf, sync::atomic::Ordering};
#[path = "agents_view.rs"]
mod agents_view;
#[path = "batch_rename_view.rs"]
mod batch_rename_view;
#[path = "columns.rs"]
mod columns;
#[path = "command_palette.rs"]
mod command_palette;
#[path = "daily.rs"]
mod daily;
#[path = "doc_pane.rs"]
mod doc_pane;
#[path = "drag_preview.rs"]
mod drag_preview;
#[path = "drag_split.rs"]
mod drag_split;
#[path = "enlarged_preview.rs"]
mod enlarged_preview;
#[path = "glass_view.rs"]
mod glass_view;
#[path = "info_resize.rs"]
mod info_resize;
#[path = "icon_view_options.rs"]
mod icon_view_options;
#[path = "image_tools_view.rs"]
mod image_tools_view;
#[path = "inspector_view.rs"]
mod inspector_view;
#[path = "interface.rs"]
mod interface;
#[path = "operation_history_view.rs"]
mod operation_history_view;
#[path = "organize_view.rs"]
mod organize_view;
#[path = "live_folders.rs"]
mod live_folders;
#[path = "panes.rs"]
mod panes;
#[path = "permanent_delete.rs"]
mod permanent_delete;
#[path = "places_view.rs"]
mod places_view;
#[path = "preview_controls.rs"]
mod preview_controls;
#[path = "rename_advanced_view.rs"]
mod rename_advanced_view;
#[path = "related.rs"]
mod related;
#[path = "saved_commands_view.rs"]
mod saved_commands_view;
#[path = "search_help.rs"]
mod search_help;
#[path = "search_view.rs"]
mod search_view;
#[path = "settings_view.rs"]
mod settings_view;
#[path = "sidebar_view.rs"]
mod sidebar_view;
#[path = "storage_view.rs"]
mod storage_view;
#[path = "cleanup_view.rs"]
mod cleanup_view;
#[path = "duplicates_view.rs"]
mod duplicates_view;
#[path = "similar_view.rs"]
mod similar_view;
#[path = "tabs.rs"]
mod tabs;
#[path = "worktrees_view.rs"]
mod worktrees_view;
#[path = "text_reader.rs"]
mod text_reader;
#[path = "updating.rs"]
mod updating;
/// The glass and background sliders, in this order: window opacity,
/// sidebar opacity, opacity behind files, opacity in empty folders, hue,
/// saturation, darkness.
pub(crate) const SLIDERS: usize = 9;
pub(crate) const SLIDER_LABELS: [&str; SLIDERS] = [
    "Window opacity",
    "Sidebar opacity",
    "Behind files",
    "In empty folders",
    "Hue",
    "Saturation",
    "Darkness",
    "Drag hover",
    "Intensity",
];
fn slider_range(index: usize) -> (f32, f32) {
    match index {
        2 => (0., 30.),
        3 => (0., 80.),
        4 => (0., 359.),
        6 => (0., 30.),
        7 => (200., 2000.),
        _ => (0., 100.),
    }
}
fn slider_value(s: &crate::settings::Settings, index: usize) -> u16 {
    match index {
        0 => s.glass_opacity as u16,
        1 => s.sidebar_opacity as u16,
        2 => s.background_opacity as u16,
        3 => s.background_empty_opacity as u16,
        4 => s.shell_hue,
        5 => s.shell_saturation as u16,
        7 => s.drag_hover_ms,
        8 => s.palette_intensity as u16,
        _ => s.shell_lightness as u16,
    }
}
fn set_slider(s: &mut crate::settings::Settings, index: usize, value: u16) {
    let byte = value.min(255) as u8;
    match index {
        0 => s.glass_opacity = byte,
        1 => s.sidebar_opacity = byte,
        2 => s.background_opacity = byte.min(30),
        3 => s.background_empty_opacity = byte.min(80),
        4 => s.shell_hue = value.min(359),
        5 => s.shell_saturation = byte.min(100),
        // Steps of 50 ms, so the slider lands on round numbers.
        7 => s.drag_hover_ms = (value.clamp(200, 2000) + 25) / 50 * 50,
        8 => s.palette_intensity = byte.min(100),
        _ => s.shell_lightness = byte.min(30),
    }
}
gpui::actions!(
    seek,
    [
        NewTab,
        CloseTab,
        NextTab,
        PreviousTab,
        ExpandFolder,
        CollapseFolder,
        CopyFiles,
        CutFiles,
        PasteFiles,
        CopyPath,
        RenameFile,
        PanImageLeft,
        PanImageRight,
        PanImageUp,
        PanImageDown,
        UndoFile,
        RedoFile,
        RecycleFiles,
        DeletePermanently,
        SplitRight,
        SplitDown,
        ClosePane,
        NextPane,
        PreviousPane,
        CopyToOtherPane,
        MoveToOtherPane,
        NewFolder,
        ToggleSidebar
    ]
);

fn keep_scroll_anchor(
    handle: &UniformListScrollHandle,
    old: usize,
    new: usize,
    row_height: Pixels,
) {
    let state = handle.0.borrow();
    if state.deferred_scroll_to_item.is_none() && old != new {
        let mut offset = state.base_handle.offset();
        offset.y += row_height * (old as f32 - new as f32);
        state.base_handle.set_offset(offset);
    }
}
fn scroll_top(handle: &UniformListScrollHandle, row_height: Pixels) -> usize {
    let state = handle.0.borrow();
    ((-state.base_handle.offset().y / row_height.max(px(1.))).max(0.)).floor() as usize
}
pub struct Startup {
    pub session: Result<Option<crate::pane_layout::Session>, String>,
    pub backend: Backend,
    pub updates: async_channel::Receiver<Update>,
    pub remembered: crate::window_state::Remembered,
    pub preferences: Settings,
    pub store: Store,
    pub atlas: Atlas,
    pub shell_atlas_rx: async_channel::Receiver<Atlas>,
    pub media: media::Worker,
    pub media_updates: async_channel::Receiver<media::Reply>,
    /// Folders to open in a new tab ("Open in Seek" from Explorer).
    pub open_requests: async_channel::Receiver<PathBuf>,
}

struct Rows {
    empty_message: String,
    columns: Vec<Column>,
    layout_key: Option<(u32, bool, bool)>,
    rows: Arc<Vec<Row>>,
    sort: Option<(usize, ColumnSort)>,
    query: String,
    content: bool,
    preferences: Settings,
    atlas: Atlas,
    cache: Arc<Mutex<media::Cache>>,
    media: media::Worker,
    visible: Arc<Mutex<std::collections::HashSet<media::Key>>>,
    extra: Arc<Mutex<std::collections::HashSet<media::Key>>>,
    base_rows: Arc<Vec<Row>>,
    children: std::collections::HashMap<PathBuf, Vec<Row>>,
    expanded: std::collections::HashSet<PathBuf>,
    depths: Vec<usize>,
    selection: Option<PathBuf>,
    selected_many: std::collections::HashSet<PathBuf>,
    selection_anchor: Option<PathBuf>,
    drag_paths: Arc<Vec<PathBuf>>,
    /// A press on a multi-selected row, collapsed to that row on release
    /// unless the press became a drag.
    pending_collapse: Option<usize>,
    owner: WeakEntity<Seek>,
    visible_range: std::ops::Range<usize>,
}
impl Rows {
    fn refresh_media(&self) {
        let keys = self
            .rows
            .iter()
            .skip(self.visible_range.start)
            .take(self.visible_range.len())
            .filter(|row| {
                interface::needs_shell(row)
                    && interface::thumbnail_allowed(row, &self.preferences)
                    && row.file.kind & 3 != seek_engine::walk::KIND_DIR
            })
            .map(|row| media::Key::new(row, 32, false))
            .collect();
        *self.visible.lock().unwrap() = keys;
        interface::request_media(&self.media, &self.visible, &self.extra, &self.cache);
    }
    fn sorted(&mut self) {
        let _profile = crate::metrics::span("sort_rows");
        let (rows, depths) = crate::tree::flatten(
            &self.base_rows,
            &mut self.children,
            &self.expanded,
            self.sort,
        );
        self.rows = Arc::new(rows);
        self.depths = depths;
    }
}
impl TableDelegate for Rows {
    fn render_tr(
        &mut self,
        i: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let Some(row) = self.rows.get(i) else {
            return div().id(("empty-row", i));
        };
        let p = crate::appearance::palette(&self.preferences, cx);
        let chosen = self.selected_many.contains(&row.file.path);
        let caret = self.selection.as_ref() == Some(&row.file.path);
        // Selected neighbours join into one block: the highlight runs into
        // the next selected row with square corners where they meet.
        let joined = |at: Option<usize>| {
            chosen
                && at
                    .and_then(|at| self.rows.get(at))
                    .is_some_and(|r| self.selected_many.contains(&r.file.path))
        };
        let join_up = joined(i.checked_sub(1));
        let join_down = joined(Some(i + 1));
        let owner = self.owner.clone();
        let folder = row.file.path.clone();
        let paths = if self.selected_many.contains(&folder) {
            self.drag_paths.clone()
        } else {
            Arc::new(vec![folder.clone()])
        };
        let drag = daily::FilesDrag {
            label: if paths.len() > 1 {
                format!("{} items", paths.len())
            } else {
                row.name.clone()
            },
            paths,
        };
        div()
            .id(("file-row", i))
            .group("file-row")
            .aria_label(format!(
                "{}{}",
                row.name,
                if chosen { " selected" } else { "" }
            ))
            .relative()
            .child(
                // An inset, rounded highlight rather than a full-width bar.
                div()
                    .absolute()
                    .top(px(if join_up { 0. } else { 1. }))
                    // Over the row's 1px bottom border, to meet the next row.
                    .bottom(px(if join_down { -1. } else { 1. }))
                    .left(px(4.))
                    .right(px(4.))
                    .when(!join_up, |d| d.rounded_t(crate::tokens::RADIUS_ROW))
                    .when(!join_down, |d| d.rounded_b(crate::tokens::RADIUS_ROW))
                    .bg(if chosen {
                        p.accent.opacity(0.26)
                    } else {
                        rgba(0).into()
                    })
                    .when(caret && !chosen, |d| {
                        d.border_1().border_color(p.muted.opacity(0.45))
                    })
                    .when(!chosen, |d| {
                        d.group_hover("file-row", move |s| s.bg(p.hover))
                    }),
            )
            .on_drag(drag, |drag, offset, _, cx| {
                drag_preview::near_cursor(drag.clone(), offset, cx)
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |table, event: &MouseDownEvent, window, cx| {
                    let current = table.selected_row();
                    let data = table.delegate_mut();
                    let keep = data.rows.get(i).is_some_and(|row| {
                        crate::selection::press_keeps_selection(
                            &data.selected_many,
                            &row.file.path,
                            event.modifiers.shift,
                            event.modifiers.control,
                        )
                    });
                    data.pending_collapse = keep.then_some(i);
                    if !keep {
                        crate::selection::change(
                            &data.rows,
                            &mut data.selected_many,
                            &mut data.selection_anchor,
                            current,
                            i,
                            crate::selection::Input {
                                shift: event.modifiers.shift,
                                control: event.modifiers.control,
                                keyboard: false,
                            },
                        );
                    }
                    table.set_selected_row(i, cx);
                    table.focus_handle(cx).focus(window, cx);
                    // Keep the clicked row as the caret, not the first selected row.
                    // The native click still supplies double-click/open behavior.
                    let data = table.delegate_mut();
                    data.drag_paths =
                        Arc::new(crate::selection::ordered(&data.rows, &data.selected_many));
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |table, event: &MouseUpEvent, _, cx| {
                    let data = table.delegate_mut();
                    if data.pending_collapse.take() != Some(i) || cx.has_active_drag() {
                        return;
                    }
                    crate::selection::change(
                        &data.rows,
                        &mut data.selected_many,
                        &mut data.selection_anchor,
                        Some(i),
                        i,
                        crate::selection::Input {
                            shift: event.modifiers.shift,
                            control: event.modifiers.control,
                            keyboard: false,
                        },
                    );
                    data.drag_paths =
                        Arc::new(crate::selection::ordered(&data.rows, &data.selected_many));
                    cx.notify();
                }),
            )
            .when(row.file.kind & 3 == seek_engine::walk::KIND_DIR, |row| {
                let feedback = folder.clone();
                let hover_path = folder.clone();
                let hover_owner = owner.clone();
                row.drag_over::<daily::FilesDrag>(move |style, drag, _, _| {
                    if daily::drop_allowed(&drag.paths, &feedback) {
                        style.bg(p.accent.opacity(0.22))
                    } else {
                        style
                    }
                })
                .on_drag_move(move |event: &DragMoveEvent<daily::FilesDrag>, window, cx| {
                    let _ = hover_owner.update(cx, |this, cx| {
                        this.hover_drop_folder(hover_path.clone(), event, window, cx)
                    });
                })
                .on_drop(move |drag: &daily::FilesDrag, window, cx| {
                    let _ = owner.update(cx, |this, cx| {
                        this.drop_files(drag, folder.clone(), window, cx)
                    });
                })
            })
    }
    fn context_menu(
        &mut self,
        i: usize,
        menu: gpui_component::menu::PopupMenu,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> gpui_component::menu::PopupMenu {
        daily::actions_menu_selection(
            daily::menu_style(menu, window),
            self.owner.clone(),
            self.rows.get(i).map(|row| row.file.path.clone()),
            self.rows
                .get(i)
                .is_some_and(|row| row.file.kind & 3 == seek_engine::walk::KIND_DIR),
            Some(
                if self
                    .rows
                    .get(i)
                    .is_some_and(|row| self.selected_many.contains(&row.file.path))
                {
                    self.rows
                        .iter()
                        .filter(|row| self.selected_many.contains(&row.file.path))
                        .map(|row| row.file.path.clone())
                        .collect()
                } else {
                    self.rows
                        .get(i)
                        .map(|row| row.file.path.clone())
                        .into_iter()
                        .collect()
                },
            ),
            cx,
        )
    }
    fn background_menu(
        &mut self,
        menu: gpui_component::menu::PopupMenu,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> gpui_component::menu::PopupMenu {
        // Seek itself is not borrowed while the table builds this menu.
        let Some(state) = self
            .owner
            .upgrade()
            .map(|seek| seek.read(cx).background_state(self.sort, cx))
        else {
            return menu;
        };
        daily::background_menu(daily::menu_style(menu, window), self.owner.clone(), state)
    }
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len() - usize::from(self.query.trim().is_empty() && !self.content)
    }
    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }
    fn column(&self, i: usize, _: &App) -> Column {
        if self.content && i == 1 {
            Column::new("match", "Matching line").width(self.columns[i].width)
        } else {
            self.columns[i].clone()
        }
    }
    fn perform_sort(
        &mut self,
        column: usize,
        direction: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        self.sort = (direction != ColumnSort::Default).then_some((column, direction));
        self.sorted();
        self.refresh_media();
        let selected = self
            .selection
            .as_ref()
            .and_then(|path| self.rows.iter().position(|row| &row.file.path == path));
        let table = cx.entity();
        cx.defer(move |cx| {
            table.update(cx, |table, cx| {
                if let Some(i) = selected {
                    table.set_selected_row(i, cx);
                }
                cx.notify();
            })
        });
        #[cfg(feature = "benchmarks")]
        crate::metrics::interaction(
            "sort",
            serde_json::json!({"column": column, "direction": format!("{direction:?}"), "rows": self.rows.iter().map(|row| &row.name).collect::<Vec<_>>() }),
        );
        cx.notify();
    }
    fn render_th(
        &mut self,
        column: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .text_size(px(13.))
            .line_height(px(18.))
            .child(self.column(column, cx).name.clone())
    }
    fn render_td(
        &mut self,
        row: usize,
        column: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let _profile = crate::metrics::span("render_cell");
        let p = crate::appearance::palette(&self.preferences, cx);
        let selected = self.selected_many.contains(&self.rows[row].file.path);
        let depth = self.depths.get(row).copied().unwrap_or(0);
        let row = &self.rows[row];
        if column == 0 {
            div()
                .h_full()
                .min_w_0()
                .flex()
                .items_center()
                .line_height(px(20.))
                .gap_2()
                .text_color(p.text)
                .child(interface::disclosure(
                    row,
                    depth,
                    self.expanded.contains(&row.file.path),
                    self.owner.clone(),
                ))
                .child(interface::file_image(
                    row,
                    16.,
                    &self.atlas,
                    &self.cache,
                    interface::thumbnail_allowed(row, &self.preferences),
                ))
                .child(StyledText::new(row.name.clone()).with_highlights(
                    row.highlights.iter().cloned().map(|range| {
                        (
                            range,
                            HighlightStyle {
                                font_weight: Some(FontWeight::SEMIBOLD),
                                ..Default::default()
                            },
                        )
                    }),
                ))
                .children(interface::cloud_badge(row, p))
                .into_any_element()
        } else {
            div()
                .id(SharedString::from(format!(
                    "cell-{column}-{}",
                    row.file.path.display()
                )))
                .min_w_0()
                .h_full()
                .flex()
                .items_center()
                .line_height(px(20.))
                .truncate()
                .when(self.content && column == 1, |d| {
                    d.role(Role::Label)
                        .aria_label(row.snippet.clone().unwrap_or_default())
                })
                .text_color(if selected { p.text } else { p.muted })
                // Dates and sizes are data: Geist Mono keeps their digits aligned.
                .when((column == 1 && !self.content) || column == 2, |d| {
                    d.font_family(crate::tokens::FONT_MONO)
                        .text_size(crate::tokens::TEXT_SMALL)
                })
                .child(match column {
                    1 => row
                        .snippet
                        .as_ref()
                        .map(|text| {
                            let first = text.lines().next().unwrap_or_default();
                            let additional = text.lines().count().saturating_sub(1);
                            if additional == 0 {
                                first.to_owned()
                            } else {
                                format!("{first} (+{additional} lines in information panel)")
                            }
                        })
                        .unwrap_or_else(|| row.modified.clone()),
                    2 => row.size.clone(),
                    3 => row.kind.clone(),
                    _ => row.folder.clone(),
                })
                .into_any_element()
        }
    }
    fn visible_rows_changed(
        &mut self,
        range: std::ops::Range<usize>,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        #[cfg(feature = "benchmarks")]
        crate::metrics::interaction(
            "visible_rows",
            serde_json::json!({"start":range.start,"end":range.end}),
        );
        self.visible_range = range;
        self.refresh_media();
    }
    fn render_empty(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(self.empty_message.clone())
    }
}

type AppliedView = (
    String,
    Option<PathBuf>,
    bool,
    Vec<PathBuf>,
    Option<bool>,
    bool,
);
impl Rows {
    #[allow(clippy::too_many_arguments)]
    fn new(
        empty_message: &str,
        preferences: &Settings,
        atlas: &Atlas,
        cache: &Arc<Mutex<media::Cache>>,
        media: &media::Worker,
        visible: &Arc<Mutex<std::collections::HashSet<media::Key>>>,
        extra: &Arc<Mutex<std::collections::HashSet<media::Key>>>,
        owner: WeakEntity<Seek>,
    ) -> Self {
        Rows {
            empty_message: empty_message.into(),
            layout_key: None,
            columns: vec![
                Column::new("name", "Name").width(px(250.)).sortable(),
                Column::new("modified", "Date Modified")
                    .width(px(145.))
                    .sortable(),
                Column::new("size", "Size").width(px(80.)).sortable(),
                Column::new("kind", "Type").width(px(130.)).sortable(),
                Column::new("folder", "Folder").width(px(250.)).sortable(),
            ],
            rows: Arc::new(vec![]),
            sort: None,
            query: String::new(),
            content: false,
            preferences: preferences.clone(),
            atlas: atlas.clone(),
            cache: cache.clone(),
            media: media.clone(),
            visible: visible.clone(),
            extra: extra.clone(),
            base_rows: Arc::new(vec![]),
            children: Default::default(),
            expanded: Default::default(),
            depths: vec![],
            selection: None,
            selected_many: Default::default(),
            selection_anchor: None,
            drag_paths: Arc::new(vec![]),
            pending_collapse: None,
            owner,
            visible_range: 0..0,
        }
    }
}
/// One results table and its cached list view. Every pane owns one.
fn results_table(
    rows: Rows,
    window: &mut Window,
    cx: &mut Context<Seek>,
) -> (Entity<TableState<Rows>>, Entity<ResultPane>) {
    let table = cx.new(|cx| {
        TableState::new(rows, window, cx)
            .loop_selection(false)
            .col_selectable(false)
    });
    let result_pane = cx.new(|cx| ResultPane {
        table: table.clone(),
        _observe: cx.observe(&table, |_, _, cx| cx.notify()),
    });
    (table, result_pane)
}
// Reuse unchanged table paint only when no accessibility tree is being built.
// GPUI's cached prepaint does not replay accessibility nodes. Active assistive
// technology always gets the normal, fully rendered table.
struct ResultPane {
    table: Entity<TableState<Rows>>,
    _observe: Subscription,
}
impl Render for ResultPane {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            DataTable::new(&self.table)
                .focus_indicator(false)
                .stripe(false)
                .bordered(false)
                .small(),
        )
    }
}
type NormalResults = (String, Option<(usize, ColumnSort)>, View);
pub struct Seek {
    pane_identity: crate::pane_layout::PaneId,
    /// Split layout; a single `Pane` leaf when the view is not split.
    layout: crate::pane_layout::Node,
    /// Panes other than the focused one, which lives in the fields below.
    panes: Vec<panes::Slot>,
    pane_motion: Option<(crate::pane_layout::PaneId, Instant)>,
    /// The focused pane fills the pane area; the others stay as they are.
    zoomed: bool,
    /// When zoom last changed, whether it zoomed in, and its generation.
    zoom_motion: Option<(Instant, bool, u64)>,
    layout_dirty: bool,
    /// Tab insertion slot under the pointer while a tab is dragged.
    tab_drop: Option<(crate::pane_layout::PaneId, usize)>,
    split_preview: Option<drag_split::SplitPreview>,
    /// The document pane holding focus during this layout pass, if any.
    focused_doc_now: Option<crate::pane_layout::PaneId>,
    /// Segmented controls sliding from an option: (from, generation).
    segment_motion: std::collections::HashMap<&'static str, (usize, u64)>,
    /// Icons view options popover (Ctrl+J) and the resize it animates.
    icon_options_open: bool,
    icon_options_motion: u64,
    icon_size_slider: Entity<gpui_component::slider::SliderState>,
    icon_spacing_slider: Entity<gpui_component::slider::SliderState>,
    /// (pane id, size before, generation) while a size change animates.
    icon_resize: Option<(u64, u16, u64)>,
    icon_resize_generation: u64,
    icon_resize_clear: Task<()>,
    split_motion: u64,
    columns: columns::State,
    sidebar_state: sidebar_view::State,
    /// Document panes (3.5) and which reader is in the shared reader fields.
    doc_panes: Vec<doc_pane::DocPane>,
    reader_slot: doc_pane::ReaderTarget,
    close_document: Option<crate::pane_layout::PaneId>,
    /// Column-view widths for the focused tab.
    column_widths: Vec<f32>,
    /// Related folders of the focused tab (3.6).
    related: Vec<related::Related>,
    related_visible: bool,
    related_edit: Option<related::ChipEdit>,
    session_store: Option<crate::pane_session::Store>,
    tabs: Vec<tabs::Tab>,
    active_tab: usize,
    next_tab_id: usize,
    restoring_input: Option<String>,
    pending_tab_restore: Option<tabs::Tab>,
    tab_motion: u64,
    closing_tab: Option<tabs::Closing>,
    motion_enabled: bool,
    tab_motion_started: std::time::Instant,
    backend: Backend,
    input: Entity<InputState>,
    table: Entity<TableState<Rows>>,
    result_pane: Entity<ResultPane>,
    status: Status,
    scope: Option<PathBuf>,
    favorites: Vec<(String, PathBuf)>,
    roots: Vec<PathBuf>,
    places: places_view::State,
    command_form: saved_commands_view::Form,
    command_run: Option<saved_commands_view::RunState>,
    command_run_revision: u64,
    command_run_focus: FocusHandle,
    /// A files drag resting on another tab switches to it after the delay.
    tab_hover: Option<usize>,
    tab_hover_task: Task<()>,
    /// Image tools confirm dialog (8.3).
    image_job: Option<image_tools_view::ImageJob>,
    image_job_focus: FocusHandle,
    drag_hover: Option<PathBuf>,
    drag_hover_task: Task<()>,
    show_hidden: bool,
    sidebar: bool,
    sidebar_from: f32,
    sidebar_width: Arc<std::sync::atomic::AtomicU32>,
    sidebar_motion: u64,
    message: Option<String>,
    history: Vec<(String, Option<PathBuf>)>,
    history_position: usize,
    subscriptions: Vec<Subscription>,
    command_palette: Option<command_palette::State>,
    settings: bool,
    settings_tab: usize,
    settings_focus: FocusHandle,
    /// Holds focus on the Storage and Worktrees pages, where no control
    /// takes it, so Ctrl+K and other shortcuts still reach Seek.
    page_focus: FocusHandle,
    settings_search: Entity<InputState>,
    opacity_sliders: [Entity<gpui_component::slider::SliderState>; SLIDERS],
    opacity_focus: [FocusHandle; SLIDERS],
    settings_open_motion: u64,
    settings_page_motion: u64,
    helper: crate::backend::HelperState,
    preferences: Settings,
    store: Store,
    atlas: Atlas,
    media: media::Worker,
    cache: Arc<Mutex<media::Cache>>,
    visible: Arc<Mutex<std::collections::HashSet<media::Key>>>,
    extra: Arc<Mutex<std::collections::HashSet<media::Key>>>,
    view: View,
    panel: bool,
    info_motion: usize,
    info_closing: bool,
    info_from: f32,
    info_progress: Arc<std::sync::atomic::AtomicU32>,
    info_close_task: Task<()>,
    preview_enlarged: bool,
    quick_look_row: Option<Row>,
    /// Where focus goes when Quick Look closes (a document pane), if not the list.
    quick_look_return: Option<FocusHandle>,
    /// Hidden sidebar Locations are listed for now.
    show_hidden_locations: bool,
    /// Storage overview (Phase 9.1), shown in place of the file list.
    storage_page: Option<storage_view::StoragePage>,
    cleanup: cleanup_view::Cleanup,
    duplicates: duplicates_view::Duplicates,
    similar: similar_view::Similar,
    /// Git worktrees (Phase 9.4), shown in place of the file list.
    worktrees_page: Option<worktrees_view::WorktreesPage>,
    /// Settings > Agents (Phase 10.5).
    agents: agents_view::AgentsPage,
    quick_look_motion: usize,
    quick_look_zoom: Option<f32>,
    quick_look_pan_offset: Point<Pixels>,
    quick_look_pan_limits: std::cell::Cell<Point<Pixels>>,
    quick_look_viewport: std::rc::Rc<std::cell::Cell<Option<(f32, f32)>>>,
    quick_look_pan: Option<Point<Pixels>>,
    quick_look_closing: bool,
    quick_look_close_task: Task<()>,
    preview_focus: FocusHandle,
    all_offset: usize,
    all_total: Option<usize>,
    preview_watchdog: Task<()>,
    entered: Instant,
    background: Option<Arc<RenderImage>>,
    background_request: Option<(PathBuf, crate::background::Effect)>,
    /// The glass grain tile, made once.
    grain: Arc<RenderImage>,
    background_task: Task<()>,
    info_resizing: bool,
    inspector: inspector_view::State,
    share: Option<crate::native_share::Session>,
    text_reader: text_reader::State,
    documents: crate::document_preview::Worker,
    document_key: Option<crate::document_preview::Key>,
    document_pages: Option<u32>,
    document: Option<Result<crate::document_preview::Preview, String>>,
    document_images_to_drop: Vec<Arc<RenderImage>>,
    sort_open: bool,
    grid_scroll: UniformListScrollHandle,
    grid_columns: usize,
    custom_locations: bool,
    locations_input: Entity<InputState>,
    exclusions_input: Entity<InputState>,
    cache_epoch: u64,
    applied_view: Option<AppliedView>,
    files: crate::file_ops::Worker,
    /// Who sent each operation still in the file worker, oldest first: an
    /// agent's job id, or `None` for the person's own (Phase 10).
    op_owners: std::collections::VecDeque<Option<String>>,
    /// The agent job the next `run_file_op` belongs to.
    next_op_owner: Option<String>,
    /// An agent's undo waiting for the worker to be idle and the undo
    /// history to be loaded, and whether the history was reloaded for it.
    pending_agent_undo: Option<String>,
    agent_undo_refreshed: bool,
    operation_history: Result<crate::operation_journal::Snapshot, String>,
    history_open: bool,
    history_loading: bool,
    files_pending: usize,
    history_focus: FocusHandle,
    history_previous: Option<FocusHandle>,
    edit: Option<daily::Edit>,
    permanent_delete: Option<permanent_delete::Confirmation>,
    organize: Option<organize_view::Review>,
    organizing: bool,
    toast: Option<organize_view::Toast>,
    largest: Option<bool>,
    normal_results: Option<NormalResults>,
    suggestions: Option<Entity<gpui_component::menu::PopupMenu>>,
    suggestion_dismiss: Option<Subscription>,
    suggestion_origin: Point<Pixels>,
    suggestion_width: Pixels,
    suggestions_at_search: bool,
    content_mode: bool,
    live_folders: live_folders::LiveFolders,
    content_failures: Vec<crate::content::Failure>,
    content_locations_input: Entity<InputState>,
    app_updates: crate::updater::State,
}
impl Seek {
    pub fn new(startup: Startup, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let Startup {
            session,
            backend,
            updates,
            remembered,
            preferences,
            store,
            atlas,
            shell_atlas_rx,
            media,
            media_updates,
            open_requests,
        } = startup;
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(target) = open_requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        // A file ("Show in Seek"): its folder, showing it.
                        if target.is_file()
                            && let Some(folder) = target.parent()
                        {
                            let name = target
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            this.new_tab(Some(folder.to_path_buf()), window, cx);
                            this.input.update(cx, |input, cx| {
                                input.set_value(format!("\"{name}\""), window, cx)
                            });
                            this.submit(cx);
                        } else {
                            this.new_tab(Some(target), window, cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        crate::metrics::phase("shell_constructing");
        let owner = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            owner
                .update(cx, |this, cx| {
                    if this.text_reader_blocks_navigation(cx) || this.documents_block_close(cx) {
                        return false;
                    }
                    this.remember_session(cx);
                    true
                })
                .unwrap_or(true)
        });
        // Agents (Phase 10): the pipe for `seek.exe --mcp`; their changes
        // arrive here and run through the same file worker as the person's.
        *crate::mcp::owner::POLICY.write().unwrap() = crate::mcp::owner::Policy {
            can_change: preferences.agents_can_change,
            folders: preferences.agent_folders.clone(),
        };
        if let Some(agent_requests) = crate::mcp::owner::start(store.directory.as_ref().clone()) {
            cx.spawn_in(window, async move |this, cx| {
                while let Ok(request) = agent_requests.recv().await {
                    let alive = this.update_in(cx, |this, window, cx| this.run_agent_request(request, window, cx));
                    if alive.is_err() {
                        break;
                    }
                }
            })
            .detach();
        }
        let (documents, document_replies) = crate::document_preview::Worker::start();
        let (files, file_replies) =
            crate::file_ops::Worker::start_with_history(store.directory.join("operations.jsonl"));
        let command_form = saved_commands_view::Form::new(window, cx);
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .context_menu(false)
                .placeholder("Search files and folders")
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        let cache = Arc::new(Mutex::new(media::Cache::default()));
        let visible = Arc::new(Mutex::new(std::collections::HashSet::new()));
        let extra = Arc::new(Mutex::new(std::collections::HashSet::new()));
        let settings_search = cx.new(|cx| {
            InputState::new(window, cx)
                .context_menu(false)
                .placeholder("Find a setting")
        });
        let icon_slider = |min: f32, max: f32, value: f32, cx: &mut Context<Self>| {
            cx.new(|_| {
                gpui_component::slider::SliderState::new()
                    // Max first: the default max (100) is below some minimums.
                    .max(max)
                    .min(min)
                    .step(4.)
                    .default_value(value)
            })
        };
        let icon_size_slider = icon_slider(
            crate::icon_view::SIZE_MIN as f32,
            crate::icon_view::SIZE_MAX as f32,
            96.,
            cx,
        );
        let icon_spacing_slider =
            icon_slider(0., crate::icon_view::SPACING_MAX as f32, 8., cx);
        let opacity_sliders = std::array::from_fn::<_, SLIDERS, _>(|index| {
            let (min, max) = slider_range(index);
            (min, max, slider_value(&preferences, index) as f32)
        })
        .map(|(min, max, value)| {
            cx.new(|_| {
                gpui_component::slider::SliderState::new()
                    // Max first: the default max (100) is below some minimums.
                    .max(max)
                    .min(min)
                    .step(1.)
                    .default_value(value)
            })
        });
        let custom_locations = !preferences.indexed_locations.is_empty();
        let path_text = |paths: &[PathBuf]| {
            paths
                .iter()
                .map(|p| p.to_string_lossy())
                .collect::<Vec<_>>()
                .join("; ")
        };
        let locations_input = cx.new(|cx| {
            InputState::new(window, cx)
                .context_menu(false)
                .default_value(path_text(&preferences.indexed_locations))
                .placeholder("Full folder paths")
        });
        let exclusions_input = cx.new(|cx| {
            InputState::new(window, cx)
                .context_menu(false)
                .default_value(path_text(&preferences.exclusions))
                .placeholder("C:\\path; C:\\other")
        });
        let owner = cx.entity().downgrade();
        let content_locations_input = cx.new(|cx| {
            InputState::new(window, cx)
                .context_menu(false)
                .default_value(path_text(&preferences.content_locations))
                .placeholder("Full folder paths, separated by semicolons")
        });
        let (table, result_pane) = results_table(
            Rows::new(
                "Indexing files...",
                &preferences,
                &atlas,
                &cache,
                &media,
                &visible,
                &extra,
                owner,
            ),
            window,
            cx,
        );
        let mut subscriptions = vec![
            cx.subscribe_in(&settings_search, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = this.settings_search.read(cx).value().to_lowercase();
                    if !query.trim().is_empty()
                        && let Some((index, _, _, _)) =
                            settings_view::SETTINGS_PAGES
                                .iter()
                                .find(|(_, label, _, keywords)| {
                                    format!("{label} {keywords}")
                                        .to_lowercase()
                                        .contains(query.trim())
                                })
                    {
                        if this.settings_tab != *index {
                            this.settings_page_motion = this.settings_page_motion.wrapping_add(1);
                        }
                        this.settings_tab = *index;
                    }
                    cx.notify();
                }
            }),
            cx.observe_window_appearance(window, |this, window, cx| {
                if this.preferences.theme == Appearance::System {
                    crate::appearance::apply(&this.preferences, Some(window), cx);
                    cx.notify();
                }
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() && this.quick_look_pan.take().is_some() {
                    cx.notify();
                }
                if window.is_window_active() {
                    if crate::appearance::refresh_system_glass() {
                        crate::appearance::apply(&this.preferences, Some(window), cx);
                    }
                    this.motion_enabled = crate::instance::motion_enabled();
                    this.table.update(cx, |_, cx| cx.notify());
                    cx.notify();
                }
            }),
            cx.observe_window_bounds(window, move |this, window, cx| {
                this.table.update(cx, |_, cx| cx.notify());
                if !this.suggestions_at_search {
                    this.suggestions = None;
                }
                cx.notify();
                *remembered.lock().unwrap() = window.window_bounds();
            }),
            cx.subscribe_in(&input, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    let _profile = crate::metrics::span("input_change");
                    if this
                        .restoring_input
                        .take()
                        .is_some_and(|text| text == this.input.read(cx).value().as_str())
                    {
                        return;
                    }
                    this.all_offset = 0;
                    this.all_total = None;
                    this.pending_tab_restore = None;
                    if !this.input.read(cx).value().trim().is_empty() {
                        this.largest = None;
                    }
                    this.table.update(cx, |table, _| {
                        table.delegate_mut().expanded.clear();
                        table.delegate_mut().children.clear();
                    });
                    this.decorate_filters(window, cx);
                    this.refresh_suggestions(window, cx);
                    this.submit(cx);
                } else if let InputEvent::PressEnter { secondary, .. } = event {
                    if this.open_address(window, cx) {
                        return;
                    }
                    this.open_selected(*secondary, window, cx);
                    this.remember(cx);
                }
                let _ = window;
            }),
            cx.subscribe_in(&table, window, Self::results_event),
        ];
        subscriptions.push(cx.observe(&icon_size_slider, |this, slider, cx| {
            let value = slider.read(cx).value().end().round() as u16;
            this.set_icon_view(|o| o.size = value, cx);
        }));
        subscriptions.push(cx.observe(&icon_spacing_slider, |this, slider, cx| {
            let value = slider.read(cx).value().end().round() as u16;
            this.set_icon_view(|o| o.spacing = value, cx);
        }));
        for (index, slider) in opacity_sliders.iter().enumerate() {
            subscriptions.push(cx.observe(slider, move |this, slider, cx| {
                let value = slider.read(cx).value().end().round() as u16;
                if slider_value(&this.preferences, index) == value {
                    return;
                }
                set_slider(&mut this.preferences, index, value);
                this.store.save(&this.preferences);
                this.table.update(cx, |table, cx| {
                    table.delegate_mut().preferences = this.preferences.clone();
                    cx.notify();
                });
                cx.notify();
            }));
        }
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(update) = updates.recv().await {
                if this
                    .update_in(cx, |this, window, cx| this.apply(update, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(atlas) = shell_atlas_rx.recv().await {
                let _ = this.update_in(cx, |this, _, cx| {
                    this.atlas = atlas.clone();
                    this.table.update(cx, |table, cx| {
                        table.delegate_mut().atlas = atlas;
                        cx.notify();
                    });
                    cx.notify();
                });
            }
        })
        .detach();
        cx.spawn_in(window, async move |this,cx| {
            while let Ok(reply)=document_replies.recv().await {
                if this.update_in(cx,|this,window,cx| {
                    if !this.documents.accepts(&reply) {return;}
                    this.preview_watchdog=Task::ready(());
                    #[cfg(feature="benchmarks")]
                    crate::metrics::interaction("document_preview",serde_json::json!({"name":reply.key.file.path.file_name().map(|n|n.to_string_lossy()),"page":reply.key.page,"elapsed_ms":reply.elapsed_ms,"success":reply.result.is_ok(),"source":reply.result.as_ref().ok().map(|p|&p.info.source),"pages":reply.result.as_ref().ok().map(|p|p.info.pages),"dimensions":reply.result.as_ref().ok().map(|p|(p.info.width,p.info.height)),"sheet_rows":reply.result.as_ref().ok().and_then(|p|p.info.sheet.as_ref().map(|s|s.rows.len()))}));
                    if let Some(Ok(old))=this.document.take() && let Some(image)=old.image {let _=window.drop_image(image);}
                    if let Ok(preview)=&reply.result {this.document_pages=(preview.info.pages>0).then_some(preview.info.pages);}
                    this.document=Some(reply.result);cx.notify();
                }).is_err(){break;}
            }
        }).detach();
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(reply) = media_updates.recv().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        if !this.media.accepts(&reply.key, reply.epoch) {
                            return;
                        }
                        #[cfg(feature = "benchmarks")]
                        crate::metrics::interaction("preview_ready", serde_json::json!({"name":reply.key.path.file_name().map(|n|n.to_string_lossy()),"edge":reply.key.edge,"details":reply.key.details,"image":matches!(&reply.value,media::Value::Image(Some(_)))}));
                        let removed = this.cache.lock().unwrap().insert(reply.key, reply.value);
                        #[cfg(feature="benchmarks")]
                        crate::metrics::interaction("preview_cache",serde_json::json!({"stats":this.cache.lock().unwrap().stats()}));
                        for image in removed {
                            let _ = window.drop_image(image);
                        }
                        this.table.update(cx, |_, cx| cx.notify());
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(mut reply) = file_replies.recv().await {
                // Coalesce already available callbacks into one table update;
                // large selections must not sort/repaint once per deleted file.
                while !reply.finished {
                    let Ok(next) = file_replies.try_recv() else { break; };
                    reply.removed.extend(next.removed);
                    reply.refresh.extend(next.refresh);
                    if next.result.is_err() { reply.result = next.result; }
                    if next.progress.is_some() { reply.progress = next.progress; }
                    if next.history.is_some() { reply.history = next.history; }
                    reply.finished = next.finished;
                }
                reply.removed.sort();reply.removed.dedup();
                reply.refresh.sort();reply.refresh.dedup();
                if this
                    .update_in(cx, |this, window, cx| {
                        #[cfg(feature="benchmarks")]
                        crate::metrics::interaction("file_operation",serde_json::json!({"success":reply.result.is_ok(),"error":reply.result.as_ref().err(),"refresh_count":reply.refresh.len()}));
                        if !reply.removed.is_empty() {
                            this.remove_confirmed_files(&reply.removed,cx);
                            this.columns.invalidate(&reply.removed);
                            this.refresh_inactive_panes(&[],&reply.removed,cx);
                            let _=this.backend.tx.send(Command::FilesRemoved(reply.removed));
                            this.submit(cx); // Cancel earlier replies before stale indexed rows can return.
                        }
                        if let Some((done,total))=reply.progress {this.message=Some(format!("Updating files: {done} of {total}"));}
                        if let Some(history)=reply.history {this.operation_history=history;this.history_loading=false;}
                        if reply.finished {
                            if this.edit.as_ref().is_some_and(|edit|edit.pending){
                                if let Some(error)=reply.result.as_ref().err(){
                                    if let Some(edit)=&mut this.edit {edit.pending=false;edit.error=Some(error.clone());edit.input.update(cx,|input,cx|input.focus(window,cx));}
                                }else{this.table.read(cx).focus_handle(cx).focus(window,cx);this.edit=None;}
                            }
                            this.files_pending=this.files_pending.saturating_sub(1);
                            if let Some(Some(job))=this.op_owners.pop_front(){crate::mcp::owner::finish(&job,reply.result.clone());}
                            if this.files_pending==0 {this.try_agent_undo(window,cx);}
                            this.message=reply.result.err();
                            this.finish_organize(window,cx);
                            this.columns.invalidate(&reply.refresh);
                            this.refresh_inactive_panes(&reply.refresh,&[],cx);
                            this.refresh_capacity(cx);
                            let _ = this.backend.tx.send(Command::RefreshParents(reply.refresh));
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        crate::metrics::phase("shell_constructed");
        let check_updates = preferences.check_updates_on_startup;
        let mut this = Self {
            pane_identity: crate::pane_layout::PaneId(1),
            layout: crate::pane_layout::Node::Pane {
                id: crate::pane_layout::PaneId(1),
            },
            panes: vec![],
            pane_motion: None,
            zoomed: false,
            zoom_motion: None,
            layout_dirty: false,
            tab_drop: None,
            split_preview: None,
            focused_doc_now: None,
            segment_motion: Default::default(),
            icon_options_open: false,
            icon_options_motion: 0,
            icon_size_slider,
            icon_spacing_slider,
            icon_resize: None,
            icon_resize_generation: 0,
            icon_resize_clear: Task::ready(()),
            split_motion: 0,
            columns: Default::default(),
            sidebar_state: Default::default(),
            doc_panes: vec![],
            reader_slot: doc_pane::ReaderTarget::QuickLook,
            close_document: None,
            column_widths: vec![],
            related: vec![],
            related_visible: false,
            related_edit: None,
            session_store: None,
            tabs: vec![tabs::Tab::initial(preferences.default_view)],
            active_tab: 0,
            next_tab_id: 3,
            restoring_input: None,
            pending_tab_restore: None,
            tab_motion: 0,
            closing_tab: None,
            motion_enabled: crate::instance::motion_enabled(),
            tab_motion_started: std::time::Instant::now(),
            backend,
            input,
            table,
            result_pane,
            status: Status {
                catching_up: true,
                ..Default::default()
            },
            scope: None,
            favorites: vec![],
            roots: vec![],
            places: places_view::State::new(),
            command_form,
            command_run: None,
            command_run_revision: 0,
            command_run_focus: cx.focus_handle(),
            tab_hover: None,
            tab_hover_task: Task::ready(()),
            image_job: None,
            image_job_focus: cx.focus_handle(),
            drag_hover: None,
            drag_hover_task: Task::ready(()),
            show_hidden: preferences.show_hidden,
            sidebar: true,
            sidebar_from: 220.,
            sidebar_width: Arc::new(std::sync::atomic::AtomicU32::new(220.0_f32.to_bits())),
            sidebar_motion: 0,
            message: None,
            history: vec![(String::new(), None)],
            history_position: 0,
            subscriptions,
            command_palette: None,
            settings: false,
            settings_tab: 0,
            settings_focus: cx.focus_handle(),
            page_focus: cx.focus_handle(),
            settings_search,
            opacity_sliders,
            opacity_focus: std::array::from_fn(|_| cx.focus_handle()),
            settings_open_motion: 0,
            settings_page_motion: 0,
            helper: crate::backend::HelperState::default(),
            view: preferences.default_view,
            preferences,
            store,
            atlas,
            media,
            cache,
            visible,
            extra,
            panel: true,
            info_motion: 0,
            info_closing: false,
            info_from: 1.,
            info_progress: Arc::new(std::sync::atomic::AtomicU32::new(1f32.to_bits())),
            info_close_task: Task::ready(()),
            preview_enlarged: false,
            quick_look_row: None,
            quick_look_return: None,
            show_hidden_locations: false,
            storage_page: None,
            cleanup: Default::default(),
            duplicates: Default::default(),
            similar: Default::default(),
            worktrees_page: None,
            agents: Default::default(),
            quick_look_motion: 0,
            quick_look_zoom: None,
            quick_look_pan_offset: Point::default(),
            quick_look_pan_limits: std::cell::Cell::new(Point::default()),
            quick_look_viewport: std::rc::Rc::new(std::cell::Cell::new(None)),
            quick_look_pan: None,
            quick_look_closing: false,
            quick_look_close_task: Task::ready(()),
            preview_focus: cx.focus_handle(),
            all_offset: 0,
            all_total: None,
            preview_watchdog: Task::ready(()),
            entered: Instant::now(),
            background: None,
            background_request: None,
            grain: crate::background::grain_tile(),
            background_task: Task::ready(()),
            info_resizing: false,
            inspector: inspector_view::State::new(),
            share: None,
            text_reader: text_reader::State::new(),
            documents,
            document_key: None,
            document_pages: None,
            document: None,
            document_images_to_drop: vec![],
            sort_open: false,
            grid_scroll: UniformListScrollHandle::new(),
            grid_columns: 4,
            custom_locations,
            locations_input,
            exclusions_input,
            cache_epoch: 0,
            applied_view: None,
            files,
            operation_history: Ok(crate::operation_journal::Snapshot::default()),
            history_open: false,
            history_loading: false,
            files_pending: 0,
            op_owners: Default::default(),
            next_op_owner: None,
            pending_agent_undo: None,
            agent_undo_refreshed: false,
            history_focus: cx.focus_handle(),
            history_previous: None,
            edit: None,
            permanent_delete: None,
            organize: None,
            organizing: false,
            toast: None,
            largest: None,
            normal_results: None,
            suggestions: None,
            suggestion_dismiss: None,
            suggestion_origin: Point::default(),
            suggestions_at_search: false,
            suggestion_width: px(420.),
            content_mode: false,
            live_folders: Default::default(),
            content_failures: vec![],
            content_locations_input,
            app_updates: crate::updater::State::default(),
        };
        match session {
            Ok(saved) => {
                // With "Reopen windows as they were" off, Seek starts with
                // one pane; the layout is still remembered for turning it on.
                let restored = saved
                    .filter(|_| this.preferences.restore_layout)
                    .map(|saved| this.restore_saved_tabs(saved, window, cx))
                    .transpose();
                match restored {
                    Ok(_) => {
                        this.session_store = Some(crate::pane_session::Store::new(
                            this.store.directory.as_ref().clone(),
                            cx,
                        ));
                    }
                    Err(error) => eprintln!("Pane session preserved without replacement: {error}"),
                }
            }
            Err(error) => eprintln!("Pane session preserved without replacement: {error}"),
        }
        this.refresh_background(cx);
        #[cfg(debug_assertions)]
        if let Some(release) = crate::updater::preview() {
            this.app_updates.available = Some(release);
        }
        // Find the coding agents in the background now, so Settings > Agents
        // is complete the first time it opens.
        {
            let owner = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = owner.update(cx, |this, cx| this.prefetch_agents(cx));
            });
        }
        if check_updates {
            let owner = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = owner.update(cx, |this, cx| this.check_for_updates(cx));
            });
            // Seek often stays open for days: check again every six hours
            // while the setting is on and nothing is already found or staged.
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(std::time::Duration::from_secs(6 * 60 * 60)).await;
                    let Ok(()) = this.update(cx, |this, cx| {
                        if this.preferences.check_updates_on_startup
                            && this.app_updates.available.is_none()
                            && this.app_updates.staged.is_none()
                        {
                            this.check_for_updates(cx);
                        }
                    }) else {
                        return;
                    };
                }
            })
            .detach();
        }
        this.start_live_folders(cx);
        this
    }
    fn submit(&mut self, cx: &mut Context<Self>) {
        self.remember_session(cx);
        self.content_failures.clear();
        let text = self.input.read(cx).value().to_string();
        if crate::location::address(&text).is_some() {
            // Cancel any previous scan, but do not send this address to either
            // filename or document-content search while the user is typing.
            self.backend.generation.fetch_add(1, Ordering::Relaxed);
            self.message = Some("Press Enter to open this folder".into());
            self.table.update(cx, |table, cx| {
                let data = table.delegate_mut();
                data.rows = Arc::new(vec![]);
                data.base_rows = Arc::new(vec![]);
                data.empty_message = "Press Enter to open this folder".into();
                table.refresh(cx);
            });
            cx.notify();
            return;
        }
        self.message = self.content_mode.then(|| "Searching contents?".into());
        let (started, native_input) = crate::metrics::input_start();
        let generation = self.backend.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.backend.tx.send(Command::Search(Request {
            all_offset: self.all_offset,
            text: self.input.read(cx).value().to_string(),
            scope: self.scope.clone(),
            show_hidden: self.show_hidden,
            exclusions: self.preferences.exclusions.clone(),
            expanded: self
                .table
                .read(cx)
                .delegate()
                .expanded
                .iter()
                .cloned()
                .collect(),
            generation,
            started: Some(started),
            native_input,
            largest: self.largest,
            content: self.content_mode,
            content_locations: self.preferences.content_locations.clone(),
            remember_document_text: self.preferences.remember_document_text,
        }));
    }
    fn decorate_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.content_mode {
            return;
        }
        let text = self.input.read(cx).value().to_string();
        if !text.ends_with(char::is_whitespace) {
            return;
        }
        let mut content = InputContent::new(text.clone());
        for (range, word) in seek_engine::query::query_words(&text) {
            let filter = word.as_str();
            if let Some((key, value)) = filter.split_once(':')
                && [
                    "ext", "type", "kind", "in", "size", "mtime", "path", "limit",
                ]
                .contains(&key)
                && !value.is_empty()
                && let Ok(next) = content.clone().with_token(
                    range.clone(),
                    InlineToken::new(filter.to_string(), text[range].to_string()).with_label(
                        if ["type", "kind", "ext"].contains(&key) {
                            crate::kinds::filter_label(filter)
                        } else {
                            format!("{key}: {value}")
                        },
                    ),
                )
            {
                content = next;
            }
        }
        self.input
            .update(cx, |input, cx| input.decorate_existing_text(content, cx));
        let _ = window;
    }
    fn apply(&mut self, update: Update, _window: &mut Window, cx: &mut Context<Self>) {
        let _profile = crate::metrics::span("apply_results");
        if update.generation != self.backend.generation.load(Ordering::Relaxed) {
            return;
        }
        if self.message.as_deref() == Some("Rebuilding the current indexed locations…")
            && !update.status.catching_up
            && update.status.catch_up_ms != self.status.catch_up_ms
        {
            self.message = None;
        }
        self.status = update.status;
        // Fetch the icons of file types these rows bring, once per type, so
        // they appear with the rows instead of file by file while scrolling.
        let types = crate::icons::unfetched_types(
            update
                .rows
                .iter()
                .chain(update.children.values().flatten())
                .map(|row| (row.file.path.as_path(), row.file.kind & 3 == seek_engine::walk::KIND_DIR)),
        );
        if !types.is_empty() {
            let job = cx.background_executor().spawn(async move { crate::icons::fetch_types(types) });
            cx.spawn(async move |this, cx| {
                job.await;
                let _ = this.update(cx, |this, cx| {
                    this.table.update(cx, |_, cx| cx.notify());
                    cx.notify();
                });
            })
            .detach();
        }
        // Progress-only replies do not replace rows or their matching page count.
        if !update.progress_only {
            self.all_total = update.all_total;
        }
        self.content_failures = update.content_failures;
        // Revalidate visible files on the worker only when the rows change
        // (below); unchanged rows keep their icons and thumbnails.
        let epoch_changed = self.cache_epoch != update.cache_epoch;
        self.cache_epoch = update.cache_epoch;
        self.helper = update.helper;
        self.favorites = update.favorites;
        self.roots = update.roots;
        self.refresh_places(cx);
        self.refresh_command_palette(cx);
        if update.message.is_some() {
            self.message = update.message.clone();
        }
        if update.progress_only {
            if epoch_changed {
                self.media.force();
            }
            cx.notify();
            return;
        }
        let text = self.input.read(cx).value().to_string();
        crate::metrics::applied(
            update.generation,
            update.started,
            update.native_input,
            &text,
            &update.rows,
            &self.status,
            _window.is_a11y_active(),
        );
        let view = (
            text.clone(),
            self.scope.clone(),
            self.show_hidden,
            self.preferences.exclusions.clone(),
            self.largest,
            self.content_mode,
        );
        let same_view = self.applied_view.as_ref() == Some(&view);
        self.applied_view = Some(view);
        // A refresh that returns the rows already shown (common while the index
        // catches up) only updates the status: no table rebuild, no resort.
        if same_view
            && update.children.is_empty()
            && self.table.read(cx).delegate().children.is_empty()
            && Row::same_rows(&self.table.read(cx).delegate().base_rows, &update.rows)
        {
            if self.table.read(cx).delegate().empty_message.as_str()
                != update.message.as_deref().unwrap_or("No matching files")
            {
                let message = update
                    .message
                    .clone()
                    .unwrap_or_else(|| "No matching files".into());
                self.table
                    .update(cx, |table, _| table.delegate_mut().empty_message = message);
            }
            cx.notify();
            return;
        }
        if epoch_changed {
            self.media.force();
        }
        let selected = self.selected(cx).map(|row| row.file.path);
        let grid_height =
            interface::grid_row_height(self.view, self.icon_view_for(self.pane_identity));
        let grid_top = scroll_top(&self.grid_scroll, grid_height);
        let grid_anchor = (same_view && grid_top > 0)
            .then(|| {
                self.table
                    .read(cx)
                    .delegate()
                    .rows
                    .get(grid_top * self.grid_columns)
                    .map(|r| r.file.path.clone())
            })
            .flatten();
        self.table.update(cx, |table, cx| {
            let row_height = gpui_component::Size::Small.table_row_height();
            let top = scroll_top(&table.vertical_scroll_handle, row_height);
            // Scrolled down, the rows in view stay put when items are added
            // above; at the top, the list stays at the top, so something new
            // (a finished download) is seen.
            let anchor = (same_view && top > 0)
                .then(|| table.delegate().rows.get(top).map(|r| r.file.path.clone()))
                .flatten();
            let pending = table
                .vertical_scroll_handle
                .0
                .borrow()
                .deferred_scroll_to_item;
            table.delegate_mut().empty_message = update
                .message
                .clone()
                .unwrap_or_else(|| "No matching files".into());
            table.delegate_mut().base_rows = Arc::new(update.rows);
            table.delegate_mut().children = update.children;
            table.delegate_mut().query = text;
            table.delegate_mut().content = self.content_mode;
            table.delegate_mut().sorted();
            {
                let data = table.delegate_mut();
                if !data.selected_many.is_empty() {
                    let existing = data
                        .rows
                        .iter()
                        .map(|r| r.file.path.as_path())
                        .collect::<std::collections::HashSet<_>>();
                    data.selected_many
                        .retain(|p| existing.contains(p.as_path()));
                }
            }
            table.delegate().refresh_media();
            let selected = selected.and_then(|path| {
                table
                    .delegate()
                    .rows
                    .iter()
                    .position(|row| row.file.path == path)
            });
            if selected != table.selected_row() {
                if let Some(index) = selected {
                    table.set_selected_row(index, cx);
                } else {
                    table.clear_selection(cx);
                }
            }
            // Restoring selection after a background update must not reveal it
            // again; the user may deliberately be scrolling somewhere else.
            table
                .vertical_scroll_handle
                .0
                .borrow_mut()
                .deferred_scroll_to_item = pending;
            if let Some(path) = anchor
                && let Some(index) = table
                    .delegate()
                    .rows
                    .iter()
                    .position(|r| r.file.path == path)
            {
                keep_scroll_anchor(&table.vertical_scroll_handle, top, index, row_height);
            }
            if !same_view {
                table.scroll_to_row(0, cx);
            }
            table.refresh(cx);
        });
        if let Some(path) = grid_anchor
            && let Some(index) = self
                .table
                .read(cx)
                .delegate()
                .rows
                .iter()
                .position(|r| r.file.path == path)
        {
            keep_scroll_anchor(
                &self.grid_scroll,
                grid_top,
                index / self.grid_columns,
                grid_height,
            );
        }
        if !same_view {
            self.grid_scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        self.restore_tab_viewport(cx);
        self.refresh_preview(cx);
        #[cfg(feature = "benchmarks")]
        crate::metrics::interaction(
            "tree_rows",
            serde_json::json!({"rows":self.table.read(cx).delegate().rows.iter().map(|r|&r.name).collect::<Vec<_>>(),"depths":self.table.read(cx).delegate().depths,"expanded":self.table.read(cx).delegate().expanded.len()}),
        );
        cx.notify();
    }
    fn selected(&self, cx: &App) -> Option<Row> {
        // The index may temporarily omit a file during atomic replacement.
        // Keep Quick Look identity independent of the browser's changing row indexes.
        if self.preview_enlarged
            && let Some(row) = &self.quick_look_row
        {
            return Some(row.clone());
        }
        let table = self.table.read(cx);
        table.delegate().rows.get(table.selected_row()?).cloned()
    }
    fn open_selected(&mut self, reveal: bool, window: &mut Window, cx: &mut Context<Self>) {
        let row = self
            .selected(cx)
            .or_else(|| self.table.read(cx).delegate().rows.first().cloned());
        if let Some(row) = row {
            let path = row.file.path;
            let zip_file = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
            if !reveal && (row.file.kind & 3 == seek_engine::walk::KIND_DIR || zip_file) {
                // ZIP files open like folders, read-only.
                self.scope_to(Some(path), window, cx);
            } else if let Some((zip, _)) = crate::archive::split(&path).filter(|(_, i)| !i.is_empty()) {
                if reveal {
                    let _ = self.backend.tx.send(Command::Open(zip, true));
                } else {
                    // Opening a file inside a ZIP extracts just that file to
                    // a private temporary folder first.
                    let job = cx
                        .background_executor()
                        .spawn(async move { crate::archive::extract_one(&path) });
                    cx.spawn(async move |this, cx| {
                        let result = job.await;
                        let _ = this.update(cx, |this, cx| match result {
                            Ok(copy) => {
                                let _ = this.backend.tx.send(Command::Open(copy, false));
                            }
                            Err(error) => {
                                this.message = Some(error);
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                }
            } else {
                let _ = self.backend.tx.send(Command::Open(path, reveal));
            }
        }
    }
    fn remember(&mut self, cx: &App) {
        let next = (self.input.read(cx).value().to_string(), self.scope.clone());
        let search = crate::settings::Search {
            query: next.0.clone(),
            scope: next.1.clone(),
            content: self.content_mode,
        };
        if !search.query.trim().is_empty()
            && self.preferences.recent_searches.first() != Some(&search)
        {
            crate::settings::retain_search(&mut self.preferences.recent_searches, search, 16);
            self.store.save(&self.preferences);
        }
        if self.history.get(self.history_position) != Some(&next) {
            self.history.truncate(self.history_position + 1);
            self.history.push(next);
            if self.history.len() > 32 {
                self.history.remove(0);
            }
            self.history_position = self.history.len() - 1;
        }
    }
    fn scope_to(&mut self, scope: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        self.close_storage();
        self.close_worktrees();
        // Keep the current search as a history entry before clearing it.
        self.remember(cx);
        self.all_offset = 0;
        self.all_total = None;
        self.pending_tab_restore = None;
        self.content_mode = false;
        self.largest = None;
        self.suggestions = None;
        let loading = if scope.is_some() { "Indexing this folder..." } else { "Loading all locations..." };
        self.message = Some(loading.into());
        self.table.update(cx, |table, _| {
            table.delegate_mut().expanded.clear();
            table.delegate_mut().children.clear();
        });
        self.table.update(cx, |table, cx| {
            let rows = table.delegate_mut();
            rows.base_rows = Arc::new(vec![]);
            rows.rows = Arc::new(vec![]);
            rows.empty_message = loading.into();
            table.refresh(cx);
        });
        self.scope = scope;
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.submit(cx);
        self.remember(cx);
        cx.notify();
    }
    fn open_address(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let text = self.input.read(cx).value().to_string();
        let Some(result) = crate::location::address(&text) else {
            return false;
        };
        match result {
            Ok(path) if path.is_dir() => self.scope_to(Some(path), window, cx),
            // A file: its folder, showing just that file.
            Ok(path) if path.is_file() && path.parent().is_some() => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.scope_to(path.parent().map(std::path::Path::to_path_buf), window, cx);
                self.input.update(cx, |input, cx| input.set_value(format!("\"{name}\""), window, cx));
                self.submit(cx);
            }
            Ok(path) => {
                self.message = Some(format!("{} doesn't exist", path.display()));
                cx.notify();
            }
            Err(error) => {
                self.message = Some(error);
                cx.notify();
            }
        }
        true
    }

    /// Ctrl+V (or Paste) into the search box with an address on the
    /// clipboard that names an existing folder or file opens it at once;
    /// anything else pastes as usual. Only a real paste does this, never text
    /// Seek restores itself when switching tabs, panes or history.
    pub(super) fn paste_address(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.input.read(cx).focus_handle(cx).is_focused(window) {
            return false;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return false;
        };
        match crate::location::address(&text) {
            Some(Ok(path)) if path.exists() => {
                let text = text.trim().to_string();
                self.input.update(cx, |input, cx| input.set_value(text, window, cx));
                self.open_address(window, cx)
            }
            _ => false,
        }
    }
    fn history(&mut self, direction: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.history_position = self
            .history_position
            .saturating_add_signed(direction)
            .min(self.history.len() - 1);
        let (text, scope) = self.history[self.history_position].clone();
        self.table.update(cx, |table, _| {
            table.delegate_mut().expanded.clear();
            table.delegate_mut().children.clear();
        });
        self.scope = scope;
        self.input
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.submit(cx);
        cx.notify();
    }
    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.permanent_delete.is_some() {
            self.cancel_permanent_delete(window, cx);
            cx.stop_propagation();
            return;
        }
        if self.suggestions.take().is_some() {
            self.input.update(cx, |input, cx| input.focus(window, cx));
            cx.notify();
            return;
        }
        if self.settings {
            self.settings = false;
            self.input.update(cx, |input, cx| input.focus(window, cx));
            cx.notify();
            return;
        }
        if !self.input.read(cx).value().is_empty() {
            self.input
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.submit(cx);
        } else {
            self.table.update(cx, |table, cx| table.clear_selection(cx));
            #[cfg(feature = "benchmarks")]
            crate::metrics::interaction("selection_cleared", serde_json::json!({}));
        }
        cx.notify();
        cx.stop_propagation();
    }
    fn button_keys(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_open || self.command_palette.is_some() {
            return;
        }
        if self.preview_enlarged && self.text_reader_focused(window, cx) {
            return;
        }
        if !matches!(event.keystroke.key.as_str(), "left" | "right")
            || event.keystroke.modifiers.control
            || event.keystroke.modifiers.alt
        {
            return;
        }
        if self.permanent_delete.is_some()
            || self.organize.is_some()
            || gpui_component::WindowExt::has_active_dialog(window, cx)
        {
            return;
        }
        if gpui_component::WindowExt::focused_input(window, cx).is_some() {
            return;
        }
        if self.edit.as_ref().is_some_and(|e| {
            e.input
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx)
                || e.batch.as_ref().is_some_and(|batch| {
                    batch
                        .second
                        .read(cx)
                        .focus_handle(cx)
                        .contains_focused(window, cx)
                })
        }) {
            return;
        }
        if self
            .input
            .read(cx)
            .focus_handle(cx)
            .contains_focused(window, cx)
            || self
                .table
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx)
        {
            return;
        }
        if [
            &self.settings_search,
            &self.locations_input,
            &self.exclusions_input,
            &self.content_locations_input,
        ]
        .iter()
        .any(|i| i.read(cx).focus_handle(cx).contains_focused(window, cx))
        {
            return;
        }
        for _ in 0..128 {
            if event.keystroke.key == "left" {
                window.focus_prev(cx);
            } else {
                window.focus_next(cx);
            }
            if !self.settings || self.settings_focus.contains_focused(window, cx) {
                break;
            }
        }
        cx.stop_propagation();
        cx.notify();
    }
    pub(super) fn escape_action(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.cancel_group_edit(window, cx) || self.cancel_chip_edit(cx) {
            return true;
        }
        if self.command_run.is_some() {
            self.close_command_result(window, cx);
            return true;
        }
        if cx.has_active_drag() {
            self.clear_split_preview();
            self.tab_drop = None;
            cx.stop_active_drag(window);
            self.drag_hover = None;
            self.drag_hover_task = Task::ready(());
            cx.notify();
            return true;
        }
        use gpui_component::WindowExt;
        if self.history_open {
            self.close_operation_history(window, cx);
            return true;
        }
        if self.image_job.is_some() {
            self.close_image_tool(window, cx);
            return true;
        }
        if self.icon_options_open {
            self.icon_options_open = false;
            cx.notify();
            return true;
        }
        if self.command_palette.is_some() {
            self.close_command_palette(window, cx);
            return true;
        }
        if self.permanent_delete.is_some() {
            self.cancel_permanent_delete(window, cx);
            return true;
        }
        if self.organize.is_some() {
            self.close_organize(window, cx);
            return true;
        }
        if window.has_active_dialog(cx) {
            return false;
        }
        if self.edit.is_some() {
            self.cancel_file_edit(window, cx);
            return true;
        }
        if self.preview_enlarged {
            if !self.escape_text_reader(window, cx) {
                self.close_quick_look(window, cx);
            }
            return true;
        }
        if self.settings
            || self.suggestions.is_some()
            || self.input.read(cx).focus_handle(cx).is_focused(window)
            || self.table.read(cx).focus_handle(cx).is_focused(window)
        {
            self.clear(window, cx);
            return true;
        }
        false
    }
    fn keys(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::WindowExt;
        if event.keystroke.key == "escape" && self.escape_action(window, cx) {
            cx.stop_propagation();
            return;
        }
        if self.command_run.is_some() {
            if event.keystroke.key == "tab" {
                for _ in 0..128 {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    if self.command_run_focus.contains_focused(window, cx) {
                        break;
                    }
                }
                cx.stop_propagation();
            }
            return;
        }
        if self.history_open {
            self.operation_history_keys(event, window, cx);
            return;
        }
        if self.command_palette.is_some() {
            if event.keystroke.key == "tab" {
                if let Some(palette) = &self.command_palette {
                    palette
                        .state
                        .update(cx, |state, cx| state.focus(window, cx));
                }
                cx.stop_propagation();
                return;
            }
            if event.keystroke.key == "escape"
                || (event.keystroke.modifiers.control && event.keystroke.key == "k")
            {
                self.close_command_palette(window, cx);
                cx.stop_propagation();
            }
            return;
        }
        if event.keystroke.modifiers.control && event.keystroke.key == "k" {
            self.open_command_palette(window, cx);
            cx.stop_propagation();
            return;
        }
        if self.permanent_delete.is_some() {
            self.permanent_delete_key(event, window, cx);
            return;
        }
        if self.organize.is_some() {
            self.organize_key(event, window, cx);
            return;
        }
        if window.has_active_dialog(cx) {
            return;
        }
        // Space in a document pane (not while editing) opens its file in Quick Look.
        if !self.preview_enlarged
            && event.keystroke.key == "space"
            && !event.keystroke.modifiers.control
            && !event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.shift
            && let Some((row, focus)) = self.document_for_quick_look(window, cx)
        {
            self.open_quick_look_row(row, focus, window, cx);
            cx.stop_propagation();
            return;
        }
        // A focused document pane's reader handles its own keys first.
        if !self.preview_enlarged
            && let Some(id) = self.focused_document(window, cx)
            && self
                .with_reader(doc_pane::ReaderTarget::Document(id), |this| {
                    this.text_reader_key(event, window, cx) || {
                        // Arrows turn a document pane's pages instead of
                        // moving focus to another pane.
                        let step: i64 = match event.keystroke.key.as_str() {
                            "left" | "up" | "pageup" => -1,
                            "right" | "down" | "pagedown" => 1,
                            _ => 0,
                        };
                        match (step, this.document_pages, this.document_key.as_ref()) {
                            (step, Some(pages), Some(key)) if step != 0 => {
                                let next = (key.page as i64 + step).clamp(0, pages as i64 - 1);
                                if next != key.page as i64 {
                                    this.document_page(next as u32, cx);
                                }
                                true
                            }
                            _ => false,
                        }
                    }
                })
                .unwrap_or(false)
        {
            cx.stop_propagation();
            return;
        }
        if self.preview_enlarged {
            if self.text_reader_key(event, window, cx) {
                return;
            }
            if event.keystroke.modifiers.shift && self.quick_look_pan_key(&event.keystroke.key, cx)
            {
                cx.stop_propagation();
                return;
            }
            if event.keystroke.modifiers.control
                && self.quick_look_zoom_key(&event.keystroke.key, window, cx)
            {
                cx.stop_propagation();
                return;
            }
            self.enlarged_key(&event.keystroke.key, window, cx);
            if event.keystroke.modifiers.control
                || matches!(
                    event.keystroke.key.as_str(),
                    "escape" | "space" | "up" | "down" | "left" | "right" | "delete"
                )
                || event.keystroke.key == "enter"
                    && self.table.read(cx).focus_handle(cx).is_focused(window)
            {
                cx.stop_propagation();
            }
            return;
        }
        #[cfg(feature = "benchmarks")]
        crate::metrics::interaction(
            "key_route",
            serde_json::json!({"key":event.keystroke.key,"control":event.keystroke.modifiers.control,"platform":event.keystroke.modifiers.platform,"input":self.input.read(cx).focus_handle(cx).is_focused(window),"list":self.table.read(cx).focus_handle(cx).is_focused(window)}),
        );
        if let Some(edit) = self.edit.as_ref() {
            match event.keystroke.key.as_str() {
                "tab" => {
                    for _ in 0..128 {
                        if event.keystroke.modifiers.shift {
                            window.focus_prev(cx);
                        } else {
                            window.focus_next(cx);
                        }
                        if edit.root_focus.contains_focused(window, cx) {
                            break;
                        }
                    }
                    if !edit.root_focus.contains_focused(window, cx) {
                        edit.input.read(cx).focus_handle(cx).focus(window, cx);
                    }
                    cx.stop_propagation();
                }
                "left" | "right"
                    if !edit
                        .input
                        .read(cx)
                        .focus_handle(cx)
                        .contains_focused(window, cx)
                        && !edit.batch.as_ref().is_some_and(|batch| {
                            batch
                                .second
                                .read(cx)
                                .focus_handle(cx)
                                .contains_focused(window, cx)
                        }) =>
                {
                    let next = if edit.cancel_focus.is_focused(window) {
                        edit.save_focus.clone()
                    } else {
                        edit.cancel_focus.clone()
                    };
                    window.focus(&next, cx);
                    cx.stop_propagation();
                }
                "enter" => {
                    if edit.cancel_focus.is_focused(window) {
                        self.cancel_file_edit(window, cx);
                    } else if edit.save_focus.is_focused(window)
                        || edit
                            .input
                            .read(cx)
                            .focus_handle(cx)
                            .contains_focused(window, cx)
                        || edit.batch.as_ref().is_some_and(|batch| {
                            batch
                                .second
                                .read(cx)
                                .focus_handle(cx)
                                .contains_focused(window, cx)
                        })
                    {
                        self.finish_edit(window, cx);
                    } else {
                        return;
                    }
                    cx.stop_propagation();
                }
                "escape" => {
                    self.cancel_file_edit(window, cx);
                    cx.stop_propagation();
                }
                _ => {}
            }
            return;
        }
        let key = event.keystroke.key.as_str();
        if key == "f1" {
            self.open_shortcuts(window, cx);
            cx.stop_propagation();
            return;
        }
        if self.settings {
            if key == "escape" {
                self.clear(window, cx);
            } else if key == "," && event.keystroke.modifiers.control {
                self.close_settings(window, cx);
                cx.stop_propagation();
            }
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.alt && !modifiers.control && !modifiers.shift {
            if key == "0" {
                self.scope_to(None, window, cx);
                cx.stop_propagation();
                return;
            }
            if let Ok(number) = key.parse::<usize>()
                && (1..=9).contains(&number)
                && self.open_numbered(number, window, cx)
            {
                cx.stop_propagation();
                return;
            }
        }
        // Pane shortcuts are routed here like the tab shortcuts, so they work
        // whichever element (list, search field, sidebar) holds focus.
        let pane_command = match (key, modifiers.control, modifiers.shift, modifiers.alt) {
            ("\\", true, false, false) => Some(0),
            ("\\" | "|", true, true, false) => Some(1),
            ("w", true, true, false) => Some(2),
            ("enter", true, true, false) => Some(7),
            ("f6", false, shift, false) => Some(if shift { 4 } else { 3 }),
            ("f5", false, false, true) => Some(5),
            ("f6", false, false, true) => Some(6),
            _ => None,
        };
        if let Some(command) = pane_command {
            use crate::pane_layout::Axis;
            match command {
                0 => self.split_pane(Axis::Horizontal, None, window, cx),
                1 => self.split_pane(Axis::Vertical, None, window, cx),
                2 => self.close_pane(window, cx),
                3 | 4 => self.focus_adjacent_pane(command == 4, window, cx),
                5 | 6 => self.send_to_other_pane(command == 6, window, cx),
                7 => self.toggle_zoom(cx),
                _ => {}
            }
            cx.stop_propagation();
            return;
        }
        if modifiers.control && matches!(key, "t" | "w" | "tab") {
            match key {
                "t" => self.new_tab(None, window, cx),
                "w" => self.close_tab(self.active_tab, window, cx),
                _ => {
                    let next = tabs::adjacent(self.active_tab, self.tabs.len(), modifiers.shift);
                    self.switch_tab(next, window, cx);
                }
            }
            cx.stop_propagation();
            return;
        }
        if modifiers.control && key == "enter" {
            let folder = self
                .selected(cx)
                .filter(|r| r.file.kind & 3 == seek_engine::walk::KIND_DIR)
                .map(|r| r.file.path);
            self.new_tab(folder, window, cx);
            cx.stop_propagation();
            return;
        }
        if modifiers.control && key == "i" {
            self.toggle_info_panel(window, cx);
            cx.stop_propagation();
            return;
        }
        // Handled here like Ctrl+I rather than only as a bound action, which
        // didn't reach Seek from every focus (stopping propagation keeps the
        // binding from toggling it a second time).
        if modifiers.control && !modifiers.shift && !modifiers.alt && key == "b" {
            self.toggle_sidebar(cx);
            cx.stop_propagation();
            return;
        }
        // Ctrl+, opens Settings, and closes it again.
        if modifiers.control && !modifiers.shift && !modifiers.alt && key == "," {
            if self.settings {
                self.close_settings(window, cx);
            } else {
                self.open_settings(window, cx);
            }
            cx.stop_propagation();
            return;
        }
        let input_focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let list_focused = self.table.read(cx).focus_handle(cx).is_focused(window);
        if !(input_focused || list_focused || modifiers.control && key == "l") {
            return;
        }
        if key == "enter" && input_focused && self.open_address(window, cx) {
            cx.stop_propagation();
            return;
        }
        if key == "enter" && input_focused && self.suggestions.is_some() {
            let text = self.input.read(cx).value().to_string();
            self.input.update(cx, |input, cx| {
                input.set_value(crate::kinds::name_contains_query(text.trim()), window, cx)
            });
            self.suggestions = None;
            self.submit(cx);
            cx.stop_propagation();
            return;
        }
        if key == "down"
            && input_focused
            && let Some(menu) = &self.suggestions
        {
            menu.focus_handle(cx).focus(window, cx);
            cx.stop_propagation();
            return;
        }
        if !self.input.read(cx).focus_handle(cx).is_focused(window) {
            let action = match (key, modifiers.control, modifiers.shift) {
                ("c", true, false) => Some(daily::Action::Copy),
                ("x", true, false) => Some(daily::Action::Cut),
                ("v", true, false) => Some(daily::Action::Paste),
                ("f2", false, _) => Some(daily::Action::Rename),
                ("delete", false, true) => Some(daily::Action::DeletePermanently),
                ("delete", false, false) => Some(daily::Action::Recycle),
                _ => None,
            };
            if let Some(action) = action {
                self.file_action(action, None, window, cx);
                cx.stop_propagation();
                return;
            }
        }
        match key {
            "left" | "right"
                if list_focused
                    && self.view == View::Columns
                    && !modifiers.shift
                    && !modifiers.control =>
            {
                self.column_key(key, window, cx);
                cx.stop_propagation();
            }
            "up" | "down" | "left" | "right"
                if list_focused
                    && (matches!(key, "up" | "down")
                        || self.view != View::List
                        || modifiers.shift
                        || modifiers.control) =>
            {
                let count = self.table.read(cx).delegate().rows.len();
                let columns = if self.view == View::List {
                    1
                } else {
                    self.grid_columns
                };
                let current = self.table.read(cx).selected_row();
                if let Some(next) = crate::selection::next(current, count, key, columns) {
                    self.table.update(cx, |table, cx| {
                        let data = table.delegate_mut();
                        crate::selection::change(
                            &data.rows,
                            &mut data.selected_many,
                            &mut data.selection_anchor,
                            current,
                            next,
                            crate::selection::Input {
                                shift: modifiers.shift,
                                control: modifiers.control,
                                keyboard: true,
                            },
                        );
                        data.drag_paths = Arc::new(data.selected_many.iter().cloned().collect());
                        table.set_selected_row(next, cx);
                        table.scroll_to_row(next, cx);
                        cx.notify();
                    });
                    if self.view != View::List {
                        self.grid_scroll
                            .scroll_to_item(next / self.grid_columns, ScrollStrategy::Center);
                    }
                    self.refresh_preview(cx);
                    cx.notify();
                }
                cx.stop_propagation();
            }
            "enter"
                if self.input.read(cx).focus_handle(cx).is_focused(window)
                    || self.table.read(cx).focus_handle(cx).is_focused(window) =>
            {
                self.open_selected(modifiers.control, window, cx);
                self.remember(cx);
                cx.stop_propagation();
            }
            "l" if modifiers.control => {
                self.input.update(cx, |input, cx| input.focus(window, cx));
                cx.stop_propagation();
            }
            "a" if modifiers.control && list_focused => {
                self.table.update(cx, |table, cx| {
                    let data = table.delegate_mut();
                    data.selected_many = data.rows.iter().map(|r| r.file.path.clone()).collect();
                    data.drag_paths = Arc::new(data.selected_many.iter().cloned().collect());
                    if table.selected_row().is_none() && !table.delegate().rows.is_empty() {
                        table.set_selected_row(0, cx);
                    }
                    cx.notify();
                });
                cx.notify();
                cx.stop_propagation();
            }
            "j" if modifiers.control && !modifiers.shift => {
                self.toggle_icon_options(window, cx);
                cx.stop_propagation();
            }
            "1" | "2" | "3" | "4" if modifiers.control => {
                self.set_view(
                    match key {
                        "1" => View::List,
                        "2" => View::Icons,
                        "3" => View::Gallery,
                        _ => View::Columns,
                    },
                    cx,
                );
                cx.stop_propagation();
            }
            "space"
                if !modifiers.control
                    && !modifiers.alt
                    && self.table.read(cx).focus_handle(cx).is_focused(window) =>
            {
                self.open_quick_look(None, window, cx);
                cx.stop_propagation();
            }
            "right" | "left" if !self.input.read(cx).focus_handle(cx).is_focused(window) => {
                if let Some(row) = self.selected(cx)
                    && row.file.kind & 3 == seek_engine::walk::KIND_DIR
                {
                    self.expand_folder(row.file.path, key == "right", cx);
                    cx.stop_propagation();
                }
            }
            "c" if modifiers.control && modifiers.shift => {
                if let Some(row) = self.selected(cx) {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        row.file.path.to_string_lossy().into_owned(),
                    ));
                }
                cx.stop_propagation();
            }
            "f5" if self.content_mode => {
                self.submit(cx);
                cx.notify();
                cx.stop_propagation();
            }
            "escape" => {
                self.clear(window, cx);
            }
            _ if self.view == View::Columns
                && list_focused
                && !modifiers.control
                && !modifiers.alt
                && !modifiers.platform
                && event
                    .keystroke
                    .key_char
                    .as_ref()
                    .is_some_and(|text| !text.trim().is_empty()) =>
            {
                // Column view: type-ahead selects instead of searching.
                let text = event.keystroke.key_char.clone().unwrap_or_default();
                self.column_type(&text, cx);
                cx.stop_propagation();
            }
            _ if !modifiers.control && !modifiers.alt && !modifiers.platform => {
                if let Some(text) = &event.keystroke.key_char
                    && !self.input.read(cx).focus_handle(cx).is_focused(window)
                {
                    let value = format!("{}{}", self.input.read(cx).value(), text);
                    self.input.update(cx, |input, cx| {
                        input.set_value(value, window, cx);
                        input.focus(window, cx);
                    });
                    self.submit(cx);
                    cx.stop_propagation();
                }
            }
            _ => (),
        }
    }
}

pub fn root(startup: Startup, window: &mut Window, cx: &mut App) -> Entity<Root> {
    let view = cx.new(|cx| Seek::new(startup, window, cx));
    cx.new(|cx| Root::new(view, window, cx))
}

#[cfg(test)]
mod viewport_tests {
    use super::{keep_scroll_anchor, scroll_top};
    use gpui::{ScrollStrategy, UniformListScrollHandle, point, px};
    #[test]
    fn deleting_above_a_scrolled_view_keeps_the_same_row_and_pixel_remainder() {
        let handle = UniformListScrollHandle::new();
        handle
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(-30015.)));
        let height = gpui_component::Size::Small.table_row_height();
        assert_eq!(scroll_top(&handle, height), 1000);
        keep_scroll_anchor(&handle, 1000, 999, height);
        assert_eq!(handle.0.borrow().base_handle.offset().y, px(-29985.));
        assert_eq!(scroll_top(&handle, height), 999);
        handle.scroll_to_item(5, ScrollStrategy::Top);
        keep_scroll_anchor(&handle, 999, 998, height);
        assert_eq!(handle.0.borrow().base_handle.offset().y, px(-29985.));
        assert_eq!(
            handle
                .0
                .borrow()
                .deferred_scroll_to_item
                .unwrap()
                .item_index,
            5
        );
    }
}
