//! Updated23cd4 cache painting and23e98 composition, with float program6 tint.
use super::{Question, text_params};
use crate::{
    adv::{dialog_view::DialogTextures, higurashi::message_glyphs::glyph_vertices},
    asset::{
        font::{GpuFontGlyphHandle, GpuFontLazy},
        system::AssetServer,
    },
    layer::{
        message_layer::outline,
        render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
    },
    render::PreRenderContext,
};
use anyhow::Result;
use glam::{Mat4, Vec2, Vec4, vec2, vec3, vec4};
use shin_core::{
    layout::{HigurashiTextLayouter, MessageTextLayouterDefaults, commands::Command},
    primitives::color::{FloatColor4, UnormColor},
};
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerFragmentShader, LayerShaderOutputKind,
    RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        texture::{TextureSampler, TextureSource, TextureTarget, TextureTargetKind},
        vertices::{PosTexVertex, PosVertex, TextVertex},
    },
};
use std::sync::Arc;

struct Text {
    size: Vec2,
    glyphs: Vec<Glyph>,
}
struct Glyph {
    point: char,
    handle: GpuFontGlyphHandle,
    vertices: [TextVertex; 4],
    border: [Vec2; 8],
}
impl Text {
    fn prepare(server: &AssetServer, font: Arc<GpuFontLazy>, text: &str, prompt: bool) -> Self {
        let layout = HigurashiTextLayouter::new(
            font.clone(),
            text_params(prompt),
            MessageTextLayouterDefaults {
                color: 0xfff,
                draw_speed: 50,
                fade: 200,
            },
        );
        let (commands, lines, size) = if prompt {
            layout.parse(text)
        } else {
            layout.parse_plain(text, true)
        };
        let characters: Vec<_> = commands
            .iter()
            .filter_map(|c| match c {
                Command::Char(c) => Some(c),
                _ => None,
            })
            .collect();
        let points: Vec<_> = characters.iter().map(|c| c.codepoint).collect();
        let context = server.context();
        let handles = font.load_glyphs(
            context.wgpu_device.clone(),
            context.wgpu_queue.clone(),
            &points,
        );
        let glyphs = characters
            .into_iter()
            .zip(handles)
            .filter_map(|(c, handle)| {
                let info = handle.info();
                if info.actual_width == 0 || info.actual_height == 0 {
                    return None;
                }
                let mut vertices = glyph_vertices(c, &lines[c.line_index], info);
                for vertex in &mut vertices {
                    vertex.position = vertex.position + vec4(2.0, 2.0, 0.0, 0.0);
                }
                // SimpleText d2054 uses unscaled glyph bearings and the
                // requested text size, rather than message line metrics.
                // Both fill uniforms are equal, so this coordinate does not
                // introduce a vertical tint gradient.
                let reciprocal = 1.0 / text_params(prompt).text_size;
                let top = if c.is_rubi {
                    0.0
                } else {
                    -(info.bearing_y as f32) * reciprocal
                };
                let bottom = if c.is_rubi {
                    0.0
                } else {
                    (info.actual_height as i32 - info.bearing_y as i32) as f32 * reciprocal
                };
                vertices[0].color = top;
                vertices[1].color = top;
                vertices[2].color = bottom;
                vertices[3].color = bottom;
                Some(Glyph {
                    point: c.codepoint,
                    vertices,
                    border: outline::distances_with_radius(
                        vec2(c.horizontal_scale, c.scale),
                        info.actual_size_f32(),
                        info.actual_size_normalized(),
                        0,
                        1.5,
                    ),
                    handle,
                })
            })
            .collect();
        Self {
            size: size + Vec2::splat(4.0),
            glyphs,
        }
    }
    fn native_transform(&self, parent: Mat4, position: Vec2, anchor: Vec2) -> Mat4 {
        let alignment = Mat4::from_translation((-self.size * anchor).extend(0.0));
        let scale = Mat4::from_scale(vec3(1.0, 1.0, 0.0));
        multiply(
            parent,
            multiply(
                Mat4::from_translation(position.extend(0.0)),
                multiply(scale, alignment),
            ),
        )
    }
    fn render(&self, pass: &mut RenderPass, native: Mat4, fill: Vec4, border: Vec4) {
        let transform = HigurashiTransform::convert_clip_depth(native);
        for outline in [true, false] {
            if (if outline { border.w } else { fill.w }) <= 0.0 {
                continue;
            }
            for glyph in &self.glyphs {
                let Ok(texture) = glyph.handle.as_texture_source() else {
                    continue;
                };
                let vertices = VertexSource::VertexData {
                    vertices: &glyph.vertices,
                };
                let program = if outline {
                    RenderProgramWithArguments::FontBorder {
                        vertices,
                        glyph: texture,
                        transform,
                        distances: glyph.border,
                        color: color(border),
                    }
                } else {
                    RenderProgramWithArguments::Font {
                        vertices,
                        glyph: texture,
                        transform,
                        color1: color(fill),
                        color2: color(fill),
                    }
                };
                pass.run(builder().build(program, DrawPrimitive::TrianglesStrip));
            }
        }
    }
}

