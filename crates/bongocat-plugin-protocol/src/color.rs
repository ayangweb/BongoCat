//! The colour notation a scene and a manifest share.
//!
//! One notation, `#rrggbbaa`, accepted with or without the leading `#` and with
//! three, four, six or eight hex digits. Six digits means fully opaque, because
//! a panel that forgot its alpha should look solid rather than invisible.
//!
//! Channels are stored as bytes rather than floats on purpose: a plugin's colour
//! travels from JSON, through this type, into an 8-bit raster and then into a
//! texture the drawable samples without conversion. There is no value in the
//! chain where the colour could be re-interpreted, which is what keeps a panel
//! looking the same on both backends.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// A straight (non-premultiplied) 8-bit colour.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: 255,
        }
    }

    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    pub fn parse(value: &str) -> Result<Self, ColorError> {
        let digits = value.strip_prefix('#').unwrap_or(value);
        if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ColorError::NotHex);
        }
        let nibble = |index: usize| -> u8 {
            let byte = digits.as_bytes()[index];
            match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => byte - b'A' + 10,
            }
        };
        // The two shorthand lengths repeat one digit (`#abc` is `#aabbcc`, so
        // three digits are not a scaled version of the six they abbreviate), and
        // the two full lengths read two adjacent digits per channel.
        let doubled = |index: usize| (nibble(index) << 4) | nibble(index);
        let pair = |index: usize| (nibble(index) << 4) | nibble(index + 1);
        Ok(match digits.len() {
            3 => Self::rgb(doubled(0), doubled(1), doubled(2)),
            4 => Self::rgba(doubled(0), doubled(1), doubled(2), doubled(3)),
            6 => Self::rgb(pair(0), pair(2), pair(4)),
            8 => Self::rgba(pair(0), pair(2), pair(4), pair(6)),
            _ => return Err(ColorError::WrongLength),
        })
    }

    pub fn to_rgba8(self) -> [u8; 4] {
        [self.red, self.green, self.blue, self.alpha]
    }

    /// Linear interpolation between two colours in straight-alpha space.
    ///
    /// Used only for a panel that fades between two declared colours. It is not
    /// a colour-space conversion: the point is that the result is written to the
    /// same 8-bit raster as everything else, so no intermediate value is ever
    /// stored somewhere the drawable would sample differently.
    pub fn lerp(self, other: Self, fraction: f32) -> Self {
        let mix = |from: u8, to: u8| {
            let from = f32::from(from);
            let to = f32::from(to);
            (from + (to - from) * fraction.clamp(0.0, 1.0))
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Self::rgba(
            mix(self.red, other.red),
            mix(self.green, other.green),
            mix(self.blue, other.blue),
            mix(self.alpha, other.alpha),
        )
    }
}

impl fmt::Display for Color {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "#{:02x}{:02x}{:02x}{:02x}",
            self.red, self.green, self.blue, self.alpha
        )
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ColorError {
    #[error("a colour must be written with `#`, 3, 4, 6 or 8 hexadecimal digits")]
    WrongLength,
    #[error("a colour must contain only hexadecimal digits")]
    NotHex,
}

#[cfg(test)]
mod tests {
    use super::{Color, ColorError};

    #[test]
    fn six_digits_are_opaque_and_the_hash_is_optional() {
        assert_eq!(Color::parse("#ff8800"), Ok(Color::rgb(255, 136, 0)));
        assert_eq!(Color::parse("ff8800"), Ok(Color::rgb(255, 136, 0)));
    }

    #[test]
    fn three_digits_expand_rather_than_scale() {
        assert_eq!(Color::parse("#abc"), Ok(Color::rgb(0xaa, 0xbb, 0xcc)));
        assert_eq!(
            Color::parse("#abcd"),
            Ok(Color::rgba(0xaa, 0xbb, 0xcc, 0xdd))
        );
    }

    #[test]
    fn eight_digits_carry_an_alpha_channel() {
        assert_eq!(
            Color::parse("#11223344"),
            Ok(Color::rgba(0x11, 0x22, 0x33, 0x44))
        );
    }

    #[test]
    fn parsing_is_case_insensitive_and_rejects_anything_else() {
        assert_eq!(Color::parse("#AABBCC"), Ok(Color::rgb(0xaa, 0xbb, 0xcc)));
        assert_eq!(Color::parse("#ff88zz"), Err(ColorError::NotHex));
        assert_eq!(Color::parse("#ff880"), Err(ColorError::WrongLength));
        assert_eq!(Color::parse("#"), Err(ColorError::WrongLength));
        assert_eq!(Color::parse("#ff88000"), Err(ColorError::WrongLength));
    }

    #[test]
    fn a_colour_round_trips_through_its_written_form() {
        let color = Color::rgba(1, 2, 3, 4);
        assert_eq!(Color::parse(&color.to_string()), Ok(color));
    }

    #[test]
    fn json_uses_the_written_form() {
        let color: Color = serde_json::from_str("\"#01020304\"").unwrap();
        assert_eq!(color, Color::rgba(1, 2, 3, 4));
        assert_eq!(serde_json::to_string(&color).unwrap(), "\"#01020304\"");
        assert!(serde_json::from_str::<Color>("\"nope\"").is_err());
    }

    #[test]
    fn interpolation_clamps_to_the_endpoints() {
        let black = Color::BLACK;
        let white = Color::WHITE;
        assert_eq!(black.lerp(white, -1.0), black);
        assert_eq!(black.lerp(white, 2.0), white);
        assert_eq!(black.lerp(white, 0.0), black);
        assert_eq!(black.lerp(white, 1.0), white);
    }
}
