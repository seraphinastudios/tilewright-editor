// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The project-art mark: one rule for every image the editor makes while
//! project art is loaded ([`crate::PreviewCatalog::contains_project_art`]).
//! The desktop editor's evidence frames, the `render` command's file and
//! the MCP servers' `render` images all go through [`marked`].
//!
//! The mark is a strip added below the image, never drawn over it: a near-black
//! separator, then an amber band with [`TEXT`] in black capitals from a
//! fixed bitmap font that is part of this module (no font file, no text
//! shaping, the same pixels on every platform). The image's own pixels
//! keep their place and their values, so a coordinate in the map part of a
//! marked image is the coordinate in the unmarked one. The height grows by
//! [`rows`]; an image narrower than [`MINIMUM_WIDTH`], the width
//! [`SHORT_TEXT`] needs, is also widened to it by [`columns`] of band colour
//! on the right, the full height, so the mark is always legible. The strip's
//! look depends on the marked width alone: the glyphs are scaled by the
//! largest whole factor up to [`MAXIMUM_SCALE`] at which [`TEXT`] fits, and
//! an image too narrow for [`TEXT`] at factor 1 gets [`SHORT_TEXT`].
use crate::ReferenceFrame;

/// What the mark says.
pub const TEXT: &str = "PROJECT ART: CHECK ITS LICENSE BEFORE SHARING";
/// What the mark says on an image too narrow for [`TEXT`].
pub const SHORT_TEXT: &str = "PROJECT ART";
/// The largest glyph scale.
pub const MAXIMUM_SCALE: u32 = 4;

/// Glyph cell: 5 × 7 pixels and a pixel of space after each glyph.
const GLYPH_WIDTH: u32 = 5;
const GLYPH_HEIGHT: u32 = 7;
const ADVANCE: u32 = GLYPH_WIDTH + 1;
/// Space left of the text, in glyph pixels.
const MARGIN: u32 = 3;
/// Band space above and below the text, in glyph pixels.
const PADDING: u32 = 2;
/// Rows per glyph pixel: the separator, then the band.
const ROWS_PER_SCALE: u32 = 1 + PADDING + GLYPH_HEIGHT + PADDING;

/// The narrowest marked image: [`SHORT_TEXT`] whole at glyph scale 1,
/// with its margins.
#[allow(clippy::cast_possible_truncation)] // 11 characters.
pub const MINIMUM_WIDTH: u32 = (SHORT_TEXT.len() as u32) * ADVANCE + 2 * MARGIN;

const SEPARATOR: [u8; 4] = [24, 24, 24, 255];
const BAND: [u8; 4] = [255, 200, 0, 255];
const INK: [u8; 4] = [0, 0, 0, 255];

/// The text and glyph scale of the mark on an image `width` pixels wide.
fn layout(width: u32) -> (&'static str, u32) {
    let needed = |text: &str, scale: u32| {
        let glyphs = u32::try_from(text.len()).unwrap_or(u32::MAX);
        glyphs
            .saturating_mul(ADVANCE)
            .saturating_add(2 * MARGIN)
            .saturating_mul(scale)
    };
    (1..=MAXIMUM_SCALE)
        .rev()
        .find(|scale| needed(TEXT, *scale) <= width)
        .map_or((SHORT_TEXT, 1), |scale| (TEXT, scale))
}

/// How many rows the mark adds below an image `width` pixels wide.
pub fn rows(width: u32) -> u32 {
    ROWS_PER_SCALE * layout(width.max(MINIMUM_WIDTH)).1
}

/// How many columns of band colour the mark adds right of an image `width`
/// pixels wide: none from [`MINIMUM_WIDTH`] on.
pub const fn columns(width: u32) -> u32 {
    MINIMUM_WIDTH.saturating_sub(width)
}

/// `[width, height]` of the marked image of a `width` × `height` one, or
/// `None` when the height overflows.
pub fn size(width: u32, height: u32) -> Option<[u32; 2]> {
    Some([width + columns(width), height.checked_add(rows(width))?])
}

