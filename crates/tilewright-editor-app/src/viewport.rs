// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Native coordinate conversion and exact renderer mask lookup. Projection and
//! rasterization stay in the shared renderer.

/// SDL positions are logical window units. Convert once at the shell boundary,
/// rejecting minimized, stale, nonfinite and outside-window input.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn physical_pixel(
    position: [f32; 2],
    logical: (u32, u32),
    physical: (u32, u32),
) -> Option<(u32, u32)> {
    if logical.0 == 0 || logical.1 == 0 || physical.0 == 0 || physical.1 == 0 {
        return None;
    }
    let [x, y] = position.map(f64::from);
    if !x.is_finite()
        || !y.is_finite()
        || x < 0.0
        || y < 0.0
        || x >= f64::from(logical.0)
        || y >= f64::from(logical.1)
    {
        return None;
    }
    let px = (x * f64::from(physical.0) / f64::from(logical.0)).floor() as u32;
    let py = (y * f64::from(physical.1) / f64::from(logical.1)).floor() as u32;
    (px < physical.0 && py < physical.1).then_some((px, py))
}

#[cfg(test)]
mod tests {
    use super::physical_pixel;

    #[test]
    fn logical_input_is_scaled_once_and_invalid_input_is_rejected() {
        assert_eq!(physical_pixel([1.5, 0.5], (2, 2), (4, 4)), Some((3, 1)));
        for input in [
            [-0.1, 0.0],
            [2.0, 0.0],
            [f32::NAN, 0.0],
            [0.0, f32::INFINITY],
        ] {
            assert_eq!(physical_pixel(input, (2, 2), (4, 4)), None);
        }
        assert_eq!(physical_pixel([0.0, 0.0], (0, 0), (0, 0)), None);
    }
}
