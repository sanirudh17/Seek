use super::*;
use crate::appearance::Palette;
use gpui_component::{
    button::ButtonCustomVariant,
    menu::{DropdownMenu, PopupMenuItem},
    switch::Switch,
};
use gpui_component_assets::IconName as Glyph;

pub(super) const SETTINGS_PAGES: [(usize, &str, Glyph, &str); 11] = [
    (
        0,
        "Appearance",
        Glyph::Sun,
        "theme system dark light accent colour color palette graphite harbor moss ember orchid tidepool dune rosewood",
    ),
    (
        5,
        "Glass and background",
        Glyph::Moon,
        "acrylic mica transparency opacity sidebar main pane tint hue saturation darkness preset clear frost tinted solid grain blur wallpaper image picture png jpeg effect dither ascii halftone scanlines haze empty",
    ),
    (
        1,
        "Files and search",
        Glyph::Search,
        "hidden default view content document cache text explorer context menu open in seek show in seek default file manager win+e drag",
    ),
    (
        7,
        "Text editing",
        Glyph::FileText,
        "auto save autosave backup recovery editor code markdown",
    ),
    (
        9,
        "Previews",
        Glyph::Eye,
        "automatic information panel image thumbnails quick look markdown images gallery large preview env secrets hide mask reveal",
    ),
    (
        10,
        "Keyboard shortcuts",
        Glyph::Command,
        "keys help F1 navigation files preview command palette",
    ),
    (
        8,
        "Saved commands",
        Glyph::Terminal,
        "program executable arguments selection launch",
    ),
    (
        2,
        "Locations",
        Glyph::Folder,
        "index include exclude folders",
    ),
    (
        3,
        "Helper",
        Glyph::HardDrive,
        "service faster indexing administrator",
    ),
    (
        11,
        "Agents",
        Glyph::SquareTerminal,
        "mcp coding agent claude codex antigravity cursor pi grok connect allowed folders activity log",
    ),
    (
        4,
        "Updates",
        Glyph::RefreshCw,
        "version github download restart startup report bug issue feedback",
    ),
];

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
        .with_size(px(32.))
        .h(px(32.))
        .px_3()
        .py_0()
        .line_height(px(16.))
        .rounded(px(8.))
        .border_1()
        .border_color(p.line)
        .bg(if dark {
            Hsla::from(rgba(0xffffff0a))
        } else {
            Hsla::from(rgba(0x0000000a))
        })
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

pub(super) fn choice_control(
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

pub(super) fn card(
    label: &'static str,
    description: &'static str,
    content: impl IntoElement,
    p: Palette,
) -> Div {
    let stacked = matches!(
        label,
        "Content search locations"
            | "Indexed folders"
            | "Excluded folders"
            | "Background"
            | "Faster indexing"
            | "Picture"
            | "Effect"
            | "Tint"
            | "Palette"
            | "Folders agents may change"
    );
    div()
        .w_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .when(stacked, |d| d.flex_col().items_start())
        .gap_6()
        .px_5()
        .py_4()
        .min_h(px(64.))
        .mb_3()
        .rounded(crate::tokens::RADIUS_CARD)
        .border_1()
        .border_color(p.text.opacity(0.1))
        .bg(p.settings_card)
        .child(
            div()
                .min_w_0()
                .flex_1()
                .when(stacked, |d| d.w_full())
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .id(SharedString::from(format!("setting-{label}")))
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(label)
                        // The explanation stays one hover away instead of a
                        // sentence under every setting.
                        .when(!description.is_empty(), |d| {
                            // A compact box that wraps, not one long line.
                            d.tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::element(move |_, _| {
                                    div()
                                        .max_w(px(300.))
                                        .whitespace_normal()
                                        .line_height(px(17.))
                                        .child(description)
                                })
                                .build(window, cx)
                            })
                        }),
                ),
        )
        .child(
            div()
                .min_w_0()
                .when(stacked, |d| d.w_full())
                .when(!stacked, |d| d.max_w(relative(0.6)))
                .child(content),
        )
}