/// The text of the mark on an image `width` pixels wide: [`TEXT`] or
/// [`SHORT_TEXT`], always whole.
pub fn text(width: u32) -> &'static str {
    layout(width.max(MINIMUM_WIDTH)).0
}

/// `frame` with the mark added: [`rows`] more rows below it and, on an
/// image narrower than [`MINIMUM_WIDTH`], [`columns`] of band colour right
/// of it; the frame's own pixels unchanged at the top left. `None` when the
/// frame's pixels are not `width × height` RGBA8 or the marked size
/// overflows.
pub fn marked(frame: &ReferenceFrame) -> Option<ReferenceFrame> {
    let original = usize::try_from(frame.width).ok()?.checked_mul(4)?;
    if Some(frame.rgba8.len()) != original.checked_mul(usize::try_from(frame.height).ok()?) {
        return None;
    }
    let [width, height] = size(frame.width, frame.height)?;
    let stride = usize::try_from(width).ok()?.checked_mul(4)?;
    let (text, scale) = layout(width);
    let mut rgba8 = Vec::with_capacity(stride.checked_mul(usize::try_from(height).ok()?)?);
    // Row by row, so a zero-width frame of any height gets its full rows
    // of band colour too.
    for row in 0..usize::try_from(frame.height).ok()? {
        rgba8.extend_from_slice(&frame.rgba8[row * original..(row + 1) * original]);
        for _ in 0..columns(frame.width) {
            rgba8.extend_from_slice(&BAND);
        }
    }
    for row in 0..height - frame.height {
        let color = if row < scale { SEPARATOR } else { BAND };
        for _ in 0..width {
            rgba8.extend_from_slice(&color);
        }
    }
    let top = usize::try_from(frame.height + scale * (1 + PADDING)).ok()?;
    for (index, character) in text.bytes().enumerate() {
        let index = u32::try_from(index).ok()?;
        let left = scale * (MARGIN + index * ADVANCE);
        for (glyph_row, bits) in glyph(character).iter().enumerate() {
            for column in 0..GLYPH_WIDTH {
                if bits & (1 << (GLYPH_WIDTH - 1 - column)) == 0 {
                    continue;
                }
                for dy in 0..scale {
                    let y =
                        top + usize::try_from(scale * u32::try_from(glyph_row).ok()? + dy).ok()?;
                    for dx in 0..scale {
                        let x = left + scale * column + dx;
                        if x >= width {
                            continue;
                        }
                        let at = y * stride + usize::try_from(x).ok()? * 4;
                        rgba8[at..at + 4].copy_from_slice(&INK);
                    }
                }
            }
        }
    }
    Some(ReferenceFrame {
        width,
        height,
        rgba8,
    })
}

