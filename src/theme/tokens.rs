//! The Sift design tokens from the approved style spec, as typed constants.
//!
//! This is the single source of truth for colour, spacing, radius and type in
//! the app. The GPUI Kit theme is generated from these values, and custom Sift
//! components read them directly. Change a value here, not at a call site.

use super::color::Rgb;

/// Every colour role in one appearance. Field names match the style spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Window background.
    pub bg: Rgb,
    /// Sidebars, footers and other recessed surfaces.
    pub bg_subtle: Rgb,
    /// Hovered rows and controls, inactive segmented backgrounds.
    pub bg_hover: Rgb,
    /// The keyboard cursor row.
    pub bg_active: Rgb,
    /// Hairline dividers between rows.
    pub line: Rgb,
    /// Control and card borders.
    pub line_strong: Rgb,
    /// Primary text.
    pub ink: Rgb,
    /// Secondary text: read subjects, labels.
    pub ink_2: Rgb,
    /// Tertiary text: previews, times, counts. Still meets 4.5:1 on `bg`.
    pub ink_3: Rgb,
    /// Violet: unread, AI, focus rings, today. Used for nothing else.
    pub accent: Rgb,
    /// Text and icons placed on `accent`.
    pub accent_ink: Rgb,
    /// AI surfaces and selected rows.
    pub accent_soft: Rgb,
    /// Borders of AI surfaces.
    pub accent_line: Rgb,
    /// Accent-coloured text on `bg` or `accent_soft`.
    pub accent_text: Rgb,
    /// The near-black (or near-white) pill for primary actions.
    pub primary: Rgb,
    /// Hovered primary pill.
    pub primary_hover: Rgb,
    /// Text on `primary`.
    pub primary_ink: Rgb,
    /// Success and "you're free" confirmations.
    pub good: Rgb,
    /// Warnings such as "needs sign-in".
    pub warn: Rgb,
    /// Destructive actions.
    pub danger: Rgb,
    /// Background of success pills.
    pub good_soft: Rgb,
    /// Background of warning banners and pills.
    pub warn_soft: Rgb,
    /// Border of warning banners.
    pub warn_line: Rgb,
    /// Hovered destructive buttons.
    pub danger_soft: Rgb,
    /// Border of destructive buttons.
    pub danger_line: Rgb,
}

/// The light appearance.
pub const LIGHT: Palette = Palette {
    bg: Rgb::from_u32(0xFFFFFF),
    bg_subtle: Rgb::from_u32(0xF8F8F9),
    bg_hover: Rgb::from_u32(0xF4F4F5),
    bg_active: Rgb::from_u32(0xF1F0FA),
    line: Rgb::from_u32(0xEDEDEF),
    line_strong: Rgb::from_u32(0xE2E2E6),
    ink: Rgb::from_u32(0x18181B),
    ink_2: Rgb::from_u32(0x3F3F46),
    ink_3: Rgb::from_u32(0x6B6B74),
    accent: Rgb::from_u32(0x6E56CF),
    accent_ink: Rgb::from_u32(0xFFFFFF),
    accent_soft: Rgb::from_u32(0xF4F2FD),
    accent_line: Rgb::from_u32(0xE0DAF8),
    accent_text: Rgb::from_u32(0x5B44B8),
    primary: Rgb::from_u32(0x18181B),
    primary_hover: Rgb::from_u32(0x2E2E33),
    primary_ink: Rgb::from_u32(0xFFFFFF),
    good: Rgb::from_u32(0x1F7A4A),
    warn: Rgb::from_u32(0xB4541C),
    danger: Rgb::from_u32(0xC42B1C),
    good_soft: Rgb::from_u32(0xE6F4EA),
    // The board used FCEBDD; lightened so warn text keeps 4.5:1.
    warn_soft: Rgb::from_u32(0xFEF3EA),
    warn_line: Rgb::from_u32(0xF2CDB0),
    danger_soft: Rgb::from_u32(0xFDECEA),
    danger_line: Rgb::from_u32(0xF3C1BC),
};

