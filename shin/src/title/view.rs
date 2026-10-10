//! Native title UI10c370/10c6d0. The owner supplies the rendered background.
use super::{TitleItem, TitleSetup};
use crate::adv::character_view::CharacterTextures;
use glam::{Mat4, Vec4, vec4};
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosColVertex},
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct RowFeedback {
    pub x: f32,
    pub y: f32,
    pub opacity: f32,
    pub focus: f32,
    pub flash: f32,
}
#[derive(Debug, Clone)]
pub(crate) struct TitleFrame {
    pub rows: Vec<RowFeedback>,
    pub logo_offset: f32,
    pub logo_alpha: f32,
    pub caption: Option<TitleItem>,
    pub caption_alpha: f32,
    pub black_alpha: f32,
}
impl TitleFrame {
    /// Native10ca20 snaps child-return rows to their ready values. Native
    /// Negative-argument10d5b0 separately queues startup logo clocks.
    pub fn ready(setup: &TitleSetup) -> Self {
        Self {
            rows: setup
                .items
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    let origin = setup.row_origin(index).unwrap();
                    RowFeedback {
                        x: origin.x,
                        y: origin.y,
                        opacity: 1.0,
                        focus: if index == setup.selected { 1.0 } else { 0.0 },
                        flash: 0.0,
                    }
                })
                .collect(),
            logo_offset: 0.0,
            logo_alpha: 1.0,
            caption: Some(setup.items[setup.selected]),
            caption_alpha: 1.0,
            black_alpha: 0.0,
        }
    }
}

