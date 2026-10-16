//! Reusable Ger Question child with the FNT6 SimpleText SDF renderer.
//! The parent supplies native question strings/defaults; CPU input and geometry
//! reuse the shared Question owner. Device publication remains a host boundary.
use crate::{
    adv::{
        backlog_rollback::{Confirmation, Outcome},
        character_view::CharacterTextures,
        dialog_input::DialogInput,
        dialog_view::DialogGeometry,
    },
    app::AppAction,
    asset::{
        font::gerokasu2::{GpuGerokasu2Font, GpuGerokasu2GlyphHandle, draw::GlyphDraw},
        system::AssetServer,
    },
    layer::render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
    render::PreRenderContext,
};
use anyhow::{Result, ensure};
use enum_map::EnumMap;
use glam::{Mat4, Vec2, Vec4, vec2, vec3, vec4};
use shin_core::{
    layout::{LayoutParams, commands::Command},
    primitives::color::{FloatColor4, UnormColor},
    time::Ticks,
};
use shin_input::ActionState;
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerFragmentShader, LayerShaderOutputKind,
    RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        texture::{TextureSampler, TextureSource, TextureTarget, TextureTargetKind},
        vertices::{PosTexVertex, PosVertex, WindowVertex},
    },
};
use std::sync::Arc;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Response {
    pub final_flag: bool,
    pub button: i32,
}
struct Text {
    size: Vec2,
    glyphs: Vec<(GpuGerokasu2GlyphHandle, GlyphDraw)>,
}
impl Text {
    fn load(
        server: &AssetServer,
        font: &Arc<GpuGerokasu2Font>,
        text: &str,
        size: f32,
    ) -> Result<Self> {
        let (commands, _, extent) = shin_core::layout::gerokasu2::generic_layouter(
            font.metrics(),
            LayoutParams {
                layout_width: 1434.,
                text_size: size,
                rubi_size: 3.,
                line_padding_above: 4.,
                line_padding_below: 4.,
                perform_soft_breaks: size == 48.,
                always_leave_space_for_rubi: false,
                follow_kinsoku_shori_rules: true,
                ..Default::default()
            },
        )
        .parse(&font.localize(text));
        let chars: Vec<_> = commands
            .into_iter()
            .filter_map(|c| {
                if let Command::Char(c) = c {
                    Some(c)
                } else {
                    None
                }
            })
            .collect();
        let points: Vec<_> = chars.iter().map(|c| c.codepoint).collect();
        let context = server.context();
        let handles = font.clone().load_glyphs(
            context.wgpu_device.clone(),
            context.wgpu_queue.clone(),
            &points,
        );
        let mut glyphs = vec![];
        for (c, h) in chars.into_iter().zip(handles) {
            let draw = GlyphDraw::new(
                h.geometry()?,
                c.position + Vec2::splat(3.),
                vec2(c.horizontal_scale, c.scale),
                size,
                c.is_rubi,
            )?;
            glyphs.push((h, draw));
        }
        Ok(Self {
            size: extent + Vec2::splat(6.),
            glyphs,
        })
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        matrix: Mat4,
        fill: FloatColor4,
        border: FloatColor4,
    ) -> Result<()> {
        let builder = builder();
        for (h, d) in &self.glyphs {
            d.render_outline(
                h,
                pass,
                builder,
                HigurashiTransform::convert_clip_depth(matrix),
                border,
            )?;
        }
        for (h, d) in &self.glyphs {
            d.render_fill(
                h,
                pass,
                builder,
                HigurashiTransform::convert_clip_depth(matrix),
                fill,
                fill,
            )?;
        }
        Ok(())
    }
}
pub(in crate::adv) fn geometry(prompt: Vec2, answers: &[Vec2]) -> DialogGeometry {
    assert!(answers.len() <= 2);
    let mut g = match answers {
        [] => DialogGeometry::notification(prompt),
        [one] => DialogGeometry::single(prompt, *one),
        [one, two] => DialogGeometry::new(prompt, [*one, *two]),
        _ => unreachable!(),
    };
    if g.vertical {
        let width = g.buttons[0].z.max(0.75 * prompt.x.max(600.));
        g.inner.x = g.inner.x.max(width);
        for button in &mut g.buttons[..g.button_count] {
            button.x = (g.inner.x - width) * 0.5;
            button.z = width;
        }
    }
    g.outer = g.inner + vec2(100., 60.);
    g.origin = (vec2(1920., 1080.) - g.inner) * 0.5 - vec2(50., 30.);
    g
}
pub struct GerDialog {
    confirmation: Confirmation,
    input: DialogInput,
    geometry: DialogGeometry,
    atlas: Arc<CharacterTextures>,
    prompt: Text,
    answers: Vec<Text>,
    _cache: wgpu::Texture,
    cache_view: wgpu::TextureView,
}
impl GerDialog {
    pub fn load(
        server: &AssetServer,
        prompt: &str,
        labels: &[&str],
        selected: i32,
        cancellable: bool,
        held: bool,
    ) -> Result<Self> {
        ensure!(
            labels.len() <= 2
                && selected >= 0
                && if labels.is_empty() {
                    selected == 0
                } else {
                    (selected as usize) < labels.len()
                },
            "Ger Question selection outside its authored labels"
        );
        let font = server.load_sync::<GpuGerokasu2Font>("/system.fnt")?;
        let atlas = server.load_sync::<CharacterTextures>("/systex.txa")?;
        atlas.require_names(
            "/systex.txa",
            &["dialog_border", "dialog_mask", "dialog_bg", "dialog_button"],
        )?;
        let prompt = Text::load(server, &font, prompt, 48.)?;
        let answers: Vec<_> = labels
            .iter()
            .map(|s| Text::load(server, &font, s, 40.))
            .collect::<Result<_>>()?;
        let sizes: Vec<_> = answers.iter().map(|t| t.size).collect();
        let geometry = geometry(prompt.size, &sizes);
        let cache = server
            .context()
            .wgpu_device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Ger Question cache"),
                size: wgpu::Extent3d {
                    width: geometry.outer.x.round() as u32,
                    height: geometry.outer.y.round() as u32,
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
        let cache_view = cache.create_view(&Default::default());
        let mut input = DialogInput::default();
        input.set_button_hit_offset(Vec2::splat(30.));
        input.ignore_pointer(held);
        let mut confirmation = if labels.is_empty() {
            Confirmation::notification()
        } else {
            Confirmation::new(labels.len(), selected as usize, cancellable)
        };
        confirmation.set_deferred_cancel(true);
        Ok(Self {
            confirmation,
            input,
            geometry,
            atlas,
            prompt,
            answers,
            _cache: cache,
            cache_view,
        })
    }
    pub fn cancel_input(&mut self) {
        if let Some(index) = self.confirmation.selected() {
            self.confirmation.cancel_hold(index);
        }
        self.input.ignore_pointer(true);
    }
    pub fn keyboard(&mut self, state: &EnumMap<AppAction, ActionState>) {
        self.input
            .keyboard(&mut self.confirmation, self.geometry.vertical, state);
    }
    pub fn pointer_state(&mut self, point: Vec2, held: bool, elapsed: f64) {
        self.input.pointer_timed(
            &mut self.confirmation,
            &mut self.geometry,
            point,
            held,
            elapsed,
        );
    }
    pub fn update(&mut self, delta: Ticks) -> Vec<Response> {
        self.input.update_motion(&mut self.geometry, delta);
        self.confirmation.update(delta);
        self.responses()
    }
    pub fn update_notification(
        &mut self,
        delta: Ticks,
        ready: impl FnMut() -> bool,
    ) -> Vec<Response> {
        self.confirmation.update_notification(delta, ready);
        self.responses()
    }
    fn responses(&mut self) -> Vec<Response> {
        let mut out = vec![];
        for (final_flag, result) in [
            (false, self.confirmation.take_intermediate_outcome()),
            (true, self.confirmation.take_outcome()),
        ] {
            if let Some(result) = result {
                out.push(Response {
                    final_flag,
                    button: if self.answers.is_empty() {
                        -2
                    } else {
                        match result {
                            Outcome::Yes => 0,
                            Outcome::No => 1,
                            Outcome::Cancel => -1,
                        }
                    },
                });
            }
        }
        out
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        self.input.take_sounds()
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) -> Result<()> {
        let mut pass = context.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::RenderTexture,
                view: &self.cache_view,
            },
            None,
            "Ger Question paint",
        );
        pass.clear(Some(UnormColor::from_rgba(0, 0, 0, 0)), None, None);
        let outer = self.geometry.outer;
        let projection = projection(outer);
        let vertices = frame_vertices(outer);
        pass.run(builder().build(
            RenderProgramWithArguments::Window {
                vertices: VertexSource::VertexAndIndexData {
                    vertices: &vertices,
                    indices: &INDICES,
                },
                border: self.atlas.get("dialog_border").as_source(),
                mask: self.atlas.get("dialog_mask").as_source(),
                background: self.atlas.get("dialog_bg").as_source(),
                transform: HigurashiTransform::convert_clip_depth(projection),
            },
            DrawPrimitive::TrianglesStrip,
        ));
        let content = content_transform(outer);
        self.prompt.render(
            &mut pass,
            content,
            color(Vec4::splat(0.8).with_w(1.)),
            color(Vec4::splat(0.2).with_w(1.)),
        )?;
        for (i, answer) in self.answers.iter().enumerate() {
            let rect = self.geometry.buttons[i];
            let (fill, outline, background) = palette(self.confirmation.focus[i].value());
            patch(&mut pass, &self.atlas, 0, rect, background, content);
            let center = vec2(rect.x, rect.y) + vec2(rect.z, rect.w) * 0.5;
            // SimpleText first aligns around its measured extent, then applies
            // its position. Preserve these as separate matrix products.
            answer.render(
                &mut pass,
                text_transform(content, center, answer.size),
                color(fill),
                color(outline),
            )?;
            if self.confirmation.selected() == Some(i) {
                patch(
                    &mut pass,
                    &self.atlas,
                    1,
                    rect,
                    vec4(0.8, 0.8, 0.8, 1.),
                    content,
                );
            }
            let flash = self.confirmation.flashes[i].value();
            if flash > 0. {
                patch(
                    &mut pass,
                    &self.atlas,
                    2,
                    rect,
                    vec4(0.8, 0.8, 0.8, flash),
                    content,
                );
            }
        }
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        let opacity = self.confirmation.opacity.value();
        let backdrop = backdrop_vertices().map(|mut vertex| {
            // Native Clear has no matrix, so bridge GL clip depth here.
            vertex.position.z = (vertex.position.z + 1.) * 0.5;
            vertex
        });
        pass.run(builder().build(
            RenderProgramWithArguments::Clear {
                vertices: VertexSource::VertexData {
                    vertices: &backdrop,
                },
                color: FloatColor4::BLACK.with_alpha(0.5 * opacity),
            },
            DrawPrimitive::Triangles,
        ));
        let vertices = composite_vertices(self.geometry.outer);
        textured(
            pass,
            VertexSource::VertexData {
                vertices: &vertices,
            },
            TextureSource {
                view: &self.cache_view,
                sampler: TextureSampler::Linear,
            },
            composite_transform(
                self.geometry.outer,
                self.geometry.origin,
                self.confirmation.scale.value(),
            ),
            vec4(1., 1., 1., opacity),
        );
        Ok(())
    }
}
fn backdrop_vertices() -> [PosVertex; 3] {
    [vec3(-1., 1., 0.), vec3(3., 1., 0.), vec3(-1., -3., 0.)].map(|position| PosVertex { position })
}
fn builder() -> RenderRequestBuilder {
    RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1)
}
fn color(v: Vec4) -> FloatColor4 {
    FloatColor4::from_vec4(v)
}
fn projection(size: Vec2) -> Mat4 {
    Mat4::from_cols_array(&[
        2. / size.x,
        0.,
        0.,
        0.,
        0.,
        2. / -size.y,
        0.,
        0.,
        0.,
        0.,
        -0.5,
        0.,
        -1.,
        1.,
        0.5,
        1.,
    ])
}
fn content_transform(outer: Vec2) -> Mat4 {
    multiply(
        projection(outer),
        Mat4::from_translation(vec3(50., 30., 0.)),
    )
}
fn text_transform(parent: Mat4, position: Vec2, size: Vec2) -> Mat4 {
    multiply(
        parent,
        multiply(
            Mat4::from_translation(position.extend(0.)),
            multiply(
                Mat4::from_scale(vec3(1., 1., 0.)),
                Mat4::from_translation((-size * 0.5).extend(0.)),
            ),
        ),
    )
}
fn composite_transform(outer: Vec2, origin: Vec2, scale: f32) -> Mat4 {
    let half = outer * 0.5;
    let transform = multiply(
        Mat4::from_scale(vec3(scale, scale, 1.)),
        Mat4::from_translation((-half).extend(0.)),
    );
    let transform = multiply(Mat4::from_translation(half.extend(0.)), transform);
    let transform = multiply(Mat4::from_translation(origin.extend(0.)), transform);
    multiply(projection(vec2(1920., 1080.)), transform)
}
fn palette(focus: f32) -> (Vec4, Vec4, Vec4) {
    // Native50018 rounds subtraction, multiplication and addition separately.
    let mix = |a: Vec4, b: Vec4| a + (b - a) * focus;
    (
        mix(vec4(0.8, 0.8, 0.8, 0.5), vec4(0.8, 0.8, 0.8, 1.)),
        mix(vec4(0.2, 0.2, 0.2, 0.5), vec4(0.2, 0.2, 0.2, 0.8)),
        mix(vec4(1., 1., 1., 0.), Vec4::ONE),
    )
}
const INDICES: [u16; 22] = [
    0, 4, 1, 5, 2, 6, 3, 7, 11, 6, 10, 5, 9, 4, 8, 12, 9, 13, 10, 14, 11, 15,
];
fn frame_vertices(outer: Vec2) -> [WindowVertex; 16] {
    let xs = [0., 16., outer.x - 16., outer.x];
    let ys = [0., 16., outer.y - 16., outer.y];
    let uv = [
        0.,
        f32::from_bits(0x3eaaaaab),
        f32::from_bits(0x3f2aaaaa),
        1.,
    ];
    let ratio = outer / vec2(1920., 1080.);
    let offset = (Vec2::ONE - ratio) * 0.5;
    std::array::from_fn(|i| {
        let x = xs[i % 4];
        let y = ys[i / 4];
        let mask = vec2(x / outer.x, y / outer.y);
        WindowVertex {
            position: vec4(x, y, uv[i % 4], uv[i / 4]),
            texture_position: vec4(
                mask.x.mul_add(ratio.x, offset.x),
                mask.y.mul_add(ratio.y, offset.y),
                mask.x,
                mask.y,
            ),
        }
    })
}
fn button_vertices(kind: usize, rect: Vec4) -> [PosTexVertex; 16] {
    let xs = [rect.x, rect.x + 8., rect.x + rect.z - 8., rect.x + rect.z];
    let ys = [rect.y, rect.y + 8., rect.y + rect.w - 8., rect.y + rect.w];
    let us = if kind == 1 {
        [0., 0.125, 0.25, 0.375]
    } else {
        [0.5, 0.625, 0.75, 0.875]
    };
    let vs = [0., 0.25, 0.5, 0.75];
    std::array::from_fn(|i| PosTexVertex {
        position: vec2(xs[i % 4], ys[i / 4]),
        texture_position: vec2(us[i % 4], vs[i / 4]),
    })
}
fn composite_vertices(outer: Vec2) -> [PosTexVertex; 4] {
    let uv = outer / outer.ceil();
    [
        vec4(0., 0., 0., 0.),
        vec4(outer.x, 0., uv.x, 0.),
        vec4(0., outer.y, 0., uv.y),
        vec4(outer.x, outer.y, uv.x, uv.y),
    ]
    .map(|v| PosTexVertex {
        position: vec2(v.x, v.y),
        texture_position: vec2(v.z, v.w),
    })
}
fn textured(
    pass: &mut RenderPass,
    vertices: VertexSource<PosTexVertex>,
    texture: TextureSource,
    matrix: Mat4,
    tint: Vec4,
) {
    pass.run(builder().build(
        RenderProgramWithArguments::Layer {
            output_kind: LayerShaderOutputKind::Layer,
            fragment_shader: LayerFragmentShader::Default,
            vertices,
            texture,
            transform: HigurashiTransform::convert_clip_depth(matrix),
            color_multiplier: color(tint),
            fragment_shader_param: Vec4::ZERO,
        },
        DrawPrimitive::TrianglesStrip,
    ));
}
fn patch(
    pass: &mut RenderPass,
    atlas: &CharacterTextures,
    kind: usize,
    rect: Vec4,
    tint: Vec4,
    matrix: Mat4,
) {
    let vertices = button_vertices(kind, rect);
    textured(
        pass,
        VertexSource::VertexAndIndexData {
            vertices: &vertices,
            indices: &INDICES,
        },
        atlas.get("dialog_button").as_source(),
        matrix,
        tint,
    );
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod localization_gpu_tests;
