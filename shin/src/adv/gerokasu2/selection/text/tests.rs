use super::*;
use shin_core::format::font::gerokasu2::texture::GlyphTextureGeometry;
use std::{
    io::{Read, Write},
    process::{Command as Process, Stdio},
};

#[test]
fn generic_fill_keeps_zero_and_multiplies_packed_alpha() {
    assert_eq!(fill_color(UnormColor(0), 0.75).into_vec4(), Vec4::ZERO);
    let color = fill_color(UnormColor(0x80102040), 0.5).into_vec4();
    let inverse = f32::from_bits(0x3b808081);
    assert_eq!(color, Vec4::new(64.0, 32.0, 16.0, 64.0) * inverse);
}

#[test]
#[ignore = "requires exact executable and native SELECT generic glyph draw oracle"]
fn ger_native_select_text_generic_packed_colors_vertices_and_thresholds() {
    let mut child = Process::new(
        std::env::var_os("SHIN_GEROKASU2_SELECTION_TEXT_DRAW_ORACLE").expect("SELECT draw oracle"),
    )
    .arg(std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").unwrap())
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .spawn()
    .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut cases = 0;
    for scale in [Vec2::ONE, vec2(0.25, 1.5)] {
        for opacity in [0.0, 0.25, 1.0] {
            for color in [0, u32::MAX, 0x80102040, 0xff20c080] {
                let geometry = GlyphTextureGeometry {
                    codepoint: u32::from('A'),
                    identity: 123,
                    origin: [-2, -41],
                    dimensions: [50, 56],
                    advance: 48.0,
                    font_metrics: [70, 10, 41],
                    scale: [1.0, 1.0],
                    extra_metric: 80.0,
                    border: 2,
                };
                let draw = GlyphDraw::new(&geometry, vec2(3.0, 44.0), scale, 48.0, false).unwrap();
                let record = [
                    1,
                    1,
                    geometry.identity,
                    geometry.codepoint,
                    geometry.origin[0] as u32,
                    geometry.origin[1] as u32,
                    geometry.dimensions[0],
                    geometry.dimensions[1],
                    geometry.advance.to_bits(),
                    geometry.font_metrics[0] as u32,
                    geometry.font_metrics[1] as u32,
                    geometry.font_metrics[2] as u32,
                    1.0f32.to_bits(),
                    1.0f32.to_bits(),
                    geometry.extra_metric.to_bits(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                ];
                let mut row = Vec::new();
                for word in record {
                    row.extend(word.to_le_bytes());
                }
                for value in [3.0, 44.0, scale.x, scale.y, 48.0] {
                    row.extend(value.to_le_bytes());
                }
                row.extend(0u32.to_le_bytes());
                for value in [
                    1.0f32, 1.0, 1.0, opacity, 1.0, 1.0, 1.0, opacity, 0.0, 0.0, 0.0, opacity,
                ] {
                    row.extend(value.to_le_bytes());
                }
                // Native packed words are ARGB; shared colors are ABGR.
                let native_color =
                    (color & 0xff00ff00) | ((color & 255) << 16) | ((color >> 16) & 255);
                row.extend(native_color.to_le_bytes());
                assert_eq!(row.len(), 164);
                input.write_all(&row).unwrap();
                input.flush().unwrap();
                let mut prefix = [0u8; 88];
                output.read_exact(&mut prefix).unwrap();
                assert_eq!(&prefix[..4], b"GFD1");
                let vertex_words = prefix[4..84]
                    .chunks_exact(4)
                    .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
                    .collect::<Vec<_>>();
                let expected = draw
                    .vertices
                    .iter()
                    .flat_map(|v| {
                        [
                            v.position.x,
                            v.position.y,
                            v.position.z,
                            v.position.w,
                            v.color,
                        ]
                        .map(f32::to_bits)
                    })
                    .collect::<Vec<_>>();
                assert_eq!(vertex_words, expected, "SELECT vertex case{cases}");
                let count = u32::from_le_bytes(prefix[84..88].try_into().unwrap());
                assert_eq!(count, if opacity > 0.0 { 2 } else { 0 });
                for outline in [true, false].into_iter().take(count as usize) {
                    let mut packet = [0u8; 128];
                    output.read_exact(&mut packet).unwrap();
                    let words = packet
                        .chunks_exact(4)
                        .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
                        .collect::<Vec<_>>();
                    assert_eq!(words[0..2], [5, 2]);
                    let thresholds = if outline {
                        draw.outline_thresholds
                    } else {
                        draw.fill_thresholds
                    };
                    assert_eq!(words[2..4], thresholds.to_array().map(f32::to_bits));
                    let rgba = if outline {
                        Vec4::new(0.0, 0.0, 0.0, opacity)
                    } else {
                        fill_color(UnormColor(color), opacity).into_vec4()
                    };
                    assert_eq!(
                        words[4..8],
                        rgba.to_array().map(f32::to_bits),
                        "SELECT color case{cases}"
                    );
                    assert_eq!(words[8..12], rgba.to_array().map(f32::to_bits));
                    assert_eq!(
                        words[12..28],
                        Mat4::IDENTITY.to_cols_array().map(f32::to_bits)
                    );
                    assert_eq!(words[28..32], [0, 1, 4, 0]);
                }
                cases += 1;
            }
        }
    }
    drop(input);
    assert_eq!(output.read(&mut [0; 1]).unwrap(), 0);
    assert!(child.wait().unwrap().success());
    println!(
        "GEROKASU2_SELECTION_TEXT_DRAW_NATIVE cases={cases} original_generic_vertices_packed_color_SDF=verified"
    );
}