pub struct View {
    textures: Arc<DialogTextures>,
    prompt: Text,
    labels: Vec<Text>,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}
impl View {
    pub async fn load(
        server: &AssetServer,
        prompt: &str,
        labels: &[&str],
        selected: i32,
        cancellable: bool,
    ) -> Result<(Question, Self)> {
        let textures = server.load("/systex.txa").await?;
        let font: Arc<GpuFontLazy> = server.load("/newrodin.fnt").await?;
        let prompt = Text::prepare(server, font.clone(), prompt, true);
        let labels: Vec<_> = labels
            .iter()
            .map(|text| Text::prepare(server, font.clone(), text, false))
            .collect();
        let model = Question::new(
            prompt.size,
            &labels.iter().map(|text| text.size).collect::<Vec<_>>(),
            selected,
            cancellable,
        )?;
        let outer = model.outer();
        let texture = server
            .context()
            .wgpu_device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Higurashi question cache"),
                size: wgpu::Extent3d {
                    width: outer.x.ceil() as u32,
                    height: outer.y.ceil() as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: shin_render::TEXTURE_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
        let view = texture.create_view(&Default::default());
        Ok((
            model,
            Self {
                textures,
                prompt,
                labels,
                texture,
                view,
            },
        ))
    }
    pub fn prepare(&self, context: &mut PreRenderContext, model: &Question) {
        let mut pass = context.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::RenderTexture,
                view: &self.view,
            },
            None,
            "Higurashi question paint",
        );
        pass.clear(Some(UnormColor::from_rgba(0, 0, 0, 0)), None, None);
        self.paint(&mut pass, model);
    }
    fn paint(&self, pass: &mut RenderPass, model: &Question) {
        let outer = model.outer();
        let projection = projection(outer);
        patch(
            pass,
            &self.textures,
            0,
            vec4(0.0, 0.0, outer.x, outer.y),
            Vec4::ONE,
            projection,
        );
        let content = multiply(projection, Mat4::from_translation(vec3(30.0, 30.0, 0.0)));
        self.prompt.render(
            pass,
            self.prompt
                .native_transform(content, Vec2::ZERO, Vec2::ZERO),
            Vec4::ONE,
            vec4(0.0, 0.0, 0.0, 0.8),
        );
        for (index, (button, text)) in model.buttons().iter().zip(&self.labels).enumerate() {
            let rect = button.rect;
            patch(pass, &self.textures, 1, rect, button.background, content);
            text.render(
                pass,
                text.native_transform(
                    content,
                    vec2(rect.x, rect.y) + vec2(rect.z, rect.w) * 0.5,
                    Vec2::splat(0.5),
                ),
                button.fill,
                button.outline,
            );
            if model.selected() == index as i32 {
                patch(pass, &self.textures, 2, rect, Vec4::ONE, content);
            }
            let flash = button.flash.value();
            if flash > 0.0 {
                patch(
                    pass,
                    &self.textures,
                    3,
                    rect,
                    vec4(1.0, 1.0, 1.0, flash),
                    content,
                );
            }
        }
    }
    fn native_transform(&self, model: &Question) -> Mat4 {
        let half = model.outer() * 0.5;
        let mut transform = Mat4::from_translation((-half).extend(0.0));
        transform = multiply(
            Mat4::from_scale(vec3(model.scale(), model.scale(), 1.0)),
            transform,
        );
        transform = multiply(Mat4::from_translation(half.extend(0.0)), transform);
        transform = multiply(
            Mat4::from_translation(model.origin().extend(0.0)),
            transform,
        );
        multiply(projection(vec2(1920.0, 1080.0)), transform)
    }
    pub fn render(&self, pass: &mut RenderPass, model: &Question) {
        let backdrop = [
            vec3(-1.0, 1.0, 0.5),
            vec3(3.0, 1.0, 0.5),
            vec3(-1.0, -3.0, 0.5),
        ]
        .map(|position| PosVertex { position });
        pass.run(builder().build(
            RenderProgramWithArguments::Clear {
                vertices: VertexSource::VertexData {
                    vertices: &backdrop,
                },
                color: FloatColor4::from_rgba(0.0, 0.0, 0.0, 0.5 * model.opacity()),
            },
            DrawPrimitive::Triangles,
        ));
        let outer = model.outer();
        let uv = outer / vec2(self.texture.width() as f32, self.texture.height() as f32);
        let vertices = [
            vec4(0.0, 0.0, 0.0, 0.0),
            vec4(outer.x, 0.0, uv.x, 0.0),
            vec4(0.0, outer.y, 0.0, uv.y),
            vec4(outer.x, outer.y, uv.x, uv.y),
        ]
        .map(|v| PosTexVertex {
            position: vec2(v.x, v.y),
            texture_position: vec2(v.z, v.w),
        });
        textured(
            pass,
            VertexSource::VertexData {
                vertices: &vertices,
            },
            TextureSource {
                view: &self.view,
                sampler: TextureSampler::Linear,
            },
            self.native_transform(model),
            vec4(1.0, 1.0, 1.0, model.opacity()),
        );
    }
}

