//! Native generic Yes/No dialog geometry and offscreen composition.
use super::{
    backlog_rollback::{ANSWERS, Confirmation, PROMPT},
    character_text::CharacterTextGlyph,
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
pub(super) struct DialogTextures {
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
pub(super) struct DialogGeometry {
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

pub(super) struct DialogView {
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
mod tests {
    use super::*;
    // The1920 helper uses ORR W0,WZR,#0x780; the other three use MOVZ.
    // Keep this decoder restricted to these verified constant-return forms.
    fn native_constant_return(exe: &[u8], address: usize) -> f32 {
        let instruction = u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap());
        match instruction {
            0x32190fe0 => 0xfu32.rotate_right(25) as f32,
            encoded if encoded & 0xffe0001f == 0x52800000 => ((encoded >> 5) & 0xffff) as f32,
            _ => panic!("Unverified native constant return {address:x}: {instruction:x}"),
        }
    }
    fn sample_rgba(bytes: &[u8], width: usize, height: usize, point: Vec2) -> [f32; 4] {
        let x = point.x - 0.5;
        let y = point.y - 0.5;
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        let fx = x - ix as f32;
        let fy = y - iy as f32;
        std::array::from_fn(|channel| {
            let p = |dx: i32, dy: i32| {
                bytes[((iy + dy).clamp(0, height as i32 - 1) as usize * width
                    + (ix + dx).clamp(0, width as i32 - 1) as usize)
                    * 4
                    + channel] as f32
                    / 255.0
            };
            (p(0, 0) * (1.0 - fx) + p(1, 0) * fx) * (1.0 - fy)
                + (p(0, 1) * (1.0 - fx) + p(1, 1) * fx) * fy
        })
    }
    #[test]
    fn fixed_native_framebuffer_extent_and_host_input_scale_are_independent() {
        let mut g = DialogGeometry::new(vec2(762.0, 130.0), [vec2(94.0, 58.0), vec2(138.0, 58.0)]);
        assert_eq!(g.texture_extent(), [838, 296]);
        g.outer = vec2(838.3, 296.25);
        assert_eq!(g.texture_extent(), [839, 297]);
        for (canvas, scale) in [
            (
                CanvasSize {
                    width: 1920,
                    height: 1080,
                },
                1.0,
            ),
            (
                CanvasSize {
                    width: 1280,
                    height: 720,
                },
                1.5,
            ),
            (
                CanvasSize {
                    width: 960,
                    height: 540,
                },
                2.0,
            ),
        ] {
            assert_eq!(
                DialogGeometry::pointer_scale(canvas),
                Some(Vec2::splat(scale))
            );
            assert_eq!(g.texture_extent(), [839, 297]);
        }
        assert_eq!(
            DialogGeometry::pointer_scale(CanvasSize {
                width: 0,
                height: 0
            }),
            None
        );
    }
    #[test]
    fn native_horizontal_and_vertical_button_layout_and_center_offset() {
        let g = DialogGeometry::new(vec2(800.0, 130.0), [vec2(94.0, 58.0), vec2(138.0, 58.0)]);
        assert!(!g.vertical);
        assert_eq!(g.inner, vec2(800.0, 220.0));
        assert_eq!(g.outer, vec2(876.0, 296.0));
        assert_eq!(g.origin, vec2(530.0, 400.0));
        assert_eq!(
            g.buttons,
            [
                vec4(167.0, 138.0, 218.0, 82.0),
                vec4(415.0, 138.0, 218.0, 82.0)
            ]
        );
        let g = DialogGeometry::new(vec2(100.0, 80.0), [vec2(400.0, 50.0), vec2(500.0, 50.0)]);
        assert!(g.vertical);
        assert_eq!(g.inner, vec2(600.0, 244.0));
        assert_eq!(
            g.buttons,
            [
                vec4(10.0, 88.0, 580.0, 74.0),
                vec4(10.0, 170.0, 580.0, 74.0)
            ]
        );
        let pixel = top_left_projection_matrix().inverse()
            * g.transform(0.5)
            * Vec4::new(g.outer.x * 0.5, g.outer.y * 0.5, 0.0, 1.0);
        assert!((pixel.truncate().truncate() - vec2(968.0, 548.0)).length() < 0.001);
        let g = DialogGeometry::new(vec2(800.0, 130.0), [vec2(94.0, 38.0), vec2(138.0, 58.0)]);
        assert_eq!(g.buttons[0], vec4(167.0, 148.0, 218.0, 62.0));
        let g = DialogGeometry::new(vec2(100.0, 80.0), [vec2(400.0, 30.0), vec2(500.0, 50.0)]);
        assert_eq!(g.inner, vec2(600.0, 224.0));
        assert_eq!(
            g.buttons,
            [
                vec4(10.0, 88.0, 580.0, 54.0),
                vec4(10.0, 150.0, 580.0, 74.0)
            ]
        );
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_dialog_geometry_palette_and_composition() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        // Constructor, layout, palette initializer, focus rates, offscreen draw
        // order, centered scale transform, normalized texture UVs and blend.
        for (address, expected) in [
            (0x22534, 0xd2a89688),
            (0x22538, 0xd2a80809),
            (0x22540, 0xf2e84b08),
            (0x22548, 0xd2a81008),
            (0x22550, 0xf2e81008),
            (0x22638, 0x52a84608),
            (0x22ad4, 0x52a882c8),
            (0x22b04, 0xbd48150f),
            (0x22ba0, 0x1e2d1001),
            (0x22bb0, 0x54000469),
            (0x22bbc, 0x1a96c116),
            (0x22c00, 0x1e2e2800),
            (0x22c18, 0x1e341000),
            (0x22c64, 0x1e27d002),
            (0x22c68, 0x1e225902),
            (0x22c90, 0x1e2c0800),
            (0x22d4c, 0x1e27d000),
            (0x22d78, 0xbd490500),
            (0x23b48, 0xbd457101),
            (0x24a2c, 0xbd457101),
            (0x24a58, 0x1e2e1000),
            (0x24a5c, 0x4ea01c01),
            (0x24f9c, 0x9104a273),
            (0x24fbc, 0x4eaa1d43),
            (0x25010, 0xbd45a50a),
            (0x25040, 0xbd497500),
            (0x2504c, 0xbd49c901),
            (0x25058, 0xbd487d02),
            (0x25060, 0x4ea81d03),
            (0x250a8, 0x1e2c1003),
            (0x24120, 0x9400011c),
            (0x24124, 0x1e27d000),
            (0x24154, 0x9403741f),
            (0x2417c, 0x320003e4),
            (0x24180, 0x94000104),
            (0x2418c, 0x94037411),
            (0x241e0, 0x321f03e4),
            (0x241ec, 0x940000e9),
            (0x24220, 0x320007e4),
            (0x2422c, 0x940000d9),
            (0x242bc, 0x1e210800),
            (0x242cc, 0x9401f759),
            (0x24314, 0x9401f757),
            (0x24364, 0x9401f733),
            (0x243b8, 0x9401f576),
            (0x24460, 0x1e200900),
            (0x244d8, 0x1e24c001),
            (0x244dc, 0x1e211800),
            (0x244e4, 0x1e24c022),
            (0x244e8, 0x1e221821),
            (0x24520, 0x321d03e0),
            (0x24528, 0x321f03e0),
            (0x245b4, 0x91115108),
            (0x245b8, 0x937c7c89),
            (0x245fc, 0x9113514a),
        ] {
            assert_eq!(word(address), expected, "native dialog {address:x}");
        }
        for (address, value) in [
            (0x159438, 600.0f32),
            (0x159814, 180.0),
            (0x159904, 76.0),
            (0x1594fc, 0.8),
            (0x1595a4, 0.7),
            (0x159974, 0.2),
            (0x1599c8, 0.5725),
            (0x15987c, 0.957),
            (0x159570, 0.1),
        ] {
            assert_eq!(
                word(address),
                value.to_bits(),
                "native dialog constant {address:x}"
            );
        }
        let tables = [
            [[16.0f32, 32.0, 16.0, 32.0], [8.0; 4], [8.0; 4], [8.0; 4]],
            [
                [0.0, 0.25, 0.5, 1.0],
                [0.5, 0.625, 0.75, 0.875],
                [0.0, 0.125, 0.25, 0.375],
                [0.5, 0.625, 0.75, 0.875],
            ],
            [
                [0.0, 0.25, 0.5, 1.0],
                [0.0, 0.25, 0.5, 0.75],
                [0.0, 0.25, 0.5, 0.75],
                [0.0, 0.25, 0.5, 0.75],
            ],
        ];
        for (base, table) in [0x165454, 0x165494, 0x1654d4].into_iter().zip(tables) {
            for (index, value) in table.into_iter().flatten().enumerate() {
                assert_eq!(
                    word(base + index * 4),
                    value.to_bits(),
                    "native dialog patch {base:x}+{index}"
                );
            }
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_dialog_fixed_framebuffer_allocation_and_uvs() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (address, expected) in [
            (0x22da4, 0x94001123),
            (0x22db0, 0x9400112c),
            (0x22db4, 0x1e290901),
            (0x22db8, 0x1e200940),
            (0x22dbc, 0x1e290020),
            (0x22dc0, 0x1e290001),
            (0x22dcc, 0x9402b5f5),
            (0x27238, 0x94035206),
            (0x2723c, 0x12003c08),
            (0x27240, 0x1e230100),
            (0x27248, 0xbd475901),
            (0x2724c, 0x1e211800),
            (0x27268, 0x940351fe),
            (0x2726c, 0x12003c08),
            (0x27270, 0x1e230100),
            (0x27278, 0xbd461501),
            (0x2727c, 0x1e211800),
            (0xfba50, 0x32190fe0),
            (0xfba54, 0xd65f03c0),
            (0xfba60, 0x52808700),
            (0xfba64, 0xd65f03c0),
            (0xfba70, 0x5280a000),
            (0xfba74, 0xd65f03c0),
            (0xfba80, 0x52805a00),
            (0xfba84, 0xd65f03c0),
            (0xd061c, 0xb9001002),
            (0xd0638, 0x29010408),
            (0xd0af0, 0x91006000),
            (0xd0b00, 0x9100c000),
            (0x240b4, 0xbd415001),
            (0x240b8, 0xbd415402),
            (0x240d8, 0x9401f6e2),
            (0x244c4, 0xb9415268),
            (0x244d0, 0xb9415669),
            (0x244d8, 0x1e24c001),
            (0x244dc, 0x1e211800),
            (0x244e4, 0x1e24c022),
            (0x244e8, 0x1e221821),
            (0x2450c, 0x9402b179),
            (0x24520, 0x321d03e0),
        ] {
            assert_eq!(
                word(address),
                expected,
                "native dialog framebuffer {address:x}"
            );
        }
        assert_eq!(f32::from_bits(word(0x159758)), 1920.0);
        assert_eq!(f32::from_bits(word(0x159614)), 1080.0);
        // These are the actual constant-return helpers called by27230/27260,
        // not the1280x720 physical-input display helpers fba70/fba80.
        let immediate = |a| native_constant_return(&exe, a);
        let outer = vec2(838.3, 296.25);
        assert_eq!(
            vec2(immediate(0xfba50), immediate(0xfba60)),
            vec2(1920.0, 1080.0)
        );
        assert_eq!(
            vec2(immediate(0xfba70), immediate(0xfba80)),
            vec2(1280.0, 720.0)
        );
        let extent =
            (outer * vec2(immediate(0xfba50) / 1920.0, immediate(0xfba60) / 1080.0)).ceil();
        assert_eq!(extent, vec2(839.0, 297.0));
    }

    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_dialog_patches_and_offscreen_transform() {
        use crate::asset::system::{
            AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache,
        };
        use shin_render::{
            dynamic_buffer::DynamicBuffer,
            pipelines::PipelineStorage,
            resize::{SurfaceResizeSource, ViewportParams},
            shaders::types::{
                buffer::BytesAddress,
                texture::{
                    DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind,
                },
            },
        };
        crate::init_test_task_pools();
        let instance = wgpu::Instance::default();
        let adapter =
            shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .unwrap();
        let (device, queue) = shin_tasks::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
                    adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
                } else {
                    wgpu::Features::empty()
                },
                ..Default::default()
            }))
        .unwrap();
        let mut io = LayeredAssetIo::new();
        io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
            .unwrap();
        let server = AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: crate::cli::Game::Umineko,
                higurashi_bustup_cache: Default::default(),
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        );
        let textures = server.load_sync::<DialogTextures>("/systex.txa").unwrap();
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            1920, 1080,
        )));
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Choice test depth"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: shin_render::DEPTH_STENCIL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&Default::default());
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Choice test target"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let samplers = TextureSamplerStore::new(&device);
        let mut pipelines = PipelineStorage::new(device.clone(), format);
        let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut context = PreRenderContext {
            breakup_texture: None,
            device: &device,
            queue: &queue,
            resize_source: &resize,
            sampler_store: &samplers,
            depth_stencil: DepthStencilTarget { view: &depth_view },
            pipeline_storage: &mut pipelines,
            dynamic_buffer: &mut dynamic,
            encoder: &mut encoder,
        };

        // A failed glyph must leave layout/healthy glyphs intact. Compare the
        // production renderer with an independently transparent A bitmap,
        // preserving the same authored ABA advances and B's visible pixels.
        let mut glyph_failure_frames = Vec::new();
        let mut expected_text_size = None;
        for corrupt_a in [false, true] {
            let font = crate::asset::font::tests::fixture(corrupt_a);
            let text = DialogText::load_with_width(&server, &font, "ABA", 32.0, 1000.0);
            if let Some(size) = expected_text_size {
                assert_eq!(text.size, size);
            } else {
                expected_text_size = Some(text.size);
            }
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    None,
                    "Glyph load failure rendering",
                );
                pass.clear(Some(UnormColor::BLACK), None, None);
                text.render_with_builder(
                    &mut pass,
                    top_left_projection_matrix()
                        * glam::Mat4::from_translation(glam::vec3(400.0, 100.0, 0.0)),
                    FloatColor4::WHITE,
                    FloatColor4::BLACK,
                    RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                );
            }
            glyph_failure_frames.push(crate::layer::user::readback_test_texture(
                &mut context,
                &target,
            ));
        }
        assert_eq!(
            glyph_failure_frames[0], glyph_failure_frames[1],
            "Failed A must draw like transparent A; B retains exact layout/pixels"
        );
        assert!(
            glyph_failure_frames[1]
                .chunks_exact(4)
                .filter(|pixel| pixel[0] > 0)
                .count()
                > 100,
            "Healthy B must remain visible after a glyph fails"
        );

        use shin_core::{
            format::rom::RomReader, primitives::stateless_reader::StatelessFile, time::Ticks,
        };
        use std::{fs::File, io::Read};
        let mut rom = RomReader::new(
            StatelessFile::new(File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file("/systex.txa").unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let atlas = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        let sample = |name: &str, u: f32, v: f32| -> [f32; 4] {
            let im = &atlas.textures[atlas.name_to_index[name]];
            let x = u * im.width() as f32 - 0.5;
            let y = v * im.height() as f32 - 0.5;
            let ix = x.floor();
            let iy = y.floor();
            let fx = x - ix;
            let fy = y - iy;
            std::array::from_fn(|channel| {
                let pixel = |dx: f32, dy: f32| {
                    im.get_pixel(
                        (ix + dx).clamp(0.0, im.width() as f32 - 1.0) as u32,
                        (iy + dy).clamp(0.0, im.height() as f32 - 1.0) as u32,
                    )[channel] as f32
                        / 255.0
                };
                (pixel(0.0, 0.0) * (1.0 - fx) + pixel(1.0, 0.0) * fx) * (1.0 - fy)
                    + (pixel(0.0, 1.0) * (1.0 - fx) + pixel(1.0, 1.0) * fx) * fy
            })
        };

        let exe = std::fs::read(
            std::path::Path::new(&std::env::var("SHIN_REFERENCE_ROM").unwrap())
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let f = |address: usize| {
            f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap())
        };
        let mut checked = 0;
        for kind in 0..4 {
            let r = vec4(300.0, 200.0, 401.0, 181.0);
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    None,
                    "Dialog native patches",
                );
                pass.clear(Some(UnormColor::BLACK), None, None);
                patch(
                    &mut pass,
                    &textures,
                    kind,
                    r,
                    FloatColor4::from_rgba(0.2, 0.5, 0.7, 0.75),
                    top_left_projection_matrix(),
                );
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            let borders: [f32; 4] = std::array::from_fn(|i| f(0x165454 + kind * 16 + i * 4));
            let xs = [r.x, r.x + borders[0], r.x + r.z - borders[1], r.x + r.z];
            let ys = [r.y, r.y + borders[2], r.y + r.w - borders[3], r.y + r.w];
            for y in 199..382 {
                for x in 299..702 {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let mut expected = [0.0; 3];
                    if let (Some(col), Some(row)) = (
                        (0..3).find(|&i| px >= xs[i] && px < xs[i + 1]),
                        (0..3).find(|&i| py >= ys[i] && py < ys[i + 1]),
                    ) {
                        let u0 = f(0x165494 + kind * 16 + col * 4);
                        let u1 = f(0x165494 + kind * 16 + (col + 1) * 4);
                        let v0 = f(0x1654d4 + kind * 16 + row * 4);
                        let v1 = f(0x1654d4 + kind * 16 + (row + 1) * 4);
                        let tex = sample(
                            if kind == 0 {
                                "dialog_frame"
                            } else {
                                "dialog_button"
                            },
                            u0 + (u1 - u0) * (px - xs[col]) / (xs[col + 1] - xs[col]),
                            v0 + (v1 - v0) * (py - ys[row]) / (ys[row + 1] - ys[row]),
                        );
                        let tint = [51.0 / 255.0, 127.0 / 255.0, 178.0 / 255.0];
                        for channel in 0..3 {
                            expected[channel] =
                                tex[channel] * tint[channel] * tex[3] * (191.0 / 255.0) * 255.0;
                        }
                    }
                    for channel in 0..3 {
                        let actual = pixels[(y * 1920 + x) * 4 + channel];
                        assert!(
                            (actual as f32 - expected[channel]).abs() <= 2.1,
                            "Dialog kind{kind} at{x},{y} channel{channel}: {actual} != {}",
                            expected[channel]
                        );
                    }
                    checked += 1;
                }
            }
        }
        let mut dialog = DialogView::load(&server).unwrap();
        assert!(dialog.geometry.outer.x > 676.0 && dialog.geometry.outer.y > 200.0);
        let mut confirmation = Confirmation::default();
        let mut input = super::super::dialog_input::DialogInput::default();
        input.set_pointer_scale(Vec2::ONE);
        let initial_origin = dialog.geometry.origin;
        let window_pointer = initial_origin + Vec2::splat(10.0);
        let drag = vec2(60.0, -40.0);
        let mut expected_origin = initial_origin;
        let mut expected_velocity = Vec2::ZERO;
        let mut button_pointer = Vec2::ZERO;
        for (frame, delta) in [
            6.0, 6.0, 0.0, 0.0, 0.0, 1.0, 0.5, 0.5, 0.0, 0.5, 3.5, 3.5, 0.0, 6.0, 6.0, 5.0,
        ]
        .into_iter()
        .enumerate()
        {
            let new_canvas = match frame {
                3 => Some((1280, 720)),
                7 => Some((2560, 1440)),
                12 => Some((960, 540)),
                15 => Some((1920, 1080)),
                _ => None,
            };
            if let Some((width, height)) = new_canvas {
                resize.resize(ViewportParams::both(winit::dpi::PhysicalSize::new(
                    width, height,
                )));
                input.set_pointer_scale(
                    DialogGeometry::pointer_scale(resize.canvas_handle().get()).unwrap(),
                );
            }
            if frame == 3 {
                dialog.geometry.outer += vec2(0.3, 0.25);
            }
            match frame {
                3 => input.pointer(
                    &mut confirmation,
                    &mut dialog.geometry,
                    window_pointer,
                    true,
                ),
                4 => {
                    input.pointer(
                        &mut confirmation,
                        &mut dialog.geometry,
                        window_pointer + drag,
                        true,
                    );
                    expected_origin += drag;
                }
                5 => {
                    input.pointer_timed(
                        &mut confirmation,
                        &mut dialog.geometry,
                        window_pointer + drag,
                        false,
                        0.016683333333333335,
                    );
                    expected_velocity = drag;
                }
                7 => {
                    let p = dialog.geometry.origin + Vec2::splat(10.0);
                    input.pointer(&mut confirmation, &mut dialog.geometry, p, true);
                    expected_velocity = Vec2::ZERO;
                }
                8 => {
                    let p = dialog.geometry.origin + Vec2::splat(10.0);
                    input.pointer(&mut confirmation, &mut dialog.geometry, p, false);
                }
                9 => {
                    let rect = dialog.geometry.buttons[0];
                    button_pointer = dialog.geometry.origin
                        + vec2(30.0, 46.0)
                        + vec2(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
                    input.pointer(
                        &mut confirmation,
                        &mut dialog.geometry,
                        button_pointer,
                        true,
                    );
                    assert_eq!(confirmation.selected(), Some(0));
                }
                10 => {
                    input.pointer(
                        &mut confirmation,
                        &mut dialog.geometry,
                        button_pointer,
                        false,
                    );
                    assert!(!confirmation.is_open());
                    assert_eq!(input.take_sounds(), vec!["decide"]);
                }
                _ => {}
            }
            expected_origin += expected_velocity;
            expected_velocity *= f(0x15957c).powf(delta);
            confirmation.update(Ticks::from_f32(delta));
            input.update_motion(&mut dialog.geometry, Ticks::from_f32(delta));
            assert!((dialog.geometry.origin - expected_origin).length() < 0.001);
            dialog.prepare(&mut context, &confirmation);
            let native_framebuffer = |a| native_constant_return(&exe, a);
            let expected_extent = [
                (dialog.geometry.outer.x * (native_framebuffer(0xfba50) / f(0x159758))).ceil()
                    as u32,
                (dialog.geometry.outer.y * (native_framebuffer(0xfba60) / f(0x159614))).ceil()
                    as u32,
            ];
            assert_eq!(
                [dialog.texture.width(), dialog.texture.height()],
                expected_extent
            );
            let source = crate::layer::user::readback_test_texture(&mut context, &dialog.texture);
            // Compare untitled interior pixels independently against native
            // palette floats and atlas samples, including partial focus/flash.
            let native_patch = |kind: usize, rect: Vec4, point: Vec2| {
                let borders: [f32; 4] = std::array::from_fn(|i| f(0x165454 + kind * 16 + i * 4));
                let xs = [
                    rect.x,
                    rect.x + borders[0],
                    rect.x + rect.z - borders[1],
                    rect.x + rect.z,
                ];
                let ys = [
                    rect.y,
                    rect.y + borders[2],
                    rect.y + rect.w - borders[3],
                    rect.y + rect.w,
                ];
                let col = (0..3)
                    .find(|&i| point.x >= xs[i] && point.x < xs[i + 1])
                    .unwrap();
                let row = (0..3)
                    .find(|&i| point.y >= ys[i] && point.y < ys[i + 1])
                    .unwrap();
                let u0 = f(0x165494 + kind * 16 + col * 4);
                let u1 = f(0x165494 + kind * 16 + (col + 1) * 4);
                let v0 = f(0x1654d4 + kind * 16 + row * 4);
                let v1 = f(0x1654d4 + kind * 16 + (row + 1) * 4);
                sample(
                    if kind == 0 {
                        "dialog_frame"
                    } else {
                        "dialog_button"
                    },
                    u0 + (u1 - u0) * (point.x - xs[col]) / (xs[col + 1] - xs[col]),
                    v0 + (v1 - v0) * (point.y - ys[row]) / (ys[row + 1] - ys[row]),
                )
            };
            for index in 0..2 {
                let rect = dialog.geometry.buttons[index];
                let outer = dialog.geometry.outer;
                let physical_size = vec2(
                    dialog.texture.width() as f32,
                    dialog.texture.height() as f32,
                );
                let desired = vec2(rect.x + 20.5, rect.y + 20.5) + Vec2::splat(30.0);
                let pixel = (desired / outer * physical_size).floor();
                let texture_point = (pixel + Vec2::splat(0.5)) / physical_size * outer;
                let point = texture_point - Vec2::splat(30.0);
                let mut expected = native_patch(0, vec4(0.0, 0.0, outer.x, outer.y), texture_point)
                    .map(|component| (component * 255.0).round());
                let p = confirmation.focus[index].value();
                let mut stages = vec![(
                    1,
                    [
                        f(0x1595a4) * p,
                        f(0x1595a4) * p,
                        f(0x1595a4) * p,
                        0.5 + (f(0x1595a4) - 0.5) * p,
                    ],
                )];
                if confirmation.selected() == Some(index) {
                    stages.push((2, [1.0; 4]));
                }
                let flash = confirmation.flashes[index].value();
                if flash > 0.0 {
                    stages.push((3, [1.0, 1.0, 1.0, flash]));
                }
                for (kind, color) in stages {
                    let tex = native_patch(kind, rect, point);
                    let color = color.map(|c| (c * 255.0) as u8 as f32 / 255.0);
                    let alpha = tex[3] * color[3];
                    for channel in 0..3 {
                        expected[channel] = (tex[channel] * color[channel] * alpha * 255.0
                            + expected[channel] * (1.0 - alpha))
                            .round();
                    }
                }
                let offset =
                    (pixel.y as usize * dialog.texture.width() as usize + pixel.x as usize) * 4;
                for channel in 0..3 {
                    assert!(
                        (source[offset + channel] as f32 - expected[channel]).abs() <= 2.1,
                        "Dialog button{index} frame{frame}: {} != {}",
                        source[offset + channel],
                        expected[channel]
                    );
                }
                checked += 1;
            }
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    None,
                    "Composed dialog",
                );
                pass.clear(Some(UnormColor::from_rgba(128, 128, 128, 255)), None, None);
                dialog.render(&mut pass, &confirmation);
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            let g = &dialog.geometry;
            let scale = confirmation.scale.value();
            let opacity = (confirmation.opacity.value() * 255.0) as u8 as f32 / 255.0;
            let background =
                128.0 * (1.0 - ((confirmation.opacity.value() * 0.5 * 255.0) as u8 as f32 / 255.0));
            let center = g.origin + g.outer * 0.5;
            let width = dialog.texture.width() as usize;
            let height = dialog.texture.height() as usize;
            let sample_source = |point: Vec2| {
                sample_rgba(
                    &source,
                    width,
                    height,
                    point / g.outer.ceil() * vec2(width as f32, height as f32),
                )
            };
            for y in (3..1080).step_by(11) {
                for x in (3..1920).step_by(13) {
                    let local =
                        (vec2(x as f32 + 0.5, y as f32 + 0.5) - center) / scale + g.outer * 0.5;
                    if local.x.abs() < 2.0
                        || local.y.abs() < 2.0
                        || (local.x - g.outer.x).abs() < 2.0
                        || (local.y - g.outer.y).abs() < 2.0
                    {
                        continue;
                    }
                    let inside = local.x >= 0.0
                        && local.x < g.outer.x
                        && local.y >= 0.0
                        && local.y < g.outer.y;
                    let tex = if inside {
                        sample_source(local)
                    } else {
                        [0.0; 4]
                    };
                    for channel in 0..3 {
                        let expected = tex[channel] * 255.0 * tex[3] * opacity
                            + background * (1.0 - tex[3] * opacity);
                        let actual = pixels[(y * 1920 + x) * 4 + channel] as f32;
                        assert!(
                            (actual - expected).abs() <= 2.1,
                            "Dialog composition at{x},{y}: {actual} != {expected}"
                        );
                    }
                    checked += 1;
                }
            }
            if frame == 2 || frame == 6 {
                let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
                for pixel in pixels.chunks_exact(4) {
                    ppm.extend_from_slice(&pixel[..3]);
                }
                std::fs::write(
                    if frame == 2 {
                        "/tmp/shin-dialog-native.ppm"
                    } else {
                        "/tmp/shin-dialog-dragged.ppm"
                    },
                    ppm,
                )
                .unwrap();
            }
        }
        let before = crate::layer::user::readback_test_texture(&mut context, &dialog.texture);
        let dimensions = [dialog.texture.width(), dialog.texture.height()];
        resize.resize(ViewportParams::both(winit::dpi::PhysicalSize::new(0, 0)));
        dialog.prepare(&mut context, &confirmation);
        assert_eq!(
            [dialog.texture.width(), dialog.texture.height()],
            dimensions
        );
        assert_eq!(
            crate::layer::user::readback_test_texture(&mut context, &dialog.texture),
            before
        );
        resize.resize(ViewportParams::with_aspect_ratio(
            winit::dpi::PhysicalSize::new(2048, 1200),
            16.0 / 9.0,
        ));
        dialog.prepare(&mut context, &confirmation);
        assert_eq!(
            [dialog.texture.width(), dialog.texture.height()],
            dialog.geometry.texture_extent()
        );
        // Native handheld display is1280x720, while the cached dialog and
        // rendering coordinates remain at the1920x1080 framebuffer resolution.
        let display = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Dialog handheld output"),
            size: wgpu::Extent3d {
                width: 1280,
                height: 720,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let display_view = display.create_view(&Default::default());
        resize.resize(ViewportParams::both(winit::dpi::PhysicalSize::new(
            1280, 720,
        )));
        let mut handheld = Confirmation::default();
        for delta in [6.0, 6.0, 0.0] {
            handheld.update(Ticks::from_f32(delta));
            dialog.prepare(&mut context, &handheld);
            assert_eq!(
                [dialog.texture.width(), dialog.texture.height()],
                [839, 297]
            );
            let source = crate::layer::user::readback_test_texture(&mut context, &dialog.texture);
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &display_view,
                    },
                    None,
                    "Dialog handheld final composition",
                );
                pass.clear(Some(UnormColor::from_rgba(128, 128, 128, 255)), None, None);
                dialog.render(&mut pass, &handheld);
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &display);
            let g = &dialog.geometry;
            let center = g.origin + g.outer * 0.5;
            let opacity = (handheld.opacity.value() * 255.0) as u8 as f32 / 255.0;
            let background =
                128.0 * (1.0 - (handheld.opacity.value() * 0.5 * 255.0) as u8 as f32 / 255.0);
            for y in (3..720).step_by(7) {
                for x in (3..1280).step_by(9) {
                    let virtual_point = vec2((x as f32 + 0.5) * 1.5, (y as f32 + 0.5) * 1.5);
                    let local = (virtual_point - center) / handheld.scale.value() + g.outer * 0.5;
                    if local.x.abs() < 2.0
                        || local.y.abs() < 2.0
                        || (local.x - g.outer.x).abs() < 2.0
                        || (local.y - g.outer.y).abs() < 2.0
                    {
                        continue;
                    }
                    let inside = local.x >= 0.0
                        && local.y >= 0.0
                        && local.x < g.outer.x
                        && local.y < g.outer.y;
                    let tex = if inside {
                        sample_rgba(
                            &source,
                            839,
                            297,
                            local / g.outer.ceil() * vec2(839.0, 297.0),
                        )
                    } else {
                        [0.0; 4]
                    };
                    for channel in 0..3 {
                        let expected = tex[channel] * 255.0 * tex[3] * opacity
                            + background * (1.0 - tex[3] * opacity);
                        assert!(
                            (pixels[(y * 1280 + x) * 4 + channel] as f32 - expected).abs() <= 2.1,
                            "Handheld composition {x},{y} channel{channel}"
                        );
                    }
                    checked += 1;
                }
            }
            if handheld.is_open() {
                let mut ppm = b"P6\n1280 720\n255\n".to_vec();
                for pixel in pixels.chunks_exact(4) {
                    ppm.extend_from_slice(&pixel[..3]);
                }
                std::fs::write("/tmp/shin-dialog-handheld.ppm", ppm).unwrap();
            }
        }
        assert_eq!(
            confirmation.take_outcome(),
            Some(super::super::backlog_rollback::Outcome::Yes)
        );
        assert_eq!(confirmation.take_outcome(), None);
        println!(
            "Native dialog: {checked} patch and offscreen scale/opacity samples; geometry {:?}",
            dialog.geometry
        );
    }
}
