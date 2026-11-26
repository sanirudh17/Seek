use crate::settings::{Appearance, Settings};
use gpui::*;
use std::sync::atomic::{AtomicBool, Ordering};
static SYSTEM_GLASS: AtomicBool = AtomicBool::new(false);

fn sidebar_tint_strength(value: u8) -> f32 {
    (value.min(100) as f32 / 100.).powi(3)
}

#[cfg(test)]
mod tests {
    #[test]
    fn sidebar_tint_is_gentle_monotonic_and_can_still_be_opaque() {
        assert_eq!(super::sidebar_tint_strength(0), 0.);
        assert!((super::sidebar_tint_strength(35) - 0.042875).abs() < 0.0001);
        assert_eq!(super::sidebar_tint_strength(100), 1.);
        assert_eq!(super::sidebar_tint_strength(255), 1.);
        assert!(
            (0..100)
                .all(|n| super::sidebar_tint_strength(n) <= super::sidebar_tint_strength(n + 1))
        );
    }

    #[test]
    fn every_tint_reads_at_three_to_one_on_its_theme_panes() {
        use gpui::{Hsla, rgb};
        let panes = |dark: bool| -> Vec<Hsla> {
            if dark {
                // Solid steps and a glass pane over a dark desktop.
                vec![0x0b0b0c, 0x0f1012, 0x131416, 0x1f2124]
            } else {
                vec![0xffffff, 0xf2f3f5, 0xeef2f7, 0xf8f0f2]
            }
            .into_iter()
            .map(|c| Hsla::from(rgb(c)))
            .collect()
        };
        for dark in [false, true] {
            let t = super::Tints::new(dark);
            for (name, tint) in [
                ("blue", t.blue),
                ("cyan", t.cyan),
                ("green", t.green),
                ("orange", t.orange),
                ("purple", t.purple),
                ("pink", t.pink),
                ("neutral", t.neutral),
            ] {
                for pane in panes(dark) {
                    let ratio = super::contrast(tint, pane);
                    assert!(ratio >= 3., "{name} on {pane:?} (dark {dark}): {ratio:.2}");
                }
            }
        }
    }

    #[test]
    fn palettes_text_reads_and_presets_round_trip() {
        use gpui::{Hsla, rgb};
        for named in super::PALETTES {
            let dark = super::contrast(
                Hsla::from(rgb(named.dark_text)),
                Hsla::from(rgb(named.dark_surface)),
            );
            let light = super::contrast(
                Hsla::from(rgb(named.light_text)),
                Hsla::from(rgb(named.light_sidebar)),
            );
            assert!(
                dark >= 7. && light >= 7.,
                "{}: {dark:.1} / {light:.1}",
                named.id
            );
            let mut settings = crate::settings::Settings::default();
            super::apply_palette(&mut settings, named.id);
            assert!(
                settings.shell_lightness >= 4,
                "{}: never pure black",
                named.id
            );
            for preset in super::Preset::ALL {
                preset.apply(&mut settings);
                assert_eq!(super::Preset::current(&settings), Some(preset));
            }
        }
        assert_eq!(
            super::Preset::current(&crate::settings::Settings::default()),
            Some(super::Preset::Frost)
        );
    }