fn builder() -> RenderRequestBuilder {
    RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1)
}
fn color(v: Vec4) -> FloatColor4 {
    FloatColor4::from_rgba(v.x, v.y, v.z, v.w)
}
fn projection(size: Vec2) -> Mat4 {
    //74d34 uses direct FDIV, including the signed zero in translation Z.
    Mat4::from_cols_array(&[
        2.0 / size.x,
        0.0,
        0.0,
        0.0,
        0.0,
        2.0 / -size.y,
        0.0,
        0.0,
        0.0,
        0.0,
        -1.0,
        0.0,
        -1.0,
        1.0,
        -0.0,
        1.0,
    ])
}
fn textured(
    pass: &mut RenderPass,
    vertices: VertexSource<PosTexVertex>,
    texture: TextureSource,
    native: Mat4,
    tint: Vec4,
) {
    pass.run(builder().build(
        RenderProgramWithArguments::Layer {
            output_kind: LayerShaderOutputKind::Layer,
            fragment_shader: LayerFragmentShader::Default,
            vertices,
            texture,
            transform: HigurashiTransform::convert_clip_depth(native),
            color_multiplier: color(tint),
            fragment_shader_param: Vec4::ZERO,
        },
        DrawPrimitive::TrianglesStrip,
    ));
}
const INDICES: [u16; 22] = [
    0, 4, 1, 5, 2, 6, 3, 7, 11, 6, 10, 5, 9, 4, 8, 12, 9, 13, 10, 14, 11, 15,
];
fn patch_vertices(kind: usize, rect: Vec4) -> [PosTexVertex; 16] {
    let border = if kind == 0 {
        vec4(16.0, 32.0, 16.0, 32.0)
    } else {
        Vec4::splat(8.0)
    };
    let x = [
        rect.x,
        rect.x + border.x,
        rect.x + rect.z - border.y,
        rect.x + rect.z,
    ];
    let y = [
        rect.y,
        rect.y + border.z,
        rect.y + rect.w - border.w,
        rect.y + rect.w,
    ];
    let u = match kind {
        0 => [0.0, 0.25, 0.5, 1.0],
        2 => [0.0, 0.125, 0.25, 0.375],
        _ => [0.5, 0.625, 0.75, 0.875],
    };
    let v = if kind == 0 {
        [0.0, 0.25, 0.5, 1.0]
    } else {
        [0.0, 0.25, 0.5, 0.75]
    };
    std::array::from_fn(|i| PosTexVertex {
        position: vec2(x[i % 4], y[i / 4]),
        texture_position: vec2(u[i % 4], v[i / 4]),
    })
}
fn patch(
    pass: &mut RenderPass,
    textures: &DialogTextures,
    kind: usize,
    rect: Vec4,
    tint: Vec4,
    native: Mat4,
) {
    let vertices = patch_vertices(kind, rect);
    textured(
        pass,
        VertexSource::VertexAndIndexData {
            vertices: &vertices,
            indices: &INDICES,
        },
        (if kind == 0 {
            &textures.frame
        } else {
            &textures.button
        })
        .as_source(),
        native,
        tint,
    );
}

#[cfg(test)]
mod tests;