/// The dark appearance.
pub const DARK: Palette = Palette {
    bg: Rgb::from_u32(0x141416),
    bg_subtle: Rgb::from_u32(0x19191C),
    bg_hover: Rgb::from_u32(0x202024),
    bg_active: Rgb::from_u32(0x24232C),
    line: Rgb::from_u32(0x26262B),
    line_strong: Rgb::from_u32(0x34343B),
    ink: Rgb::from_u32(0xEDEDEF),
    ink_2: Rgb::from_u32(0xC8C8CE),
    ink_3: Rgb::from_u32(0x9A9AA4),
    accent: Rgb::from_u32(0xA594FF),
    accent_ink: Rgb::from_u32(0x16122E),
    accent_soft: Rgb::from_u32(0x211D38),
    accent_line: Rgb::from_u32(0x3A3266),
    accent_text: Rgb::from_u32(0xBFB3FF),
    primary: Rgb::from_u32(0xEDEDEF),
    primary_hover: Rgb::from_u32(0xD4D4D8),
    primary_ink: Rgb::from_u32(0x141416),
    good: Rgb::from_u32(0x5BC88A),
    warn: Rgb::from_u32(0xF2A36B),
    danger: Rgb::from_u32(0xFF8A7A),
    good_soft: Rgb::from_u32(0x15301F),
    warn_soft: Rgb::from_u32(0x36230F),
    warn_line: Rgb::from_u32(0x5F3E1C),
    danger_soft: Rgb::from_u32(0x3A1A17),
    danger_line: Rgb::from_u32(0x6B2A24),
};

/// Account avatar tints, light appearance: (background, text). Accounts pick
/// one by index so two inboxes are easy to tell apart.
pub const AVATARS_LIGHT: [(Rgb, Rgb); 6] = [
    (Rgb::from_u32(0xE7E3FB), Rgb::from_u32(0x4A36A8)),
    (Rgb::from_u32(0xFCE7DA), Rgb::from_u32(0x8A3A10)),
    (Rgb::from_u32(0xE0F0E5), Rgb::from_u32(0x1E5B33)),
    (Rgb::from_u32(0xE3ECFB), Rgb::from_u32(0x1F3F86)),
    (Rgb::from_u32(0xFBEFD0), Rgb::from_u32(0x6E4B07)),
    (Rgb::from_u32(0xDFF0F1), Rgb::from_u32(0x1C5A60)),
];

/// Account avatar tints, dark appearance.
pub const AVATARS_DARK: [(Rgb, Rgb); 6] = [
    (Rgb::from_u32(0x2C2550), Rgb::from_u32(0xCFC6FF)),
    (Rgb::from_u32(0x3A2418), Rgb::from_u32(0xF5C3A3)),
    (Rgb::from_u32(0x1C3324), Rgb::from_u32(0xA8DDB8)),
    (Rgb::from_u32(0x1C2742), Rgb::from_u32(0xB5C8F5)),
    (Rgb::from_u32(0x3A2E12), Rgb::from_u32(0xF2D69A)),
    (Rgb::from_u32(0x183235), Rgb::from_u32(0xA6D9DD)),
];

/// Names of the account colours, in index order.
pub const AVATAR_NAMES: [&str; 6] = ["Violet", "Clay", "Green", "Blue", "Amber", "Teal"];

/// The avatar tints for `palette`.
pub fn avatar_tints(palette: &Palette) -> &'static [(Rgb, Rgb); 6] {
    if *palette == DARK {
        &AVATARS_DARK
    } else {
        &AVATARS_LIGHT
    }
}

/// Spacing steps on the 4 px grid. These are the only allowed gaps and paddings.
pub const SPACE: [f32; 8] = [2.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0, 32.0];

/// Corner radii, in pixels.
pub mod radius {
    /// Label chips.
    pub const CHIP: f32 = 6.0;
    /// View tabs in the title bar.
    pub const TAB: f32 = 7.0;
    /// Buttons and inputs.
    pub const BUTTON: f32 = 8.0;
    /// Toasts.
    pub const TOAST: f32 = 10.0;
    /// Cards, popovers and the floating toolbar.
    pub const CARD: f32 = 12.0;
}

/// The interface typeface, from the style spec.
pub const FONT: &str = "Geist";
/// The typeface for keyboard hints and other fixed-width text.
pub const MONO_FONT: &str = "Geist Mono";