    #[test]
    fn every_syntax_role_reads_at_three_to_one_on_every_palette() {
        use gpui::{Hsla, hsla, rgb};
        for dark in [false, true] {
            let syntax = super::SyntaxPalette::from_tints(super::Tints::new(dark));
            let resolved = syntax.to_syntax_colors();
            let roles = [
                ("keyword", syntax.keyword),
                ("string", syntax.string),
                ("number", syntax.number),
                ("comment", syntax.comment),
                ("function", syntax.function),
                ("type", syntax.type_),
                ("constant", syntax.constant),
                ("punctuation", syntax.punctuation),
            ];
            for (role, expected) in roles {
                let got = resolved
                    .style(role)
                    .and_then(|s| s.color)
                    .unwrap_or_else(|| panic!("missing syntax role {role}"));
                assert!(
                    (got.h - expected.h).abs() < 1e-3
                        && (got.s - expected.s).abs() < 1e-3
                        && (got.l - expected.l).abs() < 1e-3,
                    "role {role} did not round-trip into SyntaxColors: got {got:?}, expected {expected:?}"
                );
            }
            for named in super::PALETTES {
                let backgrounds: Vec<(&str, Hsla)> = if dark {
                    let tint = Hsla::from(rgb(named.dark_surface));
                    let solid_chrome = hsla(tint.h, tint.s * 0.6, 0.075, 1.);
                    let solid_pane = hsla(tint.h, (tint.s * 1.2).min(0.45), 0.072, 1.);
                    vec![
                        ("dark_surface", tint),
                        ("solid_chrome", solid_chrome),
                        ("solid_pane", solid_pane),
                        ("glass_over_dark", Hsla::from(rgb(0x1f2124))),
                    ]
                } else {
                    let surface = Hsla::from(rgb(named.light_sidebar));
                    let chrome = Hsla {
                        l: surface.l - 0.015,
                        s: (surface.s * 1.4).min(1.),
                        ..surface
                    };
                    let pane = Hsla {
                        l: surface.l + (1. - surface.l) * 0.55,
                        ..surface
                    };
                    vec![
                        ("light_sidebar", surface),
                        ("light_chrome", chrome),
                        ("light_pane", pane),
                        ("white", Hsla::from(rgb(0xffffff))),
                    ]
                };
                for (role, color) in roles {
                    for (bg_name, bg) in &backgrounds {
                        let ratio = super::contrast(color, *bg);
                        assert!(
                            ratio >= 3.0,
                            "{role} on {} {bg_name} (dark={dark}): {ratio:.2}",
                            named.id
                        );
                    }
                }
            }
        }
    }
}

/// Refresh on settings/activation, never on an idle timer or each row render.
pub fn refresh_system_glass() -> bool {
    use windows_sys::Win32::{System::Registry::*, UI::WindowsAndMessaging::*};
    #[repr(C)]
    struct HighContrast {
        size: u32,
        flags: u32,
        scheme: *mut u16,
    }
    let mut contrast = HighContrast {
        size: std::mem::size_of::<HighContrast>() as u32,
        flags: 0,
        scheme: std::ptr::null_mut(),
    };
    let mut enabled = 1u32;
    let mut bytes = 4u32;
    unsafe {
        // SPI_GETHIGHCONTRAST; HCF_HIGHCONTRASTON. Fail closed if inaccessible.
        let contrast_read = SystemParametersInfoW(
            0x0042,
            contrast.size,
            (&mut contrast as *mut HighContrast).cast(),
            0,
        ) != 0;
        let _ = RegGetValueW(
            HKEY_CURRENT_USER,
            windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
                .as_ptr(),
            windows::core::w!("EnableTransparency").as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut enabled as *mut u32).cast(),
            &mut bytes,
        );
        let allowed = contrast_read && contrast.flags & 1 == 0 && enabled != 0;
        SYSTEM_GLASS.swap(allowed, Ordering::Relaxed) != allowed
    }
}
pub fn glass_enabled(settings: &Settings, dark: bool) -> bool {
    settings.glass && dark && SYSTEM_GLASS.load(Ordering::Relaxed)
}

fn shell_tint(settings: &Settings, opacity: f32) -> Hsla {
    hsla(
        settings.shell_hue.min(360) as f32 / 360.,
        settings.shell_saturation.min(100) as f32 / 100.,
        settings.shell_lightness.min(30) as f32 / 100.,
        opacity,
    )
}
#[derive(Clone, Copy)]
pub struct Palette {
    pub text: Hsla,
    pub muted: Hsla,
    pub canvas: Hsla,
    pub pane: Hsla,
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
    /// The window's 1px edge: a highlight on glass, a line otherwise.
    pub edge: Hsla,
    /// Colour, used sparingly: kinds, places, chips, bars and badges.
    pub tints: Tints,
}

/// Six tints and a neutral, each at least 3:1 against the theme's panes
/// (tested below), after Monodrive's `theme.rs`. Colour is never the only
/// signal: every tinted thing also has a label or an icon shape.
#[derive(Clone, Copy)]
pub struct Tints {
    pub blue: Hsla,
    pub cyan: Hsla,
    pub green: Hsla,
    pub orange: Hsla,
    pub purple: Hsla,
    pub pink: Hsla,
    /// The Other category in Phase 9's storage bars.
    #[allow(dead_code)]
    pub neutral: Hsla,
}

