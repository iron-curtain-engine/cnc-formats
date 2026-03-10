// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! PAL palette parser (`.pal`).
//!
//! A `.pal` file is the simplest format in the C&C asset library: exactly
//! 768 raw bytes with no header, no footer, no magic number.
//!
//! ## Layout
//!
//! ```text
//! 768 bytes = 256 entries × 3 bytes (R, G, B)
//! ```
//!
//! ## 6-bit VGA Color Range
//!
//! Each R/G/B component is in the **6-bit VGA range (0–63)**, matching the
//! original VGA DAC register width.  To convert to the modern 8-bit (0–255)
//! range, shift each component left by 2 bits (`value << 2`).
//!
//! ## References
//!
//! Format source: `REDALERT/WIN32LIB/PALETTE.H`, `TIBERIANDAWN/WIN32LIB/LOADPAL.CPP`.

use crate::error::Error;

/// Number of colors in a C&C palette.
pub const PALETTE_SIZE: usize = 256;

/// Raw byte size of a `.pal` file on disk.
pub const PALETTE_BYTES: usize = PALETTE_SIZE * 3;

/// A single RGB color entry in the **6-bit VGA range** (components 0–63).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PalColor {
    /// Red channel (0–63).
    pub r: u8,
    /// Green channel (0–63).
    pub g: u8,
    /// Blue channel (0–63).
    pub b: u8,
}

impl PalColor {
    /// Converts this 6-bit color to 8-bit by shifting each component left 2.
    ///
    /// This matches the conversion performed by the original engine when
    /// programming the VGA DAC:
    /// ```c
    /// buffer[i] = palette[i] << 2;  // 6-bit (0–63) → 8-bit (0–252)
    /// ```
    #[inline]
    pub fn to_rgb8(self) -> [u8; 3] {
        [self.r << 2, self.g << 2, self.b << 2]
    }
}

/// A parsed 256-color C&C palette.
///
/// All color values are stored in the canonical **6-bit VGA range** (0–63).
/// Use [`PalColor::to_rgb8`] or [`Palette::to_rgb8_array`] for display-ready values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    /// The 256 color entries in 6-bit VGA range.
    pub colors: [PalColor; PALETTE_SIZE],
}

impl Palette {
    /// Parses a `.pal` file from a raw byte slice.
    ///
    /// The slice must be exactly [`PALETTE_BYTES`] (768) bytes long.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] if `data` is shorter than 768 bytes.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        if data.len() < PALETTE_BYTES {
            return Err(Error::UnexpectedEof);
        }
        let mut colors = [PalColor { r: 0, g: 0, b: 0 }; PALETTE_SIZE];
        for (i, color) in colors.iter_mut().enumerate() {
            color.r = data[i * 3];
            color.g = data[i * 3 + 1];
            color.b = data[i * 3 + 2];
        }
        Ok(Palette { colors })
    }

    /// Returns all 256 colors converted to 8-bit RGB triples.
    ///
    /// Each `[u8; 3]` is `[R, G, B]` in the range 0–252 (multiples of 4).
    pub fn to_rgb8_array(&self) -> [[u8; 3]; PALETTE_SIZE] {
        let mut out = [[0u8; 3]; PALETTE_SIZE];
        for (i, c) in self.colors.iter().enumerate() {
            out[i] = c.to_rgb8();
        }
        out
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn all_zero_pal() -> Vec<u8> {
        vec![0u8; PALETTE_BYTES]
    }

    /// 768 zero bytes → palette with all black entries.
    #[test]
    fn test_parse_all_zero() {
        let pal = Palette::parse(&all_zero_pal()).unwrap();
        assert_eq!(pal.colors.len(), PALETTE_SIZE);
        for c in &pal.colors {
            assert_eq!(*c, PalColor { r: 0, g: 0, b: 0 });
        }
    }

    /// Data shorter than 768 bytes → UnexpectedEof.
    #[test]
    fn test_parse_too_short() {
        let result = Palette::parse(&[0u8; 767]);
        assert_eq!(result, Err(Error::UnexpectedEof));
    }

    /// Empty slice → UnexpectedEof.
    #[test]
    fn test_parse_empty() {
        let result = Palette::parse(&[]);
        assert_eq!(result, Err(Error::UnexpectedEof));
    }

    /// Known values are parsed into the correct color slots.
    #[test]
    fn test_parse_known_values() {
        let mut data = all_zero_pal();
        // Color 0: (1, 2, 3)
        data[0] = 1;
        data[1] = 2;
        data[2] = 3;
        // Color 255: (63, 63, 63) — white in VGA 6-bit
        data[255 * 3] = 63;
        data[255 * 3 + 1] = 63;
        data[255 * 3 + 2] = 63;
        // Color 128: (10, 20, 30)
        data[128 * 3] = 10;
        data[128 * 3 + 1] = 20;
        data[128 * 3 + 2] = 30;

        let pal = Palette::parse(&data).unwrap();

        assert_eq!(pal.colors[0], PalColor { r: 1, g: 2, b: 3 });
        assert_eq!(
            pal.colors[255],
            PalColor {
                r: 63,
                g: 63,
                b: 63
            }
        );
        assert_eq!(
            pal.colors[128],
            PalColor {
                r: 10,
                g: 20,
                b: 30
            }
        );
    }

    /// to_rgb8 converts 6-bit values to 8-bit via left-shift of 2.
    #[test]
    fn test_to_rgb8_conversion() {
        assert_eq!(PalColor { r: 0, g: 0, b: 0 }.to_rgb8(), [0, 0, 0]);
        // 63 << 2 = 252 (not 255 — VGA 6-bit tops out at 252/255)
        assert_eq!(
            PalColor {
                r: 63,
                g: 63,
                b: 63
            }
            .to_rgb8(),
            [252, 252, 252]
        );
        assert_eq!(PalColor { r: 32, g: 16, b: 8 }.to_rgb8(), [128, 64, 32]);
        // 1 << 2 = 4
        assert_eq!(PalColor { r: 1, g: 2, b: 3 }.to_rgb8(), [4, 8, 12]);
    }

    /// to_rgb8_array converts all 256 entries.
    #[test]
    fn test_to_rgb8_array_white_entry() {
        let mut data = all_zero_pal();
        data[10 * 3] = 63;
        data[10 * 3 + 1] = 63;
        data[10 * 3 + 2] = 63;

        let pal = Palette::parse(&data).unwrap();
        let rgb8 = pal.to_rgb8_array();

        assert_eq!(rgb8[10], [252, 252, 252]);
        assert_eq!(rgb8[0], [0, 0, 0]);
    }

    /// Extra bytes beyond 768 are ignored (only first 768 are consumed).
    #[test]
    fn test_parse_extra_bytes_ignored() {
        let mut data = all_zero_pal();
        data[0] = 7;
        // Append some extra bytes — parse should still succeed
        let mut extended = data.clone();
        extended.extend_from_slice(&[0xFFu8; 10]);

        let pal = Palette::parse(&extended).unwrap();
        assert_eq!(pal.colors[0].r, 7);
    }

    /// Palette is consistent: parsing the same data twice gives equal results.
    #[test]
    fn test_parse_deterministic() {
        let mut data = all_zero_pal();
        for (i, byte) in data.iter_mut().enumerate().take(PALETTE_BYTES) {
            *byte = (i % 64) as u8;
        }
        let p1 = Palette::parse(&data).unwrap();
        let p2 = Palette::parse(&data).unwrap();
        assert_eq!(p1, p2);
    }
}
