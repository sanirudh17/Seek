use super::*;

#[derive(Clone)]
pub(super) struct InfoResize {
    width: f32,
    start_x: std::rc::Rc<std::cell::Cell<f32>>,
}
impl Render for InfoResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(0.))
    }
}

pub(super) fn panel_width(preferred: f32, viewport: f32, sidebar: f32) -> f32 {
    let max = (viewport - sidebar - 320.).clamp(240., 720.);
    preferred.clamp(240., max)
}

impl Seek {
    pub(super) fn info_width(&self, window: &Window) -> f32 {
        panel_width(
            self.preferences.info_panel_width,
            f32::from(window.viewport_size().width),
            f32::from_bits(self.sidebar_width.load(Ordering::Relaxed)),
        )
    }
    pub(super) fn info_resize_handle(&self, width: f32) -> impl IntoElement {
        gpui_base::resize_handle("info-panel-divider", Axis::Horizontal)
            .inside(gpui_base::HandleEdge::Leading)
            .role(Role::Group)
            .aria_label("Resize information panel")
            .on_drag(
                InfoResize {
                    width,
                    start_x: Default::default(),
                },
                |drag, _, window, cx| {
                    // The constructor's position is the preview offset inside
                    // the handle; movement events use window coordinates.
                    drag.start_x.set(f32::from(window.mouse_position().x));
                    cx.new(|_| drag.clone())
                },
            )
    }
    pub(super) fn resize_info(
        &mut self,
        event: &DragMoveEvent<InfoResize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let drag = event.drag(cx);
        let requested = drag.width + drag.start_x.get() - f32::from(event.event.position.x);
        let width = panel_width(
            requested,
            f32::from(window.viewport_size().width),
            f32::from_bits(self.sidebar_width.load(Ordering::Relaxed)),
        );
        if (self.preferences.info_panel_width - width).abs() > 0.1 {
            self.preferences.info_panel_width = width;
            self.info_resizing = true;
            cx.notify();
        }
        cx.stop_propagation();
    }
    pub(super) fn finish_info_resize(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        if std::mem::take(&mut self.info_resizing) {
            self.store.save(&self.preferences);
            crate::metrics::interaction(
                "info_panel_resized",
                serde_json::json!({"width":self.preferences.info_panel_width}),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::panel_width;
    #[test]
    fn width_bounds_leave_room_for_the_list_and_restore_the_preference_on_expand() {
        assert_eq!(panel_width(280., 1536., 204.), 280.);
        assert_eq!(panel_width(700., 960., 204.), 436.);
        assert_eq!(panel_width(700., 1536., 204.), 700.);
        assert_eq!(panel_width(100., 1536., 204.), 240.);
        assert_eq!(panel_width(1000., 1536., 204.), 720.);
    }
}
