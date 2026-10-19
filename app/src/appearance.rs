use crate::settings::{Appearance, Settings};
use gpui::*;
#[derive(Clone, Copy)]
pub struct Palette {
    pub text: Hsla,
    pub muted: Hsla,
    pub canvas: Hsla,
    pub chrome: Hsla,
    pub sidebar: Hsla,
    pub stripe: Hsla,
    pub line: Hsla,
    pub hover: Hsla,
    pub accent: Hsla,
    pub selected_text: Hsla,
    pub sheet: Hsla,
    pub settings_card: Hsla,
    pub control: Hsla,
    pub control_hover: Hsla,
    pub control_selected: Hsla,
}
pub fn palette(settings: &Settings, cx: &App) -> Palette {
    let dark = gpui_component::Theme::global(cx).mode.is_dark();
    let color = |value| Hsla::from(rgb(value));
    let accent = crate::settings::accent(&settings.accent).unwrap_or(0x909090);
    let r = ((accent >> 16) & 255) as f32 / 255.;
    let g = ((accent >> 8) & 255) as f32 / 255.;
    let b = (accent & 255) as f32 / 255.;
    let linear = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let lum = 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
    if dark {
        Palette {
            text: color(0xf4f4f4),
            muted: color(0xa0a0a0),
            canvas: if settings.glass {
                Hsla::from(rgba(0x000000cc))
            } else {
                color(0x080808)
            },
            chrome: if settings.glass {
                Hsla::from(rgba(0x00000000))
            } else {
                color(0x111111)
            },
            sidebar: if settings.glass {
                Hsla::from(rgba(0x00000000))
            } else {
                color(0x0b0b0b)
            },
            stripe: Hsla::from(rgba(0xffffff05)),
            line: Hsla::from(rgba(0xffffff14)),
            hover: Hsla::from(rgba(0xffffff10)),
            accent: color(accent),
            selected_text: color(if lum > 0.179 { 0x000000 } else { 0xffffff }),
            sheet: if settings.glass {
                Hsla::from(rgba(0x141414f5))
            } else {
                color(0x111111)
            },
            settings_card: Hsla::from(rgba(0xffffff04)),
            control: Hsla::from(rgba(0xffffff08)),
            control_hover: Hsla::from(rgba(0xffffff10)),
            control_selected: Hsla::from(rgba(0xffffff2e)),
        }
    } else {
        Palette {
            text: color(0x202020),
            muted: color(0x505050),
            canvas: if settings.glass {
                Hsla::from(rgba(0xffffffcc))
            } else {
                color(0xffffff)
            },
            chrome: if settings.glass {
                Hsla::from(rgba(0x00000000))
            } else {
                color(0xffffff)
            },
            sidebar: if settings.glass {
                Hsla::from(rgba(0x00000000))
            } else {
                color(0xf3f3f3)
            },
            stripe: Hsla::from(rgba(0x00000005)),
            line: Hsla::from(rgba(0x00000014)),
            hover: Hsla::from(rgba(0x0000000c)),
            accent: color(accent),
            selected_text: color(if lum > 0.179 { 0x000000 } else { 0xffffff }),
            sheet: if settings.glass {
                Hsla::from(rgba(0xf8f9fcfa))
            } else {
                color(0xffffff)
            },
            settings_card: Hsla::from(rgba(0xffffff70)),
            control: Hsla::from(rgba(0xffffff80)),
            control_hover: Hsla::from(rgba(0x00000007)),
            control_selected: Hsla::from(rgba(0x00000018)),
        }
    }
}
pub fn apply(settings: &Settings, window: Option<&mut Window>, cx: &mut App) {
    let mode = match settings.theme {
        Appearance::System => gpui_component::ThemeMode::from(
            window
                .as_ref()
                .map_or_else(|| cx.window_appearance(), |w| w.appearance()),
        ),
        Appearance::Light => gpui_component::ThemeMode::Light,
        Appearance::Dark => gpui_component::ThemeMode::Dark,
    };
    gpui_component::Theme::change(mode, None, cx);
    let p = palette(settings, cx);
    gpui_component::Theme::update(cx, |t| {
        t.font_family = "Segoe UI Variable Text".into();
        t.font_size = px(13.);
        t.radius = px(8.);
        t.radius_lg = px(14.);
        t.focus_ring = false;
        t.tokens.background = Hsla::from(rgba(0x00000000)).into();
        t.colors.background = rgba(0).into();
        t.colors.foreground = p.text;
        t.colors.muted_foreground = p.muted;
        t.colors.border = p.line;
        t.colors.primary = p.accent;
        t.colors.primary_foreground = p.selected_text;
        t.colors.button_primary = p.accent;
        t.colors.button_primary_foreground = p.selected_text;
        t.colors.button_primary_hover = p.accent;
        t.colors.button_primary_active = p.accent;
        t.colors.ring = p.muted.opacity(0.45);
        t.colors.table_head = p.chrome;
        t.colors.table = rgba(0).into();
        t.colors.table_head_foreground = p.muted;
        t.colors.table_even = p.stripe;
        t.colors.table_active = p.accent.opacity(0.18);
        t.colors.table_hover = p.hover;
        t.colors.table_row_border = Hsla::from(rgba(0x00000000));
        t.colors.input = p.line;
        t.colors.secondary = p.hover;
        t.colors.secondary_foreground = p.text;
        t.colors.accent_foreground = p.text;
        t.colors.secondary_hover = p.hover;
        // Popups live inside the same acrylic window. Keep their surface
        // translucent too; token resolution must not restore Kit's opaque fill.
        let popover = if settings.glass {
            Hsla::from(if mode.is_dark() {
                rgba(0x202020f0)
            } else {
                rgba(0xf1f1f1cc)
            })
        } else {
            p.sheet
        };
        t.tokens.popover = popover.into();
        t.colors.popover = popover;
        t.colors.popover_foreground = p.text;
        t.colors.caret = p.accent;
        t.colors.list = rgba(0).into();
        t.colors.list_even = p.stripe;
        t.tokens.table_even = Hsla::from(rgba(0)).into();
        t.tokens.table_active = p.accent.opacity(0.18).into();
        t.tokens.table_head = p.chrome.into();
        t.tokens.table = Hsla::from(rgba(0)).into();
        t.tokens.table_even = Hsla::from(rgba(0)).into();
        t.tokens.table_hover = p.hover.into();
        // Menu hover/selection is a quiet tint, even for graphite and very
        // light custom accents. Keep opaque color for explicit primary actions.
        t.tokens.accent = p.accent.opacity(0.14).into();
        t.tokens.accent_foreground = p.text.into();
        t.tokens.button_primary = p.accent.into();
    });
    // Avoid animated scrollbar fades continuously requesting frames at rest.
    gpui_base::Theme::global_mut(cx).scrollbar = gpui_base::ScrollbarTheme::new()
        .with_mode(gpui_base::ScrollbarMode::Always)
        .with_styles(
            gpui_base::ScrollbarStyles::default()
                .track(|s| {
                    s.width(px(12.))
                        .bg(transparent_black())
                        .border_color(transparent_black())
                })
                .thumb(|s| s.width(px(5.)).inset(px(3.)).radius(px(2.5)))
                .thumb_hover(|s| s.width(px(7.)).radius(px(3.5)))
                .thumb_active(|s| s.width(px(7.)).radius(px(3.5))),
        );
    if let Some(w) = window {
        w.set_background_appearance(if settings.glass {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Opaque
        });
        crate::instance::apply_backdrop(w, settings.glass, mode.is_dark());
    }
}
