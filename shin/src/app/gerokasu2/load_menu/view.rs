//! Ger B3BF0/B44C0/B46B0/B48B0 atlas packets and FNT6 GenericText rows.
use crate::adv::gerokasu2::generic_text::Text;
use crate::{
    adv::{characters::view::CharacterTextures, gerokasu2::save_menu::Controller},
    asset::{font::gerokasu2::GpuGerokasu2Font, system::AssetServer},
};
use anyhow::Result;
use glam::{Mat4, Vec4, vec3, vec4};
use shin_core::{
    format::save::gerokasu2::SaveDate,
    layout::{LayoutParams, gerokasu2::generic_layouter},
};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
};
use std::sync::Arc;
pub struct Labels {
    pub title: Text,
    pub subtitle: Option<Text>,
}
impl Labels {
    pub fn prepare(
        server: &AssetServer,
        font: &Arc<GpuGerokasu2Font>,
        title: &str,
        subtitle: Option<&str>,
    ) -> Result<Self> {
        let text = |text: &str| {
            let (commands, lines, _) = generic_layouter(
                font.metrics(),
                LayoutParams {
                    layout_width: 1246.0,
                    text_size: 44.0,
                    rubi_size: 0.0,
                    perform_soft_breaks: false,
                    always_leave_space_for_rubi: false,
                    ..Default::default()
                },
            )
            .parse(text);
            Text::prepare(
                &server.context().wgpu_device,
                &server.context().wgpu_queue,
                font,
                commands,
                lines,
                44.0,
            )
        };
        Ok(Self {
            title: text(title)?,
            subtitle: subtitle.map(text).transpose()?,
        })
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        canvas: Mat4,
        y: f32,
    ) -> Result<()> {
        let top = if self.subtitle.is_some() { 31.0 } else { 61.0 };
        self.title.render(
            pass,
            builder,
            canvas * Mat4::from_translation(vec3(724.0, y + top, 0.0)),
            Vec4::ONE,
        )?;
        if let Some(subtitle) = &self.subtitle {
            subtitle.render(
                pass,
                builder,
                canvas * Mat4::from_translation(vec3(724.0, y + 79.0, 0.0)),
                Vec4::ONE,
            )?
        }
        Ok(())
    }
}
pub fn canvas() -> Mat4 {
    Mat4::from_cols_array(&[
        2.0 / 1920.0,
        0.0,
        0.0,
        0.0,
        0.0,
        -2.0 / 1080.0,
        0.0,
        0.0,
        0.0,
        0.0,
        -0.5,
        0.0,
        -1.0,
        1.0,
        0.5,
        1.0,
    ])
}
mod art;
#[cfg(test)]
mod tests;
pub fn render(
    pass: &mut RenderPass,
    atlas: &CharacterTextures,
    system: &CharacterTextures,
    model: &Controller,
    dates: &[Option<SaveDate>; 100],
    labels: &[Option<Labels>; 100],
    thumbnails: &[Option<Arc<shin_render::render_texture::RenderTexture>>; 100],
    empty: &Labels,
    loading: &Labels,
    game: crate::cli::Game,
) -> Result<()> {
    use art::Kind;
    use shin_core::primitives::color::UnormColor;
    use shin_render::{
        DrawPrimitive, RenderProgramWithArguments,
        shaders::types::{buffer::VertexSource, vertices::BlendVertex},
    };
    let matrix = canvas();
    let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
    for p in art::plan_with_mode(
        model.mode(),
        model.progress(),
        model.scroll(),
        model.selected(),
        dates,
        |i| model.row_flash(i),
        model.close_flash(),
        game,
    ) {
        if let Kind::Text(i) = p.kind {
            labels[i]
                .as_ref()
                .unwrap_or(if dates[i].is_some() { loading } else { empty })
                .render(
                    pass,
                    builder,
                    matrix * Mat4::from_translation(vec3(p.rect.x, 0., 0.)),
                    p.rect.y,
                )?;
            continue;
        }
        let bytes = p.color.to_le_bytes();
        // Original vertex colors are packed BGRA; the shared vertex uses RGBA.
        let color = UnormColor::from_rgba(bytes[2], bytes[1], bytes[0], bytes[3]);
        let r = p.rect;
        let uv = p.uv;
        let quad = QuadVertices::new()
            .with_box(r.x, r.y, r.z, r.w)
            .with_tex_box(uv.x, uv.y, uv.z, uv.w)
            .with_color(color);
        match p.kind {
            Kind::Window { selected } => {
                let vertices = [
                    (r.x, r.y, uv.x, uv.y),
                    (r.z, r.y, uv.z, uv.y),
                    (r.x, r.w, uv.x, uv.w),
                    (r.z, r.w, uv.z, uv.w),
                ]
                .map(|(x, y, u, v)| BlendVertex {
                    position: vec3(x, y, 0.),
                    color,
                    texture_position: vec4(u, v, u, v),
                });
                let factor = if selected { 1. } else { 0. };
                pass.run(builder.build(
                    RenderProgramWithArguments::Blend {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        texture1: atlas.get("window_normal").as_source(),
                        texture2: atlas.get("window_focus").as_source(),
                        transform: matrix,
                        blend: vec4(factor, factor, factor, 1.),
                        flash: vec4(1., 1., 1., 0.),
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            }
            Kind::Fill => quad.fill(pass, builder, matrix),
            Kind::Thumbnail(slot) => {
                let texture = thumbnails[slot].as_ref().map_or_else(
                    || atlas.get("nodata").as_source(),
                    |texture| texture.as_texture_source(),
                );
                quad.render_sprite(
                    pass,
                    builder.color_blend_type(ColorBlendType::Opaque),
                    texture,
                    matrix,
                );
            }
            Kind::Button { flash } if flash > 0. => quad.render_button(
                pass,
                builder,
                atlas.get(p.texture).as_source(),
                matrix,
                Vec4::ZERO,
                vec4(1., 1., 1., flash),
            ),
            Kind::Sprite | Kind::Button { .. } => {
                let texture = if p.texture == "scroll" {
                    system.get("scroll")
                } else {
                    atlas.get(p.texture)
                };
                // Gerokasu B5668 keeps blend 2 for `top`.
                let additive = p.texture == "top" && game != crate::cli::Game::Gerokasu;
                let builder = builder.color_blend_type(if additive {
                    ColorBlendType::Layer2
                } else if p.texture == "nodata" {
                    ColorBlendType::Opaque
                } else {
                    ColorBlendType::Layer1
                });
                quad.render_sprite(pass, builder, texture.as_source(), matrix);
            }
            Kind::Text(_) => unreachable!(),
        }
    }
    Ok(())
}