/// Opens `file` in Notepad, in one window: a click while that window is
/// open (or still opening) brings it forward instead of starting another.
fn edit_in_notepad(file: &std::path::Path) -> Result<(), String> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };
    static LAUNCHED: Mutex<Option<Instant>> = Mutex::new(None);
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Notepad titles its window after the file, with * while unsaved.
    for title in [format!("{name} - Notepad"), format!("*{name} - Notepad")] {
        let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
        let window = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        if !window.is_null() {
            unsafe {
                if IsIconic(window) != 0 {
                    ShowWindow(window, SW_RESTORE);
                }
                SetForegroundWindow(window);
            }
            return Ok(());
        }
    }
    let mut launched = LAUNCHED.lock().unwrap();
    if launched.is_some_and(|at| at.elapsed() < Duration::from_secs(3)) {
        return Ok(());
    }
    // Notepad can't read a verbatim (\\?\) path and opens blank instead.
    let path = file.to_string_lossy();
    let path = path.strip_prefix(r"\\?\").unwrap_or(&path);
    // Through the shell, as Start menu and Run do: Notepad from the Store
    // opens blank when started as a child process.
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (verb, program, quoted) = (wide("open"), wide("notepad.exe"), wide(&format!("\"{path}\"")));
    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            program.as_ptr(),
            quoted.as_ptr(),
            std::ptr::null(),
            windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    } as isize;
    if result <= 32 {
        return Err(format!("Could not open Notepad (shell error {result})"));
    }
    *launched = Some(Instant::now());
    Ok(())
}

