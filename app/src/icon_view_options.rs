//! The Icons view options popover (Ctrl+J), after Tilde's View Options: icon
//! size, grid spacing, label position, the details under each name and
//! thumbnails. Every change applies at once to the focused pane only.
use super::*;
use crate::icon_view::{IconView, Labels};
use crate::pane_layout::PaneId;
use gpui_component::{ActiveTheme, checkbox::Checkbox, slider::Slider};

impl Seek {
    pub(super) fn icon_view_for(&self, pane: PaneId) -> IconView {
        self.preferences
            .icon_views
            .get(&pane.0)
            .copied()
            .unwrap_or_default()
            .bounded()
    }

    /// Changes the focused pane's options and saves them.
    pub(super) fn set_icon_view(
        &mut self,
        change: impl FnOnce(&mut IconView),
        cx: &mut Context<Self>,
    ) {
        let pane = self.pane_identity;
        let before = self.icon_view_for(pane);
        let mut after = before;
        change(&mut after);
        let after = after.bounded();
        if after == before {
            return;
        }
        if after.size != before.size {
            self.icon_resize_generation = self.icon_resize_generation.wrapping_add(1);
            self.icon_resize = Some((pane.0, before.size, self.icon_resize_generation));
            // Tiles that scroll in later should not replay the resize.
            let generation = self.icon_resize_generation;
            self.icon_resize_clear = cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(crate::motion::STATE + std::time::Duration::from_millis(40))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.icon_resize.is_some_and(|(_, _, g)| g == generation) {
                        this.icon_resize = None;
                        cx.notify();
                    }
                });
            });
        }
        self.preferences.icon_views.insert(pane.0, after);
        // Forget panes that no longer exist once the list grows long.
        if self.preferences.icon_views.len() > crate::icon_view::REMEMBERED {
            let live: Vec<u64> = std::iter::once(self.pane_identity.0)
                .chain(self.panes.iter().map(|slot| slot.id.0))
                .collect();
            self.preferences.icon_views.retain(|id, _| live.contains(id));
        }
        self.store.save(&self.preferences);
        cx.notify();
    }

    pub(super) fn toggle_icon_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.icon_options_open {
            self.icon_options_open = false;
            cx.notify();
            return;
        }
        if self.view != View::Icons {
            self.set_view(View::Icons, cx);
        }
        let options = self.icon_view_for(self.pane_identity);
        self.icon_size_slider.update(cx, |slider, cx| {
            slider.set_value(options.size as f32, window, cx)
        });
        self.icon_spacing_slider.update(cx, |slider, cx| {
            slider.set_value(options.spacing as f32, window, cx)
        });
        self.icon_options_open = true;
        self.icon_options_motion = self.icon_options_motion.wrapping_add(1);
        cx.notify();
    }

    pub(super) fn icon_options_view(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::tokens::*;
        let options = self.icon_view_for(self.pane_identity);
        let label = |text: &'static str| {
            div()
                .text_size(TEXT_BODY)
                .text_color(p.text)
                .child(text)
        };
        let value = |text: String| {
            div()
                .font_family(FONT_MONO)
                .text_size(TEXT_SMALL)
                .text_color(p.muted)
                .child(text)
        };
        let slider_row = |name: &'static str, shown: String, slider: &Entity<_>| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(label(name))
                        .child(value(shown)),
                )
                .child(Slider::new(slider).h(px(22.)))
        };
        let switch_row = |id: &'static str,
                          name: &'static str,
                          on: bool,
                          flip: fn(&mut IconView),
                          cx: &mut Context<Self>| {
            div()
                .h(px(28.))
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Checkbox::new(id)
                        .checked(on)
                        .accessibility_label(name)
                        .on_change(cx.listener(move |this, _: &bool, _, cx| {
                            this.set_icon_view(flip, cx)
                        })),
                )
                .child(label(name))
        };
        let labels = super::settings_view::segmented(
            "icon-labels",
            &["Below", "Right"],
            (options.labels == Labels::Right) as usize,
            self.segment_motion.get("icon-labels").copied(),
            self.motion_enabled,
            p,
            cx,
            move |this, index, _, cx| {
                let from = (this.icon_view_for(this.pane_identity).labels == Labels::Right) as usize;
                this.slide_segment("icon-labels", from, index);
                this.set_icon_view(
                    |o| o.labels = [Labels::Below, Labels::Right][index],
                    cx,
                );
            },
        );
        let rule = || div().h(px(1.)).my_1().bg(p.line);
        let heading = |text: &'static str| {
            div()
                .pt_1()
                .text_size(TEXT_SMALL)
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.muted)
                .child(text)
        };
        let panel = div()
            .id("icon-view-options")
            .role(Role::Dialog)
            .aria_label("Icon view options")
            .occlude()
            .w(px(272.))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .rounded(RADIUS_CARD)
            .bg(cx.theme().popover)
            .border_1()
            .border_color(p.line)
            .shadow_lg()
            .text_color(p.text)
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.icon_options_open = false;
                cx.notify();
            }))
            .child(
                div()
                    .text_size(TEXT_BODY)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Icon view"),
            )
            .child(slider_row(
                "Icon size",
                format!("{} px", options.size),
                &self.icon_size_slider,
            ))
            .child(slider_row(
                "Grid spacing",
                format!("{} px", options.spacing),
                &self.icon_spacing_slider,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label("Labels"))
                    .child(labels),
            )
            .child(rule())
            .child(heading("Show under the name"))
            .child(div().flex().flex_col().mt(px(-6.)).child(switch_row(
                "icon-show-size",
                "Size",
                options.show_size,
                |o| o.show_size = !o.show_size,
                cx,
            ))
            .child(switch_row(
                "icon-show-kind",
                "Kind",
                options.show_kind,
                |o| o.show_kind = !o.show_kind,
                cx,
            ))
            .child(switch_row(
                "icon-show-modified",
                "Date modified",
                options.show_modified,
                |o| o.show_modified = !o.show_modified,
                cx,
            )))
            .child(rule())
            .child(switch_row(
                "icon-thumbnails",
                "Thumbnails",
                options.thumbnails,
                |o| o.thumbnails = !o.thumbnails,
                cx,
            ));
        let panel = if self.motion_enabled {
            panel
                .with_animation(
                    ("icon-options-open", self.icon_options_motion as usize),
                    crate::motion::enter(),
                    |d, t| d.opacity(t).mt(px(6. * (1. - t))),
                )
                .into_any_element()
        } else {
            panel.into_any_element()
        };
        // Under the toolbar, below the view switcher.
        div()
            .absolute()
            .top(px(52.))
            .right(px(150.))
            .child(panel)
            .into_any_element()
    }
}
