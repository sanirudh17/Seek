//! Column (Miller) view (3.3). The focused column is the tab's folder and is
//! backed by the pane's own results table, so selection, multi-select, inline
//! rename, drag and drop and the context menu behave exactly as in the other
//! views. Columns to the left are read-only listings of its ancestors; the last
//! column previews the selected folder's contents or a compact file summary.
//! Ancestor and preview listings load off the UI thread with the same filters
//! and order as the search worker, every column is a virtualised list, and at
//! most `MAX_ANCESTORS` ancestors are shown.
use super::*;
use gpui_component::menu::ContextMenuExt;
use gpui_component_assets::IconName as Glyph;
use std::collections::{HashMap, HashSet};

pub(super) const MAX_ANCESTORS: usize = 6;
/// Listings kept for ancestor/preview columns before the oldest are dropped.
const CACHE_LIMIT: usize = 48;
pub(super) const DEFAULT_WIDTH: f32 = 240.;
pub(super) const MIN_WIDTH: f32 = 150.;
pub(super) const MAX_WIDTH: f32 = 640.;
/// Matches the list view's row height (gpui-component Size::Small).
const ROW: f32 = 30.;
const SUMMARY_WIDTH: f32 = 300.;

#[derive(Clone)]
struct Listing {
    rows: Arc<Vec<Row>>,
    error: Option<String>,
    loaded: Instant,
}

#[derive(Default)]
pub(super) struct State {
    cache: HashMap<PathBuf, Listing>,
    loading: HashSet<PathBuf>,
    tasks: HashMap<PathBuf, Task<()>>,
    scrolls: HashMap<PathBuf, UniformListScrollHandle>,
    /// Horizontal scroll position in pixels (content moves left).
    offset: f32,
    /// Where an animated scroll started, and its generation.
    from: Option<f32>,
    motion: u64,
    started: Option<Instant>,
    /// What the automatic scroll last targeted (columns, preview, width).
    layout: Option<(Vec<PathBuf>, Option<PathBuf>, u32)>,
    /// Select this path once the focused column lists it.
    pub(super) pending: Option<PathBuf>,
    /// A plain press on the open folder; released without a drag, it closes.
    close_on_release: Option<usize>,
    /// Type-ahead buffer and when it was last extended.
    typed: String,
    typed_at: Option<Instant>,
    /// Hidden-files setting and exclusions the cached listings were read with.
    filters: Option<(bool, Vec<PathBuf>)>,
}
impl State {
    /// Drop every listing when the visibility filters change.
    pub(super) fn sync_filters(&mut self, hidden: bool, exclusions: &[PathBuf]) {
        let filters = (hidden, exclusions.to_vec());
        if self.filters.as_ref() != Some(&filters) {
            self.cache.clear();
            self.loading.clear();
            self.tasks.clear();
            self.filters = Some(filters);
        }
    }
    /// Forget listings that may have changed on disk.
    pub(super) fn invalidate(&mut self, paths: &[PathBuf]) {
        self.cache.retain(|folder, _| {
            !paths.iter().any(|path| {
                path == folder
                    || path.parent() == Some(folder.as_path())
                    || path.starts_with(folder)
            })
        });
    }
}

/// A column-width drag handle; `index` is the column position from the left.
#[derive(Clone)]
pub(super) struct ColumnResize {
    index: usize,
    pane: crate::pane_layout::PaneId,
}
impl Render for ColumnResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Ancestors of `folder`, nearest last, at most `MAX_ANCESTORS`.
pub(super) fn ancestors(folder: &std::path::Path) -> Vec<PathBuf> {
    let mut chain: Vec<PathBuf> = folder
        .ancestors()
        .skip(1)
        .take(MAX_ANCESTORS)
        .map(PathBuf::from)
        .collect();
    chain.reverse();
    chain
}

fn is_dir(row: &Row) -> bool {
    row.file.kind & 3 == seek_engine::walk::KIND_DIR
}

