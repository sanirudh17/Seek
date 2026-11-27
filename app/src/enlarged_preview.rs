use super::*;
use gpui_component::ElementExt;
impl Seek {
    pub(super) fn open_quick_look(
        &mut self,
        target: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.preview_enlarged && self.text_reader_blocks_navigation(cx) {
            return;
        }
        if let Some(path) = target {
            let index = self
                .table
                .read(cx)
                .delegate()
                .rows
                .iter()
                .position(|row| row.file.path == path);
            let Some(index) = index else {
                return;
            };
            self.table
                .update(cx, |table, cx| table.set_selected_row(index, cx));
        }
        let Some(row) = self
            .table
            .read(cx)
            .selected_row()
            .and_then(|index| self.table.read(cx).delegate().rows.get(index).cloned())
        else {
            return;
        };
        self.quick_look_viewport.set(None);
        self.quick_look_row = Some(row);
        self.quick_look_return = None;
        self.quick_look_close_task = Task::ready(());
        self.quick_look_closing = false;
        self.quick_look_zoom = None;
        self.reset_quick_look_pan();
        self.quick_look_motion = self.quick_look_motion.wrapping_add(1);
        self.preview_enlarged = true;
        self.refresh_preview(cx);
        self.preview_focus.focus(window, cx);
        cx.notify();
    }
    /// Quick Look on a file that need not be in the list (a document pane's).
    pub(super) fn open_quick_look_row(
        &mut self,
        row: Row,
        return_focus: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.quick_look_viewport.set(None);
        self.quick_look_row = Some(row);
        self.quick_look_return = Some(return_focus);
        self.quick_look_close_task = Task::ready(());
        self.quick_look_closing = false;
        self.quick_look_zoom = None;
        self.reset_quick_look_pan();
        self.quick_look_motion = self.quick_look_motion.wrapping_add(1);
        self.preview_enlarged = true;
        self.refresh_preview(cx);
        self.preview_focus.focus(window, cx);
        cx.notify();
    }
    /// Closing Quick Look returns to where it was opened from.
    fn quick_look_focus_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.quick_look_return.take() {
            Some(focus) => focus.focus(window, cx),
            None => self.table.read(cx).focus_handle(cx).focus(window, cx),
        }
    }
    pub(super) fn finish_close_quick_look(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_video_edit_now(window, cx);
        self.quick_look_focus_back(window, cx);
        self.quick_look_close_task = Task::ready(());
        self.quick_look_closing = false;
        self.preview_enlarged = false;
        self.quick_look_row = None;
        self.quick_look_viewport.set(None);
        self.text_reader.reset_env_reveal();
        self.refresh_preview(cx);
        cx.notify();
    }
    pub(super) fn close_quick_look(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_look_closing
            || self.request_text_reader_exit(window, cx)
            || self.request_picture_edit_exit(picture_edit_view::EditHost::QuickLook, window, cx)
            || self.request_video_edit_exit(video_edit_view::VideoHost::QuickLook, window, cx)
        {
            return;
        }
        self.quick_look_pan = None;
        if !self.motion_enabled {
            // Transfer focus before disposing the focused editor and its overlays.
            self.finish_close_quick_look(window, cx);
            return;
        }
        self.quick_look_closing = true;
        self.quick_look_motion = self.quick_look_motion.wrapping_add(1);
        let generation = self.quick_look_motion;
        self.quick_look_close_task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(crate::motion::EXIT).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.quick_look_motion != generation || !this.quick_look_closing {
                    return;
                }
                // Do not drop the currently running close task from its own callback.
                this.close_video_edit_now(window, cx);
                this.quick_look_focus_back(window, cx);
                this.quick_look_closing = false;
                this.preview_enlarged = false;
                this.quick_look_row = None;
                this.quick_look_viewport.set(None);
                this.text_reader.reset_env_reveal();
                this.refresh_preview(cx);
                cx.notify();
            });
        });
        cx.notify();
    }
    pub(super) fn enlarged_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_look_closing {
            return;
        }
        match key {
            "escape" | "space" => self.close_quick_look(window, cx),
            "up" | "down" => {
                if self.text_reader_blocks_navigation(cx)
                    || self.picture_edit_for(picture_edit_view::EditHost::QuickLook)
                    || self.video_edit.as_ref().is_some_and(|v| {
                        v.host == video_edit_view::VideoHost::QuickLook && v.is_dirty()
                    })
                {
                    return;
                }
                if self.video_edit.as_ref().is_some_and(|v| {
                    v.host == video_edit_view::VideoHost::QuickLook
                }) {
                    self.close_video_edit_now(window, cx);
                }
                let current = self.table.read(cx).selected_row();
                let count = self.table.read(cx).delegate().rows.len();
                if let Some(next) = crate::selection::next(current, count, key, 1) {
                    self.quick_look_zoom = None;
                    self.reset_quick_look_pan();
                    self.preview_focus.focus(window, cx);
                    self.quick_look_row = self.table.read(cx).delegate().rows.get(next).cloned();
                    self.table
                        .update(cx, |table, cx| table.set_selected_row(next, cx));
                    self.refresh_preview(cx);
                }
            }
            "left" | "right" => {
                if let Some(Ok(preview)) = &self.document {
                    let page = preview.info.page;
                    let pages = preview.info.pages;
                    let next = if key == "left" {
                        page.saturating_sub(1)
                    } else {
                        page.saturating_add(1)
                    };
                    if next < pages && next != page {
                        self.document_page(next, cx);
                    }
                }
            }
            _ => {}
        }
        cx.notify();
    }
    fn quick_look_fit(&self, window: &Window, cx: &App) -> Option<f32> {
        let row = self.selected(cx)?;
        if crate::icons::kind(
            &row.file.path,
            row.file.kind & 3 == seek_engine::walk::KIND_DIR,
        ) != crate::icons::Kind::Image
        {
            return None;
        }
        let mut cache = self.cache.lock().unwrap();
        let image = [crate::document_preview::ENLARGED_EDGE, media::PREVIEW_EDGE]
            .into_iter()
            .find_map(
                |edge| match cache.get(&media::Key::new(&row, edge, false)) {
                    Some(media::Value::Image(Some(image))) => Some(image),
                    _ => None,
                },
            )?;
        let size = image.size(0);
        Some(image_fit(
            (size.width.0 as f32, size.height.0 as f32),
            self.quick_look_viewport.get().unwrap_or((
                f32::from(window.viewport_size().width).max(1.),
                (f32::from(window.viewport_size().height) - 132.).max(1.),
            )),
            window.scale_factor(),
        ))
    }
    fn change_quick_look_zoom(&mut self, factor: f32, window: &Window, cx: &mut Context<Self>) {
        if let Some(fit) = self.quick_look_fit(window, cx) {
            self.quick_look_zoom = Some(zoom_scale(self.quick_look_zoom.unwrap_or(fit), factor));
            cx.notify();
        }
    }
    pub(super) fn quick_look_zoom_key(
        &mut self,
        key: &str,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.quick_look_closing || self.quick_look_fit(window, cx).is_none() {
            return false;
        }
        match key {
            "+" | "=" | "add" | "numpadadd" => self.change_quick_look_zoom(1.25, window, cx),
            "-" | "_" | "subtract" | "numpadsubtract" => {
                self.change_quick_look_zoom(0.8, window, cx)
            }
            "0" => {
                self.quick_look_zoom = None;
                self.reset_quick_look_pan();
                cx.notify();
            }
            "1" => {
                self.quick_look_zoom = Some(1.);
                cx.notify();
            }
            _ => return false,
        }
        true
    }
    fn reset_quick_look_pan(&mut self) {
        self.quick_look_pan = None;
        self.quick_look_pan_offset = Point::default();
        self.quick_look_pan_limits.set(Point::default());
    }
    pub(super) fn quick_look_pan_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if !self.preview_enlarged
            || self.quick_look_closing
            || self.selected(cx).is_none_or(|row| {
                crate::icons::kind(
                    &row.file.path,
                    row.file.kind & 3 == seek_engine::walk::KIND_DIR,
                ) != crate::icons::Kind::Image
            })
        {
            return false;
        }
        let delta = match key {
            "left" => point(px(40.), px(0.)),
            "right" => point(px(-40.), px(0.)),
            "up" => point(px(0.), px(40.)),
            "down" => point(px(0.), px(-40.)),
            _ => return false,
        };
        self.quick_look_pan_offset = pan_offset(
            self.quick_look_pan_offset,
            delta,
            self.quick_look_pan_limits.get(),
        );
        crate::metrics::interaction(
            "image_pan",
            serde_json::json!({"key":key,"x":f32::from(self.quick_look_pan_offset.x),"y":f32::from(self.quick_look_pan_offset.y),"limit_x":f32::from(self.quick_look_pan_limits.get().x),"limit_y":f32::from(self.quick_look_pan_limits.get().y)}),
        );
        cx.notify();
        true
    }
    fn quick_look_image(
        &self,
        row: &Row,
        p: crate::appearance::Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (image, loading) = {
            let mut cache = self.cache.lock().unwrap();
            let mut loading = false;
            let image = [crate::document_preview::ENLARGED_EDGE, media::PREVIEW_EDGE]
                .into_iter()
                .find_map(|edge| match cache.get(&media::Key::new(row, edge, false)) {
                    Some(media::Value::Image(Some(image))) => Some(image),
                    None => {
                        loading = true;
                        None
                    }
                    _ => None,
                });
            (image, loading)
        };
        let Some(image) = image else {
            return div()
                .text_color(p.muted)
                .child(if loading {
                    "Loading image preview..."
                } else {
                    "Image preview unavailable. Open the file in its app to view it."
                })
                .into_any_element();
        };
        let size = image.size(0);
        let source = (size.width.0 as f32, size.height.0 as f32);
        let dpi = window.scale_factor();
        let room = self.quick_look_viewport.get().unwrap_or((
            f32::from(window.viewport_size().width).max(1.),
            (f32::from(window.viewport_size().height) - 132.).max(1.),
        ));
        let fit = image_fit(source, room, dpi);
        let scale = self.quick_look_zoom.unwrap_or(fit);
        let width = source.0 / dpi * scale;
        let height = source.1 / dpi * scale;
        let limits = point(
            px(((width - room.0) / 2.).max(0.)),
            px(((height - room.1) / 2.).max(0.)),
        );
        self.quick_look_pan_limits.set(limits);
        let offset = pan_offset(self.quick_look_pan_offset, Point::default(), limits);
        let movable = limits.x > px(0.5) || limits.y > px(0.5);
        let viewport = self.quick_look_viewport.clone();
        div()
            .id("quick-look-image")
            .on_prepaint(move |bounds, window, _| {
                let measured = (
                    f32::from(bounds.size.width).max(1.),
                    f32::from(bounds.size.height).max(1.),
                );
                if viewport.replace(Some(measured)) != Some(measured) {
                    window.refresh();
                }
            })
            .relative()
            .w_full()
            .h_full()
            .overflow_hidden()
            .when(movable, |view| {
                view.cursor(if self.quick_look_pan.is_some() {
                    CursorStyle::ClosedHand
                } else {
                    CursorStyle::OpenHand
                })
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if !movable {
                        return;
                    }
                    this.quick_look_pan = Some(event.position);
                    this.preview_focus.focus(window, cx);
                    cx.notify();
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                let Some(previous) = this.quick_look_pan else {
                    return;
                };
                if !event.dragging() {
                    this.quick_look_pan = None;
                    cx.notify();
                    return;
                }
                this.quick_look_pan_offset = pan_offset(
                    this.quick_look_pan_offset,
                    event.position - previous,
                    this.quick_look_pan_limits.get(),
                );
                this.quick_look_pan = Some(event.position);
                cx.notify();
                cx.stop_propagation();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.quick_look_pan.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.quick_look_pan.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_scroll_wheel(cx.listener(move |this, event: &ScrollWheelEvent, _, cx| {
                if !event.modifiers.control {
                    this.quick_look_pan_offset = pan_offset(
                        this.quick_look_pan_offset,
                        event.delta.pixel_delta(px(20.)),
                        this.quick_look_pan_limits.get(),
                    );
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                let delta = f32::from(event.delta.pixel_delta(px(20.)).y).clamp(-240., 240.);
                let current = this.quick_look_zoom.unwrap_or(fit);
                this.quick_look_zoom = Some(zoom_scale(current, (delta * 0.01).exp()));
                cx.notify();
                cx.stop_propagation();
            }))
            .child(
                img(image)
                    .absolute()
                    .left(px((room.0 - width) / 2.) + offset.x)
                    .top(px((room.1 - height) / 2.) + offset.y)
                    .w(px(width))
                    .h(px(height))
                    .flex_shrink_0(),
            )
            .into_any_element()
    }
    pub(super) fn enlarged_preview(
        &mut self,
        p: crate::appearance::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::preview_controls::control;
        use gpui_component_assets::IconName as Glyph;
        let Some(row) = self.selected(cx) else {
            return div()
                .absolute()
                .inset_0()
                .bg(p.chrome)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_4()
                .child("The file is no longer available.")
                .child(
                    control("close-missing-preview", "", p, cx)
                        .child("Close preview")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.close_quick_look(window, cx)),
                        ),
                )
                .into_any_element();
        };
        let mut content = div()
            .id("enlarged-preview-content")
            .flex_1()
            .min_h_0()
            .overflow_scroll()
            .p_4();
        let image_file = crate::icons::kind(
            &row.file.path,
            row.file.kind & 3 == seek_engine::walk::KIND_DIR,
        ) == crate::icons::Kind::Image;
        let editing_picture = self.picture_edit_for(picture_edit_view::EditHost::QuickLook);
        if editing_picture {
            content = content
                .p_0()
                .overflow_hidden()
                .child(self.picture_edit_view(p, window, cx));
        } else if super::text_reader::text_file(&row) {
            content = content
                .p_0()
                .overflow_hidden()
                .child(self.text_reader_view(p, window, cx));
        } else if crate::document_preview::Key::new(&row, 0).is_some() {
            let extension = row
                .file
                .path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(extension.as_str(), "docx" | "xlsx") {
                content = content.flex().flex_col().items_center().child(
                    div()
                        .w_full()
                        .max_w(px(900.))
                        .flex_shrink_0()
                        .when(extension == "xlsx", |view| {
                            view.min_h(relative(1.)).flex().flex_col().justify_center()
                        })
                        .child(self.document_view(p, window, cx)),
                );
            } else {
                content = content.child(self.document_view(p, window, cx));
            }
        } else if image_file {
            content = content
                .p_0()
                .overflow_hidden()
                .child(self.quick_look_image(&row, p, window, cx));
        } else if crate::video_edit::is_video(&row.file.path) && !self.quick_look_closing {
            content = content
                .p_0()
                .overflow_hidden()
                .child(self.video_edit_view(
                    video_edit_view::VideoHost::QuickLook,
                    row.file.path.clone(),
                    p,
                    window,
                    cx,
                ));
        } else {
            let details = match self
                .cache
                .lock()
                .unwrap()
                .get(&media::Key::new(&row, 0, true))
            {
                Some(media::Value::Details(d)) => Some(d),
                _ => None,
            };
            if let Some(text) = details.and_then(|d| d.text) {
                content = content
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .child("Text preview · up to 80 lines / 8 KiB"),
                    )
                    .child(
                        div()
                            .mt_3()
                            .text_size(px(16.))
                            .line_height(px(24.))
                            .font_family(crate::tokens::FONT_MONO)
                            .child(text),
                    );
            } else {
                let kind = crate::icons::kind(
                    &row.file.path,
                    row.file.kind & 3 == seek_engine::walk::KIND_DIR,
                );
                let visual = matches!(kind, crate::icons::Kind::Image | crate::icons::Kind::Video);
                content = content.child(
                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .items_center()
                        .justify_center()
                        .child(interface::file_image(
                            &row,
                            if visual {
                                (f32::from(window.viewport_size().height) - 180.).clamp(100., 1000.)
                            } else {
                                64.
                            },
                            &self.atlas,
                            &self.cache,
                            true,
                        ))
                        .when(!visual, |d| {
                            d.child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(p.muted)
                                    .child("No preview available for this file type"),
                            )
                            .when(
                                kind == crate::icons::Kind::Powerpoint,
                                |d| {
                                    d.child(
                                        div()
                                            .text_color(p.muted)
                                            .child(format!("{} - {}", row.kind, row.size)),
                                    )
                                },
                            )
                        }),
                );
            }
        }
        let view = div()
            .id("enlarged-preview")
            .key_context(if image_file {
                "SeekImagePreview"
            } else {
                "SeekQuickLook"
            })
            .track_focus(&self.preview_focus)
            .role(Role::Region)
            .aria_label("Quick Look")
            .absolute()
            .inset_0()
            .occlude()
            .bg(p.chrome)
            .text_color(p.text)
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(56.))
                    .flex_shrink_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .id("preview-file-navigation")
                            .role(Role::Group)
                            .aria_label("Browse files")
                            .flex()
                            .gap_1()
                            .child(
                                control("preview-previous-file", "Previous file", p, cx)
                                    .w(px(34.))
                                    .px_0()
                                    .icon(Glyph::ChevronLeft)
                                    .tooltip("Previous file (Up)")
                                    .disabled(
                                        self.table.read(cx).selected_row().is_none_or(|i| i == 0),
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.enlarged_key("up", window, cx)
                                    })),
                            )
                            .child(
                                control("preview-next-file", "Next file", p, cx)
                                    .w(px(34.))
                                    .px_0()
                                    .icon(Glyph::ChevronRight)
                                    .tooltip("Next file (Down)")
                                    .disabled(self.table.read(cx).selected_row().is_none_or(|i| {
                                        i + 1 >= self.table.read(cx).delegate().rows.len()
                                    }))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.enlarged_key("down", window, cx)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(row.name.clone()),
                    )
                    .child(
                        control("close-enlarged-preview", "Close preview", p, cx)
                            .w(px(34.))
                            .px_0()
                            .icon(Glyph::WindowClose)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.enlarged_key("escape", window, cx)
                            })),
                    ),
            )
            .when(image_file && !editing_picture, |view| {
                let edit_path = row.file.path.clone();
                let can_edit = crate::image_tools::readable(&edit_path);
                let resolution = match self.cache.lock().unwrap().get(&media::Key::new(&row, crate::document_preview::ENLARGED_EDGE, false)) {
                    Some(media::Value::Image(Some(image))) => {
                        let size = image.size(0);
                        format!("{} × {}", size.width.0, size.height.0)
                    }
                    Some(_) => "Preview size".into(),
                    None => String::new(),
                };
                let fit_mode = self.quick_look_zoom.is_none();
                let actual_mode = self
                    .quick_look_zoom
                    .is_some_and(|zoom| (zoom - 1.).abs() < 0.0001);
                let shown_scale = self
                    .quick_look_zoom
                    .or_else(|| self.quick_look_fit(window, cx));
                view.child(
                    div()
                        .h(px(44.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .flex_shrink_0()
                                .child(
                                    control("quick-look-zoom-out", "", p, cx)
                                        .accessibility_label("Zoom out")
                                        .tooltip("Zoom out (Ctrl+-)")
                                        .h(crate::tokens::BUTTON_HEIGHT)
                                        .w(crate::tokens::BUTTON_HEIGHT)
                                        .min_w(crate::tokens::BUTTON_HEIGHT)
                                        .flex_shrink_0()
                                        .px_0()
                                        .icon(Glyph::WindowMinimize)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.change_quick_look_zoom(0.8, window, cx)
                                        })),
                                )
                                .child(
                                    control("quick-look-zoom-in", "", p, cx)
                                        .accessibility_label("Zoom in")
                                        .tooltip("Zoom in (Ctrl++)")
                                        .h(crate::tokens::BUTTON_HEIGHT)
                                        .w(crate::tokens::BUTTON_HEIGHT)
                                        .min_w(crate::tokens::BUTTON_HEIGHT)
                                        .flex_shrink_0()
                                        .px_0()
                                        .icon(Glyph::Plus)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.change_quick_look_zoom(1.25, window, cx)
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .flex_shrink_0()
                                .child(
                                    control("quick-look-fit", "", p, cx)
                                        .accessibility_label("Fit image")
                                        .tooltip("Fit image to the frame and reset pan (Ctrl+0)")
                                        .h(crate::tokens::BUTTON_HEIGHT)
                                        .px_3()
                                        .flex_shrink_0()
                                        .when(fit_mode, |button| {
                                            button
                                                .bg(p.accent.opacity(0.16))
                                                .border_1()
                                                .border_color(p.accent.opacity(0.45))
                                        })
                                        .child(super::interface::button_text("Fit"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.quick_look_zoom = None;
                                            this.reset_quick_look_pan();
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    control("quick-look-actual", "", p, cx)
                                        .accessibility_label("100% actual image pixels")
                                        .tooltip("Actual decoded image pixels, without fitting (Ctrl+1); resolution is shown beside zoom")
                                        .h(crate::tokens::BUTTON_HEIGHT)
                                        .px_3()
                                        .flex_shrink_0()
                                        .when(actual_mode, |button| {
                                            button
                                                .bg(p.accent.opacity(0.16))
                                                .border_1()
                                                .border_color(p.accent.opacity(0.45))
                                        })
                                        .child(super::interface::button_text("100%"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.quick_look_zoom = Some(1.);
                                            this.reset_quick_look_pan();
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .w(px(52.))
                                .flex_shrink_0()
                                .font_family(crate::tokens::FONT_MONO)
                                .text_size(crate::tokens::TEXT_SMALL)
                                .text_color(p.muted)
                                .child(
                                    shown_scale
                                        .map(|scale| format!("{:.0}%", scale * 100.))
                                        .unwrap_or_default(),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(p.muted)
                                .font_family(crate::tokens::FONT_MONO)
                                .text_size(crate::tokens::TEXT_CAPTION)
                                .child(resolution),
                        )
                        .when(can_edit, |bar| {
                            bar.child(
                                super::settings_view::control("quick-look-edit-picture", "Edit", p, cx)
                                    .accessibility_label("Edit picture")
                                    .tooltip("Edit picture (crop, rotate, flip, annotate, redact)")
                                    .h(crate::tokens::BUTTON_HEIGHT)
                                    .flex_shrink_0()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_picture_edit(
                                            picture_edit_view::EditHost::QuickLook,
                                            edit_path.clone(),
                                            window,
                                            cx,
                                        );
                                    })),
                            )
                        }),
                )
            })
            .child(content)
            .children(
                self.document_navigation(p, cx)
                    .map(|bar| div().px_4().border_t_1().border_color(p.line).child(bar)),
            )
;
        if self.motion_enabled {
            let closing = self.quick_look_closing;
            view.with_animation(
                ("quick-look-motion", self.quick_look_motion),
                if closing {
                    crate::motion::exit()
                } else {
                    crate::motion::enter()
                },
                move |view, t| {
                    let progress = if closing { 1. - t } else { t };
                    let offset = px(6. * (1. - progress));
                    view.opacity(progress).top(offset).bottom(-offset)
                },
            )
            .into_any_element()
        } else {
            view.into_any_element()
        }
    }
}

