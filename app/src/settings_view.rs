use super::*;
use crate::appearance::Palette;
use gpui_component::{
    Selectable,
    button::ButtonCustomVariant,
    color_picker::ColorPicker,
    menu::{DropdownMenu, PopupMenuItem},
    switch::Switch,
};
use gpui_component_assets::IconName as Glyph;

pub(super) fn control(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    p: Palette,
    cx: &App,
) -> Button {
    let label = label.into();
    let dark = p.text.l > 0.5;
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(p.control)
                .foreground(p.text)
                .hover(p.control_hover)
                .active(p.control_selected),
        )
        .with_size(px(34.))
        .h(px(34.))
        .px_3()
        .py_0()
        .line_height(px(16.))
        .rounded(px(8.))
        .border_1()
        .border_color(p.line)
        .bg(linear_gradient(
            180.,
            linear_color_stop(
                Hsla::from(if dark {
                    rgba(0xffffff0e)
                } else {
                    rgba(0xffffffb0)
                }),
                0.,
            ),
            linear_color_stop(
                Hsla::from(if dark {
                    rgba(0xffffff03)
                } else {
                    rgba(0xffffff45)
                }),
                1.,
            ),
        ))
        .font_weight(FontWeight::NORMAL)
        .when(!label.is_empty(), |b| {
            b.accessibility_label(label.clone()).child(
                div()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .child(label),
            )
        })
}

fn choice_control(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    p: Palette,
    cx: &App,
) -> Button {
    let label = label.into();
    control(id, "", p, cx)
        .accessibility_label(label.clone())
        .child(
            div()
                .relative()
                .size_full()
                .flex()
                .items_center()
                .justify_start()
                .text_size(px(13.))
                .line_height(px(20.))
                .child(label)
                .child(
                    Icon::new(Glyph::ChevronDown)
                        .size(px(12.))
                        .absolute()
                        .right_0(),
                ),
        )
}

fn card(
    label: &'static str,
    description: &'static str,
    content: impl IntoElement,
    p: Palette,
) -> Div {
    div()
        .w_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded(px(12.))
        .border_1()
        .border_color(p.line)
        .bg(p.settings_card)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(label),
                )
                .when(!description.is_empty(), |d| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(p.muted)
                            .child(description),
                    )
                }),
        )
        .child(content)
}

fn section(title: &'static str) -> Div {
    div().w_full().flex().flex_col().gap_3().child(
        div()
            .text_size(px(18.))
            .font_weight(FontWeight::SEMIBOLD)
            .mb_1()
            .child(title),
    )
}