impl Tints {
    pub fn new(dark: bool) -> Tints {
        let c = |light: u32, night: u32| Hsla::from(rgb(if dark { night } else { light }));
        Tints {
            blue: c(0x2563eb, 0x60a5fa),
            cyan: c(0x0e7490, 0x22d3ee),
            green: c(0x15803d, 0x4ade80),
            orange: c(0xc2620e, 0xf0954f),
            purple: c(0x8250df, 0xb18cf2),
            pink: c(0xc4307e, 0xf07ab0),
            neutral: c(0x6b7280, 0x9ca3af),
        }
    }
}

/// The eight syntax-highlighting roles, mapped directly from the active
/// palette's six tints and neutral so code previews, Quick Look, document
/// panes and the editor share identical, palette-matched colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyntaxPalette {
    pub keyword: Hsla,
    pub string: Hsla,
    pub number: Hsla,
    pub comment: Hsla,
    pub function: Hsla,
    pub type_: Hsla,
    pub constant: Hsla,
    pub punctuation: Hsla,
}

impl SyntaxPalette {
    pub fn from_tints(t: Tints) -> Self {
        Self {
            keyword: t.purple,
            string: t.green,
            number: t.orange,
            comment: t.neutral,
            function: t.blue,
            type_: t.cyan,
            constant: t.pink,
            punctuation: t.neutral,
        }
    }

    pub fn from_palette(p: Palette) -> Self {
        Self::from_tints(p.tints)
    }

    pub fn style(self, role: &str) -> Option<HighlightStyle> {
        let color = match role {
            "keyword" | "preproc" | "label" | "title" => self.keyword,
            "string" | "string.escape" | "string.regex" | "string.special" => self.string,
            "number" => self.number,
            "comment" | "comment.doc" => {
                return Some(HighlightStyle {
                    color: Some(self.comment),
                    font_style: Some(gpui::FontStyle::Italic),
                    ..Default::default()
                });
            }
            "function" | "property" | "attribute" => self.function,
            "type" | "constructor" | "tag" | "tag.doctype" => self.type_,
            "constant" | "boolean" | "enum" | "variant" | "variable.special" => self.constant,
            "punctuation"
            | "punctuation.bracket"
            | "punctuation.delimiter"
            | "punctuation.list_marker"
            | "punctuation.special"
            | "operator" => self.punctuation,
            _ => return None,
        };
        Some(HighlightStyle {
            color: Some(color),
            ..Default::default()
        })
    }

    pub fn to_syntax_colors(self) -> gpui_component::highlighter::SyntaxColors {
        let entry = |c: Hsla| serde_json::json!({ "color": c });
        let italic = |c: Hsla| serde_json::json!({ "color": c, "font_style": "italic" });
        serde_json::from_value(serde_json::json!({
            "keyword": entry(self.keyword),
            "preproc": entry(self.keyword),
            "label": entry(self.keyword),
            "title": entry(self.keyword),
            "string": entry(self.string),
            "string.escape": entry(self.string),
            "string.regex": entry(self.string),
            "string.special": entry(self.string),
            "string.special.symbol": entry(self.constant),
            "number": entry(self.number),
            "comment": italic(self.comment),
            "comment.doc": italic(self.comment),
            "function": entry(self.function),
            "property": entry(self.function),
            "attribute": entry(self.function),
            "type": entry(self.type_),
            "constructor": entry(self.type_),
            "tag": entry(self.type_),
            "tag.doctype": entry(self.type_),
            "constant": entry(self.constant),
            "boolean": entry(self.constant),
            "enum": entry(self.constant),
            "variant": entry(self.constant),
            "variable.special": entry(self.constant),
            "punctuation": entry(self.punctuation),
            "punctuation.bracket": entry(self.punctuation),
            "punctuation.delimiter": entry(self.punctuation),
            "punctuation.list_marker": entry(self.punctuation),
            "punctuation.special": entry(self.punctuation),
            "operator": entry(self.punctuation),
        }))
        .expect("valid palette syntax colours")
    }
}

/// A named palette: the glass tint, the text and the accent, for each theme.
/// The colours are adapted from Monocode's workspace themes (MIT).
pub struct Named {
    pub id: &'static str,
    pub name: &'static str,
    pub dark_surface: u32,
    pub dark_text: u32,
    pub dark_accent: u32,
    pub light_sidebar: u32,
    pub light_text: u32,
    pub light_accent: u32,
}

