//! Drag previews that stay at the pointer. GPUI draws a drag view at the same
//! offset from the pointer as the point where the source was grabbed, so a
//! wide row grabbed near its right edge would show its small chip far to the
//! left. `near_cursor` pads the preview by that offset, so the chip sits just
//! under the pointer wherever the source was grabbed.
use super::*;

pub(super) struct NearCursor<T: Render> {
    inner: Entity<T>,
    offset: Point<Pixels>,
}

/// Where the pointer sits inside the chip: over its leading icon, centred
/// vertically on a 28 px chip.
const HOLD: Point<Pixels> = Point {
    x: px(10.),
    y: px(14.),
};

pub(super) fn near_cursor<T: Render>(
    value: T,
    offset: Point<Pixels>,
    cx: &mut App,
) -> Entity<NearCursor<T>> {
    let inner = cx.new(|_| value);
    cx.new(|_| NearCursor { inner, offset })
}

impl<T: Render> Render for NearCursor<T> {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .pl((self.offset.x - HOLD.x).max(px(0.)))
            .pt((self.offset.y - HOLD.y).max(px(0.)))
            .child(self.inner.clone())
    }
}
