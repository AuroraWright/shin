//! Native generic Yes/No dialog geometry and offscreen composition.
use crate::adv::{
    backlog::rollback::{ANSWERS, Confirmation, PROMPT},
    characters::text::CharacterTextGlyph,
};
use crate::{
    asset::{
        font::{GpuFontGlyphHandle, GpuFontLazy},
        system::AssetServer,
        texture_archive::TextureArchive,
    },
    render::{PreRenderContext, top_left_projection_matrix},
};
use glam::{Mat4, Vec2, Vec4, vec2, vec4};
use shin_core::{
    layout::{
        LayoutParams, MessageTextLayouter, MessageTextLayouterDefaults, commands::Command,
        font::FontMetrics,
    },
    primitives::color::{FloatColor4, UnormColor},
};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    resize::CanvasSize,
    shaders::types::{
        buffer::VertexSource,
        texture::{TextureSampler, TextureSource, TextureTarget, TextureTargetKind},
        vertices::TextVertex,
    },
    shin_orthographic_projection_matrix,
};
use std::sync::Arc;

#[derive(TextureArchive)]
pub(crate) struct DialogTextures {
    #[txa(name = "dialog_frame")]
    pub frame: GpuTexture,
    #[txa(name = "dialog_button")]
    pub button: GpuTexture,
    #[txa(name = "drag")]
    pub drag: GpuTexture,
    #[txa(name = "scroll")]
    pub scroll: GpuTexture,
}

#[derive(Debug)]
pub(crate) struct DialogGeometry {
    pub button_count: usize,
    pub inner: Vec2,
    pub outer: Vec2,
    pub origin: Vec2,
    /// Rectangles in the content coordinate system (texture origin +30,+30).
    pub buttons: [Vec4; 2],
    pub vertical: bool,
}
impl DialogGeometry {
    pub fn notification(prompt: Vec2) -> Self {
        let inner = vec2(prompt.x.max(600.0), prompt.y);
        Self {
            button_count: 0,
            inner,
            outer: inner + Vec2::splat(76.0),
            origin: (vec2(1920.0, 1080.0) - inner) * 0.5 - Vec2::splat(30.0),
            buttons: [Vec4::ZERO; 2],
            vertical: false,
        }
    }
    pub fn new(prompt: Vec2, answers: [Vec2; 2]) -> Self {
        let mut inner = vec2(prompt.x.max(600.0), prompt.y);
        let button = vec2(
            answers
                .iter()
                .map(|s| (s.x + 80.0).max(180.0))
                .fold(0.0, f32::max),
            answers.iter().map(|s| s.y + 24.0).fold(0.0, f32::max),
        );
        let vertical = button.x * 2.0 > inner.x * 0.75;
        let heights = answers.map(|answer| answer.y + 24.0);
        let buttons = if vertical {
            // Native22bb4 caps width by .75*content, but never below the largest
            // original button; with two equalized buttons this retains button.x.
            inner.x = inner.x.max(button.x);
            let x = (inner.x - button.x) * 0.5;
            inner.y = prompt.y + heights[0] + heights[1] + 16.0;
            [
                vec4(x, prompt.y + 8.0, button.x, heights[0]),
                vec4(x, prompt.y + heights[0] + 16.0, button.x, heights[1]),
            ]
        } else {
            let gap = (inner.x * 0.75 - button.x * 2.0).min(30.0);
            let x = (inner.x - button.x * 2.0 - gap) * 0.5;
            inner.y = prompt.y + 8.0 + button.y;
            [
                vec4(
                    x,
                    prompt.y + 8.0 + (button.y - heights[0]) * 0.5,
                    button.x,
                    heights[0],
                ),
                vec4(
                    x + button.x + gap,
                    prompt.y + 8.0 + (button.y - heights[1]) * 0.5,
                    button.x,
                    heights[1],
                ),
            ]
        };
        let outer = inner + Vec2::splat(76.0);
        let origin = (vec2(1920.0, 1080.0) - inner) * 0.5 - Vec2::splat(30.0);
        Self {
            button_count: 2,
            inner,
            outer,
            origin,
            buttons,
            vertical,
        }
    }
    pub fn single(prompt: Vec2, answer: Vec2) -> Self {
        let button = vec2((answer.x + 80.0).max(180.0), answer.y + 24.0);
        let mut inner = vec2(prompt.x.max(600.0), prompt.y + 8.0 + button.y);
        let vertical = button.x > inner.x * 0.75;
        if vertical {
            inner.x = inner.x.max(button.x);
        }
        let outer = inner + Vec2::splat(76.0);
        Self {
            button_count: 1,
            inner,
            outer,
            origin: (vec2(1920.0, 1080.0) - inner) * 0.5 - Vec2::splat(30.0),
            buttons: [
                vec4(
                    (inner.x - button.x) * 0.5,
                    prompt.y + 8.0,
                    button.x,
                    button.y,
                ),
                Vec4::ZERO,
            ],
            vertical,
        }
    }
    ///27230/27260 divide fba50/fba60 by1920/1080. Both framebuffer
    /// helpers are constants in the supplied executable, so the scale is1.
    /// Host canvas resizing must not reduce this native cached resolution.
    pub fn texture_extent(&self) -> [u32; 2] {
        [self.outer.x.ceil() as u32, self.outer.y.ceil() as u32]
    }
    pub fn pointer_scale(canvas: CanvasSize) -> Option<Vec2> {
        (canvas.width > 0 && canvas.height > 0)
            .then(|| vec2(1920.0 / canvas.width as f32, 1080.0 / canvas.height as f32))
    }
    pub fn transform(&self, scale: f32) -> Mat4 {
        top_left_projection_matrix()
            * Mat4::from_translation((self.origin + self.outer * 0.5).extend(0.0))
            * Mat4::from_scale(vec2(scale, scale).extend(1.0))
            * Mat4::from_translation((-self.outer * 0.5).extend(0.0))
    }
}