/// Fixed heights, in pixels.
pub mod size {
    /// The custom title bar.
    pub const TITLE_BAR: f32 = 44.0;
    /// A view tab inside the title bar.
    pub const TAB: f32 = 28.0;
    /// A message row at comfortable density.
    pub const ROW: f32 = 40.0;
    /// A message row at compact density.
    pub const ROW_COMPACT: f32 = 32.0;
    /// Icon buttons in toolbars.
    pub const ICON_BUTTON: f32 = 32.0;
}

/// Type scale, in pixels. Base UI text is 13 px, matching the references.
pub mod text {
    /// Empty states and onboarding.
    pub const DISPLAY: f32 = 36.0;
    /// Thread subject.
    pub const SUBJECT: f32 = 24.0;
    /// View title.
    pub const TITLE: f32 = 20.0;
    /// Message body.
    pub const BODY: f32 = 14.0;
    /// Rows, buttons and general UI.
    pub const UI: f32 = 13.0;
    /// Meta text and counts.
    pub const META: f32 = 12.0;
    /// Section dividers and keyboard hints.
    pub const SMALL: f32 = 11.0;
}

// The scales must stay ordered; breaking that is a compile error, not a test failure.
const _: () = {
    assert!(size::ROW_COMPACT < size::ROW);
    assert!(size::TAB < size::TITLE_BAR);
    assert!(radius::CHIP < radius::BUTTON && radius::BUTTON < radius::CARD);
    assert!(text::SMALL < text::UI && text::UI < text::DISPLAY);
};

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG AA for body text.
    const AA: f64 = 4.5;

    fn check(palette: &Palette, name: &str) {
        let pairs = [
            ("ink on bg", palette.ink, palette.bg),
            ("ink_2 on bg", palette.ink_2, palette.bg),
            ("ink_3 on bg", palette.ink_3, palette.bg),
            ("ink_3 on bg_hover", palette.ink_3, palette.bg_hover),
            ("accent_text on bg", palette.accent_text, palette.bg),
            (
                "accent_text on accent_soft",
                palette.accent_text,
                palette.accent_soft,
            ),
            (
                "primary_ink on primary",
                palette.primary_ink,
                palette.primary,
            ),
            (
                "primary_ink on primary_hover",
                palette.primary_ink,
                palette.primary_hover,
            ),
            ("accent_ink on accent", palette.accent_ink, palette.accent),
            ("good on bg", palette.good, palette.bg),
            ("warn on bg", palette.warn, palette.bg),
            ("danger on bg", palette.danger, palette.bg),
            ("good on good_soft", palette.good, palette.good_soft),
            ("warn on warn_soft", palette.warn, palette.warn_soft),
            ("danger on danger_soft", palette.danger, palette.danger_soft),
        ];
        for (label, fg, bg) in pairs {
            let ratio = fg.contrast(bg);
            assert!(ratio >= AA, "{name}: {label} is {ratio:.2}:1, below 4.5:1");
        }
    }

    #[test]
    fn light_text_pairs_meet_aa() {
        check(&LIGHT, "light");
    }

    #[test]
    fn dark_text_pairs_meet_aa() {
        check(&DARK, "dark");
    }

    #[test]
    fn surfaces_step_away_from_the_background() {
        for palette in [LIGHT, DARK] {
            let bg = palette.bg.relative_luminance();
            for surface in [palette.bg_hover, palette.bg_active, palette.line] {
                assert_ne!(surface.relative_luminance(), bg);
            }
        }
    }

    #[test]
    fn avatar_tints_are_readable() {
        for (palette, tints) in [(LIGHT, &AVATARS_LIGHT), (DARK, &AVATARS_DARK)] {
            assert_eq!(avatar_tints(&palette), tints);
            for (bg, fg) in tints {
                assert!(fg.contrast(*bg) >= AA, "{fg:?} on {bg:?}");
            }
        }
    }

    #[test]
    fn spacing_is_on_the_grid_and_ascending() {
        assert!(SPACE.windows(2).all(|w| w[0] < w[1]));
        assert!(SPACE.iter().skip(1).all(|s| s % 4.0 == 0.0 || *s == 6.0));
    }
}
