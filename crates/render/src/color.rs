use std::sync::OnceLock;

fn srgb_to_linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f64) -> f64 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB-encoded byte -> linear light.
pub fn to_linear(v: u8) -> f32 {
    static T: OnceLock<[f32; 256]> = OnceLock::new();
    T.get_or_init(|| std::array::from_fn(|i| srgb_to_linear(i as f64 / 255.0) as f32))[v as usize]
}

const STEPS: usize = 1 << 16;

/// Linear light (clamped to 0..=1) -> sRGB-encoded byte. Table-driven so output is identical on every machine.
pub fn to_srgb8(v: f32) -> u8 {
    static T: OnceLock<Vec<u8>> = OnceLock::new();
    let t = T.get_or_init(|| {
        (0..STEPS)
            .map(|i| (linear_to_srgb(i as f64 / (STEPS - 1) as f64) * 255.0).round() as u8)
            .collect()
    });
    let v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
    t[(v * (STEPS - 1) as f32).round() as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_byte_round_trips() {
        for b in 0..=255u8 {
            assert_eq!(to_srgb8(to_linear(b)), b, "byte {b}");
        }
    }

    #[test]
    fn known_values() {
        assert_eq!(to_srgb8(0.0), 0);
        assert_eq!(to_srgb8(1.0), 255);
        assert_eq!(to_srgb8(0.5), 188);
        assert_eq!(to_srgb8(f32::NAN), 0);
        assert_eq!(to_srgb8(7.0), 255);
    }
}
