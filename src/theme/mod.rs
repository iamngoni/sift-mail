//! Turns the Sift design tokens into the running theme.
//!
//! GPUI Kit components read the global [`Theme`]; custom Sift components read
//! [`SiftTheme`]. Both come from [`tokens`], so the two can never drift apart.
//! [`install`] must run once, after `gpui_kit::init`, before any window opens.

pub mod color;
pub mod tokens;

use std::rc::Rc;

use gpui_kit::component::{ActiveTheme as _, Theme, ThemeConfig, ThemeMode, ThemeSet};
use gpui_kit::{App, Global, Window};
use serde_json::{Map, Value, json};

use self::tokens::{DARK, LIGHT, Palette};

/// Name of the light theme as GPUI Kit's registry knows it.
pub const LIGHT_THEME_NAME: &str = "Sift Light";
/// Name of the dark theme as GPUI Kit's registry knows it.
pub const DARK_THEME_NAME: &str = "Sift Dark";

/// What the user picked in onboarding or settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Appearance {
    /// Always light.
    Light,
    /// Always dark.
    Dark,
    /// Follow the operating system, including changes while the app runs.
    #[default]
    System,
}

impl Appearance {
    /// The next choice in the toolbar's cycle: System, Light, Dark, System…
    pub fn next(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }

    /// Stored form, for preferences.
    pub fn key(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => "system",
        }
    }

    /// Parses [`Appearance::key`]; anything else matches the system.
    pub fn from_key(key: &str) -> Self {
        match key {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }

    /// Human label, as shown in the toolbar tooltip.
    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::System => "Match system",
        }
    }
}

