use serde::de::{self, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// An sRGB colour with straight (non-premultiplied) alpha.
///
/// Components are normally in the range `0.0..=1.0`. Values above `1.0` are
/// allowed and are kept as they are.
///
/// In scene files a colour is written as a hex string such as `"#ff8800"` or
/// `"#ff880080"`. A tuple `(r, g, b, a)` is also accepted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);
    pub const RED: Color = Color::rgb(1.0, 0.0, 0.0);
    pub const GREEN: Color = Color::rgb(0.0, 1.0, 0.0);
    pub const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);
    pub const YELLOW: Color = Color::rgb(1.0, 1.0, 0.0);
    pub const CYAN: Color = Color::rgb(0.0, 1.0, 1.0);
    pub const MAGENTA: Color = Color::rgb(1.0, 0.0, 1.0);
    pub const GRAY: Color = Color::rgb(0.5, 0.5, 0.5);

    /// An opaque colour from red, green and blue in `0.0..=1.0`.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// A colour from red, green, blue and alpha in `0.0..=1.0`.
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// An opaque colour from a `0xRRGGBB` number, for example `Color::hex(0xff8800)`.
    pub const fn hex(rgb: u32) -> Self {
        Self {
            r: ((rgb >> 16) & 0xff) as f32 / 255.0,
            g: ((rgb >> 8) & 0xff) as f32 / 255.0,
            b: (rgb & 0xff) as f32 / 255.0,
            a: 1.0,
        }
    }

    /// The same colour with a different alpha.
    pub const fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Blends towards `other`. `t = 0.0` gives `self`, `t = 1.0` gives `other`.
    pub fn lerp(self, other: Color, t: f32) -> Color {
        Color {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a + (other.a - self.a) * t,
        }
    }

    /// Parses `"#rgb"`, `"#rrggbb"` or `"#rrggbbaa"`. The `#` is optional.
    pub fn parse(text: &str) -> Option<Color> {
        let s = text.trim().trim_start_matches('#');
        if !s.is_ascii() {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok().map(|v| v as f32 / 255.0);
        let nibble = |i: usize| u8::from_str_radix(&s[i..i + 1], 16).ok().map(|v| (v * 17) as f32 / 255.0);
        match s.len() {
            3 => Some(Color::rgb(nibble(0)?, nibble(1)?, nibble(2)?)),
            6 => Some(Color::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Color::rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    /// The colour as a hex string, `"#rrggbb"` or `"#rrggbbaa"` when not opaque.
    pub fn to_hex_string(self) -> String {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        if q(self.a) == 255 {
            format!("#{:02x}{:02x}{:02x}", q(self.r), q(self.g), q(self.b))
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", q(self.r), q(self.g), q(self.b), q(self.a))
        }
    }

    /// Converts to linear light, as the renderer needs. Alpha is unchanged.
    pub fn to_linear(self) -> [f32; 4] {
        fn lin(c: f32) -> f32 {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        [lin(self.r), lin(self.g), lin(self.b), self.a]
    }

    fn fits_in_hex(self) -> bool {
        [self.r, self.g, self.b, self.a].iter().all(|v| {
            let scaled = v * 255.0;
            (0.0..=255.0).contains(&scaled) && (scaled - scaled.round()).abs() < 1e-3
        })
    }
}

impl Default for Color {
    fn default() -> Self {
        Color::WHITE
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.fits_in_hex() {
            serializer.serialize_str(&self.to_hex_string())
        } else {
            let mut t = serializer.serialize_tuple(4)?;
            t.serialize_element(&self.r)?;
            t.serialize_element(&self.g)?;
            t.serialize_element(&self.b)?;
            t.serialize_element(&self.a)?;
            t.end()
        }
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ColorVisitor;

        impl<'de> Visitor<'de> for ColorVisitor {
            type Value = Color;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a colour like \"#ff8800\" or (r, g, b, a)")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Color, E> {
                Color::parse(v).ok_or_else(|| E::custom(format!("`{v}` is not a colour, expected something like \"#ff8800\"")))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Color, A::Error> {
                let mut next = |index: usize| -> Result<Option<f32>, A::Error> {
                    let _ = index;
                    seq.next_element::<f32>()
                };
                let r = next(0)?.ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let g = next(1)?.ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let b = next(2)?.ok_or_else(|| de::Error::invalid_length(2, &self))?;
                let a = next(3)?.unwrap_or(1.0);
                Ok(Color::rgba(r, g, b, a))
            }
        }

        deserializer.deserialize_any(ColorVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let c = Color::hex(0xff8800);
        assert_eq!(c.to_hex_string(), "#ff8800");
        assert_eq!(Color::parse("#ff8800"), Some(c));
        assert_eq!(Color::parse("f80"), Some(c));
        assert_eq!(Color::parse("#ff880080").unwrap().to_hex_string(), "#ff880080");
        assert_eq!(Color::parse("nope"), None);
        assert_eq!(Color::parse("#ffé"), None);
    }

    #[test]
    fn serde_forms() {
        let c: Color = ron::from_str("\"#336699\"").unwrap();
        assert_eq!(c, Color::hex(0x336699));
        let c: Color = ron::from_str("(2.0, 0.5, 0.25, 1.0)").unwrap();
        assert_eq!(c, Color::rgba(2.0, 0.5, 0.25, 1.0));
        assert_eq!(ron::to_string(&Color::hex(0x336699)).unwrap(), "\"#336699\"");
        let bright = ron::to_string(&Color::rgb(2.0, 0.5, 0.25)).unwrap();
        assert_eq!(ron::from_str::<Color>(&bright).unwrap(), Color::rgb(2.0, 0.5, 0.25));
    }

    #[test]
    fn linear_conversion() {
        let [r, g, b, a] = Color::rgba(1.0, 0.0, 0.5, 0.25).to_linear();
        assert_eq!((r, g, a), (1.0, 0.0, 0.25));
        assert!((b - 0.214).abs() < 0.001);
    }
}