/// Width of one segment in a segmented control.
const SEGMENT: f32 = 92.;
/// A segmented control whose raised indicator slides to the chosen option
/// on the motion spring. `motion` is the option it slides from and a
/// generation that restarts the slide.
#[allow(clippy::too_many_arguments)]
pub(super) fn segmented(
    id: &'static str,
    labels: &[&'static str],
    selected: usize,
    motion: Option<(usize, u64)>,
    motion_on: bool,
    p: Palette,
    cx: &mut Context<Seek>,
    on_select: impl Fn(&mut Seek, usize, &mut Window, &mut Context<Seek>) + 'static,
) -> AnyElement {
    use crate::tokens::*;
    let on_select = std::rc::Rc::new(on_select);
    // Segments share one width, wide enough for the longest label with
    // breathing room on both sides.
    let longest = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let segment = SEGMENT.max(longest as f32 * 7.4 + 36.);
    let dark = p.text.l > 0.5;
    let count = labels.len();
    // Positions are fractions of the track, so the control can shrink to fit
    // a narrow card instead of spilling past its edge.
    let share = 1. / count.max(1) as f32;
    let indicator = div()
        .absolute()
        .top_0()
        .h_full()
        .w(relative(share))
        .rounded(px(7.))
        .bg(if dark {
            Hsla::from(rgba(0xffffff1f))
        } else {
            Hsla::from(rgba(0xffffffff))
        })
        .border_1()
        .border_color(p.text.opacity(if dark { 0.14 } else { 0.1 }))
        .shadow_sm();
    let start = move |index: f32| relative(index * share);
    let indicator: AnyElement = match motion.filter(|(from, _)| motion_on && *from != selected) {
        // Nothing was selected (custom glass): the pill fades in where it lands.
        Some((from, generation)) if from >= count => indicator
            .left(start(selected as f32))
            .with_animation(
                SharedString::from(format!("{id}-appear-{generation}")),
                crate::motion::reversible(),
                |d, t| d.opacity(t),
            )
            .into_any_element(),
        Some((from, generation)) => indicator
            .left(start(selected as f32))
            .with_animation(
                SharedString::from(format!("{id}-slide-{generation}")),
                crate::motion::reversible(),
                move |d, t| {
                    d.left(start(from as f32 + (selected as f32 - from as f32) * t))
                },
            )
            .into_any_element(),
        None => indicator.left(start(selected as f32)).into_any_element(),
    };
    let mut segments = div()
        .relative()
        .size_full()
        .flex()
        // No indicator when nothing matches (custom glass settings).
        .when(selected < count, |inner| inner.child(indicator));
    for (index, label) in labels.iter().enumerate() {
        let on_select = on_select.clone();
        let chosen = index == selected;
        segments = segments.child(
            div()
                .id((id, index))
                .role(Role::RadioButton)
                .aria_label(*label)
                .aria_selected(chosen)
                .relative()
                .flex_1()
                .flex_basis(px(0.))
                .min_w_0()
                .h_full()
                .px_1()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_size(TEXT_BODY)
                .font_weight(if chosen {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if chosen { p.text } else { p.muted })
                .when(!chosen, |d| d.hover(move |s| s.text_color(p.text)))
                .child(div().min_w_0().truncate().child(*label))
                .on_click(
                    cx.listener(move |this, _, window, cx| on_select(this, index, window, cx)),
                ),
        );
    }
    div()
        .id(id)
        .role(Role::RadioGroup)
        .min_w_0()
        .h(px(36.))
        // Its natural width, but never wider than the space it is given.
        .w(px(segment * count as f32 + 8.))
        .max_w_full()
        .p(px(3.))
        .rounded(px(10.))
        .border_1()
        .border_color(p.text.opacity(0.08))
        .bg(p.text.opacity(if dark { 0.04 } else { 0.05 }))
        .child(segments)
        .into_any_element()
}

pub(super) fn section(title: &'static str) -> Div {
    div().w_full().flex().flex_col().gap_0().child(
        div()
            .text_size(px(20.))
            .font_weight(FontWeight::SEMIBOLD)
            .mb_6()
            .child(title),
    )
}

impl Seek {
    /// Start a segmented control's slide from `from` to `to`.
    pub(super) fn slide_segment(&mut self, id: &'static str, from: usize, to: usize) {
        if from != to {
            let generation = self.segment_motion.get(id).map_or(0, |(_, g)| g + 1);
            self.segment_motion.insert(id, (from, generation));
        }
    }
    /// Settings on its last page, from the toolbar or Ctrl+,.
    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview_enlarged || self.edit.is_some() || self.permanent_delete.is_some() || self.history_open {
            return;
        }
        self.settings = true;
        self.settings_open_motion = self.settings_open_motion.wrapping_add(1);
        self.settings_page_motion = self.settings_page_motion.wrapping_add(1);
        self.settings_focus.focus(window, cx);
        let _ = self.backend.tx.send(Command::HelperStatus);
        if self.agents.detected.is_none() {
            self.refresh_agents(cx);
        }
        cx.notify();
    }

    pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings = false;
        self.table.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    pub(super) fn open_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview_enlarged
            || self.edit.is_some()
            || self.permanent_delete.is_some()
            || self.history_open
        {
            return;
        }
        self.settings = true;
        self.settings_tab = 10;
        self.settings_search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.settings_open_motion = self.settings_open_motion.wrapping_add(1);
        self.settings_page_motion = self.settings_page_motion.wrapping_add(1);
        self.settings_focus.focus(window, cx);
        // Look for agents now, so the Agents page has them when it opens.
        if self.agents.detected.is_none() {
            self.refresh_agents(cx);
        }
        cx.notify();
    }
    fn keyboard_settings(&self, p: Palette) -> AnyElement {
        use crate::tokens::*;
        // One rounded card per group; action on the left, keys as caps on the right.
        let keycap = |label: String| {
            div()
                .h(px(20.))
                .min_w(px(20.))
                .px(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .border_1()
                .border_color(p.line)
                .bg(p.text.opacity(0.05))
                .font_family(FONT_MONO)
                .text_size(TEXT_CAPTION)
                .text_color(p.text)
                .child(label)
        };
        let keys_view = |keys: &str| {
            let mut row = div().flex().items_center().gap_1().flex_shrink_0();
            for (n, alternative) in keys.split(" / ").enumerate() {
                if n > 0 {
                    row = row.child(
                        div()
                            .px_1()
                            .text_size(TEXT_SMALL)
                            .text_color(p.muted)
                            .child("/"),
                    );
                }
                let parts: Vec<String> =
                    if alternative.contains('+') && !alternative.ends_with("++") {
                        alternative.split('+').map(str::to_string).collect()
                    } else if let Some(stripped) = alternative.strip_suffix("++") {
                        vec![stripped.to_string(), "+".to_string()]
                    } else {
                        vec![alternative.to_string()]
                    };
                for part in parts {
                    row = row.child(keycap(part));
                }
            }
            row
        };
        let mut view = section("Keyboard shortcuts");
        let mut group = "";
        let mut card: Option<Div> = None;
        let card_style = || {
            div()
                .flex()
                .flex_col()
                .rounded(RADIUS_CARD)
                .border_1()
                .border_color(p.text.opacity(0.1))
                .bg(p.settings_card)
                .overflow_hidden()
        };
        for shortcut in crate::shortcuts::ALL {
            if shortcut.group != group {
                if let Some(done) = card.take() {
                    view = view.child(done);
                }
                group = shortcut.group;
                view = view.child(
                    div()
                        .pt_6()
                        .pb_2()
                        .px_1()
                        .text_size(TEXT_SMALL)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.muted)
                        .child(group),
                );
                card = Some(card_style());
            }
            let row = div()
                .min_h(px(44.))
                .px_4()
                .py_2()
                .flex()
                .items_center()
                .gap_6()
                .border_b_1()
                .border_color(p.line)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(TEXT_BODY)
                        .text_color(p.text)
                        .child(shortcut.action),
                )
                .child(keys_view(shortcut.keys));
            card = card.map(|c| c.child(row));
        }
        if let Some(done) = card.take() {
            view = view.child(done);
        }
        view.into_any_element()
    }
    fn previews_settings(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let automatic = Switch::new("automatic-previews")
            .color(p.accent)
            .checked(self.preferences.automatic_previews)
            .accessibility_label("Automatic information panel previews")
            .on_change(cx.listener(|this, checked, window, cx| {
                this.preferences.automatic_previews = *checked;
                this.save_preferences(window, cx);
            }));
        let thumbnails = Switch::new("image-thumbnails")
            .color(p.accent)
            .checked(self.preferences.image_thumbnails)
            .accessibility_label("Image and video thumbnails")
            .on_change(cx.listener(|this, checked, window, cx| {
                this.preferences.image_thumbnails = *checked;
                this.save_preferences(window, cx);
            }));
        let gallery = Switch::new("gallery-preview")
            .color(p.accent)
            .checked(self.preferences.gallery_preview)
            .accessibility_label("Gallery large preview")
            .on_change(cx.listener(|this, checked, window, cx| {
                this.preferences.gallery_preview = *checked;
                this.save_preferences(window, cx);
            }));
        let hide_secrets = Switch::new("hide-env-secrets")
            .color(p.accent)
            .checked(self.preferences.hide_env_secrets)
            .accessibility_label("Hide secrets in env previews")
            .on_change(cx.listener(|this, checked, window, cx| {
                this.preferences.hide_env_secrets = *checked;
                this.save_preferences(window, cx);
            }));
        section("Previews")
            .child(card("Automatic previews", "Show file content in the information panel. Metadata and explicit Quick Look remain available when this is off.", automatic, p))
            .child(card("Image and video thumbnails", "Show thumbnails while browsing. Quick Look and the gallery's enlarged preview remain available when this is off.", thumbnails, p))
            .child(card("Gallery large preview", "Show the selected item large above the gallery's strip. The preview's × hides it; Show preview brings it back.", gallery, p))
            .child(card("Hide secrets in env previews", "Mask values in .env, .env.* and *.env previews until you reveal them. Hidden values are never copied.", hide_secrets, p))

            .into_any_element()
    }
    pub(super) fn opacity_choice(
        &self,
        kind: &'static str,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let index = super::SLIDER_LABELS
            .iter()
            .position(|label| *label == kind)
            .unwrap_or(0);
        let slider = self.opacity_sliders[index].clone();
        let value = slider.read(cx).value().end() as u16;
        let shown = match index {
            4 => format!("{value}°"),
            7 => format!("{value} ms"),
            _ => format!("{value}%"),
        };
        let focus = self.opacity_focus[index].clone();
        div()
            .id(("opacity-control", index))
            .track_focus(&focus)
            .tab_stop(true)
            .text_color(p.muted)
            .focus_visible(|d| d.underline().text_color(p.text))
            .role(Role::Group)
            .aria_label(kind)
            .w(px(210.))
            .flex()
            .flex_col()
            .gap_2()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| this.opacity_focus[index].focus(window, cx)),
            )
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                let state = this.opacity_sliders[index].read(cx);
                let value = state.value().end();
                let min = state.min_value();
                let max = state.max_value();
                let next = match event.keystroke.key.as_str() {
                    "left" | "down" => value - 1.,
                    "right" | "up" => value + 1.,
                    "home" => min,
                    "end" => max,
                    _ => return,
                };
                this.opacity_sliders[index].update(cx, |state, cx| {
                    state.set_value(next.clamp(min, max), window, cx)
                });
                cx.stop_propagation();
            }))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(12.))
                    // Sliders that share a card name themselves.
                    .when((4..7).contains(&index), |d| d.child(kind))
                    .child(div().font_family(crate::tokens::FONT_MONO).child(shown)),
            )
            .child(gpui_component::slider::Slider::new(&slider).h(px(24.)))
            .into_any_element()
    }
    pub(super) fn refresh_background(&mut self, cx: &mut Context<Self>) {
        let request = self
            .preferences
            .background_image
            .clone()
            .map(|path| (path, self.preferences.background_effect));
        if request == self.background_request {
            return;
        }
        self.background_request = request.clone();
        self.background = None;
        self.background_task = Task::ready(());
        let Some((path, effect)) = request.clone() else {
            return;
        };
        let job = cx
            .background_executor()
            .spawn(async move { crate::background::load(&path, effect) });
        self.background_task = cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                if this.background_request != request {
                    return;
                }
                match result {
                    Ok(image) => this.background = Some(image),
                    Err(error) => this.message = Some(format!("Background unavailable: {error}")),
                }
                cx.notify();
            });
        });
    }
    pub(super) fn settings_sheet(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let themes = {
            let options = [Appearance::System, Appearance::Light, Appearance::Dark];
            let selected = options
                .iter()
                .position(|theme| *theme == self.preferences.theme)
                .unwrap_or(0);
            segmented(
                "theme-choice",
                &["System", "Light", "Dark"],
                selected,
                self.segment_motion.get("theme-choice").copied(),
                self.motion_enabled,
                p,
                cx,
                move |this, index, window, cx| {
                    this.slide_segment("theme-choice", selected, index);
                    this.preferences.theme = options[index];
                    this.save_preferences(window, cx);
                },
            )
        };
        // Theme, then the palette that colours it.
        let appearance = section("Appearance")
            .child(card(
                "Theme",
                "System follows Windows appearance.",
                themes,
                p,
            ))
            .child(
                div()
                    .mt_3()
                    .mb_6()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .px_1()
                            .flex()
                            .items_baseline()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(crate::tokens::TEXT_BODY)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Palette"),
                            )
                            .child(
                                div()
                                    .text_size(crate::tokens::TEXT_SMALL)
                                    .text_color(p.muted)
                                    .child(if p.text.l > 0.5 {
                                        "Shown as it looks in Dark"
                                    } else {
                                        "Shown as it looks in Light"
                                    }),
                            ),
                    )
                    .child(self.palette_picker(p, cx)),
            )
            .child(card(
                "Intensity",
                "How strongly the palette colours the window.",
                self.opacity_choice("Intensity", p, cx),
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
        let views = {
            let options = [View::List, View::Icons, View::Gallery, View::Columns];
            let selected = options
                .iter()
                .position(|view| *view == self.preferences.default_view)
                .unwrap_or(0);
            segmented(
                "default-view-options",
                &["List", "Icons", "Gallery", "Columns"],
                selected,
                self.segment_motion.get("default-view-options").copied(),
                self.motion_enabled,
                p,
                cx,
                move |this, index, window, cx| {
                    this.slide_segment("default-view-options", selected, index);
                    let view = options[index];
                    this.preferences.default_view = view;
                    this.set_view(view, cx);
                    this.save_preferences(window, cx);
                },
            )
        };
        let content_locations = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .items_start()
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
            .flex_row_reverse()
            .items_center()
            .gap_3()
            .child(
                Switch::new("remember-document-text")
                    .color(p.accent)
                    .gap_4()
                    .py_1()
                    .checked(self.preferences.remember_document_text)
                    .color(p.accent)
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
        let ranking_file = self.store.directory.join("ranking.json");
        // Where the ranking stands, said beside the buttons: the weights are
        // read once at startup, so an edit waits for the next start.
        let saved = std::fs::read(&ranking_file).ok();
        let parsed = saved.as_deref().map(|bytes| {
            serde_json::from_slice::<Vec<seek_engine::index::FolderWeight>>(bytes).ok()
        });
        let wanted = parsed
            .clone()
            .flatten()
            .unwrap_or_else(seek_engine::index::default_weights);
        let ranking_note = if matches!(parsed, Some(None)) {
            Some("Not valid JSON")
        } else if wanted != seek_engine::index::weights() {
            Some("Restart Seek to apply")
        } else if wanted != seek_engine::index::default_weights() {
            Some("Custom")
        } else {
            None
        };
        let ranking = div()
            .flex()
            .items_center()
            .gap_2()
            .when_some(ranking_note, |d, note| {
                d.child(
                    div()
                        .mr_1()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child(note),
                )
            })
            .child(
                control("reset-ranking", "Reset", p, cx)
                    .disabled(saved.is_none())
                    .on_click(cx.listener({
                        let file = ranking_file.clone();
                        move |this, _, _, cx| {
                            this.message = std::fs::remove_file(&file)
                                .err()
                                .map(|e| format!("Could not reset search ranking: {e}"));
                            cx.notify();
                        }
                    })),
            )
            .child(control("edit-ranking", "Edit", p, cx).on_click(cx.listener(
                move |this, _, _, cx| {
                    if !ranking_file.exists() {
                        let defaults = seek_engine::index::default_weights();
                        if let Ok(json) = serde_json::to_string_pretty(&defaults) {
                            let _ = std::fs::write(&ranking_file, json);
                        }
                    }
                    this.message = edit_in_notepad(&ranking_file).err();
                    cx.notify();
                },
            )));
        let keep_backups = Switch::new("keep-text-backups")
            .color(p.accent)
            .gap_4()
            .py_1()
            .checked(self.preferences.keep_text_backups)
            .accessibility_label("Keep text file backups")
            .on_change(cx.listener(|this, checked, window, cx| {
                this.preferences.keep_text_backups = *checked;
                this.save_preferences(window, cx);
            }));
        let auto_save = Switch::new("auto-save-text")
            .color(p.accent)
            .gap_4()
            .py_1()
            .checked(self.preferences.auto_save_text)
            .accessibility_label("Auto-save text files")
            .on_change(cx.listener(|this, checked, window, cx| {
                this.preferences.auto_save_text = *checked;
                this.save_preferences(window, cx);
            }));
        let editing=section("Text editing")
            .child(card("Auto-save", "Save text, code and Markdown one second after you stop typing. Conflicts pause auto-save and keep your draft.", auto_save, p))
            .child(card("Keep backups", "Keep a .bak copy after successful saves. When off, Undo saved change remains available for this session. Failed saves retain recovery copies.", keep_backups, p));
        let restore_layout = Switch::new("restore-layout")
            .color(p.accent)
            .checked(self.preferences.restore_layout)
            .accessibility_label("Reopen panes and tabs as they were")
            .on_change(cx.listener(|this, on: &bool, window, cx| {
                this.preferences.restore_layout = *on;
                this.save_preferences(window, cx);
            }));
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
            .child(card(
                "Reopen panes and tabs",
                "Open Seek with the panes, tabs and folders you had when you closed it. When off, Seek opens with one pane.",
                restore_layout,
                p,
            ))
            .child(card(
                "Default file manager",
                "Folders and drives you open, from Explorer or any app, and Win+E open in Seek. Control Panel, the Recycle Bin and other system places still open in File Explorer. Turn off to give everything back to File Explorer.",
                Switch::new("default-file-manager")
                    .color(p.accent)
                    .checked(crate::explorer_menu::is_default())
                    .accessibility_label("Make Seek the default file manager")
                    .on_change(cx.listener(|this, on: &bool, _, cx| {
                        this.message = crate::explorer_menu::set_default(*on)
                            .err()
                            .map(|e| format!("Default file manager: {e}"));
                        cx.notify();
                    })),
                p,
            ))
            .child(card(
                "Open while dragging",
                "How long a drag rests on a folder or a tab before it opens.",
                self.opacity_choice("Drag hover", p, cx),
                p,
            ))
            .child(card(
                "Search ranking",
                "Folders whose files rank higher or lower, as a JSON list of names and weights. Changes apply the next time Seek starts.",
                ranking,
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
            .child(card("Indexed folders", "Folders and drives to include in the index. Separate folders with semicolons; changes apply on the next launch.",
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
            .color(p.accent).gap_4().py_1()
                    .checked(self.preferences.check_updates_on_startup)
                    .color(p.accent)
                    .accessibility_label("Check for updates on startup")
                    .on_change(cx.listener(|this, checked, window, cx| {
                        this.preferences.check_updates_on_startup = *checked;
                        this.save_preferences(window, cx);
                    })), p))
            .child(card("Version", "Updates preserve your saved index, settings and document text. Installing closes all tabs and restarts Seek.",
                div().flex().items_center().gap_3()
                    .when(!self.app_updates.message.is_empty(), |d| d.child(
                        div().text_size(px(12.)).text_color(p.muted).child(self.app_updates.message.clone())))
                    .child(div().text_size(px(13.)).child(crate::updater::CURRENT))
                    .child(control("check-updates-now", "Check now", p, cx)
                        .disabled(self.app_updates.busy)
                        .on_click(cx.listener(|this, _, _, cx|this.check_for_updates(cx))))
                    .when(self.app_updates.available.is_some(), |d|d.child(
                        control("install-app-update", if self.app_updates.staged.is_some() { "Restart" } else { "Download" }, p, cx)
                            .disabled(self.app_updates.busy)
                            .on_click(cx.listener(|this, _, _, cx|this.install_app_update(cx))))), p))
            .child(card("Report a bug", "Opens a new GitHub issue in your browser with Seek's version, your Windows version and where the diagnostics log is. Nothing is sent until you submit it.",
                control("report-bug", "Report a bug", p, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.report_bug(cx))), p));
        let mut nav = div()
            .id("settings-navigation")
            .min_h_0()
            .overflow_y_scroll()
            .w(px(224.))
            .flex_shrink_0()
            .h_full()
            .p_4()
            .border_r_1()
            .border_color(p.line)
            .flex()
            .flex_col()
            .gap_1();
        let query = self.settings_search.read(cx).value().to_lowercase();
        nav = nav.child(Input::new(&self.settings_search).h(px(32.)).w_full().mb_4());
        for (i, label, icon, keywords) in SETTINGS_PAGES {
            if !query.trim().is_empty()
                && !format!("{label} {keywords}")
                    .to_lowercase()
                    .contains(query.trim())
            {
                continue;
            }
            nav = nav.child(
                control(("settings-category", i), "", p, cx)
                    .border_0()
                    .w_full()
                    .h(px(34.))
                    .px_3()
                    .py_0()
                    .rounded(px(7.))
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
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(Icon::new(icon).size(px(16.)).text_color(
                                        if self.settings_tab == i { p.text } else { p.muted },
                                    )),
                            )
                            .child(
                                div()
                                    .h(px(20.))
                                    .line_height(px(20.))
                                    .text_size(px(13.))
                                    .child(label),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.settings_tab != i {
                            this.settings_page_motion = this.settings_page_motion.wrapping_add(1);
                        }
                        this.settings_tab = i;
                        if i == 11 {
                            // Shows what was found when Settings opened at
                            // once, and refreshes it underneath.
                            this.refresh_agents(cx);
                        }
                        this.message = None;
                        this.settings_focus.focus(window, cx);
                        cx.notify();
                    })),
            );
        }
        let content = match self.settings_tab {
            1 => files.into_any_element(),
            2 => locations.into_any_element(),
            3 => helper.into_any_element(),
            4 => updates.into_any_element(),
            5 | 6 => self.glass_settings(p, cx),
            7 => editing.into_any_element(),
            8 => self.saved_commands_settings(p, cx).into_any_element(),
            9 => self.previews_settings(p, cx),
            10 => self.keyboard_settings(p),
            11 => self.agents_settings(p, cx),
            _ => appearance.into_any_element(),
        };
        let content = if self.motion_enabled {
            div()
                .child(content)
                .with_animation(
                    ("settings-page", self.settings_page_motion as usize),
                    crate::motion::state(),
                    |d, t| d.opacity(0.88 + 0.12 * t),
                )
                .into_any_element()
        } else {
            content.into_any_element()
        };
        let header = div()
            .h(px(56.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_end()
            .px_6()
            .child(control("close-settings", "Done", p, cx).on_click(cx.listener(
                |this, _, window, cx| {
                    this.settings = false;
                    this.suggestions = None;
                    this.table.read(cx).focus_handle(cx).focus(window, cx);
                    cx.notify();
                },
            )));
        let modal = div()
            .id("settings-sheet")
            .occlude()
            .track_focus(&self.settings_focus)
            .w(px(900.))
            .max_w(relative(0.94))
            .h(px(650.))
            .max_h(relative(0.92))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(16.))
            .bg(p.sheet)
            .text_color(p.text)
            .border_1()
            .border_color(p.line)
            .relative()
            .child(
                div().flex_1().min_h_0().flex().child(nav).child(
                    // Done has its own row, so pages never scroll under it.
                    div().flex_1().min_w_0().h_full().flex().flex_col().child(header).child(
                    div()
                        .id(("settings-scroll", self.settings_tab))
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px_8()
                        .pt_0()
                        .pb_6()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(div().w_full().max_w(px(1024.)).mx_auto().child(content))
                        .when(self.settings_tab == 3, |d| {
                            d.when_some(self.helper.message.clone(), |d, msg| {
                                d.child(div().text_size(px(12.)).text_color(p.muted).child(msg))
                            })
                        })
                        .when_some(
                            self.message.clone().filter(|message| {
                                !message.starts_with("All locations")
                                    && !message.starts_with("Folder contents")
                            }),
                            |d, msg| {
                                d.child(div().text_size(px(12.)).text_color(p.muted).child(msg))
                            },
                        ),
                )),
            );
        div()
            .absolute()
            .inset_0()
            .top(px(52.))
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .child(if self.motion_enabled {
                modal
                    .with_animation(
                        ("settings-open", self.settings_open_motion as usize),
                        crate::motion::state(),
                        |d, t| d.opacity(0.92 + 0.08 * t),
                    )
                    .into_any_element()
            } else {
                modal.into_any_element()
            })
            .into_any_element()
    }
}
