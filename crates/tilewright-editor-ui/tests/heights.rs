// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The height tint as drawn: its bands keep their lightness order once
//! blended over the neutral sample's own ground. Reads the sample's art
//! (MIT), which only an integration test may do.
use tilewright_editor_ui::heights::{RAMP, TINT_ALPHA};

/// CIE L* of a gamma-encoded sRGB colour.
fn lightness([r, g, b]: [f64; 3]) -> f64 {
    let linear = |c: f64| {
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let y = 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
    if y > 216.0 / 24_389.0 {
        116.0 * y.cbrt() - 16.0
    } else {
        24_389.0 / 27.0 * y
    }
}

/// The mean colour of a resource of the neutral sample (an `ATX1` file:
/// the tag, width and height as little-endian u32, then RGBA8 pixels),
/// as gamma-encoded sRGB in 0..1.
fn mean_colour(resource: &str) -> [f64; 3] {
    let path = tilewright_editor_testkit::fixture_world("prototype-neutral")
        .join(format!("preview-assets/{resource}.atx1"));
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[..4], b"ATX1");
    let pixels = &bytes[12..];
    let count = f64::from(u32::try_from(pixels.len() / 4).unwrap());
    let mut sum = [0.0; 3];
    for pixel in pixels.chunks_exact(4) {
        for (total, value) in sum.iter_mut().zip(pixel) {
            *total += f64::from(*value);
        }
    }
    sum.map(|total| total / count / 255.0)
}

#[test]
fn composited_bands_keep_their_order_over_any_ground() {
    // Grounds: the mean colour of the sample's grass (about 92, 146, 78
    // of 255) and sand (about 205, 180, 118) art, and the preview's
    // clear colour (0.04, 0.05, 0.07), which shows on a tile without
    // ground. The renderer blends in gamma-encoded sRGB:
    // `alpha × tint + (1 − alpha) × ground`.
    let grounds = [
        mean_colour("grass"),
        mean_colour("sand"),
        [0.04, 0.05, 0.07],
    ];
    assert!((grounds[0][1] * 255.0 - 146.3).abs() < 0.1, "{grounds:?}");
    assert!((grounds[1][0] * 255.0 - 205.2).abs() < 0.1, "{grounds:?}");
    let alpha = f64::from(TINT_ALPHA);
    let drawn = |band: usize, ground: [f64; 3]| {
        let mut colour = [0.0; 3];
        for (channel, value) in colour.iter_mut().enumerate() {
            *value = alpha * f64::from(RAMP[band][channel]) + (1.0 - alpha) * ground[channel];
        }
        lightness(colour)
    };
    // Band k + 1 over any ground is lighter than band k over any ground
    // (every pair), by at least 2 L* (2.6 at alpha 0.9).
    let mut worst = f64::INFINITY;
    for band in 0..RAMP.len() - 1 {
        for low in grounds {
            for high in grounds {
                worst = worst.min(drawn(band + 1, high) - drawn(band, low));
            }
        }
    }
    assert!(worst >= 2.0, "worst margin {worst} L*");
}