#[rustfmt::skip]
pub const PALETTES: &[Named] = &[
    Named { id: "graphite", name: "Graphite", dark_surface: 0x121418, dark_text: 0xf3f4f6, dark_accent: 0x909090, light_sidebar: 0xf2f3f5, light_text: 0x1f2024, light_accent: 0x6b6f76 },
    Named { id: "harbor", name: "Harbor", dark_surface: 0x0b1018, dark_text: 0xdbe4f0, dark_accent: 0x5b9cf2, light_sidebar: 0xeef2f7, light_text: 0x1b2533, light_accent: 0x2f6fd0 },
    Named { id: "moss", name: "Moss", dark_surface: 0x0d110e, dark_text: 0xdde5dc, dark_accent: 0x6fbf73, light_sidebar: 0xf1f4ee, light_text: 0x1f2a20, light_accent: 0x3d8a45 },
    Named { id: "ember", name: "Ember", dark_surface: 0x120e0c, dark_text: 0xecdfd6, dark_accent: 0xf08a4b, light_sidebar: 0xf7f2ee, light_text: 0x2d211b, light_accent: 0xc75c1e },
    Named { id: "orchid", name: "Orchid", dark_surface: 0x110d16, dark_text: 0xe6def0, dark_accent: 0xb57cf0, light_sidebar: 0xf4f0f8, light_text: 0x261e33, light_accent: 0x8048c8 },
    Named { id: "tidepool", name: "Tidepool", dark_surface: 0x0a1213, dark_text: 0xd8e8e8, dark_accent: 0x3fc1b5, light_sidebar: 0xedf5f5, light_text: 0x162a2b, light_accent: 0x138a80 },
    Named { id: "dune", name: "Dune", dark_surface: 0x13110d, dark_text: 0xebe3d2, dark_accent: 0xd9b25c, light_sidebar: 0xf6f1e7, light_text: 0x2e2819, light_accent: 0xa57d1e },
    Named { id: "rosewood", name: "Rosewood", dark_surface: 0x140e10, dark_text: 0xefdfe3, dark_accent: 0xe7798f, light_sidebar: 0xf8f0f2, light_text: 0x2f1f24, light_accent: 0xc0405c },
];

pub fn named(id: &str) -> &'static Named {
    PALETTES.iter().find(|p| p.id == id).unwrap_or(&PALETTES[0])
}

/// Choosing a palette moves the tint to the palette's own surface colour
/// (hue, saturation and darkness), as Monodrive does, and sets the text and
/// the accent. How see-through the glass is stays with the preset.
pub fn apply_palette(settings: &mut Settings, id: &str) {
    let named = named(id);
    let tint = Hsla::from(rgb(named.dark_surface));
    settings.palette = named.id.into();
    settings.shell_hue = (tint.h * 360.).round() as u16 % 360;
    settings.shell_saturation = ((tint.s * 100.).round() as u8).min(100);
    settings.shell_lightness = ((tint.l * 100.).round() as u8).clamp(4, 30);
}

/// The glass presets. Each keeps the palette's hue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Clear,
    Frost,
    Tinted,
    Solid,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::Clear, Preset::Frost, Preset::Tinted, Preset::Solid];
    pub fn label(self) -> &'static str {
        match self {
            Preset::Clear => "Clear",
            Preset::Frost => "Frost",
            Preset::Tinted => "Tinted",
            Preset::Solid => "Solid",
        }
    }
    /// (window opacity, sidebar opacity, grain). The tint's colour belongs
    /// to the palette, so presets never change it.
    fn values(self) -> (u8, u8, bool) {
        match self {
            Preset::Clear => (40, 25, false),
            Preset::Frost => (62, 40, false),
            Preset::Tinted => (74, 60, false),
            Preset::Solid => (100, 0, false),
        }
    }
    pub fn apply(self, settings: &mut Settings) {
        settings.glass = self != Preset::Solid;
        if self != Preset::Solid {
            let (opacity, sidebar, grain) = self.values();
            settings.glass_opacity = opacity;
            settings.sidebar_opacity = sidebar;
            settings.grain = grain;
        }
    }
    /// The preset the settings match, if any.
    pub fn current(settings: &Settings) -> Option<Preset> {
        if !settings.glass {
            return Some(Preset::Solid);
        }
        let now = (
            settings.glass_opacity,
            settings.sidebar_opacity,
            settings.grain,
        );
        Preset::ALL
            .into_iter()
            .find(|p| *p != Preset::Solid && p.values() == now)
    }
}

