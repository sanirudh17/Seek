//! Drag-to-split (3.2). While a tab or a sidebar folder is dragged over a pane,
//! a translucent accent rectangle previews the half the new pane will take.
//! It grows and fades in, slides between halves on a spring, disappears when
//! the drag leaves or Esc is pressed, and the new pane settles in on drop.
use super::panes::TabDrag;
use super::*;
use crate::pane_layout::{Axis, MAX_PANES, Node, PaneId, SplitId};

/// A sidebar folder being dragged.
#[derive(Clone)]
pub(super) struct PlaceDrag {
    /// None for a saved search, which can be arranged but not split.
    pub(super) path: Option<PathBuf>,
    pub(super) label: String,
    /// The sidebar item, when dragged from Favorites or a group.
    pub(super) item: Option<crate::sidebar_groups::Item>,
}
impl Render for PlaceDrag {
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
            .child(Icon::new(gpui_component::IconName::Folder).size(px(13.)))
            .child(div().max_w(px(200.)).truncate().child(self.label.clone()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Zone {
    Left,
    Right,
    Top,
    Bottom,
}
impl Zone {
    /// The half nearest the pointer, from its position as fractions (0..1)
    /// of the pane body.
    pub(super) fn at(x: f32, y: f32) -> Self {
        if (x - 0.5).abs() >= (y - 0.5).abs() {
            if x < 0.5 { Self::Left } else { Self::Right }
        } else if y < 0.5 {
            Self::Top
        } else {
            Self::Bottom
        }
    }
    /// Left, top, width and height of the previewed area, as fractions.
    pub(super) fn rect(self) -> [f32; 4] {
        match self {
            Self::Left => [0., 0., 0.5, 1.],
            Self::Right => [0.5, 0., 0.5, 1.],
            Self::Top => [0., 0., 1., 0.5],
            Self::Bottom => [0., 0.5, 1., 0.5],
        }
    }
    pub(super) fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Horizontal,
            Self::Top | Self::Bottom => Axis::Vertical,
        }
    }
    /// Whether the new pane goes before (left/above) the target pane.
    pub(super) fn first(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }
}

pub(super) struct SplitPreview {
    pub(super) pane: PaneId,
    pub(super) zone: Zone,
    /// The rectangle shown before the latest zone change, for sliding.
    from: Option<[f32; 4]>,
    motion: u64,
}

enum Source {
    Tab(TabDrag),
    Place(PlaceDrag),
}

/// Height of a pane's tab strip; dragging over it reorders or moves tabs
/// instead of splitting.
const STRIP: f32 = 38.;

impl Seek {
    /// Attach drag-to-split handlers to a pane's container. `strip` is true
    /// when the container includes the pane's own tab strip at its top.
    pub(super) fn split_target(
        &self,
        element: Stateful<Div>,
        pane: PaneId,
        strip: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        element
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<TabDrag>, _, cx| {
                    let drag = event.drag(cx).clone();
                    this.track_split(
                        pane,
                        strip,
                        event.bounds,
                        event.event.position,
                        Some(&drag),
                        cx,
                    );
                }),
            )
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<PlaceDrag>, _, cx| {
                    if event.drag(cx).path.is_some() {
                        this.track_split(pane, strip, event.bounds, event.event.position, None, cx);
                    }
                }),
            )
            .on_drop(cx.listener(move |this, drag: &TabDrag, window, cx| {
                this.drop_split(pane, Source::Tab(drag.clone()), window, cx)
            }))
            .on_drop(cx.listener(move |this, drag: &PlaceDrag, window, cx| {
                this.drop_split(pane, Source::Place(drag.clone()), window, cx)
            }))
    }

    fn track_split(
        &mut self,
        pane: PaneId,
        strip: bool,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
        tab: Option<&TabDrag>,
        cx: &mut Context<Self>,
    ) {
        let top = bounds.origin.y + px(if strip { STRIP } else { 0. });
        let inside = bounds.contains(&position) && position.y > top;
        // A pane's only tab cannot split that same pane.
        let lone = tab.is_some_and(|drag| drag.pane == pane && self.pane_tab_count(pane) <= 1);
        if !inside || lone || self.panes.len() + self.doc_panes.len() + 1 >= MAX_PANES {
            if self.split_preview.as_ref().is_some_and(|p| p.pane == pane) {
                self.split_preview = None;
                cx.notify();
            }
            return;
        }
        let height = f32::from(bounds.bottom() - top).max(1.);
        let x = f32::from(position.x - bounds.origin.x) / f32::from(bounds.size.width).max(1.);
        let y = f32::from(position.y - top) / height;
        let zone = Zone::at(x, y);
        let from = match &self.split_preview {
            Some(preview) if preview.pane == pane && preview.zone == zone => return,
            Some(preview) if preview.pane == pane => Some(preview.zone.rect()),
            _ => None,
        };
        self.split_motion = self.split_motion.wrapping_add(1);
        self.split_preview = Some(SplitPreview {
            pane,
            zone,
            from,
            motion: self.split_motion,
        });
        cx.notify();
    }

    fn pane_tab_count(&self, pane: PaneId) -> usize {
        if pane == self.pane_identity {
            self.tabs.len()
        } else {
            self.panes
                .iter()
                .find(|slot| slot.id == pane)
                .map_or(0, |slot| slot.tabs.len())
        }
    }

    /// The preview rectangle for `pane`, if a split is being previewed there.
    pub(super) fn split_overlay(
        &self,
        pane: PaneId,
        strip: bool,
        p: crate::appearance::Palette,
    ) -> Option<AnyElement> {
        let preview = self
            .split_preview
            .as_ref()
            .filter(|preview| preview.pane == pane)?;
        let to = preview.zone.rect();
        let from = preview.from;
        let area = |d: Div, [left, top, width, height]: [f32; 4]| {
            d.left(relative(left))
                .top(relative(top))
                .w(relative(width))
                .h(relative(height))
        };
        let fill = div()
            .size_full()
            .rounded(px(8.))
            .bg(p.accent.opacity(0.16))
            .border_2()
            .border_color(p.accent.opacity(0.65));
        let body = div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .top(px(if strip { STRIP } else { 0. }));
        let rect = area(div().absolute().p(px(6.)), to).child(fill);
        if !self.motion_enabled {
            return Some(body.child(rect).into_any_element());
        }
        let animated = rect.with_animation(
            ("split-preview", preview.motion as usize),
            if from.is_some() {
                crate::motion::reversible()
            } else {
                crate::motion::preview()
            },
            move |rect, t| match from {
                // Slide between halves.
                Some(from) => {
                    let mix: [f32; 4] = std::array::from_fn(|i| from[i] + (to[i] - from[i]) * t);
                    area(rect, mix)
                }
                // Grow from the centre of its half and fade in.
                None => {
                    let inset = (1. - t) * 0.08;
                    let grown = [
                        to[0] + to[2] * inset,
                        to[1] + to[3] * inset,
                        to[2] * (1. - 2. * inset),
                        to[3] * (1. - 2. * inset),
                    ];
                    area(rect, grown).opacity(t)
                }
            },
        );
        Some(body.child(animated).into_any_element())
    }

    pub(super) fn clear_split_preview(&mut self) -> bool {
        self.split_preview.take().is_some()
    }

    fn drop_split(
        &mut self,
        target: PaneId,
        source: Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tab_drop = None;
        let Some(preview) = self
            .split_preview
            .take()
            .filter(|preview| preview.pane == target)
        else {
            cx.notify();
            return;
        };
        self.split_at(target, preview.zone, source, window, cx);
        cx.notify();
    }

    fn split_at(
        &mut self,
        target: PaneId,
        zone: Zone,
        source: Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.tabs_available(window, cx) {
            return;
        }
        if self.panes.len() + self.doc_panes.len() + 1 >= MAX_PANES {
            self.message = Some(format!(
                "Seek shows up to {MAX_PANES} panes. Close a pane before splitting again."
            ));
            return;
        }
        self.tabs[self.active_tab] = self.capture_tab(cx);
        let mut tab = match &source {
            Source::Tab(drag) => {
                if drag.pane == target && self.pane_tab_count(target) <= 1 {
                    return;
                }
                let found = if drag.pane == self.pane_identity {
                    self.tabs.iter().find(|t| t.id == drag.tab).cloned()
                } else {
                    self.panes
                        .iter()
                        .find(|slot| slot.id == drag.pane)
                        .and_then(|slot| slot.tabs.iter().find(|t| t.id == drag.tab).cloned())
                };
                let Some(tab) = found else {
                    return;
                };
                tab
            }
            Source::Place(place) => {
                let Some(path) = place.path.clone() else {
                    return;
                };
                tabs::folder_tab(&self.tabs[self.active_tab], path)
            }
        };
        let (Some(pane), Some(tab_id), Some(split)) =
            (self.allocate_id(), self.allocate_id(), self.allocate_id())
        else {
            return;
        };
        let pane = PaneId(pane);
        tab.id = tab_id as usize;
        tab.created = None;
        let new = Node::Pane { id: pane };
        let old = Node::Pane { id: target };
        let (first, second) = if zone.first() { (new, old) } else { (old, new) };
        let mut layout = self.layout.clone();
        if !layout.replace(
            target,
            Node::Split {
                id: SplitId(split),
                axis: zone.axis(),
                fraction: 5000,
                first: Box::new(first),
                second: Box::new(second),
            },
        ) || layout.validate_tree().is_err()
        {
            return;
        }
        let slot = self.new_slot(pane, vec![tab], 0, window, cx);
        self.layout = layout;
        self.panes.push(slot);
        self.unzoom();
        self.pane_motion = Some((pane, Instant::now()));
        self.focus_pane(pane, window, cx);
        // A dragged tab moves: it leaves its old pane, which closes when empty.
        if let Source::Tab(drag) = source
            && let Some(index) = self.panes.iter().position(|slot| slot.id == drag.pane)
        {
            self.take_from_slot(index, drag.tab, cx);
        }
        self.remember_session(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::Zone;
    #[test]
    fn zones_follow_the_nearest_half() {
        assert_eq!(Zone::at(0.1, 0.5), Zone::Left);
        assert_eq!(Zone::at(0.9, 0.4), Zone::Right);
        assert_eq!(Zone::at(0.5, 0.1), Zone::Top);
        assert_eq!(Zone::at(0.45, 0.95), Zone::Bottom);
        assert!(Zone::Left.first() && Zone::Top.first());
        assert!(!Zone::Right.first() && !Zone::Bottom.first());
        for zone in [Zone::Left, Zone::Right, Zone::Top, Zone::Bottom] {
            let [l, t, w, h] = zone.rect();
            assert!(l + w <= 1. && t + h <= 1. && w * h == 0.5);
        }
    }
}
