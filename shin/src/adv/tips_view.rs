//! TipsMode107930/1084b0: authored shell, rows, text and native scrollbar.
use super::{
    character_view::CharacterTextures,
    tips::{TipsModel, TipsPhase},
    tips_text::TipsText,
};
use glam::{Mat4, Vec4, vec2, vec3, vec4};
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

fn sprite(
    pass: &mut RenderPass,
    atlas: &CharacterTextures,
    name: &str,
    rect: Vec4,
    uv: Vec4,
    color: UnormColor,
    blend: ColorBlendType,
    transform: Mat4,
) {
    QuadVertices::new()
        .with_box(rect.x, rect.y, rect.z, rect.w)
        .with_tex_box(uv.x, uv.y, uv.z, uv.w)
        .with_color(color)
        .render_sprite(
            pass,
            RenderRequestBuilder::new().color_blend_type(blend),
            atlas.get(name).as_source(),
            transform,
        );
}
fn white(alpha: f32) -> UnormColor {
    FloatColor4::from_rgba(1.0, 1.0, 1.0, alpha).into_unorm()
}

pub fn window_slices() -> Vec<(Vec4, Vec4, ColorBlendType)> {
    let xs = [0.0, 80.0, 1840.0, 1920.0];
    let ys = [0.0, 80.0, 810.0, 898.0];
    let mut slices = Vec::new();
    for row in 0..3 {
        for column in 0..3 {
            slices.push((
                vec4(
                    xs[column],
                    112.0 + ys[row],
                    xs[column + 1],
                    112.0 + ys[row + 1],
                ),
                vec4(
                    xs[column] / 1920.0,
                    ys[row] / 898.0,
                    xs[column + 1] / 1920.0,
                    ys[row + 1] / 898.0,
                ),
                if row == 1 && column == 1 {
                    ColorBlendType::Opaque
                } else {
                    ColorBlendType::Layer1
                },
            ));
        }
    }
    slices
}
pub fn row_geometry(row: usize, title: u16) -> (Vec4, Vec4, Vec4) {
    let y = 193.0 + row as f32 * 88.0;
    (
        vec4(90.0, y, 548.0, y + 88.0),
        vec4(90.0, y + 12.0, 548.0, y + 76.0),
        vec4(
            0.0,
            title as f32 / 26.0,
            1.0,
            title as f32 / 26.0 + 1.0 / 26.0,
        ),
    )
}
/// Native scroller truncates position and ceilings content height before ratios.
/// Two eight-vertex strips each form three bands, with eight-pixel caps.
pub fn scrollbar_slices(height: f32, scroll: f32) -> Vec<(Vec4, Vec4)> {
    let height = height.ceil();
    let scroll = scroll.trunc();
    if height <= 730.0 || scroll < 0.0 || scroll + 730.0 > height {
        return Vec::new();
    }
    let start = 714.0 * scroll / height;
    let end = 714.0 * (scroll + 730.0) / height;
    let mut result = Vec::new();
    for (u, ys) in [
        (0.0, [188.0, 196.0, 910.0, 918.0]),
        (
            0.5,
            [188.0 + start, 196.0 + start, 196.0 + end, 204.0 + end],
        ),
    ] {
        for band in 0..3 {
            result.push((
                vec4(1838.0, ys[band], 1854.0, ys[band + 1]),
                vec4(u, band as f32 * 0.25, u + 0.5, (band + 1) as f32 * 0.25),
            ));
        }
    }
    result
}
fn hint(
    pass: &mut RenderPass,
    atlas: &CharacterTextures,
    right: f32,
    y: f32,
    row: usize,
    color: UnormColor,
    canvas: Mat4,
) -> f32 {
    let (button, width, label_y) = match row {
        0 => (1, 206.0, 0.0),
        1 => (4, 72.0, 48.0),
        3 => (9, 174.0, 144.0),
        _ => unreachable!(),
    };
    let left = right - width;
    sprite(
        pass,
        atlas,
        "help_tips",
        vec4(left, y, right, y + 48.0),
        vec4(
            0.0,
            label_y / 192.0,
            width / 320.0,
            (label_y + 48.0) / 192.0,
        ),
        color,
        ColorBlendType::Layer1,
        canvas,
    );
    sprite(
        pass,
        atlas,
        "key",
        vec4(left - 48.0, y, left, y + 48.0),
        vec4(
            (button & 7) as f32 / 8.0,
            (button >> 3) as f32 / 2.0,
            (button & 7) as f32 / 8.0 + 0.125,
            (button >> 3) as f32 / 2.0 + 0.5,
        ),
        color,
        ColorBlendType::Layer1,
        canvas,
    );
    left - 58.0
}
fn edge_fades(pass: &mut RenderPass, atlas: &CharacterTextures, transform: Mat4) {
    for (top, bottom, a, b) in [(183.0, 188.0, 1.0, 0.0), (918.0, 923.0, 0.0, 1.0)] {
        let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| {
            let x = 86.0 + 1748.0 * corner.x;
            let y = top + (bottom - top) * corner.y;
            PosColTexVertex {
                position: vec3(x, y, 0.0),
                texture_position: vec2(x / 1920.0, (y - 112.0) / 898.0),
                color: white(a + (b - a) * corner.y),
            }
        });
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        sprite: atlas.get("window").as_source(),
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}
pub fn render(
    model: &TipsModel,
    text: &TipsText,
    atlas: &CharacterTextures,
    system: &CharacterTextures,
    pass: &mut RenderPass,
    canvas: Mat4,
) {
    if model.phase() == TipsPhase::Closed {
        return;
    }
    let p = model.progress();
    let top = -160.0 + 160.0 * p;
    sprite(
        pass,
        atlas,
        "bg",
        vec4(0.0, 0.0, 1920.0, 1080.0),
        vec4(0.0, 0.0, 1.0, 1.0),
        white(p),
        ColorBlendType::Layer1,
        canvas,
    );
    sprite(
        pass,
        atlas,
        "obi",
        vec4(0.0, top, 1920.0, top + 160.0),
        vec4(0.0, 0.0, 1.0, 0.5),
        white(0.25),
        ColorBlendType::Layer2,
        canvas,
    );
    sprite(
        pass,
        atlas,
        "obi",
        vec4(0.0, 1080.0 - 160.0 * p, 1920.0, 1240.0 - 160.0 * p),
        vec4(0.0, 0.5, 1.0, 1.0),
        white(0.25),
        ColorBlendType::Layer2,
        canvas,
    );
    let transform = canvas * Mat4::from_translation(vec3(1920.0 * (1.0 - p), 0.0, 0.0));
    for (rect, uv, blend) in window_slices() {
        sprite(
            pass,
            atlas,
            "window",
            rect,
            uv,
            UnormColor::WHITE,
            blend,
            transform,
        );
    }
    sprite(
        pass,
        atlas,
        "separator",
        vec4(568.0, 203.0, 586.0, 903.0),
        vec4(0.0, 0.0, 1.0, 1.0),
        UnormColor::WHITE,
        ColorBlendType::Layer1,
        transform,
    );
    for (i, row) in model.rows().iter().enumerate() {
        let (rect, title, uv) = row_geometry(i, row.title_index);
        let select = vec4(1.0, 1.0, 1.0, row.selection() * 0.5);
        let flash = vec4(1.0, 1.0, 1.0, row.flash());
        atlas.render_button(
            pass,
            "menu",
            rect,
            vec4(0.0, 0.0, 1.0, 1.0),
            UnormColor::WHITE,
            select,
            flash,
            transform,
        );
        atlas.render_button(
            pass,
            "items",
            title,
            uv,
            UnormColor::WHITE,
            select,
            flash,
            transform,
        );
    }
    text.render(pass, atlas, model.scroll(), transform);
    edge_fades(pass, atlas, transform);
    for (rect, uv) in scrollbar_slices(model.content_height(), model.scroll()) {
        sprite(
            pass,
            system,
            "scroll",
            rect,
            uv,
            UnormColor::WHITE,
            ColorBlendType::Layer1,
            transform,
        );
    }
    sprite(
        pass,
        atlas,
        "caption",
        vec4(480.0, top, 1440.0, top + 128.0),
        vec4(0.0, 0.0, 1.0, 1.0),
        UnormColor::WHITE,
        ColorBlendType::Layer1,
        canvas,
    );
    atlas.render_button(
        pass,
        "close",
        vec4(1764.0, top + 32.0, 1848.0, top + 116.0),
        vec4(0.0, 0.0, 1.0, 1.0),
        UnormColor::WHITE,
        Vec4::ZERO,
        vec4(1.0, 1.0, 1.0, model.close_flash()),
        canvas,
    );
    let y = 1100.0 - 88.0 * p;
    let right = hint(pass, atlas, 1872.0, y, 1, UnormColor::WHITE, canvas);
    let disabled = UnormColor::from_rgba(128, 128, 128, 255);
    let right = hint(
        pass,
        atlas,
        right,
        y,
        3,
        if model.content_height() > 730.0 {
            UnormColor::WHITE
        } else {
            disabled
        },
        canvas,
    );
    hint(pass, atlas, right, y, 0, UnormColor::WHITE, canvas);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn window_rows_and_integer_scrollbar_retain_native_geometry() {
        let window = window_slices();
        assert_eq!(window.len(), 9);
        assert_eq!(window[4].0, vec4(80.0, 192.0, 1840.0, 922.0));
        assert_eq!(window[4].2, ColorBlendType::Opaque);
        assert_eq!(
            window
                .iter()
                .filter(|s| s.2 == ColorBlendType::Opaque)
                .count(),
            1
        );
        assert_eq!(window[8].0, vec4(1840.0, 922.0, 1920.0, 1010.0));
        let (background, title, uv) = row_geometry(2, 25);
        assert_eq!(background, vec4(90.0, 369.0, 548.0, 457.0));
        assert_eq!(title, vec4(90.0, 381.0, 548.0, 445.0));
        assert!((uv.y - 25.0 / 26.0).abs() < 1e-7 && uv.w == 1.0);
        assert!(scrollbar_slices(730.0, 0.0).is_empty());
        assert!(scrollbar_slices(2000.0, -1.0).is_empty());
        assert!(scrollbar_slices(2000.0, 1271.0).is_empty());
        let slices = scrollbar_slices(2000.0, 400.9);
        assert_eq!(slices.len(), 6);
        assert_eq!(
            slices[0],
            (
                vec4(1838.0, 188.0, 1854.0, 196.0),
                vec4(0.0, 0.0, 0.5, 0.25)
            )
        );
        assert_eq!(slices[2].0, vec4(1838.0, 910.0, 1854.0, 918.0));
        assert!((slices[3].0.y - 330.8).abs() < 1e-4);
        assert!((slices[5].0.w - 607.41).abs() < 1e-4);
        assert_eq!(slices, scrollbar_slices(1999.2, 400.1));
        let bottom = scrollbar_slices(2000.0, 1270.0);
        assert_eq!(bottom[5].0.w, 918.0);
    }
    #[test]
    #[ignore = "requires supplied ROM/executable (SHIN_REFERENCE_ROM)"]
    fn reference_native_tips_shell_window_row_scroller_and_hints() {
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x107a68, 0x1e2e0800),
            (0x107a6c, 0x1e212808),
            (0x107ac8, 0x1e2a100d),
            (0x107b08, 0x1e2e0803),
            (0x107b20, 0x4ea81d0e),
            (0x107b84, 0x1e20396b),
            (0x107cd0, 0xbd439d0f),
            (0x107cd8, 0xbd490108),
            (0x107d14, 0x1e2e080a),
            (0x107e40, 0x1e2f2863),
            (0x107e48, 0x1e2e0863),
            (0x107e4c, 0x1e232821),
            (0x107e50, 0x1e282823),
            (0x107e60, 0x1e275001),
            (0x107e68, 0x1e211801),
            (0x1085c0, 0xf100127f),
            (0x108610, 0x97ff1c5c),
            (0x1086cc, 0x321f03e0),
            (0x108754, 0xf100327f),
            (0x108424, 0x2a1403e1),
            (0x108428, 0x320003e2),
            (0x108444, 0x72bff008),
            (0x10844c, 0x1e202028),
            (0x108454, 0x5a9fd101),
            (0x108458, 0x320007e2),
            (0x10846c, 0x2a1f03e2),
            (0xeee98, 0x1e241008),
            (0xef020, 0x1e361001),
            (0xef03c, 0x1e212800),
            (0xef04c, 0x1e211842),
            (0xef050, 0x1e282803),
            (0xef058, 0x1e341003),
            (0xef060, 0x1e220802),
            (0xef06c, 0x1e232842),
            (0xef0dc, 0x0b0a016a),
            (0xef194, 0x1e211861),
            (0xef198, 0x1e210800),
            (0xef19c, 0x1e282801),
            (0xef260, 0x320003e0),
            (0xef264, 0x321d03e1),
            (0xef268, 0x2a1f03e2),
            (0xef274, 0x321d03e1),
            (0xef278, 0x321d03e2),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        let ys = [0.0_f32, 80.0, 810.0, 898.0];
        for (i, y) in ys.into_iter().enumerate() {
            assert_eq!(
                f32::from_le_bytes(
                    exe[0x16ddc0 + 256 + i * 4..0x16ddc0 + 260 + i * 4]
                        .try_into()
                        .unwrap()
                ),
                y
            );
        }
        let hints = [1, -1, 0, 206, 4, -1, 48, 72, 3, -1, 96, 74, 9, -1, 144, 174];
        for (i, hint) in hints.into_iter().enumerate() {
            assert_eq!(
                i32::from_le_bytes(
                    exe[0x1862cc + 256 + i * 4..0x1862cc + 260 + i * 4]
                        .try_into()
                        .unwrap()
                ),
                hint
            );
        }
        let mut rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap())
                .unwrap();
        for path in ["/tips.txa", "/systex.txa"] {
            let file = rom.find_file(path).unwrap();
            let mut bytes = Vec::new();
            rom.open_file(file)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            let atlas = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
            let names: &[&str] = if path == "/tips.txa" {
                &[
                    "bg",
                    "obi",
                    "window",
                    "separator",
                    "menu",
                    "items",
                    "bar",
                    "caption",
                    "close",
                    "help_tips",
                    "key",
                ]
            } else {
                &["scroll"]
            };
            for &name in names {
                let image = &atlas.textures[atlas.name_to_index[name]];
                let expected = match name {
                    "bg" => (960, 540),
                    "obi" => (1920, 320),
                    "window" => (1920, 898),
                    "separator" => (18, 700),
                    "menu" => (458, 88),
                    "items" => (458, 1664),
                    "bar" => (512, 16),
                    "caption" => (960, 128),
                    "close" => (84, 84),
                    "help_tips" => (320, 192),
                    "key" => (384, 96),
                    "scroll" => (64, 64),
                    _ => unreachable!(),
                };
                assert_eq!(image.dimensions(), expected);
                eprintln!("Tips {path} {name} {}x{}", image.width(), image.height());
            }
        }
    }
}