impl Seek {
    pub(super) fn settings_sheet(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut themes = div().flex().gap_2();
        for (id, label, icon, theme) in [
            (
                "theme-system",
                "System",
                "icons/monitor.svg",
                Appearance::System,
            ),
            ("theme-light", "Light", "icons/sun.svg", Appearance::Light),
            ("theme-dark", "Dark", "icons/moon.svg", Appearance::Dark),
        ] {
            let selected = self.preferences.theme == theme;
            themes = themes.child(
                control(id, "", p, cx)
                    .w(px(100.))
                    .accessibility_label(label)
                    .child(
                        div()
                            .h_full()
                            .flex()
                            .items_center()
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .gap_2()
                            .child(Icon::default().path(icon).size(px(16.)).flex_shrink_0())
                            .child(label),
                    )
                    .selected(selected)
                    .toggled(selected)
                    .when(selected, |b| {
                        b.bg(p.control_selected).border_color(p.muted.opacity(0.5))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.preferences.theme = theme;
                        this.save_preferences(window, cx);
                    })),
            );
        }
        let glass = Switch::new("glass-toggle")
            .checked(self.preferences.glass)
            .color(p.accent)
            .label(if self.preferences.glass { "On" } else { "Off" })
            .accessibility_label(if self.preferences.glass {
                "Glass: on"
            } else {
                "Glass: off"
            })
            .on_change(cx.listener(|this, checked, window, cx| {
                this.preferences.glass = *checked;
                this.save_preferences(window, cx);
            }));
        let mut swatches = div().flex().gap_2().items_center();
        for (i, (name, value)) in [
            ("Graphite", "#909090"),
            ("Blue", "#006fe5"),
            ("Purple", "#8250c4"),
            ("Rose", "#c84476"),
            ("Green", "#267e58"),
            ("Orange", "#b66a20"),
        ]
        .into_iter()
        .enumerate()
        {
            let selected = self.preferences.accent.eq_ignore_ascii_case(value);
            let color = rgb(crate::settings::accent(value).unwrap());
            swatches = swatches.child(
                control(("accent", i), "", p, cx)
                    .w(px(32.))
                    .h(px(32.))
                    .p_0()
                    .rounded(px(9.))
                    .accessibility_label(name)
                    .tooltip(name)
                    .selected(selected)
                    .bg(rgba(0))
                    .border_1()
                    .border_color(if selected {
                        p.muted.opacity(0.65)
                    } else {
                        rgba(0).into()
                    })
                    .child(
                        div()
                            .size(px(24.))
                            .flex_shrink_0()
                            .rounded(px(6.))
                            .bg(color),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.preferences.accent = value.into();
                        this.accent_input
                            .update(cx, |input, cx| input.set_value(value, window, cx));
                        this.color_picker
                            .update(cx, |picker, cx| picker.set_value(color, window, cx));
                        this.save_preferences(window, cx);
                    })),
            );
        }
        let custom_accent = div()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.accent_input).h(px(34.))),
            )
            .child(
                div()
                    .size(px(34.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(p.line)
                    .bg(p.control)
                    .child(
                        ColorPicker::new(&self.color_picker)
                            .icon(Icon::default().path("icons/pipette.svg").size(px(16.)))
                            .accessibility_label("Choose accent color")
                            .small(),
                    ),
            )
            .child(
                control("apply-accent", "Apply", p, cx)
                    .w(px(76.))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let value = this.accent_input.read(cx).value().to_string();
                        if let Some(color) = crate::settings::accent(&value) {
                            this.preferences.accent = format!("#{color:06x}");
                            this.color_picker
                                .update(cx, |picker, cx| picker.set_value(rgb(color), window, cx));
                            this.message = None;
                            this.save_preferences(window, cx);
                        } else {
                            this.message = Some("Use a six-digit color, such as #909090.".into());
                            cx.notify();
                        }
                    })),
            );
        let appearance = section("Appearance")
            .child(card(
                "Theme",
                "System follows Windows theme mode.",
                themes,
                p,
            ))
            .child(card(
                "Acrylic glass",
                "Blur the desktop behind Seek. Windows transparency must be enabled.",
                glass,
                p,
            ))
            .child(card(
                "Accent color",
                "Highlight color for selections and filters.",
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(swatches)
                    .child(custom_accent),
                p,
            ));

        let owner = cx.entity().downgrade();
        let show_hidden = self.show_hidden;
        let hidden = choice_control(
            "settings-hidden",
            if show_hidden {
                "Hidden files: shown"
            } else {
                "Hidden files: hidden"
            },
            p,
            cx,
        )
        .w(px(188.))
        .dropdown_menu(move |menu, window, _| {
            let menu = super::daily::menu_style(menu, window);
            let shown_owner = owner.clone();
            let hidden_owner = owner.clone();
            menu.item(PopupMenuItem::new("Shown").checked(show_hidden).on_click(
                move |_, window, cx| {
                    let _ = shown_owner.update(cx, |this, cx| {
                        this.show_hidden = true;
                        this.save_preferences(window, cx);
                        this.submit(cx);
                    });
                },
            ))
            .item(PopupMenuItem::new("Hidden").checked(!show_hidden).on_click(
                move |_, window, cx| {
                    let _ = hidden_owner.update(cx, |this, cx| {
                        this.show_hidden = false;
                        this.save_preferences(window, cx);
                        this.submit(cx);
                    });
                },
            ))
        });
        let mut views = div()
            .id("default-view-options")
            .role(Role::Group)
            .aria_label("Default view options")
            .w(px(242.))
            .flex_shrink_0()
            .flex()
            .rounded(px(8.))
            .border_1()
            .border_color(p.line)
            .overflow_hidden();
        for (i, (id, label, view)) in [
            ("default-list", "List", View::List),
            ("default-icons", "Icons", View::Icons),
            ("default-gallery", "Gallery", View::Gallery),
        ]
        .into_iter()
        .enumerate()
        {
            views = views.child(
                control(id, label, p, cx)
                    .w(px(80.))
                    .rounded(px(0.))
                    .when(i == 0, |b| b.rounded_tl(px(7.)).rounded_bl(px(7.)))
                    .when(i == 2, |b| b.rounded_tr(px(7.)).rounded_br(px(7.)))
                    .border_0()
                    .when(i != 0, |b| b.border_l_1().border_color(p.line))
                    .selected(self.preferences.default_view == view)
                    .toggled(self.preferences.default_view == view)
                    .when(self.preferences.default_view == view, |b| {
                        b.bg(p.control_selected).rounded(px(7.))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.preferences.default_view = view;
                        this.set_view(view, cx);
                        this.save_preferences(window, cx);
                    })),
            );
        }
        let content_locations = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(Input::new(&self.content_locations_input).large().w_full())
            .child(
                control("save-content-locations", "Save content locations", p, cx).on_click(
                    cx.listener(|this, _, window, cx| {
                        let paths = this
                            .content_locations_input
                            .read(cx)
                            .value()
                            .split(';')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(PathBuf::from)
                            .collect::<Vec<_>>();
                        if paths.len() > 64
                            || paths
                                .iter()
                                .any(|p| !p.is_absolute() || crate::content::secret(p))
                        {
                            this.message = Some(
                                "Choose up to 64 full folder paths outside protected locations."
                                    .into(),
                            );
                            cx.notify();
                            return;
                        }
                        this.preferences.content_locations = paths;
                        this.save_preferences(window, cx);
                        this.submit(cx);
                    }),
                ),
            );
        let document_text = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                Switch::new("remember-document-text")
                    .checked(self.preferences.remember_document_text)
                    .color(p.accent)
                    .label(if self.preferences.remember_document_text {
                        "On"
                    } else {
                        "Off"
                    })
                    .accessibility_label("Remember document text for faster content search")
                    .on_change(cx.listener(|this, checked, window, cx| {
                        this.preferences.remember_document_text = *checked;
                        this.save_preferences(window, cx);
                        this.submit(cx);
                    })),
            )
            .child(
                control("clear-document-text", "Clear cache", p, cx).on_click(cx.listener(
                    |this, _, _, cx| {
                        let generation =
                            this.backend.generation.fetch_add(1, Ordering::Relaxed) + 1;
                        let _ = this
                            .backend
                            .tx
                            .send(Command::ClearDocumentCache(generation));
                        this.message = Some("Clearing saved document text...".into());
                        cx.notify();
                    },
                )),
            );
        let files = section("Files and search")
            .child(card(
                "Hidden and system files",
                "Visibility toggle; items remain indexed.",
                hidden,
                p,
            ))
            .child(card(
                "Default view",
                "Layout mode when opening Seek.",
                views,
                p,
            ))
            .child(card("Content search locations", "Downloads and Documents by default. Separate folders with semicolons. Leave empty to disable. Text, code, Markdown, DOCX and PDFs with a text filter; protected files and links are skipped.", content_locations, p))
            .child(card("Remember document text for faster content search", "Saved until the file changes, is removed or leaves your search locations. Limit: 128 MiB; least recently used entries are removed when full. Encrypted for your Windows account. Turning this off removes saved text.", document_text, p));

        let owner = cx.entity().downgrade();
        let custom = self.custom_locations;
        let location_mode = choice_control(
            "location-mode",
            if custom {
                "Custom folders"
            } else {
                "All drives (default)"
            },
            p,
            cx,
        )
        .w_full()
        .dropdown_menu(move |menu, window, _| {
            let menu = super::daily::menu_style(menu, window);
            let all_owner = owner.clone();
            let custom_owner = owner.clone();
            menu.item(
                PopupMenuItem::new("All drives (default)")
                    .checked(!custom)
                    .on_click(move |_, window, cx| {
                        let _ = all_owner.update(cx, |this, cx| {
                            this.custom_locations = false;
                            this.locations_input
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            cx.notify();
                        });
                    }),
            )
            .item(
                PopupMenuItem::new("Custom folders")
                    .checked(custom)
                    .on_click(move |_, window, cx| {
                        let _ = custom_owner.update(cx, |this, cx| {
                            this.custom_locations = true;
                            this.locations_input
                                .update(cx, |input, cx| input.focus(window, cx));
                            cx.notify();
                        });
                    }),
            )
        });
        let locations = section("Indexed locations")
            .child(div().text_size(px(12.)).text_color(p.muted)
                .child("Leave blank to index all available drives. Separate multiple folders with semicolons (;). Changes take effect on next launch."))
            .child(card("Indexed folders", "Folders and drives to include in the index.",
                div().w_full().flex().flex_col().gap_2().child(location_mode)
                    .when(custom, |d| d.child(Input::new(&self.locations_input).h(px(34.)))), p))
            .child(card("Excluded folders", "Hide these folders from results. Separate multiple folders with semicolons (;).",
                Input::new(&self.exclusions_input).h(px(34.)), p))
            .child(div().flex().gap_2().pt_2()
                .child(control("save-locations", "Save locations", p, cx)
                    .px_5()
                    .on_click(cx.listener(|this, _, window, cx| {
                        let parse = |value: SharedString| value.split(';').map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from).collect::<Vec<_>>();
                        let locations = parse(this.locations_input.read(cx).value());
                        let exclusions = parse(this.exclusions_input.read(cx).value());
                        if locations.iter().chain(&exclusions).any(|p| !p.is_absolute()) {
                            this.message = Some("Use absolute paths to existing folders.".into());
                            cx.notify(); return;
                        }
                        if locations.len() > 64 || exclusions.len() > 256 {
                            this.message = Some("Choose at most 64 locations and 256 exclusions.".into());
                            cx.notify(); return;
                        }
                        this.custom_locations = !locations.is_empty();
                        this.preferences.indexed_locations = locations;
                        this.preferences.exclusions = exclusions;
                        this.save_preferences(window, cx);
                        this.submit(cx);
                        this.message = Some("Saved. Indexed locations apply next launch.".into());
                    })))
                .child(control("rebuild-index", "Rebuild current index", p, cx)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let _ = this.backend.tx.send(Command::Rebuild);
                        this.message = Some("Rebuilding the current index…".into());
                        cx.notify();
                    }))));
        let helper = section("Faster indexing").child(card(
            if self.helper.installed {
                "Helper installed"
            } else {
                "Optional helper"
            },
            "Catch up through the NTFS journal. Installation asks for administrator approval.",
            control(
                "helper-toggle",
                if self.helper.busy {
                    "Working…"
                } else if self.helper.installed {
                    "Uninstall helper"
                } else {
                    "Install helper"
                },
                p,
                cx,
            )
            .w(px(160.))
            .disabled(self.helper.busy)
            .on_click(cx.listener(|this, _, _, _| {
                if !this.helper.busy {
                    let _ = this
                        .backend
                        .tx
                        .send(Command::HelperAction(!this.helper.installed));
                }
            })),
            p,
        ));

        let updates = section("Updates")
            .child(card("Check for updates on startup", "Check GitHub once when Seek opens. No background service or scheduled task.",
                Switch::new("check-updates-on-startup")
                    .checked(self.preferences.check_updates_on_startup)
                    .color(p.accent)
                    .label(if self.preferences.check_updates_on_startup {"On"} else {"Off"})
                    .accessibility_label("Check for updates on startup")
                    .on_change(cx.listener(|this, checked, window, cx| {
                        this.preferences.check_updates_on_startup = *checked;
                        this.save_preferences(window, cx);
                    })), p))
            .child(card("Seek", "Updates preserve your saved index, settings and document text. Installing closes all tabs and restarts Seek.",
                div().flex().flex_col().gap_3()
                    .child(div().text_size(px(13.)).child(format!("Installed version: {}", crate::updater::CURRENT)))
                    .child(div().text_size(px(12.)).text_color(p.muted).child(self.app_updates.message.clone()))
                    .child(div().flex().gap_2()
                        .child(control("check-updates-now", "Check now", p, cx)
                            .disabled(self.app_updates.busy)
                            .on_click(cx.listener(|this, _, _, cx|this.check_for_updates(cx))))
                        .when(self.app_updates.available.is_some(), |d|d.child(
                            control("install-app-update", "Download and restart", p, cx)
                                .disabled(self.app_updates.busy)
                                .on_click(cx.listener(|this, _, _, cx|this.install_app_update(cx)))))), p));
        let mut nav = div()
            .w(px(200.))
            .flex_shrink_0()
            .h_full()
            .p_3()
            .border_r_1()
            .border_color(p.line)
            .flex()
            .flex_col()
            .gap_1();
        for (i, (label, icon)) in [
            ("Appearance", Glyph::Sun),
            ("Files and search", Glyph::Search),
            ("Locations", Glyph::Folder),
            ("Helper", Glyph::HardDrive),
            ("Updates", Glyph::RefreshCw),
        ]
        .into_iter()
        .enumerate()
        {
            nav = nav.child(
                control(("settings-category", i), "", p, cx)
                    .border_0()
                    .w_full()
                    .h(px(40.))
                    .px_3()
                    .py_0()
                    .rounded(px(9.))
                    .text_color(p.text)
                    .accessibility_label(label)
                    .bg(if self.settings_tab == i {
                        p.control_selected
                    } else {
                        rgba(0).into()
                    })
                    .child(
                        div()
                            .w_full()
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .size(px(20.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(Icon::new(icon).size(px(18.))),
                            )
                            .child(
                                div()
                                    .h(px(20.))
                                    .line_height(px(20.))
                                    .text_size(px(13.))
                                    .child(label),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_tab = i;
                        this.message = None;
                        cx.notify();
                    })),
            );
        }
        let content = match self.settings_tab {
            1 => files,
            2 => locations,
            3 => helper,
            4 => updates,
            _ => appearance,
        };
        let modal = div()
            .id("settings-sheet")
            .occlude()
            .track_focus(&self.settings_focus)
            .w(px(820.))
            .max_w(relative(0.94))
            .h(px(620.))
            .max_h(relative(0.92))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(16.))
            .bg(p.sheet)
            .text_color(p.text)
            .border_1()
            .border_color(p.line)
            .shadow(vec![BoxShadow {
                inset: false,
                color: rgba(0x00000024).into(),
                offset: point(px(0.), px(12.)),
                blur_radius: px(32.),
                spread_radius: px(0.),
            }])
            .child(
                div()
                    .h(px(54.))
                    .flex_shrink_0()
                    .px_5()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .text_size(px(19.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Settings"),
                    )
                    .child(
                        control("close-settings", "Done", p, cx)
                            .w(px(68.))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.settings = false;
                                this.suggestions = None;
                                this.input.update(cx, |input, cx| input.focus(window, cx));
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div().flex_1().min_h_0().flex().child(nav).child(
                    div()
                        .id("settings-scroll")
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .overflow_y_scroll()
                        .p_5()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(content)
                        .when(self.settings_tab == 3, |d| {
                            d.when_some(self.helper.message.clone(), |d, msg| {
                                d.child(div().text_size(px(12.)).text_color(p.muted).child(msg))
                            })
                        })
                        .when_some(self.message.clone(), |d, msg| {
                            d.child(div().text_size(px(12.)).text_color(p.muted).child(msg))
                        }),
                ),
            );
        div()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000028))
            .child(modal)
            .into_any_element()
    }
}