/// Why the generated theme could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    /// GPUI Kit rejected the generated theme JSON. This is a programmer error in
    /// [`theme_set_json`] or a breaking change in GPUI Kit's theme schema.
    #[error("GPUI Kit rejected the Sift theme: {0}")]
    Schema(#[from] serde_json::Error),
    /// The generated set did not contain both appearances.
    #[error("the Sift theme set is missing {0}")]
    Missing(&'static str),
}

/// The Sift-specific half of the theme: the chosen appearance and the token
/// palette that matches what is on screen.
#[derive(Debug, Clone, Copy)]
pub struct SiftTheme {
    appearance: Appearance,
    palette: Palette,
}

impl Global for SiftTheme {}

impl SiftTheme {
    /// The current Sift theme. Panics only if [`install`] was never called,
    /// which is a wiring error caught by every UI test.
    pub fn get(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    /// What the user chose.
    pub fn appearance(&self) -> Appearance {
        self.appearance
    }

    /// The tokens for the appearance currently on screen.
    pub fn palette(&self) -> &Palette {
        &self.palette
    }
}

/// Builds the GPUI Kit theme set for both appearances from [`tokens`].
///
/// Keys follow GPUI Kit's theme schema. Anything not listed falls back to
/// GPUI Kit's defaults, which are then tinted by the colours we do set.
pub fn theme_set_json() -> Value {
    json!({
        "name": "Sift",
        "author": "Sift Mail",
        "themes": [
            theme_config_json(LIGHT_THEME_NAME, "light", &LIGHT),
            theme_config_json(DARK_THEME_NAME, "dark", &DARK),
        ]
    })
}

fn theme_config_json(name: &str, mode: &str, p: &Palette) -> Value {
    let pairs = [
        ("background", p.bg),
        ("foreground", p.ink),
        ("border", p.line_strong),
        ("input.border", p.line_strong),
        ("ring", p.accent),
        ("caret", p.accent),
        ("link", p.accent_text),
        ("link.hover", p.accent_text),
        ("selection.background", p.accent_soft),
        ("muted.background", p.bg_hover),
        ("muted.foreground", p.ink_3),
        ("accent.background", p.bg_hover),
        ("accent.foreground", p.ink),
        ("primary.background", p.primary),
        ("primary.hover.background", p.primary_hover),
        ("primary.active.background", p.primary),
        ("primary.foreground", p.primary_ink),
        ("secondary.background", p.bg),
        ("secondary.hover.background", p.bg_hover),
        ("secondary.active.background", p.bg_hover),
        ("secondary.foreground", p.ink),
        ("popover.background", p.bg),
        ("popover.foreground", p.ink),
        ("list.background", p.bg),
        ("list.hover.background", p.bg_hover),
        ("list.active.background", p.bg_active),
        ("list.active.border", p.accent_line),
        ("title_bar.background", p.bg),
        ("title_bar.border", p.line),
        ("sidebar.background", p.bg_subtle),
        ("sidebar.border", p.line),
        ("sidebar.foreground", p.ink_2),
        ("sidebar.accent.background", p.bg),
        ("sidebar.accent.foreground", p.ink),
        ("tab_bar.background", p.bg),
        ("tab.background", p.bg),
        ("tab.foreground", p.ink_2),
        ("tab.active.background", p.bg_hover),
        ("tab.active.foreground", p.ink),
        ("switch.background", p.line_strong),
        ("success.background", p.good),
        ("warning.background", p.warn),
        ("danger.background", p.danger),
        ("window.border", p.line_strong),
    ];
    let colors: Map<String, Value> = pairs
        .into_iter()
        .map(|(key, value)| (key.to_owned(), Value::String(value.to_string())))
        .collect();
    json!({
        "name": name,
        "mode": mode,
        "font.size": tokens::text::UI,
        // GPUI Kit stores radii as whole pixels.
        "radius": tokens::radius::BUTTON.round() as u32,
        "radius.lg": tokens::radius::CARD.round() as u32,
        "shadow": false,
        "colors": colors,
    })
}

/// Parses [`theme_set_json`] into GPUI Kit's light and dark configs.
pub fn theme_configs() -> Result<(ThemeConfig, ThemeConfig), ThemeError> {
    let set: ThemeSet = serde_json::from_value(theme_set_json())?;
    let mut light = None;
    let mut dark = None;
    for config in set.themes {
        if config.mode.is_dark() {
            dark = Some(config);
        } else {
            light = Some(config);
        }
    }
    Ok((
        light.ok_or(ThemeError::Missing(LIGHT_THEME_NAME))?,
        dark.ok_or(ThemeError::Missing(DARK_THEME_NAME))?,
    ))
}

/// Registers the Sift themes with GPUI Kit and applies `appearance`.
pub fn install(appearance: Appearance, cx: &mut App) -> Result<(), ThemeError> {
    let (light, dark) = theme_configs()?;
    Theme::update(cx, |theme| {
        theme.light_theme = Rc::new(light);
        theme.dark_theme = Rc::new(dark);
    });
    cx.set_global(SiftTheme {
        appearance,
        palette: LIGHT,
    });
    apply(appearance, None, cx);
    Ok(())
}

/// Switches appearance. `System` follows `window`'s appearance when given,
/// otherwise the app-wide one.
pub fn apply(appearance: Appearance, window: Option<&mut Window>, cx: &mut App) {
    match appearance {
        Appearance::Light => Theme::change(ThemeMode::Light, window, cx),
        Appearance::Dark => Theme::change(ThemeMode::Dark, window, cx),
        Appearance::System => Theme::sync_system_appearance(window, cx),
    }
    let palette = if cx.theme().is_dark() { DARK } else { LIGHT };
    cx.set_global(SiftTheme {
        appearance,
        palette,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_keys_round_trip() {
        for a in [Appearance::Light, Appearance::Dark, Appearance::System] {
            assert_eq!(Appearance::from_key(a.key()), a);
        }
        assert_eq!(Appearance::from_key("sepia"), Appearance::System);
    }

    #[test]
    fn appearance_cycles_through_all_three() {
        let mut seen = vec![Appearance::default()];
        for _ in 0..3 {
            let next = seen[seen.len() - 1].next();
            seen.push(next);
        }
        assert_eq!(
            seen,
            [
                Appearance::System,
                Appearance::Light,
                Appearance::Dark,
                Appearance::System
            ]
        );
        assert_eq!(Appearance::Light.label(), "Light");
        assert_eq!(Appearance::Dark.label(), "Dark");
        assert_eq!(Appearance::System.label(), "Match system");
    }

    #[test]
    fn generated_themes_parse_and_carry_the_tokens() {
        let (light, dark) = theme_configs().unwrap();
        assert_eq!(light.name.as_ref(), LIGHT_THEME_NAME);
        assert_eq!(dark.name.as_ref(), DARK_THEME_NAME);
        assert!(!light.mode.is_dark());
        assert!(dark.mode.is_dark());
        assert_eq!(light.font_size, Some(tokens::text::UI));
        assert_eq!(light.radius, Some(8));
        assert_eq!(light.radius_lg, Some(12));

        let json = theme_set_json();
        let colors = &json["themes"][0]["colors"];
        assert_eq!(colors["background"], LIGHT.bg.to_string());
        assert_eq!(colors["primary.background"], LIGHT.primary.to_string());
        assert_eq!(
            json["themes"][1]["colors"]["background"],
            DARK.bg.to_string()
        );
    }

    #[test]
    fn theme_errors_explain_themselves() {
        let missing = ThemeError::Missing(DARK_THEME_NAME);
        assert!(missing.to_string().contains(DARK_THEME_NAME));
        let schema = ThemeError::from(serde_json::from_str::<Value>("{").unwrap_err());
        assert!(schema.to_string().starts_with("GPUI Kit rejected"));
    }

    #[gpui_kit::test]
    fn install_registers_both_themes_and_applies_the_choice(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            install(Appearance::Dark, cx).unwrap();
            assert!(cx.theme().is_dark());
            assert_eq!(cx.theme().theme_name().as_ref(), DARK_THEME_NAME);
            assert_eq!(SiftTheme::get(cx).appearance(), Appearance::Dark);
            assert_eq!(SiftTheme::get(cx).palette(), &DARK);

            apply(Appearance::Light, None, cx);
            assert!(!cx.theme().is_dark());
            assert_eq!(cx.theme().theme_name().as_ref(), LIGHT_THEME_NAME);
            assert_eq!(SiftTheme::get(cx).palette(), &LIGHT);

            apply(Appearance::System, None, cx);
            assert_eq!(SiftTheme::get(cx).appearance(), Appearance::System);
        });
    }
}
