//! Cell styling: font emphasis, colours, alignment and borders.

/// An sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0, 0, 0);
    pub const WHITE: Rgb = Rgb(255, 255, 255);

    /// Parse `RRGGBB` or `AARRGGBB` hex (alpha ignored).
    pub fn from_hex(hex: &str) -> Option<Rgb> {
        let hex = hex.trim_start_matches('#');
        let hex = if hex.len() == 8 { &hex[2..] } else { hex };
        if hex.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(hex, 16).ok()?;
        Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    pub fn to_u32(self) -> u32 {
        (self.0 as u32) << 16 | (self.1 as u32) << 8 | self.2 as u32
    }

    /// Relative luminance, 0 (black) to 1 (white).
    pub fn luminance(self) -> f32 {
        let lin = |c: u8| {
            let c = c as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.0) + 0.7152 * lin(self.1) + 0.0722 * lin(self.2)
    }

    /// Lighten (`tint > 0`) or darken (`tint < 0`) the way Excel applies theme tints.
    pub fn tinted(self, tint: f32) -> Rgb {
        let t = |c: u8| {
            let c = c as f32;
            let v = if tint < 0.0 {
                c * (1.0 + tint)
            } else {
                c + (255.0 - c) * tint
            };
            v.round().clamp(0.0, 255.0) as u8
        };
        Rgb(t(self.0), t(self.1), t(self.2))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HAlign {
    /// Numbers right, text left, booleans and errors centred.
    #[default]
    General,
    Left,
    Center,
    Right,
}

/// Which edges of a cell have a (thin) border.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Borders {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

impl Borders {
    pub const NONE: Borders = Borders {
        top: false,
        bottom: false,
        left: false,
        right: false,
    };
    pub const ALL: Borders = Borders {
        top: true,
        bottom: true,
        left: true,
        right: true,
    };

    pub fn any(self) -> bool {
        self.top || self.bottom || self.left || self.right
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// Text colour; `None` follows the theme.
    pub color: Option<Rgb>,
    /// Background fill; `None` is no fill.
    pub fill: Option<Rgb>,
    pub align: HAlign,
    pub borders: Borders,
}

impl Style {
    pub fn is_default(&self) -> bool {
        *self == Style::default()
    }
}

/// The colour palette offered in the toolbar: a row of theme-like hues with
/// lighter and darker shades.
pub const PALETTE: [[Rgb; 10]; 4] = [
    [
        Rgb(0, 0, 0),
        Rgb(68, 68, 68),
        Rgb(192, 0, 0),
        Rgb(237, 125, 49),
        Rgb(255, 192, 0),
        Rgb(112, 173, 71),
        Rgb(22, 163, 74),
        Rgb(68, 114, 196),
        Rgb(112, 48, 160),
        Rgb(255, 255, 255),
    ],
    [
        Rgb(89, 89, 89),
        Rgb(128, 128, 128),
        Rgb(255, 99, 99),
        Rgb(244, 176, 132),
        Rgb(255, 230, 153),
        Rgb(169, 208, 142),
        Rgb(134, 239, 172),
        Rgb(155, 194, 230),
        Rgb(196, 160, 222),
        Rgb(242, 242, 242),
    ],
    [
        Rgb(166, 166, 166),
        Rgb(191, 191, 191),
        Rgb(255, 199, 206),
        Rgb(252, 228, 214),
        Rgb(255, 242, 204),
        Rgb(226, 239, 218),
        Rgb(220, 252, 231),
        Rgb(221, 235, 247),
        Rgb(234, 222, 244),
        Rgb(217, 217, 217),
    ],
    [
        Rgb(38, 38, 38),
        Rgb(13, 13, 13),
        Rgb(128, 0, 0),
        Rgb(197, 90, 17),
        Rgb(191, 143, 0),
        Rgb(84, 130, 53),
        Rgb(21, 128, 61),
        Rgb(47, 84, 150),
        Rgb(84, 36, 120),
        Rgb(166, 166, 166),
    ],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours() {
        assert_eq!(Rgb::from_hex("FF16A34A"), Some(Rgb(22, 163, 74)));
        assert_eq!(Rgb::from_hex("#ffffff"), Some(Rgb::WHITE));
        assert_eq!(Rgb::from_hex("12345"), None);
        assert_eq!(Rgb(22, 163, 74).to_u32(), 0x16A34A);
        assert!(Rgb::WHITE.luminance() > 0.99 && Rgb::BLACK.luminance() < 0.01);
        assert_eq!(Rgb(100, 100, 100).tinted(-0.5), Rgb(50, 50, 50));
        assert_eq!(Rgb(100, 100, 100).tinted(0.5), Rgb(178, 178, 178));
    }
}