/// Everything a pane's column view needs; unfocused panes pass their own.
pub(super) struct Source {
    pub(super) pane: crate::pane_layout::PaneId,
    pub(super) table: Entity<TableState<Rows>>,
    pub(super) scroll: UniformListScrollHandle,
    pub(super) scope: Option<PathBuf>,
    pub(super) folder_view: bool,
    pub(super) widths: Vec<f32>,
    pub(super) width: f32,
    pub(super) active: bool,
}

impl Seek {
    fn column_width(widths: &[f32], index: usize) -> f32 {
        widths
            .get(index)
            .copied()
            .unwrap_or(DEFAULT_WIDTH)
            .clamp(MIN_WIDTH, MAX_WIDTH)
    }

    /// Start loading a folder listing for an ancestor or preview column.
    fn load_column(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        if self.columns.cache.contains_key(&folder) || self.columns.loading.contains(&folder) {
            return;
        }
        while self.columns.cache.len() >= CACHE_LIMIT {
            let Some(oldest) = self
                .columns
                .cache
                .iter()
                .min_by_key(|(_, listing)| listing.loaded)
                .map(|(path, _)| path.clone())
            else {
                break;
            };
            self.columns.cache.remove(&oldest);
            self.columns.scrolls.remove(&oldest);
        }
        self.columns.loading.insert(folder.clone());
        let hidden = self.show_hidden;
        let excluded = self.preferences.exclusions.clone();
        let key = folder.clone();
        let task = cx.spawn(async move |this, cx| {
            let read = folder.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut dates = HashMap::new();
                    crate::browse::listing(&read, hidden, &excluded).map(|files| {
                        files
                            .into_iter()
                            .map(|file| Row::with_dates(file, &mut dates))
                            .collect::<Vec<_>>()
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.columns.loading.remove(&folder);
                this.columns.tasks.remove(&folder);
                let (mut rows, error) = match result {
                    Ok(rows) => (rows, None),
                    Err(error) => (vec![], Some(error)),
                };
                // Folders first, then names, as in Finder-style columns.
                rows.sort_by(|a, b| {
                    is_dir(b)
                        .cmp(&is_dir(a))
                        .then_with(|| a.sort_name.cmp(&b.sort_name))
                });
                this.columns.cache.insert(
                    folder,
                    Listing {
                        rows: Arc::new(rows),
                        error,
                        loaded: Instant::now(),
                    },
                );
                cx.notify();
            });
        });
        self.columns.tasks.insert(key, task);
    }

    /// Enter a folder from the focused column; its cached listing fills the
    /// column at once and the search worker's results replace it.
    pub(super) fn column_enter(
        &mut self,
        folder: PathBuf,
        select: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cached = self.columns.cache.get(&folder).map(|l| l.rows.clone());
        self.scope_to(Some(folder), window, cx);
        if let Some(rows) = cached {
            self.table.update(cx, |table, cx| {
                let data = table.delegate_mut();
                data.base_rows = rows;
                data.sorted();
                table.refresh(cx);
            });
        }
        self.columns.pending = select.or_else(|| {
            let data = self.table.read(cx).delegate();
            data.rows.first().map(|row| row.file.path.clone())
        });
        self.table.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// Right: enter the selected folder. Left: go to the parent, keeping the
    /// folder we came from selected. Returns whether the key was used.
    pub(super) fn column_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match key {
            "right" => {
                if let Some(row) = self.selected(cx).filter(is_dir) {
                    self.column_enter(row.file.path, None, window, cx);
                }
                true
            }
            "left" => {
                if let Some(scope) = self.scope.clone()
                    && let Some(parent) = scope.parent().map(PathBuf::from)
                    && self.input.read(cx).value().is_empty()
                {
                    self.column_enter(parent, Some(scope), window, cx);
                }
                true
            }
            _ => false,
        }
    }

    /// Type-ahead in the focused column: select the first name starting with
    /// what was typed in the last 800 ms.
    pub(super) fn column_type(&mut self, text: &str, cx: &mut Context<Self>) {
        let fresh = self
            .columns
            .typed_at
            .is_none_or(|at| at.elapsed() > std::time::Duration::from_millis(800));
        if fresh {
            self.columns.typed.clear();
        }
        self.columns.typed.push_str(&text.to_lowercase());
        self.columns.typed_at = Some(Instant::now());
        let typed = self.columns.typed.clone();
        let index = self
            .table
            .read(cx)
            .delegate()
            .rows
            .iter()
            .position(|row| row.name.to_lowercase().starts_with(&typed));
        if let Some(index) = index {
            self.select_row_only(index, cx);
        }
    }

