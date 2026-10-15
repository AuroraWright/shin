use glam::{Vec2, vec2};

/// Native text0xa4950..0xa4a80: eight circular taps, optionally shifted for
/// saved outline mode1. Other stored values retain the centered outline.
pub(crate) fn distances(scale: Vec2, glyph_pixels: Vec2, glyph_uv: Vec2, mode: u8) -> [Vec2; 8] {
    distances_with_radius(scale, glyph_pixels, glyph_uv, mode, 2.5)
}

pub(crate) fn distances_with_radius(
    scale: Vec2,
    glyph_pixels: Vec2,
    glyph_uv: Vec2,
    mode: u8,
    radius: f32,
) -> [Vec2; 8] {
    let radius = Vec2::splat(radius) / scale / glyph_pixels * glyph_uv;
    let diagonal = f32::from_bits(0x3f34fdf4); // native0.707, not1/sqrt(2)
    let shift = if mode == 1 {
        Vec2::splat(f32::from_bits(0x3f99999a)) / scale / glyph_pixels * glyph_uv
    } else {
        Vec2::ZERO
    };
    [
        vec2(-diagonal, -diagonal),
        vec2(0.0, -1.0),
        vec2(diagonal, -diagonal),
        vec2(-1.0, 0.0),
        vec2(1.0, 0.0),
        vec2(-diagonal, diagonal),
        vec2(0.0, 1.0),
        vec2(diagonal, diagonal),
    ]
    .map(|direction| direction * radius - shift)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outline_mode_one_shifts_every_tap_and_preserves_anisotropic_and_ruby_scale() {
        for scale in [vec2(1.0, 1.0), vec2(0.8, 1.25), vec2(0.4, 0.4)] {
            let pixels = vec2(40.0, 60.0);
            let uv = vec2(0.625, 0.9375);
            let centered = distances(scale, pixels, uv, 0);
            let shadow = distances(scale, pixels, uv, 1);
            for (a, b) in centered.into_iter().zip(shadow) {
                let screen = (a - b) * pixels / uv * scale;
                assert!((screen - Vec2::splat(1.2)).abs().max_element() < 0.00001);
            }
            assert_eq!(centered, distances(scale, pixels, uv, 2));
            assert_eq!(centered, distances(scale, pixels, uv, 3));
            let upward = centered[1] * pixels / uv * scale;
            assert!((upward - vec2(0.0, -2.5)).abs().max_element() < 0.00001);
            let diagonal = centered[0] * pixels / uv * scale;
            assert!((diagonal - Vec2::splat(-1.7675)).abs().max_element() < 0.00001);
        }
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_outline_geometry_and_setting() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (a, expected) in [
            (0xa459c, 0x1e20900e),
            (0xa4954, 0x1e2119c4),
            (0xa4964, 0x1e2019c0),
            (0xa496c, 0x1e231891),
            (0xa4984, 0x1e221805),
            (0xa4988, 0x1e300a20),
            (0xa4990, 0x1e2a0803),
            (0xa49d8, 0xf9424929),
            (0xa49dc, 0x3942e129),
            (0xa49e0, 0x7100053f),
            (0xa49e4, 0x54000501),
            (0xa49ec, 0xbd45c130),
            (0xa49f4, 0x1e311a11),
            (0xa4a04, 0x1e321a31),
            (0xa4a0c, 0x1e320a31),
            (0xa4a14, 0x1e313821),
            (0xa4a3c, 0x1e3038c6),
            (0xa4a50, 0x1e303800),
            (0xa4a7c, 0xbd014be1),
        ] {
            assert_eq!(word(a), expected, "native text outline{a:x}");
        }
        for (a, expected) in [
            (0x159468, 0xbf34fdf4),
            (0x159784, 0x3f34fdf4),
            (0x1595c0, 0x3f99999a),
        ] {
            assert_eq!(word(a), expected, "native outline constant{a:x}");
        }
    }
}