pub(super) fn sprite(
    pass: &mut RenderPass,
    texture: TextureSource<'_>,
    rect: Vec4,
    uv: Vec4,
    color: UnormColor,
    transform: Mat4,
) {
    QuadVertices::new()
        .with_box(rect.x, rect.y, rect.z, rect.w)
        .with_tex_box(uv.x, uv.y, uv.z, uv.w)
        .with_color(color)
        .render_sprite(
            pass,
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
            texture,
            transform,
        );
}
pub(super) fn fill(pass: &mut RenderPass, rect: Vec4, color: UnormColor, transform: Mat4) {
    let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| PosColVertex {
        position: glam::vec3(
            rect.x + (rect.z - rect.x) * corner.x,
            rect.y + (rect.w - rect.y) * corner.y,
            0.0,
        ),
        color,
    });
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .build(
                RenderProgramWithArguments::Fill {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    transform,
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}
pub(super) fn rgba(r: f32, g: f32, b: f32, a: f32) -> UnormColor {
    FloatColor4::from_rgba(r, g, b, a).into_unorm()
}
pub(super) fn render(
    setup: &TitleSetup,
    frame: &TitleFrame,
    atlas: &CharacterTextures,
    background: TextureSource<'_>,
    pass: &mut RenderPass,
    transform: Mat4,
) {
    assert_eq!(setup.items.len(), frame.rows.len());
    let full = vec4(0.0, 0.0, 1.0, 1.0);
    // Native background uses an opaque full-canvas rendered picture before UI.
    QuadVertices::new()
        .with_box(0.0, 0.0, 1920.0, 1080.0)
        .with_tex_box(0.0, 0.0, 1.0, 1.0)
        .render_sprite(
            pass,
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
            background,
            transform,
        );
    let logo_x = setup.logo_x + frame.logo_offset;
    sprite(
        pass,
        atlas.get("logo").as_source(),
        vec4(logo_x, 64.0, logo_x + 1014.0, 424.0),
        full,
        rgba(1.0, 1.0, 1.0, frame.logo_alpha),
        transform,
    );
    sprite(
        pass,
        atlas.get("copyright").as_source(),
        vec4(0.0, 952.0, 672.0, 1080.0),
        full,
        UnormColor::WHITE,
        transform,
    );
    if let Some(caption) = frame.caption {
        sprite(
            pass,
            atlas.get(caption.caption()).as_source(),
            vec4(96.0, 448.0, 976.0, 948.0),
            full,
            rgba(1.0, 1.0, 1.0, frame.caption_alpha),
            transform,
        );
    }
    for (index, (&item, row)) in setup.items.iter().zip(&frame.rows).enumerate() {
        let plate = vec4(row.x, row.y + 1.0, row.x + 704.0, row.y + 81.0);
        let label = vec4(row.x, row.y, row.x + 704.0, row.y + 82.0);
        // Native113a90 defines two float colors;56810 interpolates all four
        // channels by focus before56840 truncates/saturates to RGBA8.
        let r = 0.2157 + (1.0 - 0.2157) * row.focus;
        let gb = 0.1059 + (0.1608 - 0.1059) * row.focus;
        // Native shares the 902 denominator with menu, even though the
        // supplied plate texture is only 820 pixels high.
        sprite(
            pass,
            atlas.get("plate").as_source(),
            plate,
            vec4(
                0.0,
                (index as f32 * 82.0 + 1.0) / 902.0,
                1.0,
                (index as f32 * 82.0 + 81.0) / 902.0,
            ),
            rgba(r, gb, gb, 1.0),
            transform,
        );
        sprite(
            pass,
            atlas.get("menu").as_source(),
            label,
            vec4(
                0.0,
                item as u8 as f32 * 82.0 / 902.0,
                1.0,
                (item as u8 as f32 + 1.0) / 11.0,
            ),
            UnormColor::WHITE,
            transform,
        );
        if row.flash > 0.0 {
            fill(pass, plate, rgba(1.0, 1.0, 1.0, row.flash), transform);
        }
        if row.opacity < 1.0 {
            // Native10c940 covers each row by its matching background region.
            // This is not a multiplicative alpha on the UI's individual quads.
            sprite(
                pass,
                background,
                label,
                vec4(
                    row.x / 1920.0,
                    row.y / 1080.0,
                    (row.x + 704.0) / 1920.0,
                    (row.y + 82.0) / 1080.0,
                ),
                rgba(1.0, 1.0, 1.0, 1.0 - row.opacity),
                transform,
            );
        }
    }
    if frame.black_alpha > 0.0 {
        fill(
            pass,
            vec4(0.0, 0.0, 1920.0, 1080.0),
            rgba(0.0, 0.0, 0.0, frame.black_alpha),
            transform,
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_title_sprite_geometry_palette_and_uv_sources() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x10c488, 0xbd423a61),
            (0x10c49c, 0x1e212802),
            (0x10c4b4, 0x91074260),
            (0x10c7e4, 0xb9400288),
            (0x10c7ec, 0x1b167d08),
            (0x10c7f0, 0x32000108),
            (0x10c7fc, 0xbd49c108),
            (0x10c804, 0x1e281801),
            (0x10c808, 0xbd48ad00),
            (0x10c89c, 0xb9400688),
            (0x10c8b0, 0x1e281801),
            (0x10c8b4, 0xbd4a5500),
            (0x10c820, 0xbd400be2),
            (0x10c82c, 0x4eaa1d43),
            (0x10c940, 0xbd400fe8),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, expected) in [
            (0x159aec, 1014.0f32),
            (0x159900, 64.0),
            (0x1596bc, 424.0),
            (0x159608, 952.0),
            (0x1599a8, 672.0),
            (0x159614, 1080.0),
            (0x159818, 96.0),
            (0x15938c, 448.0),
            (0x159674, 976.0),
            (0x159ab0, 948.0),
            (0x159634, 704.0),
            (0x159a68, 80.0),
            (0x159690, 82.0),
            (0x1599c0, 902.0),
            (0x1598ac, 80.0 / 902.0),
            (0x159a54, 1.0 / 11.0),
            (0x1599b0, 0.2157),
            (0x1598d8, 0.1059),
            (0x159780, 0.1608),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native float{address:x}"
            );
        }
    }
}
