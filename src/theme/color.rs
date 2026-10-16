//! A validated sRGB colour, the unit every design token is written in.
//!
//! Tokens are written as `#RRGGBB`. Parsing happens once, here, so the rest of
//! the theme works with a value that is always a real, opaque colour.

use std::fmt;

/// Why a string could not be read as a `#RRGGBB` colour.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ColorError {
    /// The value did not start with `#`.
    #[error("colour {0:?} must start with '#'")]
    MissingHash(String),
    /// The value was not exactly six hex digits after `#`.
    #[error("colour {0:?} must have exactly six hex digits")]
    WrongLength(String),
    /// A digit was not 0-9 or A-F.
    #[error("colour {0:?} contains a non-hex digit")]
    NotHex(String),
}

/// An opaque sRGB colour. Construct it with [`Rgb::from_u32`] for constants or
/// [`Rgb::parse`] for text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

impl Rgb {
    /// Builds a colour from `0xRRGGBB`. Bits above the low 24 are ignored.
    pub const fn from_u32(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xFF) as u8,
            g: ((hex >> 8) & 0xFF) as u8,
            b: (hex & 0xFF) as u8,
        }
    }

    /// Parses `#RRGGBB` (either case).
    pub fn parse(text: &str) -> Result<Self, ColorError> {
        let digits = text
            .strip_prefix('#')
            .ok_or_else(|| ColorError::MissingHash(text.to_owned()))?;
        if digits.len() != 6 {
            return Err(ColorError::WrongLength(text.to_owned()));
        }
        let value =
            u32::from_str_radix(digits, 16).map_err(|_| ColorError::NotHex(text.to_owned()))?;
        Ok(Self::from_u32(value))
    }

    /// The colour packed as `0xRRGGBB`, the form GPUI's `rgb()` takes.
    pub const fn to_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }

    /// WCAG 2.x relative luminance, from 0 (black) to 1 (white).
    pub fn relative_luminance(self) -> f64 {
        fn channel(value: u8) -> f64 {
            let c = f64::from(value) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }

    /// WCAG contrast ratio between two colours, from 1 to 21. Order does not matter.
    pub fn contrast(self, other: Self) -> f64 {
        let (a, b) = (self.relative_luminance(), other.relative_luminance());
        let (light, dark) = if a >= b { (a, b) } else { (b, a) };
        (light + 0.05) / (dark + 0.05)
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

impl From<Rgb> for gpui_kit::Hsla {
    fn from(color: Rgb) -> Self {
        gpui_kit::rgb(color.to_u32()).into()
    }
}

impl From<Rgb> for gpui_kit::Fill {
    fn from(color: Rgb) -> Self {
        gpui_kit::Hsla::from(color).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_prints_round_trip() {
        let c = Rgb::parse("#6e56cf").unwrap();
        assert_eq!(c, Rgb::from_u32(0x6E56CF));
        assert_eq!(c.to_string(), "#6E56CF");
        assert_eq!(c.to_u32(), 0x6E56CF);
    }

    #[test]
    fn rejects_malformed_values() {
        assert_eq!(
            Rgb::parse("6E56CF"),
            Err(ColorError::MissingHash("6E56CF".into()))
        );
        assert_eq!(
            Rgb::parse("#6E56C"),
            Err(ColorError::WrongLength("#6E56C".into()))
        );
        assert_eq!(
            Rgb::parse("#6E56CFF"),
            Err(ColorError::WrongLength("#6E56CFF".into()))
        );
        assert_eq!(
            Rgb::parse("#GG56CF"),
            Err(ColorError::NotHex("#GG56CF".into()))
        );
        assert!(ColorError::NotHex("#GG".into()).to_string().contains("hex"));
    }

    #[test]
    fn ignores_bits_above_24() {
        assert_eq!(Rgb::from_u32(0xFF_123456), Rgb::from_u32(0x123456));
    }

    #[test]
    fn contrast_matches_wcag_reference_values() {
        let black = Rgb::from_u32(0x000000);
        let white = Rgb::from_u32(0xFFFFFF);
        assert!((black.contrast(white) - 21.0).abs() < 1e-9);
        assert!((white.contrast(black) - 21.0).abs() < 1e-9);
        assert!((white.contrast(white) - 1.0).abs() < 1e-9);
        // #777777 on white is the classic "just under AA" grey.
        let grey = Rgb::from_u32(0x777777);
        let ratio = grey.contrast(white);
        assert!(ratio > 4.47 && ratio < 4.49, "{ratio}");
    }

    #[test]
    fn converts_to_gpui_colour() {
        let hsla: gpui_kit::Hsla = Rgb::from_u32(0xFFFFFF).into();
        assert!((hsla.l - 1.0).abs() < 1e-6);
        assert!((hsla.a - 1.0).abs() < 1e-6);
    }
}