fn pan_offset(offset: Point<Pixels>, delta: Point<Pixels>, limits: Point<Pixels>) -> Point<Pixels> {
    let x = limits.x.max(px(0.));
    let y = limits.y.max(px(0.));
    point(
        (offset.x.clamp(-x, x) + delta.x).clamp(-x, x),
        (offset.y.clamp(-y, y) + delta.y).clamp(-y, y),
    )
}

fn zoom_scale(current: f32, factor: f32) -> f32 {
    (current * factor).clamp(0.05, 16.)
}

// Scale relative to decoded preview pixels, with physical/logical DPI conversion.
fn image_fit(image: (f32, f32), room: (f32, f32), dpi: f32) -> f32 {
    (room.0 * dpi / image.0.max(1.))
        .min(room.1 * dpi / image.1.max(1.))
        .min(16.)
}
#[cfg(test)]
mod tests {
    #[test]
    fn panning_tracks_drag_direction_and_clamps_at_all_edges() {
        use gpui::{point, px};
        let limits = point(px(100.), px(80.));
        let up = super::pan_offset(point(px(0.), px(0.)), point(px(0.), px(40.)), limits);
        assert_eq!(up, point(px(0.), px(40.)));
        assert_eq!(
            super::pan_offset(up, point(px(0.), px(-40.)), limits),
            point(px(0.), px(0.))
        );

        assert_eq!(
            super::pan_offset(
                point(px(-50.), px(-40.)),
                point(px(20.), px(-100.)),
                point(px(100.), px(80.))
            ),
            point(px(-30.), px(-80.))
        );
        assert_eq!(
            super::pan_offset(
                point(px(-50.), px(-40.)),
                point(px(500.), px(500.)),
                point(px(100.), px(80.))
            ),
            point(px(100.), px(80.))
        );
        assert_eq!(
            super::pan_offset(
                point(px(-50.), px(-40.)),
                point(px(-500.), px(-500.)),
                point(px(100.), px(80.))
            ),
            point(px(-100.), px(-80.))
        );
        assert_eq!(
            super::pan_offset(
                point(px(-50.), px(-40.)),
                point(px(-500.), px(500.)),
                point(px(0.), px(0.))
            ),
            point(px(0.), px(0.))
        );
    }
    #[test]
    fn keyboard_zoom_round_trips_and_stays_bounded() {
        assert!((super::zoom_scale(super::zoom_scale(0.5, 1.25), 0.8) - 0.5).abs() < 0.0001);
        assert_eq!(super::zoom_scale(16., 1.25), 16.);
        assert_eq!(super::zoom_scale(0.05, 0.8), 0.05);
    }
    #[test]
    fn fitting_preserves_orientation_and_physical_pixel_scale() {
        assert!((super::image_fit((1600., 800.), (800., 600.), 1.) - 0.5).abs() < 0.0001);
        assert!((super::image_fit((800., 1600.), (800., 600.), 1.) - 0.375).abs() < 0.0001);
        assert_eq!(super::image_fit((1600., 800.), (800., 600.), 2.), 1.);
        assert_eq!(super::image_fit((100., 100.), (800., 600.), 1.), 6.);
        assert_eq!(super::image_fit((1., 1.), (800., 600.), 1.), 16.);
    }
}