/// WCAG contrast ratio between two opaque colours (the tint and palette tests).
#[cfg_attr(not(test), allow(dead_code))]
pub fn contrast(a: Hsla, b: Hsla) -> f32 {
    let luminance = |c: Hsla| {
        let c = c.to_rgb();
        let f = |v: f32| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
    };
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Highest combined opacity of the window tint and the sidebar's: glass
/// stays glass, however the sliders are set.
const GLASS_CAP: f32 = 0.92;

pub fn palette(settings: &Settings, cx: &App) -> Palette {
    let dark = gpui_component::Theme::global(cx).mode.is_dark();
    let glass = glass_enabled(settings, dark);
    let color = |value| Hsla::from(rgb(value));
    let named = named(&settings.palette);
    // The palette owns the accent, one for each theme.
    let accent = if dark {
        named.dark_accent
    } else {
        named.light_accent
    };
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
    let tint = shell_tint(settings, 1.);
    // How strongly the palette colours the frame, 0 to 1 (Appearance > Intensity).
    let k = settings.palette_intensity.min(100) as f32 / 100.;
    if dark {
        let text = color(named.dark_text);
        // Hue-aware solid steps for when glass is off: never pure black.
        let solid = |l: f32| hsla(tint.h, tint.s * 0.6 * k, l, 1.);
        // The frame's colour; the file list takes a faint share of it so the
        // step from sidebar to list is gentle, not abrupt.
        let frame = Hsla {
            s: (tint.s * 1.3).min(0.6),
            l: (tint.l + 0.06 * k).min(0.2),
            ..tint
        };
        let window = (settings.glass_opacity.min(100) as f32 / 100.).min(0.85);
        let side = (sidebar_tint_strength(settings.sidebar_opacity) * 0.6)
            .min((GLASS_CAP - window) / (1. - window))
            .max(0.);
        Palette {
            text,
            muted: hsla(text.h, text.s * 0.3, text.l * 0.66, 1.),
            canvas: if glass {
                Hsla { a: window, ..tint }
            } else {
                solid(0.045)
            },
            // The content pane is clear over glass; only the window's one
            // tint sits between it and the desktop.
            // Panes carry a light share of the palette's colour that grows
            // with Intensity, so at full intensity they belong with the
            // frame instead of reading as a neutral block; still quieter
            // than the frame, so the file list stays the calm surface.
            pane: if glass && settings.main_pane_glass {
                Hsla {
                    a: 0.16 * k,
                    ..frame
                }
            } else if glass {
                Hsla {
                    h: frame.h,
                    s: (tint.s * (0.6 + 0.9 * k)).min(0.42),
                    l: tint.l * 0.7 + 0.02 + 0.025 * k,
                    a: 0.94,
                }
            } else {
                hsla(tint.h, (tint.s * (0.6 + 0.6 * k)).min(0.45), 0.06 + 0.012 * k, 1.)
            },
            // The frame (toolbar, tabs, status bar) and the sidebar carry the
            // palette's colour, a step lighter than the window tint so the
            // hue reads; the file list stays clear.
            chrome: if glass {
                Hsla {
                    a: 0.28 * k,
                    ..frame
                }
            } else {
                solid(0.075)
            },
            sidebar: if glass {
                Hsla {
                    s: (tint.s * 1.4).min(0.65),
                    l: (tint.l + 0.08 * k).min(0.22),
                    a: side.max(0.42 * k),
                    ..tint
                }
            } else {
                solid(0.038)
            },
            stripe: if glass {
                Hsla::from(rgba(0))
            } else {
                Hsla::from(rgba(0xffffff05))
            },
            line: Hsla::from(rgba(0xffffff14)),
            hover: Hsla::from(rgba(0xffffff10)),
            accent: color(accent),
            selected_text: color(if lum > 0.179 { 0x000000 } else { 0xffffff }),
            // Settings is glass like the window: the blurred desktop shows
            // through a light tint, enough to keep text easy to read.
            sheet: if glass {
                Hsla { a: 0.3, ..tint }
            } else {
                solid(0.07)
            },
            settings_card: if glass {
                Hsla::from(rgba(0xffffff09))
            } else {
                Hsla::from(rgba(0xffffff08))
            },
            control: Hsla::from(rgba(0xffffff08)),
            control_hover: Hsla::from(rgba(0xffffff10)),
            control_selected: Hsla::from(rgba(0xffffff2e)),
            edge: Hsla::from(rgba(if glass { 0xffffff1c } else { 0xffffff14 })),
            tints: Tints::new(true),
        }
    } else {
        let text = color(named.light_text);
        // Light themes stay opaque, but take the palette's colour: the
        // sidebar is its surface, the window and bars a lighter wash of it,
        // the file list nearly white.
        // Lower intensity moves the palette's surface toward neutral.
        let surface = {
            let raw = color(named.light_sidebar);
            Hsla {
                s: raw.s * (0.25 + 0.75 * k),
                ..raw
            }
        };
        let wash = |toward_white: f32| Hsla {
            l: surface.l + (1. - surface.l) * toward_white,
            ..surface
        };
        Palette {
            text,
            muted: hsla(text.h, text.s * 0.3, text.l + (1. - text.l) * 0.27, 1.),
            // The frame (toolbar, tabs, status bar) shares the sidebar's
            // colour; the file list is a pale wash of it, never plain white.
            canvas: Hsla {
                l: surface.l - 0.015 * k,
                s: (surface.s * 1.4).min(1.),
                ..surface
            },
            pane: wash(0.55 + 0.3 * (1. - k)),
            chrome: Hsla {
                l: surface.l - 0.015 * k,
                s: (surface.s * 1.4).min(1.),
                ..surface
            },
            // Deep enough that each palette reads as its own colour.
            sidebar: Hsla {
                l: surface.l - 0.03 * k,
                s: (surface.s * 1.6).min(1.),
                ..surface
            },
            stripe: Hsla::from(rgba(0x00000005)),
            line: Hsla { a: 0.1, ..text },
            hover: Hsla { a: 0.06, ..text },
            accent: color(accent),
            selected_text: color(if lum > 0.179 { 0x000000 } else { 0xffffff }),
            sheet: wash(0.6),
            settings_card: Hsla::from(rgba(0xffffff90)),
            control: Hsla::from(rgba(0xffffff80)),
            control_hover: Hsla::from(rgba(0x00000007)),
            control_selected: Hsla::from(rgba(0x00000018)),
            edge: Hsla::from(rgba(0x00000014)),
            tints: Tints::new(false),
        }
    }
}
pub fn apply(settings: &Settings, window: Option<&mut Window>, cx: &mut App) {
    refresh_system_glass();
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
        t.font_family = crate::tokens::FONT_UI.into();
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
        // Column headers sit on the pane's own surface (glass shows through),
        // set apart by their divider, not a darker band of frame colour.
        t.colors.table_head = rgba(0).into();
        t.colors.table = rgba(0).into();
        t.colors.table_head_foreground = p.muted;
        t.colors.table_even = p.stripe;
        t.colors.table_active = rgba(0).into();
        t.colors.table_hover = p.hover;
        t.colors.table_row_border = Hsla::from(rgba(0x00000000));
        t.colors.input = p.line;
        t.colors.secondary = p.hover;
        t.colors.secondary_foreground = p.text;
        t.colors.accent_foreground = p.text;
        t.colors.secondary_hover = p.hover;
        // Menus and popovers sit over busy file lists, so their surface is
        // opaque even with glass on: text in a popup never shows rows through.
        let popover = if glass_enabled(settings, mode.is_dark()) {
            Hsla::from(if mode.is_dark() {
                rgb(0x232323)
            } else {
                rgb(0xf7f7f7)
            })
        } else {
            p.sheet
        };
        t.tokens.popover = popover.into();
        t.colors.popover = popover;
        t.colors.popover_foreground = p.text;
        t.colors.caret = p.accent;
        let mut highlight = (*t.highlight_theme).clone();
        highlight.style.syntax = SyntaxPalette::from_palette(p).to_syntax_colors();
        // A slightly darker column for line numbers, distinct from the text.
        highlight.style.editor_gutter_background = Some(Hsla::from(if mode.is_dark() {
            rgba(0x0000001f)
        } else {
            rgba(0x00000008)
        }));
        highlight.style.editor_line_number = Some(p.muted.opacity(0.7));
        t.highlight_theme = std::sync::Arc::new(highlight);
        t.colors.list = rgba(0).into();
        t.colors.list_even = p.stripe;
        t.tokens.table_even = Hsla::from(rgba(0)).into();
        t.tokens.table_active = Hsla::from(rgba(0)).into();
        t.tokens.table_head = Hsla::from(rgba(0)).into();
        t.tokens.table = Hsla::from(rgba(0)).into();
        t.tokens.table_even = Hsla::from(rgba(0)).into();
        // Rows draw their own inset hover; the table adds none.
        t.tokens.table_hover = Hsla::from(rgba(0)).into();
        // Menu hover/selection is a quiet tint, even for graphite and very
        // light custom accents. Keep opaque color for explicit primary actions.
        // Menu and list highlights are neutral, so menus look the same with
        // any accent; the accent marks selection and primary actions.
        t.tokens.accent = p.text.opacity(0.08).into();
        t.tokens.accent_foreground = p.text.into();
        t.tokens.button_primary = p.accent.into();
    });
    // Overlay scrollbars: visible while scrolling or hovered, then a finite
    // fade (no frames are requested once it ends). A thin, soft thumb in the
    // text colour, so it belongs to either theme.
    let thumb = p.text.opacity(0.22);
    let thumb_hover = p.text.opacity(0.38);
    let motion_on = crate::motion::enabled();
    gpui_base::Theme::global_mut(cx).scrollbar = gpui_base::ScrollbarTheme::new()
        .with_mode(gpui_base::ScrollbarMode::Scrolling)
        .with_motion(
            gpui_base::ScrollbarMotion::default()
                .with_idle(crate::motion::SCROLLBAR_IDLE)
                .with_enter(if motion_on {
                    crate::motion::STATE
                } else {
                    std::time::Duration::ZERO
                })
                .with_exit(if motion_on {
                    crate::motion::EXIT
                } else {
                    std::time::Duration::ZERO
                }),
        )
        .with_styles(
            gpui_base::ScrollbarStyles::default()
                .track(|s| {
                    s.width(px(10.))
                        .bg(transparent_black())
                        .border_color(transparent_black())
                })
                .thumb(move |s| {
                    s.width(px(4.))
                        .inset(px(3.))
                        .radius(px(2.))
                        .bg(thumb)
                        .min_length(px(28.))
                })
                .thumb_hover(move |s| s.width(px(6.)).radius(px(3.)).bg(thumb_hover))
                .thumb_active(move |s| s.width(px(6.)).radius(px(3.)).bg(thumb_hover)),
        );
    if let Some(w) = window {
        let glass = glass_enabled(settings, mode.is_dark());
        w.set_background_appearance(if glass {
            match settings.blur {
                crate::settings::Blur::Strong => WindowBackgroundAppearance::Blurred,
                crate::settings::Blur::Soft => WindowBackgroundAppearance::MicaAltBackdrop,
            }
        } else {
            WindowBackgroundAppearance::Opaque
        });
        crate::instance::apply_backdrop(w, glass, mode.is_dark());
    }
}

// Task dialogs need a stable reading surface above the acrylic workspace.
/// The command palette's and dialogs' surface: opaque for reading, with a
/// faint share of the palette's colour (more at higher Intensity) so pop-ups
/// belong with the window.
pub fn modal_surface(p: Palette) -> Hsla {
    // The frame's alpha over glass grows with Intensity (0.28 at full).
    let strength = if p.chrome.a < 1. { (p.chrome.a / 0.28).clamp(0., 1.) } else { 1. };
    let s = (p.chrome.s * 0.3 * strength).min(0.18);
    if p.text.l > 0.5 {
        hsla(p.chrome.h, s, 0.106 + 0.012 * strength, 1.)
    } else {
        hsla(p.chrome.h, s * 0.7, 0.973 - 0.01 * strength, 1.)
    }
}
pub fn modal_border(p: Palette) -> Hsla {
    p.muted.opacity(0.35)
}