/// The 5 × 7 glyph of an uppercase letter, `:` or space: one byte per row,
/// top first, the leftmost pixel the highest of the five low bits. Every
/// other byte is blank; [`TEXT`] and [`SHORT_TEXT`] use none.
const fn glyph(character: u8) -> [u8; 7] {
    match character {
        b'A' => [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        b'B' => [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e],
        b'C' => [0x0e, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0e],
        b'E' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f],
        b'F' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10],
        b'G' => [0x0e, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0f],
        b'H' => [0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        b'I' => [0x0e, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0e],
        b'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0c],
        b'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        b'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1f],
        b'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        b'O' => [0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        b'P' => [0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10],
        b'R' => [0x1e, 0x11, 0x11, 0x1e, 0x14, 0x12, 0x11],
        b'S' => [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e],
        b'T' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b':' => [0x00, 0x0c, 0x0c, 0x00, 0x0c, 0x0c, 0x00],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ADVANCE, BAND, INK, MARGIN, MAXIMUM_SCALE, MINIMUM_WIDTH, SEPARATOR, SHORT_TEXT, TEXT,
        columns, glyph, marked, rows, size, text,
    };
    use crate::ReferenceFrame;

    fn frame(width: u32, height: u32) -> ReferenceFrame {
        let rgba8 = (0..width * height)
            .flat_map(|index| {
                let [a, b, ..] = index.to_le_bytes();
                [a, b, 7, 255]
            })
            .collect();
        ReferenceFrame {
            width,
            height,
            rgba8,
        }
    }

    fn pixel(frame: &ReferenceFrame, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * frame.width + x) * 4) as usize;
        frame.rgba8[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn every_character_of_the_texts_has_a_glyph() {
        for character in TEXT.bytes().chain(SHORT_TEXT.bytes()) {
            assert_eq!(
                glyph(character) == [0; 7],
                character == b' ',
                "{}",
                char::from(character)
            );
        }
        // Distinct glyphs, so the text reads as written.
        let mut seen = std::collections::BTreeMap::new();
        for character in TEXT.bytes().filter(|c| *c != b' ') {
            let previous = seen.insert(glyph(character), character);
            assert!(previous.is_none_or(|p| p == character));
        }
    }

    #[test]
    fn the_scale_is_the_largest_at_which_the_text_fits() {
        let one = (u32::try_from(TEXT.len()).unwrap() * ADVANCE) + 2 * MARGIN;
        assert_eq!(one, 276);
        assert_eq!((text(one - 1), rows(one - 1)), (SHORT_TEXT, 12));
        assert_eq!((text(one), rows(one)), (TEXT, 12));
        assert_eq!((text(2 * one - 1), rows(2 * one - 1)), (TEXT, 12));
        assert_eq!((text(2 * one), rows(2 * one)), (TEXT, 24));
        assert_eq!(rows(720), 24);
        assert_eq!(rows(1280), 48);
        assert_eq!(rows(8192), 12 * MAXIMUM_SCALE);
        assert_eq!((text(2), rows(2)), (SHORT_TEXT, 12));
        assert_eq!(MINIMUM_WIDTH, 72);
        for (width, added) in [(1, 71), (32, 40), (71, 1), (72, 0), (276, 0)] {
            assert_eq!(columns(width), added, "{width}");
            assert_eq!(size(width, 5), Some([width + added, 5 + 12]));
        }
        assert_eq!(size(10, u32::MAX - 11), None);
    }

    #[test]
    fn the_strip_is_added_below_and_leaves_the_image_unchanged() {
        for (original_width, height) in [(1, 3), (2, 2), (60, 10), (300, 40), (720, 480)] {
            let original = frame(original_width, height);
            let marked = marked(&original).unwrap();
            let width = original_width.max(MINIMUM_WIDTH);
            let scale = rows(width) / 12;
            assert_eq!(marked.width, width);
            assert_eq!(marked.height, height + rows(width));
            assert_eq!(
                marked.rgba8.len(),
                (width * marked.height * 4) as usize,
                "{width}"
            );
            for y in 0..height {
                for x in 0..width {
                    let expected = if x < original_width {
                        pixel(&original, x, y)
                    } else {
                        BAND
                    };
                    assert_eq!(pixel(&marked, x, y), expected, "{original_width} {x} {y}");
                }
            }
            for x in 0..width {
                for y in height..height + scale {
                    assert_eq!(pixel(&marked, x, y), SEPARATOR);
                }
                // The band's top padding and its last rows are band only.
                for y in
                    (height + scale..height + 3 * scale).chain(height + 10 * scale..marked.height)
                {
                    assert_eq!(pixel(&marked, x, y), BAND, "{width} {x} {y}");
                }
            }
            let strip: Vec<[u8; 4]> = (height..marked.height)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .map(|(x, y)| pixel(&marked, x, y))
                .collect();
            assert!(strip.iter().all(|p| [SEPARATOR, BAND, INK].contains(p)));
            assert!(strip.contains(&INK), "{width}");
        }
    }

    /// Whether `text` is drawn whole at glyph scale 1 in the strip of
    /// `marked`, whose unmarked height was `height`, and nothing else is.
    fn reads(marked: &ReferenceFrame, height: u32, text: &str) -> bool {
        let top = height + 3;
        let mut expected = vec![false; (marked.width * 7) as usize];
        for (index, character) in text.bytes().enumerate() {
            let left = MARGIN + u32::try_from(index).unwrap() * ADVANCE;
            for (row, bits) in glyph(character).iter().enumerate() {
                for column in 0..5 {
                    if bits & (1 << (4 - column)) != 0 {
                        let x = left + column;
                        if x >= marked.width {
                            return false;
                        }
                        expected[(u32::try_from(row).unwrap() * marked.width + x) as usize] = true;
                    }
                }
            }
        }
        (0..7).all(|row| {
            (0..marked.width).all(|x| {
                (pixel(marked, x, top + row) == INK) == expected[(row * marked.width + x) as usize]
            })
        })
    }

    #[test]
    fn a_narrow_image_is_widened_so_the_short_text_is_whole() {
        for width in [1, 32, 71, 72, 275] {
            let original = frame(width, 9);
            let marked = marked(&original).unwrap();
            assert_eq!(marked.width, width.max(72), "{width}");
            assert!(reads(&marked, 9, SHORT_TEXT), "{width}");
            assert!(!reads(&marked, 9, "PROJECT AR"), "{width}");
        }
        let marked = marked(&frame(276, 9)).unwrap();
        assert!(reads(&marked, 9, TEXT));
    }

    #[test]
    fn the_glyphs_are_drawn_where_the_layout_puts_them() {
        let original = frame(720, 4);
        let marked = marked(&original).unwrap();
        // Scale 2: the first glyph, P, starts 6 pixels in and 6 rows below
        // the image (2 separator rows, 4 padding rows).
        let top = 4 + 6;
        let p = glyph(b'P');
        for (row, bits) in p.iter().enumerate() {
            for column in 0..5u32 {
                let ink = bits & (1 << (4 - column)) != 0;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let expected = if ink { INK } else { BAND };
                    assert_eq!(
                        pixel(
                            &marked,
                            6 + 2 * column + dx,
                            top + 2 * u32::try_from(row).unwrap() + dy
                        ),
                        expected
                    );
                }
            }
        }
        // Nothing is inked right of the text.
        let end = 2 * (MARGIN + u32::try_from(TEXT.len()).unwrap() * ADVANCE);
        for y in 4..marked.height {
            for x in end..720 {
                assert_ne!(pixel(&marked, x, y), INK);
            }
        }
    }

    #[test]
    fn a_zero_width_or_height_frame_is_marked_too() {
        for (width, height) in [(0, 5), (0, 0), (5, 0)] {
            let original = ReferenceFrame {
                width,
                height,
                rgba8: Vec::new(),
            };
            let marked = marked(&original).unwrap();
            assert_eq!(
                (marked.width, marked.height),
                (MINIMUM_WIDTH, height + 12),
                "{width}x{height}"
            );
            assert_eq!(
                marked.rgba8.len(),
                (marked.width * marked.height * 4) as usize
            );
            for y in 0..height {
                for x in 0..MINIMUM_WIDTH {
                    assert_eq!(pixel(&marked, x, y), BAND);
                }
            }
            assert!(reads(&marked, height, SHORT_TEXT));
        }
    }

    #[test]
    fn an_inconsistent_frame_is_refused() {
        let mut broken = frame(4, 4);
        broken.rgba8.pop();
        assert!(marked(&broken).is_none());
        broken.rgba8.extend_from_slice(&[0, 0]);
        assert!(marked(&broken).is_none());
    }

    #[test]
    fn marking_is_deterministic() {
        let original = frame(333, 21);
        assert_eq!(marked(&original), marked(&original));
    }
}
