//! Updated7c31c panel, viewport stencil, chapter header and integer scrollbar.
use super::{
    Navigation,
    labels::{Cache, Glyphs},
};
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::{browser_controls::BrowserControls, browser_history::Row},
    },
    asset::{font::GpuFontLazy, system::AssetServer},
    layer::render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
};
use anyhow::{Result, ensure};
use glam::{Mat4, Vec4, vec2, vec3, vec4};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, DepthStencilState, DrawPrimitive, RenderProgramWithArguments,
    RenderRequestBuilder, StencilFunction, StencilOperation, StencilPipelineState, StencilState,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        vertices::{PosColTexVertex, PosColVertex},
    },
};
use std::sync::Arc;

pub struct View {
    atlas: Arc<CharacterTextures>,
    font: Arc<GpuFontLazy>,
    rows: Arc<[Row]>,
    cache: Cache<Glyphs>,
}
#[derive(Clone, Copy)]
struct Part {
    rect: Vec4,
    left: u32,
    right: u32,
    texture: bool,
}
impl Part {
    fn fill(rect: Vec4, left: u32, right: u32) -> Self {
        Self {
            rect,
            left,
            right,
            texture: false,
        }
    }
    fn vertices(&self) -> [PosColVertex; 4] {
        [
            vec2(0.0, 0.0),
            vec2(1.0, 0.0),
            vec2(0.0, 1.0),
            vec2(1.0, 1.0),
        ]
        .map(|v| PosColVertex {
            position: vec3(
                self.rect.x + self.rect.z * v.x,
                self.rect.y + self.rect.w * v.y,
                0.0,
            ),
            color: UnormColor(if v.x == 0.0 { self.left } else { self.right }),
        })
    }
    fn sprite_vertices(&self) -> [PosColTexVertex; 4] {
        let vertices = self.vertices();
        std::array::from_fn(|i| PosColTexVertex {
            position: vertices[i].position,
            color: vertices[i].color,
            texture_position: vec2((i % 2) as f32, (i / 2) as f32),
        })
    }
}
fn transform(slide: f32) -> Mat4 {
    let projection = Mat4::from_cols_array(&[
        2.0 / 1920.0,
        0.0,
        0.0,
        0.0,
        0.0,
        2.0 / -1080.0,
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
    ]);
    multiply(
        projection,
        Mat4::from_translation(vec3(1920.0 - slide * 1920.0, 0.0, 0.0)),
    )
}
fn background() -> [Part; 2] {
    [
        Part::fill(vec4(420.0, 0.0, 1500.0, 1080.0), 0xc0808080, 0xc0808080),
        Part::fill(vec4(0.0, 0.0, 420.0, 1080.0), 0x00808080, 0xc0808080),
    ]
}
fn mask() -> Part {
    Part::fill(vec4(0.0, 240.0, 1920.0, 720.0), 0xffff0000, 0xffff0000)
}
fn foreground(count: usize, scroll: f32) -> Vec<Part> {
    let mut parts = vec![
        Part {
            rect: vec4(1152.0, 144.0, 648.0, 96.0),
            left: u32::MAX,
            right: u32::MAX,
            texture: true,
        },
        Part::fill(vec4(420.0, 234.0, 1500.0, 6.0), 0x00ffffff, u32::MAX),
        Part::fill(vec4(420.0, 960.0, 1500.0, 6.0), 0x00ffffff, u32::MAX),
    ];
    let height = (count as i32).wrapping_mul(60);
    if height > 720 {
        let numerator = (scroll as i32).wrapping_mul(720);
        let top = numerator / height;
        let bottom = numerator.wrapping_add(518400) / height;
        parts.push(Part::fill(
            vec4(1808.0, 240.0, 24.0, 720.0),
            0x60101040,
            0x60101040,
        ));
        parts.push(Part::fill(
            vec4(
                1808.0,
                top.wrapping_add(240) as f32,
                24.0,
                (bottom - top) as f32,
            ),
            0xc0c0c0c0,
            0xc0c0c0c0,
        ));
    }
    parts
}
fn builder(
    function: StencilFunction,
    operation: StencilOperation,
    reference: u8,
    blend: ColorBlendType,
) -> RenderRequestBuilder {
    RenderRequestBuilder::new()
        .color_blend_type(blend)
        .depth_stencil(DepthStencilState {
            stencil: StencilState {
                pipeline: StencilPipelineState {
                    function,
                    pass_operation: operation,
                    ..Default::default()
                },
                stencil_reference: reference,
            },
            ..Default::default()
        })
}
impl View {
    pub async fn load(assets: &AssetServer, rows: Arc<[Row]>) -> Result<Self> {
        let atlas: Arc<CharacterTextures> = assets.load("/logview.txa").await?;
        ensure!(
            atlas.contains("chapter"),
            "Browser atlas has no chapter header"
        );
        let font = assets.load("/newrodin.fnt").await?;
        Ok(Self {
            atlas,
            font,
            rows,
            cache: Cache::default(),
        })
    }
    pub fn prepare(&mut self, assets: &AssetServer, model: &Navigation) -> Result<()> {
        let context = assets.context();
        self.cache
            .synchronize(model.entries(), &self.rows, model.scroll(), |label| {
                Glyphs::prepare(
                    &context.wgpu_device,
                    &context.wgpu_queue,
                    self.font.clone(),
                    label,
                )
            })
    }
    fn part(&self, pass: &mut RenderPass, part: Part, matrix: Mat4, request: RenderRequestBuilder) {
        let matrix = HigurashiTransform::convert_clip_depth(matrix);
        if part.texture {
            let vertices = part.sprite_vertices();
            pass.run(request.build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    sprite: self.atlas.get("chapter").as_source(),
                    transform: matrix,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        } else {
            let vertices = part.vertices();
            pass.run(request.build(
                RenderProgramWithArguments::Fill {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    transform: matrix,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
    }
    pub fn render(&self, pass: &mut RenderPass, model: &Navigation, browser: &BrowserControls) {
        let matrix = transform(model.slide());
        let ordinary = builder(
            StencilFunction::Always,
            StencilOperation::Keep,
            0,
            ColorBlendType::Layer1,
        );
        for part in background() {
            self.part(pass, part, matrix, ordinary);
        }
        pass.clear(None, Some(0), None);
        // Original descriptor is Always/Increment with reference255. The
        // cleared viewport becomes1; labels test Equal1 without modifying it.
        self.part(
            pass,
            mask(),
            matrix,
            builder(
                StencilFunction::Always,
                StencilOperation::Increment,
                255,
                ColorBlendType::NoColor,
            ),
        );
        let viewport = browser.viewport();
        let clipped = builder(
            StencilFunction::Equal,
            StencilOperation::Keep,
            1,
            ColorBlendType::Layer1,
        );
        for cached in &self.cache.entries {
            let label = &cached.label;
            let fill = label.fill(
                model.selected(),
                model.entries()[label.index].row,
                viewport.first,
                viewport.end,
            );
            cached.value.render(
                pass,
                label.index,
                model.scroll(),
                model.slide(),
                fill,
                clipped,
            );
        }
        for part in foreground(model.entries().len(), model.scroll()) {
            self.part(pass, part, matrix, ordinary);
        }
    }
}

#[cfg(test)]
mod tests;