    pub(super) fn clear_column_selection(&mut self, cx: &mut Context<Self>) {
        self.columns.pending = None;
        self.table.update(cx, |table, cx| {
            let data = table.delegate_mut();
            data.selected_many.clear();
            data.selection = None;
            data.selection_anchor = None;
            data.drag_paths = Arc::new(vec![]);
            table.clear_selection(cx);
        });
        self.refresh_preview(cx);
        cx.notify();
    }

    fn select_row_only(&mut self, index: usize, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let data = table.delegate_mut();
            let Some(path) = data.rows.get(index).map(|row| row.file.path.clone()) else {
                return;
            };
            data.selected_many = [path.clone()].into();
            data.selection_anchor = Some(path);
            data.drag_paths = Arc::new(data.selected_many.iter().cloned().collect());
            table.set_selected_row(index, cx);
        });
        self.grid_scroll
            .scroll_to_item(index, ScrollStrategy::Center);
        self.refresh_preview(cx);
        cx.notify();
    }

    /// The column view for a pane.
    pub(super) fn columns_view(
        &mut self,
        source: Source,
        p: crate::appearance::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Source {
            pane,
            table,
            scroll,
            scope,
            folder_view,
            widths,
            width: viewport,
            active,
        } = source;
        if active {
            self.grid_columns = 1;
            // Apply a pending selection once the focused column lists it.
            if let Some(path) = self.columns.pending.clone() {
                let index = table
                    .read(cx)
                    .delegate()
                    .rows
                    .iter()
                    .position(|row| row.file.path == path);
                if let Some(index) = index {
                    self.columns.pending = None;
                    self.select_row_only(index, cx);
                }
            }
        }
        let chain = match (&scope, folder_view) {
            (Some(scope), true) => ancestors(scope),
            _ => vec![],
        };
        let selected = {
            let table = table.read(cx);
            table
                .selected_row()
                .and_then(|row| table.delegate().rows.get(row).cloned())
        };
        let preview_folder = selected
            .as_ref()
            .filter(|row| is_dir(row))
            .map(|row| row.file.path.clone());
        for folder in chain.iter().cloned().chain(preview_folder.clone()) {
            self.load_column(folder, cx);
        }

        // Column widths: ancestors, the focused column, then the preview.
        let count = chain.len() + 2;
        let column_widths: Vec<f32> = (0..count)
            .map(|index| {
                if index + 1 == count && preview_folder.is_none() {
                    SUMMARY_WIDTH.max(Self::column_width(&widths, index))
                } else {
                    Self::column_width(&widths, index)
                }
            })
            .collect();
        let total: f32 = column_widths.iter().sum();
        let max_offset = (total - viewport).max(0.);

        // Keep the focused column and preview in view: scroll (animated) when
        // the column set changes, never while the user scrolls by hand.
        // Show the preview when it fits, but never scroll the focused column
        // out of view in a narrow pane. Unfocused panes use the same rule.
        let focused_left: f32 = column_widths[..chain.len()].iter().sum();
        let resting = max_offset.min(focused_left);
        if active {
            let layout = (
                chain.clone(),
                preview_folder.clone(),
                viewport.round() as u32,
            );
            if self.columns.layout.as_ref() != Some(&layout) {
                let target = resting;
                if (target - self.columns.offset).abs() > 0.5 {
                    self.columns.from = Some(self.columns.offset);
                    self.columns.motion = self.columns.motion.wrapping_add(1);
                    self.columns.started = Some(Instant::now());
                }
                self.columns.offset = target;
                self.columns.layout = Some(layout);
            }
        }
        let offset = if active {
            self.columns.offset.clamp(0., max_offset)
        } else {
            resting
        };

        let mut strip = div().flex().h_full().flex_shrink_0();
        let mut position = 0;
        for (index, folder) in chain.iter().enumerate() {
            let next = chain.get(index + 1).or(scope.as_ref()).cloned();
            let column = self.listing_column(
                folder.clone(),
                next,
                column_widths[index],
                pane,
                position,
                p,
                cx,
            );
            strip = strip.child(column);
            position += 1;
        }
        strip = strip.child(self.focused_column(
            &table,
            &scroll,
            scope.clone(),
            column_widths[chain.len()],
            pane,
            position,
            active,
            p,
            cx,
        ));
        position += 1;
        let last = column_widths[count - 1];
        strip = strip.child(match (&preview_folder, &selected) {
            (Some(folder), _) => {
                self.listing_column(folder.clone(), None, last, pane, position, p, cx)
            }
            (None, Some(row)) if active => self.summary_column(row, last, p, window, cx),
            _ => div()
                .w(px(last))
                .h_full()
                .flex_shrink_0()
                .into_any_element(),
        });

        let animate = active
            && self.motion_enabled
            && self
                .columns
                .started
                .is_some_and(|at| at.elapsed() < crate::motion::SPRING);
        let from = self.columns.from.unwrap_or(offset);
        let strip = if animate {
            strip
                .with_animation(
                    ("column-scroll", self.columns.motion as usize),
                    crate::motion::reversible(),
                    move |strip, t| strip.ml(px(-(from + (offset - from) * t))),
                )
                .into_any_element()
        } else {
            strip.ml(px(-offset)).into_any_element()
        };
        div()
            .id(("columns", pane.0 as usize))
            .flex()
            .size_full()
            .overflow_hidden()
            .on_scroll_wheel(cx.listener(move |this, event: &ScrollWheelEvent, _, cx| {
                if !active {
                    return;
                }
                let delta = event.delta.pixel_delta(px(20.));
                // Horizontal wheels, or Shift with a vertical wheel.
                let dx = if f32::from(delta.x).abs() > 0. {
                    f32::from(delta.x)
                } else if event.modifiers.shift {
                    f32::from(delta.y)
                } else {
                    return;
                };
                let next = (this.columns.offset - dx).clamp(0., max_offset);
                if (next - this.columns.offset).abs() > 0.1 {
                    this.columns.offset = next;
                    this.columns.started = None;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<ColumnResize>, _, cx| {
                    let drag = event.drag(cx).clone();
                    if drag.pane != pane || drag.pane != this.pane_identity {
                        return;
                    }
                    let left = f32::from(event.bounds.origin.x) - offset
                        + this_widths_before(&this.column_widths, drag.index);
                    let width =
                        (f32::from(event.event.position.x) - left).clamp(MIN_WIDTH, MAX_WIDTH);
                    if this.column_widths.len() <= drag.index {
                        this.column_widths.resize(drag.index + 1, DEFAULT_WIDTH);
                    }
                    if (this.column_widths[drag.index] - width).abs() > 0.5 {
                        this.column_widths[drag.index] = width;
                        this.columns.layout = None;
                        this.layout_dirty = true;
                        cx.notify();
                    }
                }),
            )
            .child(strip)
            .into_any_element()
    }

    fn column_frame(
        &self,
        id: (&'static str, usize),
        width: f32,
        pane: crate::pane_layout::PaneId,
        position: usize,
        p: crate::appearance::Palette,
    ) -> Stateful<Div> {
        let resize = ColumnResize {
            index: position,
            pane,
        };
        div()
            .id(id)
            .relative()
            .w(px(width))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(p.line)
            .child(
                div()
                    .id((id.0, id.1 + 1_000_000))
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(-3.))
                    .w(px(6.))
                    .cursor_col_resize()
                    .hover(move |d| d.bg(p.accent.opacity(0.28)))
                    .on_drag(resize, |drag, _, _, cx| cx.new(|_| drag.clone())),
            )
    }

    fn column_row(
        row: &Row,
        highlighted: bool,
        chosen: bool,
        p: crate::appearance::Palette,
        atlas: &Atlas,
        cache: &Arc<Mutex<media::Cache>>,
    ) -> Div {
        div()
            .h(px(ROW))
            .mx_1()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            // The list's cell size (gpui-component text_sm, 0.875 rem), so split
            // panes in different views show the same type.
            .text_size(rems(0.875))
            .text_color(p.text)
            .rounded(crate::tokens::RADIUS_ROW)
            .bg(if chosen {
                p.accent.opacity(0.26)
            } else if highlighted {
                p.text.opacity(0.08)
            } else {
                rgba(0).into()
            })
            .child(
                div()
                    .size(px(16.))
                    .flex_shrink_0()
                    .child(super::interface::file_image(row, 16., atlas, cache, false)),
            )
            .child(div().flex_1().min_w_0().truncate().child(row.name.clone()))
            .when(is_dir(row), |d| {
                d.child(
                    Icon::new(Glyph::ChevronRight)
                        .size(px(12.))
                        .text_color(p.muted),
                )
            })
    }

    /// A read-only listing (ancestor or preview). Clicking a row moves the
    /// focused column there; folders accept dropped files.
    #[allow(clippy::too_many_arguments)]
    fn listing_column(
        &mut self,
        folder: PathBuf,
        highlight: Option<PathBuf>,
        width: f32,
        pane: crate::pane_layout::PaneId,
        position: usize,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let listing = self.columns.cache.get(&folder).cloned();
        let scroll = self
            .columns
            .scrolls
            .entry(folder.clone())
            .or_default()
            .clone();
        let frame = self.column_frame(("column", position), width, pane, position, p);
        let drop_folder = folder.clone();
        let frame = frame
            .drag_over::<super::daily::FilesDrag>(move |style, _, _, _| {
                style.bg(p.accent.opacity(0.06))
            })
            .on_drop(
                cx.listener(move |this, drag: &super::daily::FilesDrag, window, cx| {
                    this.drop_files(drag, drop_folder.clone(), window, cx)
                }),
            );
        let Some(listing) = listing else {
            return frame
                .child(
                    div()
                        .p_3()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child("Loading..."),
                )
                .into_any_element();
        };
        if let Some(error) = listing.error {
            return frame
                .child(
                    div()
                        .p_3()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child(format!("Unavailable: {error}")),
                )
                .into_any_element();
        }
        if listing.rows.is_empty() {
            return frame
                .child(
                    div()
                        .p_3()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child("Empty folder"),
                )
                .into_any_element();
        }
        if let Some(path) = &highlight
            && let Some(index) = listing.rows.iter().position(|row| &row.file.path == path)
            && scroll.0.borrow().base_handle.offset().y == px(0.)
        {
            scroll.scroll_to_item(index, ScrollStrategy::Center);
        }
        let rows = listing.rows.clone();
        let this = cx.entity().downgrade();
        let list = uniform_list(
            ("column-list", position),
            rows.len(),
            move |range, _, cx| {
                let rows = rows.clone();
                let highlight = highlight.clone();
                let folder = folder.clone();
                this.update(cx, |this, cx| {
                    range
                        .filter_map(|i| rows.get(i).map(|row| (i, row)))
                        .map(|(i, row)| {
                            let target = row.file.path.clone();
                            let parent = folder.clone();
                            let dir = is_dir(row);
                            let drop_path = row.file.path.clone();
                            Self::column_row(
                                row,
                                highlight.as_ref() == Some(&row.file.path),
                                false,
                                p,
                                &this.atlas,
                                &this.cache,
                            )
                            .id(("column-item", position * 1_000_000 + i))
                            .role(Role::Button)
                            .aria_label(row.name.clone())
                            .cursor_pointer()
                            .hover(move |d| d.bg(p.hover))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                // The clicked item's folder becomes the focused column.
                                let reselect = this.scope.as_ref() == Some(&parent)
                                    && this.selected(cx).is_some_and(|row| row.file.path == target);
                                if reselect {
                                    // Clicking the open folder again closes its preview.
                                    this.clear_column_selection(cx);
                                } else if this.scope.as_ref() != Some(&parent) {
                                    this.column_enter(
                                        parent.clone(),
                                        Some(target.clone()),
                                        window,
                                        cx,
                                    );
                                } else {
                                    this.columns.pending = Some(target.clone());
                                    cx.notify();
                                }
                            }))
                            .when(dir, |d| {
                                d.drag_over::<super::daily::FilesDrag>(move |style, _, _, _| {
                                    style.bg(p.accent.opacity(0.22))
                                })
                                .on_drop(cx.listener(
                                    move |this, drag: &super::daily::FilesDrag, window, cx| {
                                        this.drop_files(drag, drop_path.clone(), window, cx)
                                    },
                                ))
                            })
                            .into_any_element()
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
            },
        )
        .track_scroll(&scroll)
        .size_full()
        .py_1();
        frame
            .child(div().flex_1().min_h_0().child(list))
            .into_any_element()
    }

    /// The focused column: the pane's results table as a narrow list.
    #[allow(clippy::too_many_arguments)]
    fn focused_column(
        &mut self,
        table: &Entity<TableState<Rows>>,
        scroll: &UniformListScrollHandle,
        scope: Option<PathBuf>,
        width: f32,
        pane: crate::pane_layout::PaneId,
        position: usize,
        active: bool,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let frame = self.column_frame(("column", position), width, pane, position, p);
        let frame = match scope {
            Some(folder) => frame
                .drag_over::<super::daily::FilesDrag>(move |style, _, _, _| {
                    style.bg(p.accent.opacity(0.06))
                })
                .on_drop(
                    cx.listener(move |this, drag: &super::daily::FilesDrag, window, cx| {
                        this.drop_files(drag, folder.clone(), window, cx)
                    }),
                ),
            None => frame,
        };
        let count = table.read(cx).delegate().rows.len();
        if count == 0 {
            let text = table.read(cx).delegate().empty_message.clone();
            return frame
                .child(
                    div()
                        .p_3()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child(text),
                )
                .into_any_element();
        }
        let this = cx.entity().downgrade();
        let source = table.clone();
        let list = uniform_list(
            ("column-focused", pane.0 as usize),
            count,
            move |range, _, cx| {
                let source = source.clone();
                this.update(cx, |this, cx| {
                    let table = source.read(cx);
                    let caret = table.selected_row();
                    let data = table.delegate();
                    range
                        .filter_map(|i| data.rows.get(i).map(|row| (i, row)))
                        .map(|(i, row)| {
                            let chosen = data.selected_many.contains(&row.file.path);
                            let paths = if chosen {
                                data.drag_paths.clone()
                            } else {
                                Arc::new(vec![row.file.path.clone()])
                            };
                            let drag = super::daily::FilesDrag {
                                label: if paths.len() > 1 {
                                    format!("{} items", paths.len())
                                } else {
                                    row.name.clone()
                                },
                                paths,
                            };
                            let context_owner = cx.entity().downgrade();
                            let context_path = row.file.path.clone();
                            let dir = is_dir(row);
                            let drop_path = row.file.path.clone();
                            let feedback_path = row.file.path.clone();
                            Self::column_row(
                                row,
                                caret == Some(i) && !chosen,
                                chosen,
                                p,
                                &this.atlas,
                                &this.cache,
                            )
                            .id(("column-row", i))
                            .role(Role::ListItem)
                            .aria_label(row.name.clone())
                            .aria_selected(chosen)
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    let plain = !event.modifiers.shift && !event.modifiers.control;
                                    let only = {
                                        let table = this.table.read(cx);
                                        let data = table.delegate();
                                        table.selected_row() == Some(i)
                                            && data.selected_many.len() <= 1
                                            && data.rows.get(i).is_some_and(is_dir)
                                    };
                                    // Clicking the open folder again closes its preview and
                                    // returns to the folder view; a double-click still opens it.
                                    if plain && only && event.click_count == 1 {
                                        this.columns.close_on_release = Some(i);
                                        return;
                                    }
                                    this.columns.close_on_release = None;
                                    this.select_modified(
                                        i,
                                        event.modifiers.shift,
                                        event.modifiers.control,
                                        window,
                                        cx,
                                    );
                                    if event.click_count >= 2 {
                                        this.open_selected(false, window, cx);
                                    }
                                }),
                            )
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                    if cx.has_active_drag() {
                                        this.columns.close_on_release = None;
                                        return;
                                    }
                                    if this.columns.close_on_release.take() == Some(i) {
                                        this.clear_column_selection(cx);
                                    } else {
                                        this.release_selection(i, cx);
                                    }
                                }),
                            )
                            .on_drag(drag, |drag, offset, _, cx| {
                                super::drag_preview::near_cursor(drag.clone(), offset, cx)
                            })
                            .when(dir, |d| {
                                d.drag_over::<super::daily::FilesDrag>(move |style, drag, _, _| {
                                    if super::daily::drop_allowed(&drag.paths, &feedback_path) {
                                        style.bg(p.accent.opacity(0.22))
                                    } else {
                                        style
                                    }
                                })
                                .on_drop(cx.listener(
                                    move |this, drag: &super::daily::FilesDrag, window, cx| {
                                        this.drop_files(drag, drop_path.clone(), window, cx)
                                    },
                                ))
                            })
                            .context_menu(move |menu, window, cx| {
                                super::daily::actions_menu(
                                    super::daily::menu_style(menu, window),
                                    context_owner.clone(),
                                    Some(context_path.clone()),
                                    dir,
                                    cx,
                                )
                            })
                            .into_any_element()
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
            },
        )
        .key_context("SeekFiles")
        .track_scroll(scroll)
        .when(active, |list| {
            list.track_focus(&table.read(cx).focus_handle(cx))
        })
        .size_full()
        .py_1();
        frame
            .child(div().flex_1().min_h_0().child(list))
            .into_any_element()
    }

    /// Compact summary of the selected file, using the info panel's previews.
    fn summary_column(
        &mut self,
        row: &Row,
        width: f32,
        p: crate::appearance::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let preview = if super::text_reader::text_file(row) {
            self.compact_text_preview(row, p, cx).into_any_element()
        } else if crate::document_preview::Key::new(row, 0).is_some() {
            self.document_view(p, window, cx).into_any_element()
        } else {
            div()
                .h(px(180.))
                .flex()
                .items_center()
                .justify_center()
                .child(super::interface::file_image(
                    row,
                    160.,
                    &self.atlas,
                    &self.cache,
                    true,
                ))
                .into_any_element()
        };
        let line = |label: &'static str, value: String| {
            div()
                .flex()
                .justify_between()
                .gap_3()
                .text_size(px(11.))
                .child(div().text_color(p.muted).child(label))
                .child(div().min_w_0().truncate().child(value))
        };
        div()
            .id("column-summary")
            .role(Role::Region)
            .aria_label(format!("Summary of {}", row.name))
            .w(px(width))
            .h_full()
            .flex_shrink_0()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .overflow_y_scroll()
            .child(div().max_h(px(260.)).overflow_hidden().child(preview))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(row.name.clone()),
            )
            .child(line("Kind", row.kind.clone()))
            .child(line("Size", row.size.clone()))
            .child(line("Modified", row.modified.clone()))
            .into_any_element()
    }
}