///24590/165454: asymmetric window borders, eight-pixel button borders.
fn patch(
    pass: &mut RenderPass,
    textures: &DialogTextures,
    kind: usize,
    rect: Vec4,
    color: FloatColor4,
    transform: Mat4,
) {
    let borders = if kind == 0 { [16.0, 32.0] } else { [8.0, 8.0] };
    let x = [
        rect.x,
        rect.x + borders[0],
        rect.x + rect.z - borders[1],
        rect.x + rect.z,
    ];
    let y = [
        rect.y,
        rect.y + borders[0],
        rect.y + rect.w - borders[1],
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
    let texture = if kind == 0 {
        &textures.frame
    } else {
        &textures.button
    };
    for row in 0..3 {
        for col in 0..3 {
            QuadVertices::new()
                .with_box(x[col], y[row], x[col + 1], y[row + 1])
                .with_tex_box(u[col], v[row], u[col + 1], v[row + 1])
                .with_color(color.into_unorm())
                .render_sprite(
                    pass,
                    RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                    texture.as_source(),
                    transform,
                );
        }
    }
}

/// Storage error notices must remain usable when their own atlas is damaged.
/// Normal dialogs still require the native artwork; only the error adapter
/// selects these plain panels, retaining the same geometry, text and input.
fn notice_patch(
    pass: &mut RenderPass,
    textures: Option<&DialogTextures>,
    kind: usize,
    rect: Vec4,
    color: FloatColor4,
    transform: Mat4,
) {
    if let Some(textures) = textures {
        patch(pass, textures, kind, rect, color, transform);
        return;
    }
    let fill = |pass: &mut RenderPass, r: Vec4, tint: FloatColor4| {
        QuadVertices::new()
            .with_box(r.x, r.y, r.x + r.z, r.y + r.w)
            .with_color(tint.into_unorm())
            .fill(
                pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                transform,
            );
    };
    let border = |pass: &mut RenderPass, tint: FloatColor4| {
        for r in [
            vec4(rect.x, rect.y, rect.z, 4.0),
            vec4(rect.x, rect.y + rect.w - 4.0, rect.z, 4.0),
            vec4(rect.x, rect.y + 4.0, 4.0, rect.w - 8.0),
            vec4(rect.x + rect.z - 4.0, rect.y + 4.0, 4.0, rect.w - 8.0),
        ] {
            fill(pass, r, tint);
        }
    };
    match kind {
        0 => {
            fill(pass, rect, FloatColor4::from_rgba(0.06, 0.08, 0.12, 0.98));
            border(pass, FloatColor4::from_rgba(0.38, 0.42, 0.50, 1.0));
        }
        1 => fill(pass, rect, FloatColor4::from_rgba(0.18, 0.22, 0.30, 1.0)),
        2 => border(pass, FloatColor4::from_rgba(0.25, 0.65, 1.0, 1.0)),
        _ => fill(pass, rect, color),
    }
}

pub(crate) struct DialogText {
    glyphs: Vec<CharacterTextGlyph>,
    handles: Vec<GpuFontGlyphHandle>,
    pub size: Vec2,
}
impl DialogText {
    #[cfg(test)]
    pub(crate) fn reference_glyphs(&self) -> &[CharacterTextGlyph] {
        &self.glyphs
    }
    fn load(server: &AssetServer, font: &Arc<GpuFontLazy>, text: &str, size: f32) -> Self {
        Self::load_with_width(server, font, text, size, 1440.0)
    }
    pub fn load_with_width(
        server: &AssetServer,
        font: &Arc<GpuFontLazy>,
        text: &str,
        size: f32,
        width: f32,
    ) -> Self {
        Self::load_with_params(
            server,
            font,
            text,
            LayoutParams {
                layout_width: width - 6.0,
                text_size: size,
                rubi_size: 3.0,
                line_padding_above: 4.0,
                line_padding_below: 4.0,
                ..Default::default()
            },
        )
    }

    /// Native SimpleText style1 with zeroed ruby/wrap and line-padding fields.
    /// Gallery completion labels use this layout rather than dialog spacing.
    pub(crate) fn load_simple_text(
        server: &AssetServer,
        font: &Arc<GpuFontLazy>,
        text: &str,
        size: f32,
        width: f32,
    ) -> Self {
        Self::load_with_params(
            server,
            font,
            text,
            LayoutParams {
                layout_width: width - 6.0,
                text_size: size,
                perform_soft_breaks: false,
                ..Default::default()
            },
        )
    }

    fn load_with_params(
        server: &AssetServer,
        font: &Arc<GpuFontLazy>,
        text: &str,
        params: LayoutParams,
    ) -> Self {
        let (commands, _, extent) = MessageTextLayouter::new(
            font.as_ref(),
            font.as_ref(),
            params,
            MessageTextLayouterDefaults {
                color: 999,
                draw_speed: 50,
                fade: 200,
            },
        )
        .parse(text);
        let mut glyphs = Vec::new();
        for command in commands {
            let Command::Char(ch) = command else { continue };
            let info = font.get_glyph_info(ch.codepoint).unwrap();
            if info.actual_width == 0 || info.actual_height == 0 {
                continue;
            }
            let scale = vec2(ch.horizontal_scale, ch.scale);
            let pixels = info.actual_size_f32() * scale;
            let uv = info.actual_size_normalized();
            let radius = uv * (Vec2::splat(3.0) / pixels);
            let pos = ch.position + info.bearing_screenspace_f32() * scale;
            glyphs.push(CharacterTextGlyph {
                codepoint: ch.codepoint,
                rect: vec4(pos.x, pos.y, pixels.x + 6.0, pixels.y + 6.0),
                uv: vec4(-radius.x, -radius.y, uv.x + radius.x, uv.y + radius.y),
                distances: [
                    vec2(-0.707, -0.707),
                    vec2(0.0, -1.0),
                    vec2(0.707, -0.707),
                    vec2(-1.0, 0.0),
                    vec2(1.0, 0.0),
                    vec2(-0.707, 0.707),
                    vec2(0.0, 1.0),
                    vec2(0.707, 0.707),
                ]
                .map(|d| d * radius),
                color: FloatColor4::WHITE,
            });
        }
        let points: Vec<_> = glyphs.iter().map(|g| g.codepoint).collect();
        let handles = font.clone().load_glyphs(
            server.context().wgpu_device.clone(),
            server.context().wgpu_queue.clone(),
            &points,
        );
        Self {
            glyphs,
            handles,
            size: extent + Vec2::splat(6.0),
        }
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        transform: Mat4,
        fill: FloatColor4,
        border: FloatColor4,
    ) {
        self.render_with_builder(
            pass,
            transform,
            fill,
            border,
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
        );
    }
    pub fn render_with_builder(
        &self,
        pass: &mut RenderPass,
        transform: Mat4,
        fill: FloatColor4,
        border: FloatColor4,
        builder: RenderRequestBuilder,
    ) {
        for outline in [true, false] {
            for (g, h) in self.glyphs.iter().zip(&self.handles) {
                let Ok(glyph_texture) = h.as_texture_source() else {
                    continue;
                };
                let r = g.rect;
                let u = g.uv;
                let vertices = [
                    vec4(r.x, r.y, u.x, u.y),
                    vec4(r.x + r.z, r.y, u.z, u.y),
                    vec4(r.x, r.y + r.w, u.x, u.w),
                    vec4(r.x + r.z, r.y + r.w, u.z, u.w),
                ]
                .map(|position| TextVertex {
                    position,
                    color: 0.0,
                });
                let vertices = VertexSource::VertexData {
                    vertices: &vertices,
                };
                let program = if outline {
                    RenderProgramWithArguments::FontBorder {
                        vertices,
                        glyph: glyph_texture,
                        transform,
                        distances: g.distances,
                        color: border,
                    }
                } else {
                    RenderProgramWithArguments::Font {
                        vertices,
                        glyph: glyph_texture,
                        transform,
                        color1: fill,
                        color2: fill,
                    }
                };
                pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
            }
        }
    }
}

pub(crate) struct DialogView {
    pub geometry: DialogGeometry,
    textures: Option<Arc<DialogTextures>>,
    prompt: DialogText,
    answers: [DialogText; 2],
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}
impl DialogView {
    pub fn load(server: &AssetServer) -> anyhow::Result<Self> {
        Self::load_text(server, PROMPT, &ANSWERS)
    }
    pub fn load_text(server: &AssetServer, prompt: &str, labels: &[&str]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            labels.len() <= 2,
            "Generic dialog supports up to two answers"
        );
        let textures = server.load_sync::<DialogTextures>("/systex.txa")?;
        Self::load_text_with_textures(server, prompt, labels, Some(textures))
    }
    pub(crate) fn load_storage_notice(
        server: &AssetServer,
        prompt: &str,
        labels: &[&str],
    ) -> anyhow::Result<Self> {
        let textures = match server.load_sync::<DialogTextures>("/systex.txa") {
            Ok(textures) => Some(textures),
            Err(error) => {
                tracing::warn!(%error, "Storage notice artwork failed; using plain frame");
                None
            }
        };
        Self::load_text_with_textures(server, prompt, labels, textures)
    }
    fn load_text_with_textures(
        server: &AssetServer,
        prompt: &str,
        labels: &[&str],
        textures: Option<Arc<DialogTextures>>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            labels.len() <= 2,
            "Generic dialog supports up to two answers"
        );
        let font = server.load_sync::<GpuFontLazy>("/system.fnt")?;
        let prompt = DialogText::load(server, &font, prompt, 54.0);
        let answers = std::array::from_fn(|index| {
            DialogText::load(
                server,
                &font,
                labels.get(index).copied().unwrap_or(""),
                44.0,
            )
        });
        let geometry = match labels.len() {
            0 => DialogGeometry::notification(prompt.size),
            1 => DialogGeometry::single(prompt.size, answers[0].size),
            _ => DialogGeometry::new(prompt.size, [answers[0].size, answers[1].size]),
        };
        let (texture, view) =
            Self::allocate(&server.context().wgpu_device, geometry.texture_extent());
        Ok(Self {
            geometry,
            textures,
            prompt,
            answers,
            texture,
            view,
        })
    }
    fn allocate(device: &wgpu::Device, size: [u32; 2]) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Native scene jump dialog"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
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
        (texture, view)
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext, confirmation: &Confirmation) {
        let canvas = context.resize_source.canvas_handle().get();
        if canvas.width == 0 || canvas.height == 0 {
            return;
        }
        let size = self.geometry.texture_extent();
        if size != [self.texture.width(), self.texture.height()] {
            (self.texture, self.view) = Self::allocate(context.device, size);
        }
        let mut pass = context.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::RenderTexture,
                view: &self.view,
            },
            None,
            "Native dialog composition",
        );
        pass.clear(Some(UnormColor::from_rgba(0, 0, 0, 0)), None, None);
        let g = &self.geometry;
        let projection =
            shin_orthographic_projection_matrix(0.0, g.outer.x, g.outer.y, 0.0, -1.0, 1.0);
        notice_patch(
            &mut pass,
            self.textures.as_deref(),
            0,
            vec4(0.0, 0.0, g.outer.x, g.outer.y),
            FloatColor4::WHITE,
            projection,
        );
        let content = projection * Mat4::from_translation(Vec2::splat(30.0).extend(0.0));
        self.prompt.render(
            &mut pass,
            content,
            FloatColor4::WHITE,
            FloatColor4::from_rgba(0.0, 0.0, 0.0, 0.8),
        );
        for index in 0..g.button_count {
            let r = g.buttons[index];
            let focus = confirmation.focus[index].value();
            notice_patch(
                &mut pass,
                self.textures.as_deref(),
                1,
                r,
                FloatColor4::from_rgba(0.7 * focus, 0.7 * focus, 0.7 * focus, 0.5 + 0.2 * focus),
                content,
            );
            let pos = vec2(r.x, r.y) + (vec2(r.z, r.w) - self.answers[index].size) * 0.5;
            self.answers[index].render(
                &mut pass,
                content * Mat4::from_translation(pos.extend(0.0)),
                FloatColor4::from_rgba(1.0, 1.0, 1.0, 1.0 - 0.3 * focus),
                FloatColor4::from_rgba(
                    0.2 * focus,
                    0.5725 * focus,
                    0.957 * focus,
                    0.8 * (1.0 - focus),
                ),
            );
            if confirmation.selected() == Some(index) {
                notice_patch(
                    &mut pass,
                    self.textures.as_deref(),
                    2,
                    r,
                    FloatColor4::WHITE,
                    content,
                );
            }
            let flash = confirmation.flashes[index].value();
            if flash > 0.0 {
                notice_patch(
                    &mut pass,
                    self.textures.as_deref(),
                    3,
                    r,
                    FloatColor4::from_rgba(1.0, 1.0, 1.0, flash),
                    content,
                );
            }
        }
    }
    pub fn render(&self, pass: &mut RenderPass, confirmation: &Confirmation) {
        let opacity = confirmation.opacity.value();
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        QuadVertices::new()
            .with_box(0.0, 0.0, 1920.0, 1080.0)
            .with_color(FloatColor4::from_rgba(0.0, 0.0, 0.0, 0.5 * opacity).into_unorm())
            .fill(pass, builder, top_left_projection_matrix());
        let g = &self.geometry;
        QuadVertices::new()
            .with_box(0.0, 0.0, g.outer.x, g.outer.y)
            .with_tex_box(
                0.0,
                0.0,
                g.outer.x / g.outer.x.ceil(),
                g.outer.y / g.outer.y.ceil(),
            )
            .with_color(FloatColor4::from_rgba(1.0, 1.0, 1.0, opacity).into_unorm())
            .render_sprite(
                pass,
                builder,
                TextureSource {
                    view: &self.view,
                    sampler: TextureSampler::Linear,
                },
                g.transform(confirmation.scale.value()),
            );
    }
    #[cfg(test)]
    pub(crate) fn reference_uses_plain_frame(&self) -> bool {
        self.textures.is_none()
    }
}

#[cfg(test)]
mod tests;
