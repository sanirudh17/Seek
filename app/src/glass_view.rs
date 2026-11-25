//! Settings > Glass and background, with a live preview, and the palette
//! picker on the Appearance page.
use super::settings_view::{card, control, section, segmented};
use super::*;
use crate::appearance::{PALETTES, Palette, Preset};
use crate::background::{Effect, Scope};
use crate::settings::Blur;
use gpui_component::switch::Switch;

impl Seek {
    /// What sits between the desktop and the content on dark glass: the
    /// picture (by its scope), its edge fade, a soft lift at the top and the
    /// grain. Nothing on light themes or with glass off.
    pub(super) fn glass_layers(&self, p: Palette, window: &Window, cx: &App) -> Vec<AnyElement> {
        let prefs = &self.preferences;
        if !crate::appearance::glass_enabled(prefs, p.text.l > 0.5) {
            return Vec::new();
        }
        let mut layers = Vec::new();
        let empty = self.background_state(None, cx).folder.is_some()
            && self.table.read(cx).delegate().rows.is_empty();
        let opacity = match (prefs.background_scope, empty) {
            (_, true) => prefs.background_empty_opacity.min(80),
            (Scope::All, false) => prefs.background_opacity.min(30),
            (Scope::Empty, false) => 0,
        } as f32
            / 100.;
        if let Some(image) = self.background.clone().filter(|_| opacity > 0.) {
            layers.push(
                img(image)
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .opacity(opacity)
                    .into_any_element(),
            );
            // Fade the picture's edges into the glass: the window's own tint
            // over the top and bottom bands.
            let fade = Hsla { a: 0.9, ..p.canvas };
            let clear = Hsla { a: 0., ..p.canvas };
            for (top, angle) in [(true, 0.), (false, 180.)] {
                layers.push(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .h(relative(0.3))
                        .when(top, |d| d.top_0())
                        .when(!top, |d| d.bottom_0())
                        .bg(linear_gradient(
                            angle,
                            linear_color_stop(clear, 0.),
                            linear_color_stop(fade, 1.),
                        ))
                        .into_any_element(),
                );
            }
        }
        // A soft lift where light would catch the top of the window, in the
        // palette's own hue, so the palette reads even over a grey desktop.
        let hue = p.accent.h;
        let glow = if p.accent.s < 0.08 {
            hsla(0., 0., 1., 0.035)
        } else {
            hsla(
                hue,
                0.7,
                0.55,
                0.09 * prefs.palette_intensity.min(100) as f32 / 100.,
            )
        };
        layers.push(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(240.))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(glow, 0.),
                    linear_color_stop(Hsla { a: 0., ..glow }, 1.),
                ))
                .into_any_element(),
        );
        if prefs.grain {
            let side = crate::background::GRAIN_SIDE as f32;
            let size = window.viewport_size();
            let (cols, rows) = (
                (size.width.as_f32() / side).ceil() as usize,
                (size.height.as_f32() / side).ceil() as usize,
            );
            for row in 0..rows.min(16) {
                for col in 0..cols.min(16) {
                    layers.push(
                        img(self.grain.clone())
                            .absolute()
                            .left(px(col as f32 * side))
                            .top(px(row as f32 * side))
                            .w(px(side))
                            .h(px(side))
                            .into_any_element(),
                    );
                }
            }
        }
        layers
    }

    /// Put the sliders back in step with the settings after a preset or a
    /// palette changed several values at once.
    pub(super) fn sync_sliders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (index, slider) in self.opacity_sliders.clone().iter().enumerate() {
            let value = super::slider_value(&self.preferences, index) as f32;
            slider.update(cx, |state, cx| state.set_value(value, window, cx));
        }
    }

    /// A small window drawn with the same palette as the real one, over a
    /// bright stand-in for the desktop, so the tint and opacity show at once.
    fn glass_preview(&self, p: Palette) -> AnyElement {
        let glass = crate::appearance::glass_enabled(&self.preferences, p.text.l > 0.5);
        let bar = |width: f32, alpha: f32| {
            div()
                .h(px(6.))
                .w(relative(width))
                .rounded(px(3.))
                .bg(p.text.opacity(alpha))
        };
        let row = |width: f32, selected: bool| {
            div()
                .h(px(20.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(5.))
                .when(selected, |d| d.bg(p.text.opacity(0.1)))
                .child(
                    div()
                        .size(px(8.))
                        .rounded(px(2.))
                        .bg(p.tints.blue.opacity(0.8)),
                )
                .child(bar(width, 0.28))
        };
        let picture = self.background.clone().filter(|_| glass).map(|image| {
            img(image)
                .absolute()
                .inset_0()
                .size_full()
                .object_fit(ObjectFit::Cover)
                .opacity(self.preferences.background_opacity.min(30) as f32 / 100.)
        });
        div()
            .id("glass-preview")
            .role(Role::Image)
            .aria_label("Preview of the glass")
            .relative()
            .w_full()
            .h(px(156.))
            .mb_3()
            .rounded(crate::tokens::RADIUS_CARD)
            .overflow_hidden()
            .border_1()
            .border_color(p.text.opacity(0.1))
            .bg(linear_gradient(
                120.,
                linear_color_stop(rgb(0x2b6cb0), 0.),
                linear_color_stop(rgb(0xd97745), 1.),
            ))
            .child(
                div()
                    .absolute()
                    .top(px(18.))
                    .left(px(24.))
                    .right(px(24.))
                    .bottom(px(-1.))
                    .rounded_t(px(10.))
                    .overflow_hidden()
                    .border_1()
                    .border_color(p.edge)
                    .bg(p.canvas)
                    .flex()
                    .child(
                        div()
                            .w(px(120.))
                            .h_full()
                            .bg(p.sidebar)
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(bar(0.5, 0.2))
                            .child(bar(0.8, 0.32))
                            .child(bar(0.7, 0.32))
                            .child(bar(0.75, 0.32)),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .h_full()
                            .bg(p.pane)
                            .children(picture)
                            .child(
                                div()
                                    .relative()
                                    .p_2()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(row(0.45, false))
                                    .child(row(0.6, true))
                                    .child(row(0.35, false))
                                    .child(row(0.5, false)),
                            ),
                    )
                    .when(glass && self.preferences.grain, |d| {
                        d.child(
                            img(self.grain.clone())
                                .absolute()
                                .top_0()
                                .left_0()
                                .w(px(crate::background::GRAIN_SIDE as f32))
                                .h(px(crate::background::GRAIN_SIDE as f32)),
                        )
                    }),
            )
            .into_any_element()
    }

    pub(super) fn glass_settings(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let prefs = &self.preferences;
        let preset = Preset::current(prefs)
            .and_then(|current| Preset::ALL.iter().position(|p| *p == current))
            .unwrap_or(Preset::ALL.len());
        let presets = segmented(
            "glass-preset",
            &Preset::ALL.map(Preset::label),
            preset,
            self.segment_motion.get("glass-preset").copied(),
            self.motion_enabled,
            p,
            cx,
            move |this, index, window, cx| {
                // From custom settings the pill fades in (see `segmented`).
                this.slide_segment("glass-preset", preset, index);
                Preset::ALL[index].apply(&mut this.preferences);
                this.sync_sliders(window, cx);
                this.save_preferences(window, cx);
            },
        );
        let blur_index = (prefs.blur == Blur::Soft) as usize;
        let blur = segmented(
            "glass-blur",
            &["Strong", "Soft"],
            blur_index,
            self.segment_motion.get("glass-blur").copied(),
            self.motion_enabled,
            p,
            cx,
            move |this, index, window, cx| {
                this.slide_segment("glass-blur", blur_index, index);
                this.preferences.blur = [Blur::Strong, Blur::Soft][index];
                this.save_preferences(window, cx);
            },
        );
        let switch = |id: &'static str, label: &'static str, on: bool| {
            Switch::new(id)
                .color(p.accent)
                .gap_4()
                .py_1()
                .checked(on)
                .accessibility_label(label)
        };
        let pane_glass = switch(
            "main-pane-glass",
            "Glass in the file list",
            prefs.main_pane_glass,
        )
        .on_change(cx.listener(|this, checked, window, cx| {
            this.preferences.main_pane_glass = *checked;
            this.save_preferences(window, cx);
        }));
        let grain = switch("glass-grain", "Grain", prefs.grain).on_change(cx.listener(
            |this, checked, window, cx| {
                this.preferences.grain = *checked;
                this.save_preferences(window, cx);
            },
        ));
        let tint = div()
            .w_full()
            .flex()
            .flex_wrap()
            .gap_x_6()
            .gap_y_3()
            .child(self.opacity_choice("Hue", p, cx))
            .child(self.opacity_choice("Saturation", p, cx))
            .child(self.opacity_choice("Darkness", p, cx));

        let effect_index = Effect::ALL
            .iter()
            .position(|e| *e == prefs.background_effect)
            .unwrap_or(0);
        let effect = segmented(
            "background-effect",
            &Effect::ALL.map(Effect::label),
            effect_index,
            self.segment_motion.get("background-effect").copied(),
            self.motion_enabled,
            p,
            cx,
            move |this, index, window, cx| {
                this.slide_segment("background-effect", effect_index, index);
                this.preferences.background_effect = Effect::ALL[index];
                this.save_preferences(window, cx);
            },
        );
        let scope_index = (prefs.background_scope == Scope::Empty) as usize;
        let scope = segmented(
            "background-scope",
            &["Every folder", "Empty only"],
            scope_index,
            self.segment_motion.get("background-scope").copied(),
            self.motion_enabled,
            p,
            cx,
            move |this, index, window, cx| {
                this.slide_segment("background-scope", scope_index, index);
                this.preferences.background_scope = [Scope::All, Scope::Empty][index];
                this.save_preferences(window, cx);
            },
        );
        let picture_name = prefs
            .background_image
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned());
        let has_picture = picture_name.is_some();
        let picture = div()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(if has_picture { p.text } else { p.muted })
                    .child(picture_name.unwrap_or_else(|| "None".into())),
            )
            .when(has_picture, |d| {
                d.child(
                    control("remove-background", "Remove", p, cx).on_click(cx.listener(
                        |this, _, window, cx| {
                            this.preferences.background_image = None;
                            this.save_preferences(window, cx);
                        },
                    )),
                )
            })
            .child(
                control("choose-background", "Choose…", p, cx).on_click(cx.listener(
                    |_, _, window, cx| {
                        let paths = cx.prompt_for_paths(PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: false,
                            prompt: Some("Choose a picture".into()),
                        });
                        cx.spawn_in(window, async move |this, cx| {
                            if let Ok(Ok(Some(paths))) = paths.await {
                                let _ = this.update_in(cx, |this, window, cx| {
                                    this.preferences.background_image = paths.first().cloned();
                                    this.save_preferences(window, cx);
                                });
                            }
                        })
                        .detach();
                    },
                )),
            );

        let subheading = |title: &'static str| {
            div()
                .mt_6()
                .mb_3()
                .text_size(crate::tokens::TEXT_TITLE)
                .font_weight(FontWeight::SEMIBOLD)
                .child(title)
        };
        section("Glass and background")
            .child(self.glass_preview(p))
            .child(card(
                "Style",
                "Clear shows the most of the desktop; Solid turns glass off. Dark themes only: light themes stay opaque, as do Windows high contrast and transparency off.",
                presets,
                p,
            ))
            .child(card(
                "Tint",
                "The colour of the glass. Darkness 0 is black; the presets keep the palette's hue.",
                tint,
                p,
            ))
            .child(card(
                "Window opacity",
                "How much of the tint covers the desktop.",
                self.opacity_choice("Window opacity", p, cx),
                p,
            ))
            .child(card(
                "Sidebar opacity",
                "Extra tint behind the sidebar. Together with the window's, it never covers more than 92%.",
                self.opacity_choice("Sidebar opacity", p, cx),
                p,
            ))
            .child(card(
                "Glass in the file list",
                "Off gives the file list a solid reading surface.",
                pane_glass,
                p,
            ))
            .child(card(
                "Blur",
                "Strong blurs whatever is behind the window (acrylic). Soft shows only the blurred desktop picture (Mica).",
                blur,
                p,
            ))
            .child(card("Grain", "A fine noise over the glass.", grain, p))
            .child(subheading("Background"))
            .child(card(
                "Picture",
                "A local PNG, JPEG, WebP, GIF or BMP, up to 16 MiB and 16 million pixels. Your file is never changed. Dark glass only.",
                picture,
                p,
            ))
            .child(card(
                "Effect",
                "Baked into a copy once, off the interface thread.",
                effect,
                p,
            ))
            .child(card("Show in", "", scope, p))
            .child(card(
                "Behind files",
                "Opacity in folders and results with files.",
                self.opacity_choice("Behind files", p, cx),
                p,
            ))
            .child(card(
                "In empty folders",
                "Opacity where there is nothing to read over.",
                self.opacity_choice("In empty folders", p, cx),
                p,
            ))
            .into_any_element()
    }

    /// One swatch per named palette: its glass, its text and its accent.
    /// Palette tiles: a small window in the palette's colours, as it looks
    /// in the current theme (sidebar, file list, a selected row in the
    /// accent), with the name below.
    pub(super) fn palette_picker(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut grid = div().w_full().grid().grid_cols(4).gap_3();
        let dark = p.text.l > 0.5;
        for (index, named) in PALETTES.iter().enumerate() {
            let (sidebar, pane, text, accent) = if dark {
                let surface = Hsla::from(rgb(named.dark_surface));
                (
                    surface,
                    Hsla {
                        l: surface.l + 0.035,
                        ..surface
                    },
                    named.dark_text,
                    named.dark_accent,
                )
            } else {
                let surface = Hsla::from(rgb(named.light_sidebar));
                (
                    Hsla {
                        l: surface.l - 0.03,
                        s: (surface.s * 1.6).min(1.),
                        ..surface
                    },
                    Hsla {
                        l: surface.l + (1. - surface.l) * 0.75,
                        ..surface
                    },
                    named.light_text,
                    named.light_accent,
                )
            };
            let text = Hsla::from(rgb(text));
            let accent = Hsla::from(rgb(accent));
            let chosen = self.preferences.palette == named.id;
            let id = named.id;
            let line = |width: f32, alpha: f32| {
                div()
                    .h(px(4.))
                    .w(relative(width))
                    .rounded(px(2.))
                    .bg(text.opacity(alpha))
            };
            let window = div()
                .h(px(72.))
                .w_full()
                .flex()
                .overflow_hidden()
                .rounded(px(9.))
                .child(
                    div()
                        .w(px(38.))
                        .h_full()
                        .bg(sidebar)
                        .px(px(6.))
                        .py(px(8.))
                        .flex()
                        .flex_col()
                        .gap(px(5.))
                        .child(div().size(px(6.)).rounded_full().bg(accent))
                        .child(line(0.9, 0.3))
                        .child(line(0.7, 0.3)),
                )
                .child(
                    div()
                        .flex_1()
                        .h_full()
                        .bg(pane)
                        .px(px(8.))
                        .py(px(8.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(line(0.75, 0.55))
                        .child(
                            div()
                                .h(px(10.))
                                .w_full()
                                .rounded(px(3.))
                                .bg(accent.opacity(0.3))
                                .px(px(4.))
                                .flex()
                                .items_center()
                                .child(line(0.6, 0.7)),
                        )
                        .child(line(0.55, 0.3)),
                );
            grid = grid.child(
                div()
                    .id(("palette", index))
                    .role(Role::RadioButton)
                    .aria_label(named.name)
                    .aria_selected(chosen)
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .cursor_pointer()
                    .child(
                        // The ring sits outside the preview, so the chosen
                        // palette reads by its outline as well as its name.
                        div()
                            .p(px(3.))
                            .rounded(px(12.))
                            .border_2()
                            .border_color(if chosen {
                                accent
                            } else {
                                p.text.opacity(0.08)
                            })
                            .hover(move |d| {
                                if chosen {
                                    d
                                } else {
                                    d.border_color(p.text.opacity(0.22))
                                }
                            })
                            .child(window),
                    )
                    .child(
                        div()
                            .px_1()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(crate::tokens::TEXT_SMALL)
                            .text_color(if chosen { p.text } else { p.muted })
                            .when(chosen, |d| d.font_weight(FontWeight::MEDIUM))
                            .child(named.name)
                            .when(chosen, |d| {
                                d.child(
                                    Icon::new(gpui_component_assets::IconName::Check)
                                        .size(px(12.))
                                        .text_color(accent),
                                )
                            }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        crate::appearance::apply_palette(&mut this.preferences, id);
                        this.sync_sliders(window, cx);
                        this.save_preferences(window, cx);
                    })),
            );
        }
        grid.into_any_element()
    }
}
