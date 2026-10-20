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
use std::{path::PathBuf, sync::atomic::Ordering};
#[path = "daily.rs"]
mod daily;
#[path = "info_resize.rs"]
mod info_resize;
#[path = "interface.rs"]
mod interface;
#[path = "permanent_delete.rs"]
mod permanent_delete;
#[path = "settings_view.rs"]
mod settings_view;
#[path = "tabs.rs"]
mod tabs;
#[path = "updating.rs"]
mod updating;
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
        RecycleFiles,
        DeletePermanently
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
    pub backend: Backend,
    pub updates: async_channel::Receiver<Update>,
    pub remembered: crate::window_state::Remembered,
    pub preferences: Settings,
    pub store: Store,
    pub atlas: Atlas,
    pub shell_atlas_rx: async_channel::Receiver<Atlas>,
    pub media: media::Worker,
    pub media_updates: async_channel::Receiver<media::Reply>,
}

struct Rows {
    empty_message: String,
    columns: Vec<Column>,
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
                interface::needs_shell(row) && row.file.kind & 3 != seek_engine::walk::KIND_DIR
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
            .aria_label(format!(
                "{}{}",
                row.name,
                if chosen { " selected" } else { "" }
            ))
            .relative()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(if chosen {
                        p.accent.opacity(0.18)
                    } else {
                        rgba(0).into()
                    })
                    .when(caret && !chosen, |d| {
                        d.border_1().border_color(p.muted.opacity(0.5))
                    }),
            )
            .on_drag(drag, |drag, _, window, cx| {
                let _ = window;
                cx.new(|_| drag.clone())
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |table, event: &MouseDownEvent, window, cx| {
                    let current = table.selected_row();
                    let data = table.delegate_mut();
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
                    table.set_selected_row(i, cx);
                    table.focus_handle(cx).focus(window, cx);
                    // Keep the clicked row as the caret, not the first selected row.
                    // The native click still supplies double-click/open behavior.
                    let data = table.delegate_mut();
                    data.drag_paths = Arc::new(data.selected_many.iter().cloned().collect());
                    cx.notify();
                }),
            )
            .when(row.file.kind & 3 == seek_engine::walk::KIND_DIR, |row| {
                row.on_drop(move |drag: &daily::FilesDrag, window, cx| {
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
        daily::actions_menu(
            daily::menu_style(menu, window),
            self.owner.clone(),
            self.rows.get(i).map(|r| r.file.path.clone()),
            self.rows
                .get(i)
                .is_some_and(|r| r.file.kind & 3 == seek_engine::walk::KIND_DIR),
            cx,
        )
    }
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len() - usize::from(self.query.trim().is_empty() && !self.content)
    }
    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }
    fn column(&self, i: usize, _: &App) -> Column {
        if self.content && i == 1 {
            Column::new("match", "Matching line").width(px(360.))
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
                .child(interface::file_image(row, 16., &self.atlas, &self.cache))
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
                .child(match column {
                    1 => row.snippet.clone().unwrap_or_else(|| row.modified.clone()),
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
                .stripe(false)
                .bordered(false)
                .small(),
        )
    }
}
type NormalResults = (String, Option<(usize, ColumnSort)>, View);
pub struct Seek {
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
    show_hidden: bool,
    sidebar: bool,
    sidebar_from: f32,
    sidebar_width: Arc<std::sync::atomic::AtomicU32>,
    sidebar_motion: u64,
    message: Option<String>,
    history: Vec<(String, Option<PathBuf>)>,
    history_position: usize,
    subscriptions: Vec<Subscription>,
    settings: bool,
    settings_tab: usize,
    settings_focus: FocusHandle,
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
    info_resizing: bool,
    documents: crate::document_preview::Worker,
    document_key: Option<crate::document_preview::Key>,
    document: Option<Result<crate::document_preview::Preview, String>>,
    document_images_to_drop: Vec<Arc<RenderImage>>,
    sort_open: bool,
    grid_scroll: UniformListScrollHandle,
    grid_columns: usize,
    accent_input: Entity<InputState>,
    color_picker: Entity<gpui_component::color_picker::ColorPickerState>,
    custom_locations: bool,
    locations_input: Entity<InputState>,
    exclusions_input: Entity<InputState>,
    cache_epoch: u64,
    applied_view: Option<AppliedView>,
    files: crate::file_ops::Worker,
    edit: Option<daily::Edit>,
    permanent_delete: Option<permanent_delete::Confirmation>,
    largest: Option<bool>,
    normal_results: Option<NormalResults>,
    suggestions: Option<Entity<gpui_component::menu::PopupMenu>>,
    suggestion_dismiss: Option<Subscription>,
    suggestion_origin: Point<Pixels>,
    suggestion_width: Pixels,
    suggestions_at_search: bool,
    content_mode: bool,
    content_failures: Vec<crate::content::Failure>,
    content_locations_input: Entity<InputState>,
    app_updates: crate::updater::State,
}
impl Seek {
    pub fn new(startup: Startup, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let Startup {
            backend,
            updates,
            remembered,
            preferences,
            store,
            atlas,
            shell_atlas_rx,
            media,
            media_updates,
        } = startup;
        crate::metrics::phase("shell_constructing");
        let (documents, document_replies) = crate::document_preview::Worker::start();
        let (files, file_replies) = crate::file_ops::Worker::start();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .context_menu(false)
                .placeholder("Search files and folders")
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        let cache = Arc::new(Mutex::new(media::Cache::default()));
        let visible = Arc::new(Mutex::new(std::collections::HashSet::new()));
        let extra = Arc::new(Mutex::new(std::collections::HashSet::new()));
        let accent_input = cx.new(|cx| {
            InputState::new(window, cx)
                .context_menu(false)
                .default_value(preferences.accent.clone())
                .placeholder("Hex color")
        });
        let color_picker = cx.new(|cx| {
            gpui_component::color_picker::ColorPickerState::new(window, cx).default_value(rgb(
                crate::settings::accent(&preferences.accent).unwrap_or(0x909090),
            ))
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
        let table = cx.new(|cx| {
            TableState::new(
                Rows {
                    empty_message: "Indexing files...".into(),
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
                    owner,
                    visible_range: 0..0,
                },
                window,
                cx,
            )
            .loop_selection(false)
            .col_selectable(false)
        });
        let result_pane = cx.new(|cx| ResultPane {
            table: table.clone(),
            _observe: cx.observe(&table, |_, _, cx| cx.notify()),
        });
        let subscriptions = vec![
            cx.subscribe_in(&color_picker, window, |this, _, event, window, cx| {
                use gpui_component::{Colorize, color_picker::ColorPickerEvent};
                if let ColorPickerEvent::Change(Some(color)) = event {
                    let value = color.to_hex().to_lowercase();
                    this.accent_input.update(cx, |input, cx| input.set_value(value.clone(), window, cx));
                    this.preferences.accent = value;
                    this.save_preferences(window, cx);
                }
            }),
            cx.observe_window_appearance(window,|this,window,cx|{if this.preferences.theme==Appearance::System{crate::appearance::apply(&this.preferences,Some(window),cx);cx.notify();}}),
            cx.observe_window_activation(window,|this,window,cx| {
                if window.is_window_active(){this.motion_enabled=crate::instance::motion_enabled();this.table.update(cx,|_,cx|cx.notify());cx.notify();}
            }),
            cx.observe_window_bounds(window, move |this, window, cx| {
                this.table.update(cx,|_,cx|cx.notify());
                if !this.suggestions_at_search {this.suggestions=None;}
                cx.notify();
                *remembered.lock().unwrap() = window.window_bounds();
            }),
            cx.subscribe_in(&input, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    let _profile=crate::metrics::span("input_change");
                    if this.restoring_input.take().is_some_and(|text| text == this.input.read(cx).value().as_str()) { return; }
                    this.pending_tab_restore = None;
                    if !this.input.read(cx).value().trim().is_empty(){this.largest=None;}
                    this.table.update(cx, |table, _| {
                        table.delegate_mut().expanded.clear();
                        table.delegate_mut().children.clear();
                    });
                    this.decorate_filters(window, cx);
                    this.refresh_suggestions(window,cx);
                    this.submit(cx);
                } else if let InputEvent::PressEnter { secondary, .. } = event {
                    this.open_selected(*secondary, cx);
                    this.remember(cx);
                }
                let _ = window;
            }),
            cx.subscribe_in(&table, window, |this, _, event, _, cx| {
                if matches!(event, TableEvent::ClearSelection) {
                    this.table.update(cx, |table, _| {let data=table.delegate_mut();data.selection = None;data.selected_many.clear();data.drag_paths=Arc::new(vec![]);});
                }
                if let TableEvent::SelectRow(index)=event {
                    this.table.update(cx,|table,_|{table.delegate_mut().selection=table.delegate().rows.get(*index).map(|row|row.file.path.clone());});
                }
                #[cfg(feature = "benchmarks")]
                if let TableEvent::SelectRow(index) = event {
                    crate::metrics::interaction("select", serde_json::json!({"row": index, "name": this.table.read(cx).delegate().rows.get(*index).map(|row| &row.name) }));
                }
                if matches!(event, TableEvent::DoubleClickedRow(_)) {
                    this.open_selected(false, cx);
                }
                this.refresh_preview(cx);
                cx.notify();
            }),
        ];
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
                    #[cfg(feature="benchmarks")]
                    crate::metrics::interaction("document_preview",serde_json::json!({"name":reply.key.file.path.file_name().map(|n|n.to_string_lossy()),"page":reply.key.page,"elapsed_ms":reply.elapsed_ms,"success":reply.result.is_ok(),"source":reply.result.as_ref().ok().map(|p|&p.info.source),"pages":reply.result.as_ref().ok().map(|p|p.info.pages),"dimensions":reply.result.as_ref().ok().map(|p|(p.info.width,p.info.height)),"sheet_rows":reply.result.as_ref().ok().and_then(|p|p.info.sheet.as_ref().map(|s|s.rows.len()))}));
                    if let Some(Ok(old))=this.document.take() && let Some(image)=old.image {let _=window.drop_image(image);}
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
                    reply.finished = next.finished;
                }
                reply.removed.sort();reply.removed.dedup();
                reply.refresh.sort();reply.refresh.dedup();
                if this
                    .update_in(cx, |this, _, cx| {
                        #[cfg(feature="benchmarks")]
                        crate::metrics::interaction("file_operation",serde_json::json!({"success":reply.result.is_ok(),"error":reply.result.as_ref().err(),"refresh_count":reply.refresh.len()}));
                        if !reply.removed.is_empty() {
                            this.remove_confirmed_files(&reply.removed,cx);
                            let _=this.backend.tx.send(Command::FilesRemoved(reply.removed));
                            this.submit(cx); // Cancel earlier replies before stale indexed rows can return.
                        }
                        if let Some((done,total))=reply.progress {this.message=Some(format!("Updating files: {done} of {total}"));}
                        if reply.finished {
                            this.message=reply.result.err();
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
        let this = Self {
            tabs: vec![tabs::Tab::initial(preferences.default_view)],
            active_tab: 0,
            next_tab_id: 1,
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
            show_hidden: preferences.show_hidden,
            sidebar: true,
            sidebar_from: 204.,
            sidebar_width: Arc::new(std::sync::atomic::AtomicU32::new(204.0_f32.to_bits())),
            sidebar_motion: 0,
            message: None,
            history: vec![(String::new(), None)],
            history_position: 0,
            subscriptions,
            settings: false,
            settings_tab: 0,
            settings_focus: cx.focus_handle(),
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
            info_resizing: false,
            documents,
            document_key: None,
            document: None,
            document_images_to_drop: vec![],
            sort_open: false,
            grid_scroll: UniformListScrollHandle::new(),
            grid_columns: 4,
            accent_input,
            color_picker,
            custom_locations,
            locations_input,
            exclusions_input,
            cache_epoch: 0,
            applied_view: None,
            files,
            edit: None,
            permanent_delete: None,
            largest: None,
            normal_results: None,
            suggestions: None,
            suggestion_dismiss: None,
            suggestion_origin: Point::default(),
            suggestions_at_search: false,
            suggestion_width: px(420.),
            content_mode: false,
            content_failures: vec![],
            content_locations_input,
            app_updates: crate::updater::State::default(),
        };
        if check_updates {
            let owner = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = owner.update(cx, |this, cx| this.check_for_updates(cx));
            });
        }
        this
    }
    fn submit(&mut self, cx: &mut Context<Self>) {
        self.content_failures.clear();
        self.message = self.content_mode.then(|| "Searching contents?".into());
        let (started, native_input) = crate::metrics::input_start();
        let generation = self.backend.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.backend.tx.send(Command::Search(Request {
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
        self.content_failures = update.content_failures;
        if self.cache_epoch != update.cache_epoch {
            self.cache_epoch = update.cache_epoch;
            // Revalidate visible files on the worker. Unrelated writes no longer
            // discard every image and repeatedly cancel in-flight previews.
            self.media.force();
        }
        self.helper = update.helper;
        self.favorites = update.favorites;
        self.roots = update.roots;
        if update.message.is_some() {
            self.message = update.message.clone();
        }
        if update.progress_only {
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
        let selected = self.selected(cx).map(|row| row.file.path);
        let grid_height = interface::grid_row_height(self.view);
        let grid_top = scroll_top(&self.grid_scroll, grid_height);
        let grid_anchor = same_view
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
            let anchor = same_view
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
        let table = self.table.read(cx);
        table.delegate().rows.get(table.selected_row()?).cloned()
    }
    fn open_selected(&self, reveal: bool, cx: &mut Context<Self>) {
        let row = self
            .selected(cx)
            .or_else(|| self.table.read(cx).delegate().rows.first().cloned());
        if let Some(row) = row {
            let _ = self.backend.tx.send(Command::Open(row.file.path, reveal));
        }
    }
    fn remember(&mut self, cx: &App) {
        let next = (self.input.read(cx).value().to_string(), self.scope.clone());
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
        self.pending_tab_restore = None;
        self.content_mode = false;
        self.largest = None;
        self.suggestions = None;
        self.message = Some("Indexing this folder...".into());
        self.table.update(cx, |table, _| {
            table.delegate_mut().expanded.clear();
            table.delegate_mut().children.clear();
        });
        self.table.update(cx, |table, cx| {
            let rows = table.delegate_mut();
            rows.base_rows = Arc::new(vec![]);
            rows.rows = Arc::new(vec![]);
            rows.empty_message = "Indexing this folder...".into();
            table.refresh(cx);
        });
        self.scope = scope;
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.submit(cx);
        self.remember(cx);
        cx.notify();
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
        if !matches!(event.keystroke.key.as_str(), "left" | "right")
            || event.keystroke.modifiers.control
            || event.keystroke.modifiers.alt
        {
            return;
        }
        if self.permanent_delete.is_some()
            || gpui_component::WindowExt::has_active_dialog(window, cx)
        {
            return;
        }
        if self.edit.as_ref().is_some_and(|e| {
            e.input
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx)
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
            &self.accent_input,
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
    fn keys(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::WindowExt;
        if self.permanent_delete.is_some() {
            self.permanent_delete_key(event, window, cx);
            return;
        }
        if window.has_active_dialog(cx) {
            return;
        }
        #[cfg(feature = "benchmarks")]
        crate::metrics::interaction(
            "key_route",
            serde_json::json!({"key":event.keystroke.key,"control":event.keystroke.modifiers.control,"platform":event.keystroke.modifiers.platform,"input":self.input.read(cx).focus_handle(cx).is_focused(window),"list":self.table.read(cx).focus_handle(cx).is_focused(window)}),
        );
        if let Some(edit) = self.edit.as_ref() {
            match event.keystroke.key.as_str() {
                "left" | "right"
                    if !edit
                        .input
                        .read(cx)
                        .focus_handle(cx)
                        .contains_focused(window, cx) =>
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
                        self.edit = None;
                        cx.notify();
                    } else {
                        self.finish_edit(window, cx);
                    }
                    cx.stop_propagation();
                }
                "escape" => {
                    self.edit = None;
                    cx.notify();
                    cx.stop_propagation();
                }
                _ => {}
            }
            return;
        }
        let key = event.keystroke.key.as_str();
        if self.settings {
            if key == "escape" {
                self.clear(window, cx);
            }
            return;
        }
        let modifiers = event.keystroke.modifiers;
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
        let input_focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let list_focused = self.table.read(cx).focus_handle(cx).is_focused(window);
        if !(input_focused || list_focused || modifiers.control && key == "l") {
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
                self.open_selected(modifiers.control, cx);
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
            "1" | "2" | "3" if modifiers.control => {
                self.set_view(
                    match key {
                        "1" => View::List,
                        "2" => View::Icons,
                        _ => View::Gallery,
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
                self.panel = !self.panel;
                self.refresh_preview(cx);
                cx.notify();
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
