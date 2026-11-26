//! Seek's motion vocabulary. This is the only module allowed to define
//! animation durations or easing curves (`scripts/check-motion.ps1` enforces it).
//!
//! - Entrances take [`ENTER`] (200 ms), exits take [`EXIT`] (150 ms), and small
//!   state changes (selection fills, hover tints) take [`STATE`] (120 ms).
//! - Every curve decelerates: [`ease_out`] is the quintic-like
//!   `cubic-bezier(0.22, 1, 0.36, 1)`. Nothing eases in.
//! - Movement the user can reverse partway (sidebar, split drop zones, column
//!   scrolling) uses the critically damped [`spring`], always animated from the
//!   current on-screen value so a reversal never jumps.
//! - When Windows "Show animations" is off, [`enabled`] is false and callers
//!   render the end state at once. No animation loops while idle.
use gpui::Animation;
use std::time::Duration;

/// Appearing surfaces: panels, panes, drop previews, tabs.
pub const ENTER: Duration = Duration::from_millis(200);
/// Disappearing surfaces; deliberately faster than entrances.
pub const EXIT: Duration = Duration::from_millis(150);
/// Small in-place state changes such as an active fill or a window fade-in.
pub const STATE: Duration = Duration::from_millis(120);
/// Drop previews (drag-to-split) growing and fading in.
pub const PREVIEW: Duration = Duration::from_millis(150);
/// Short settle after a drop lands.
pub const SETTLE: Duration = Duration::from_millis(180);
/// Spring-driven reversible movement (sidebar, drop zones, column scroll).
/// Scrollbars stay this long after the last scroll, drag or hover, then fade.
pub const SCROLLBAR_IDLE: Duration = Duration::from_secs(3);
/// Brief quiet confirmation toasts (Copy, Cut, Paste, Copy path, Undo, Redo, Add to sidebar).
pub const TOAST_QUIET: Duration = Duration::from_millis(2500);
pub const SPRING: Duration = Duration::from_millis(260);

/// Whether Windows allows client-area animation. Read on demand; callers
/// that render every frame should cache it alongside other appearance state.
pub fn enabled() -> bool {
    crate::instance::motion_enabled()
}

/// `cubic-bezier(0.22, 1, 0.36, 1)`: fast start, long gentle landing.
pub fn ease_out(t: f32) -> f32 {
    bezier(0.22, 1., 0.36, 1., t)
}

/// Critically damped spring response from 0 to 1, normalised to land exactly.
pub fn spring(t: f32) -> f32 {
    const OMEGA: f32 = 9.;
    let t = t.clamp(0., 1.);
    let raw = |t: f32| 1. - (1. + OMEGA * t) * (-OMEGA * t).exp();
    raw(t) / raw(1.)
}

pub fn enter() -> Animation {
    Animation::new(ENTER).with_easing(ease_out)
}
pub fn exit() -> Animation {
    Animation::new(EXIT).with_easing(ease_out)
}
pub fn state() -> Animation {
    Animation::new(STATE).with_easing(ease_out)
}
pub fn preview() -> Animation {
    Animation::new(PREVIEW).with_easing(ease_out)
}
pub fn settle() -> Animation {
    Animation::new(SETTLE).with_easing(spring)
}
pub fn reversible() -> Animation {
    Animation::new(SPRING).with_easing(spring)
}

fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let x = x.clamp(0., 1.);
    if x == 0. || x == 1. {
        return x;
    }
    let curve = |a: f32, b: f32, s: f32| {
        let u = 1. - s;
        3. * u * u * s * a + 3. * u * s * s * b + s * s * s
    };
    let slope = |a: f32, b: f32, s: f32| {
        let u = 1. - s;
        3. * u * u * a + 6. * u * s * (b - a) + 3. * s * s * (1. - b)
    };
    let mut s = x;
    for _ in 0..8 {
        let error = curve(x1, x2, s) - x;
        if error.abs() < 1e-5 {
            break;
        }
        let d = slope(x1, x2, s);
        if d.abs() < 1e-6 {
            break;
        }
        s = (s - error / d).clamp(0., 1.);
    }
    // Bisection fallback keeps the result monotonic even if Newton stalls.
    if (curve(x1, x2, s) - x).abs() >= 1e-4 {
        let (mut lo, mut hi) = (0f32, 1f32);
        for _ in 0..32 {
            s = (lo + hi) / 2.;
            if curve(x1, x2, s) < x {
                lo = s;
            } else {
                hi = s;
            }
        }
    }
    curve(y1, y2, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curves_start_and_land_exactly_and_never_ease_in() {
        for curve in [ease_out as fn(f32) -> f32, spring] {
            assert_eq!(curve(0.), 0.);
            assert!((curve(1.) - 1.).abs() < 1e-4);
            let mut last = 0.;
            for step in 1..=100 {
                let value = curve(step as f32 / 100.);
                assert!(value + 1e-5 >= last, "monotonic");
                last = value;
            }
            // Decelerating: the first tenth covers more than a tenth.
            assert!(curve(0.1) > 0.2);
        }
        // Reference value of cubic-bezier(0.22, 1, 0.36, 1) at x = 0.5.
        assert!((ease_out(0.5) - 0.9614).abs() < 0.002);
        assert!(EXIT < ENTER);
    }
}