/// Left edge of column `index` relative to the strip, from saved widths.
fn this_widths_before(widths: &[f32], index: usize) -> f32 {
    (0..index)
        .map(|i| {
            widths
                .get(i)
                .copied()
                .unwrap_or(DEFAULT_WIDTH)
                .clamp(MIN_WIDTH, MAX_WIDTH)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{MAX_ANCESTORS, ancestors, this_widths_before};
    use std::path::PathBuf;
    #[test]
    fn ancestors_are_capped_and_ordered_from_the_top() {
        let chain = ancestors(&PathBuf::from(r"C:\a\b\c"));
        assert_eq!(chain, [r"C:\", r"C:\a", r"C:\a\b"].map(PathBuf::from));
        let deep = PathBuf::from(r"C:\1\2\3\4\5\6\7\8\9");
        let chain = ancestors(&deep);
        assert_eq!(chain.len(), MAX_ANCESTORS);
        assert_eq!(chain.last().unwrap(), &PathBuf::from(r"C:\1\2\3\4\5\6\7\8"));
        assert!(ancestors(&PathBuf::from(r"C:\")).is_empty());
    }
    #[test]
    fn column_edges_use_saved_widths_with_bounds() {
        assert_eq!(this_widths_before(&[], 2), 480.);
        assert_eq!(this_widths_before(&[100., 900.], 2), 150. + 640.);
    }
}
