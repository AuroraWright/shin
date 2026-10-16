//! Native BGM title overlay (0x9e200..0x9e838).
use super::characters::text::CharacterTextGlyph;
use crate::asset::{
    font::{GpuFontGlyphHandle, GpuFontLazy},
    system::AssetServer,
};
use glam::{Mat4, Vec2, vec2, vec4};
use shin_core::{
    layout::{
        LayoutParams, MessageTextLayouter, MessageTextLayouterDefaults, commands::Command,
        font::FontMetrics,
    },
    primitives::color::FloatColor4,
    time::{Ticks, Tween, Tweener},
};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::TextVertex},
};
use std::sync::Arc;

pub(super) struct TitleText {
    glyphs: Vec<CharacterTextGlyph>,
    handles: Vec<GpuFontGlyphHandle>,
    size: Vec2,
}
impl TitleText {
    fn load(server: &AssetServer, font: &Arc<GpuFontLazy>, text: &str) -> Self {
        Self::load_sized(server, font, text, 36.0)
    }
    pub(super) fn load_sized(
        server: &AssetServer,
        font: &Arc<GpuFontLazy>,
        text: &str,
        text_size: f32,
    ) -> Self {
        Self::load_gpu(
            &server.context().wgpu_device,
            &server.context().wgpu_queue,
            font,
            text,
            text_size,
            1920.0,
        )
    }
    pub(super) fn size(&self) -> Vec2 {
        self.size
    }
    pub(super) fn load_gpu(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font: &Arc<GpuFontLazy>,
        text: &str,
        text_size: f32,
        width: f32,
    ) -> Self {
        let (commands, _, size) = MessageTextLayouter::new(
            font.as_ref(),
            font.as_ref(),
            LayoutParams {
                layout_width: width,
                text_size,
                rubi_size: text_size * 0.4,
                perform_soft_breaks: false,
                always_leave_space_for_rubi: false,
                ..Default::default()
            },
            MessageTextLayouterDefaults {
                color: 999,
                draw_speed: 50,
                fade: 200,
            },
        )
        .parse(text);
        let mut glyphs = Vec::new();
        for command in commands {
            let Command::Char(ch) = command else {
                continue;
            };
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
        let handles = font
            .clone()
            .load_glyphs(device.clone(), queue.clone(), &points);
        Self {
            glyphs,
            handles,
            size: size + Vec2::splat(6.0),
        }
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        position: Vec2,
        anchor: Vec2,
        scale: Vec2,
        opacity: f32,
    ) {
        self.render_colored(
            pass,
            position,
            anchor,
            scale,
            opacity,
            vec4(0.75, 1.0, 1.0, 1.0),
        );
    }
    pub(super) fn render_colored(
        &self,
        pass: &mut RenderPass,
        position: Vec2,
        anchor: Vec2,
        scale: Vec2,
        opacity: f32,
        color: glam::Vec4,
    ) {
        let transform = crate::render::top_left_projection_matrix()
            * Mat4::from_translation(position.extend(0.0))
            * Mat4::from_scale(scale.extend(1.0))
            * Mat4::from_translation((-self.size * anchor).extend(0.0));
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        for border in [true, false] {
            for (g, handle) in self.glyphs.iter().zip(&self.handles) {
                let Ok(glyph_texture) = handle.as_texture_source() else {
                    continue;
                };
                let r = g.rect;
                let uv = g.uv;
                let vertices = [
                    vec4(r.x, r.y, uv.x, uv.y),
                    vec4(r.x + r.z, r.y, uv.z, uv.y),
                    vec4(r.x, r.y + r.w, uv.x, uv.w),
                    vec4(r.x + r.z, r.y + r.w, uv.z, uv.w),
                ]
                .map(|position| TextVertex {
                    position,
                    color: 0.0,
                });
                let vertices = VertexSource::VertexData {
                    vertices: &vertices,
                };
                let program = if border {
                    RenderProgramWithArguments::FontBorder {
                        vertices,
                        glyph: glyph_texture,
                        transform,
                        distances: g.distances,
                        color: FloatColor4::from_rgba(0.0, 0.0, 0.0, opacity),
                    }
                } else {
                    RenderProgramWithArguments::Font {
                        vertices,
                        glyph: glyph_texture,
                        transform,
                        color1: FloatColor4::from_rgba(color.x, color.y, color.z, opacity),
                        color2: FloatColor4::from_rgba(color.x, color.y, color.z, opacity),
                    }
                };
                pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
            }
        }
    }
}

pub(super) struct BgmTitle {
    title: Option<TitleText>,
    note: Option<TitleText>,
    motion: TitleMotion,
    enabled: bool,
}
#[derive(Clone)]
struct TitleMotion {
    slide: Tweener,
    opacity: f32,
    elapsed: f32,
    hidden: bool,
}
impl Default for TitleMotion {
    fn default() -> Self {
        Self {
            slide: Tweener::new(0.0),
            opacity: 1.0,
            elapsed: 0.0,
            hidden: false,
        }
    }
}
impl TitleMotion {
    fn start(&mut self, height: f32) {
        let extent = height + 4.0;
        self.slide = Tweener::new(0.0);
        self.slide
            .enqueue(extent, Tween::linear(Ticks::from_f32(extent / 2.0)));
        self.slide
            .enqueue(extent, Tween::linear(Ticks::from_f32(180.0)));
        self.slide
            .enqueue(0.0, Tween::linear(Ticks::from_f32(extent / 2.0)));
        self.elapsed = 0.0;
    }
    fn cancel(&mut self) {
        if !self.slide.is_idle() {
            let duration = Ticks::from_f32(self.slide.value().abs() / 2.0);
            self.slide.enqueue_now(0.0, Tween::linear(duration));
        }
    }
    fn update(&mut self, delta: Ticks) {
        let step = delta.as_f32() * 0.1;
        self.opacity = (self.opacity + if self.hidden { -step } else { step }).clamp(0.0, 1.0);
        if self.opacity >= 1.0 {
            self.slide.update(delta);
            self.elapsed += delta.as_f32();
        }
    }
    fn note_pose(&self) -> (Vec2, f32) {
        let sine = (self.elapsed / 40.0 * f32::from_bits(0x40490fd0)).sin();
        if sine < 0.0 {
            (
                vec2(1.0 + sine * (-0.4), 1.0 + sine * 0.3),
                self.slide.value(),
            )
        } else {
            (Vec2::ONE, self.slide.value() - 8.0 * sine)
        }
    }
}
impl BgmTitle {
    #[cfg(test)]
    pub(super) fn reference_motion(&self) -> (f32, f32, f32, bool) {
        (
            self.motion.slide.value(),
            self.motion.opacity,
            self.motion.elapsed,
            self.motion.slide.is_idle(),
        )
    }
    pub fn new() -> Self {
        Self {
            title: None,
            note: None,
            motion: TitleMotion::default(),
            enabled: true,
        }
    }
    pub fn set_hidden(&mut self, hidden: bool) {
        self.motion.hidden = hidden;
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    pub fn show(&mut self, server: &AssetServer, name: &str) {
        if name.is_empty() {
            return;
        }
        let font: Arc<GpuFontLazy> = server.load_sync("/system.fnt").unwrap();
        self.title = Some(TitleText::load(server, &font, name));
        self.note = Some(TitleText::load(server, &font, "♪"));
        self.motion.start(self.note.as_ref().unwrap().size.y);
    }
    /// Native saved/SceneJump loads retain glyph ownership while replacing
    /// the current motion with its exit. A newly reconstructed label wins.
    pub(super) fn restore_previous_for_exit(&mut self, previous: &mut Self) {
        if self.title.is_none() {
            let enabled = self.enabled;
            *self = std::mem::replace(previous, Self::new());
            self.enabled = enabled;
            self.motion.cancel();
        }
    }
    pub fn update(&mut self, delta: Ticks) {
        self.motion.update(delta);
    }
    pub fn render(&self, pass: &mut RenderPass) {
        if !self.enabled || self.motion.opacity <= 0.0 || self.motion.slide.is_idle() {
            return;
        }
        let (Some(title), Some(note)) = (&self.title, &self.note) else {
            return;
        };
        title.render(
            pass,
            vec2(note.size.x + 4.0, self.motion.slide.value()),
            vec2(0.0, 1.0),
            Vec2::ONE,
            self.motion.opacity,
        );
        let (scale, y) = self.motion.note_pose();
        note.render(
            pass,
            vec2(note.size.x * 0.5 + 4.0, y),
            vec2(0.5, 1.0),
            scale,
            self.motion.opacity,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_exits_from_current_value_and_discards_hold_queue() {
        let mut motion = TitleMotion::default();
        motion.start(40.0);
        motion.update(Ticks::from_f32(10.0));
        let before = motion.slide.value();
        assert!(before > 0.0);
        motion.cancel();
        assert_eq!(motion.slide.value(), before);
        let duration = before / 2.0;
        motion.update(Ticks::from_f32(duration / 2.0));
        assert!((motion.slide.value() - before / 2.0).abs() < 0.00001);
        motion.cancel();
        motion.update(Ticks::from_f32(duration / 2.0 + 1.0));
        assert_eq!(motion.slide.value(), 0.0);
        assert!(motion.slide.is_idle());
        motion.cancel();
        motion.update(Ticks::from_f32(400.0));
        assert_eq!(motion.slide.value(), 0.0);
        assert!(motion.slide.is_idle());
        motion.start(40.0);
        motion.cancel();
        assert!(
            motion.slide.is_idle(),
            "cancel at zero must not enter the hold queue"
        );
        let mut previous = BgmTitle::new();
        previous.set_enabled(false);
        let mut restored = BgmTitle::new();
        restored.set_enabled(true);
        restored.restore_previous_for_exit(&mut previous);
        assert!(
            restored.enabled,
            "reconstructed settings must override retired display settings"
        );
    }

    #[test]
    fn native_title_motion_holds_and_pauses_while_hidden() {
        let mut motion = TitleMotion::default();
        motion.start(40.0);
        motion.update(Ticks::from_f32(11.0));
        assert_eq!(motion.slide.value(), 22.0);
        motion.update(Ticks::from_f32(11.0));
        assert_eq!(motion.slide.value(), 44.0);
        motion.hidden = true;
        motion.update(Ticks::from_f32(5.0));
        assert_eq!(motion.opacity, 0.5);
        assert_eq!(motion.slide.value(), 44.0);
        assert_eq!(motion.elapsed, 22.0);
        motion.hidden = false;
        motion.update(Ticks::from_f32(5.0));
        assert_eq!(motion.opacity, 1.0);
        assert_eq!(motion.elapsed, 27.0);
        motion.update(Ticks::from_f32(175.0));
        assert_eq!(motion.slide.value(), 44.0);
        motion.update(Ticks::from_f32(11.0));
        assert_eq!(motion.slide.value(), 22.0);
        motion.update(Ticks::from_f32(11.0));
        assert!(motion.slide.is_idle());
        assert_eq!(motion.slide.value(), 0.0);
        motion.elapsed = 20.0;
        let (scale, y) = motion.note_pose();
        assert_eq!(scale, Vec2::ONE);
        assert!((y + 8.0).abs() < 0.00001);
        motion.elapsed = 60.0;
        let (scale, y) = motion.note_pose();
        assert!((scale.x - 1.4).abs() < 0.00001);
        assert!((scale.y - 0.7).abs() < 0.00001);
        assert_eq!(y, 0.0);
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_bgm_title_setting_animation_and_note() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        assert_eq!(word(0x159880), 0x40490fd0);
        assert_eq!(word(0x159814), 180.0f32.to_bits());
        assert_eq!(word(0x159570), 0.1f32.to_bits());
        assert_eq!(&exe[0x161373 + 256..0x161373 + 259], "♪".as_bytes());
        for (a, expected) in [
            (0x9e554, 0x94007527),
            (0x9e558, 0x360000e0),
            (0x9e560, 0x1e201001),
            (0x9e564, 0x52802801),
            (0x9e570, 0x94007524),
            (0x1ab04, 0x94020e8f),
            (0x126ec, 0x9401478d),
            (0x126f4, 0x94022fcf),
            (0x12c48, 0x91056260),
            (0x12c4c, 0x9403b5c9),
            (0x6c40, 0x360000a0),
            (0x6c50, 0x94025d6c),
            (0x9e248, 0x340011e8),
            (0x9e3e8, 0x1e2c1000),
            (0x9e434, 0x1e221000),
            (0x9e43c, 0x1e201009),
            (0x9e444, 0x321a03e1),
            (0x9e45c, 0x52821001),
            (0x9e474, 0x2a1f03e2),
            (0x9e47c, 0xbd017268),
            (0x9e480, 0xb901767f),
            (0x9e59c, 0x1e200900),
            (0x9e5ac, 0x34000148),
            (0x9e5fc, 0x540000e4),
            (0x9e608, 0x94007512),
            (0x9e614, 0xbd017660),
            (0x9e654, 0x3942fd08),
            (0x9e658, 0x34000f28),
            (0x9e678, 0x36000e20),
            (0x9e6bc, 0x1e2a2800),
            (0x9e6ec, 0x1e211800),
            (0x9e6f4, 0x1e210800),
            (0x9e730, 0x1e210800),
            (0x9e754, 0x1e210800),
            (0x9e774, 0xbd400688),
            (0x9e778, 0x1e2d1009),
        ] {
            assert_eq!(word(a), expected, "native BGM title{a:x}");
        }
    }
}

#[cfg(test)]
impl BgmTitle {
    pub(crate) fn assert_reference_render(
        context: &mut crate::render::PreRenderContext,
        server: &AssetServer,
        scenario: &shin_core::format::scenario::Scenario,
    ) {
        use shin_core::primitives::color::UnormColor;
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Native BGM title"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let mut draw = |title: &Self| {
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    None,
                    "Native BGM title",
                );
                pass.clear(Some(UnormColor::from_rgba(32, 64, 96, 255)), None, None);
                title.render(&mut pass);
            }
            crate::layer::user::readback_test_texture(context, &target)
        };
        let mut title = Self::new();
        let blank = draw(&title);
        let name = scenario
            .info_tables()
            .bgm_info
            .iter()
            .find(|b| !b.display_name.as_str().is_empty())
            .unwrap()
            .display_name
            .as_str();
        title.show(server, name);
        assert_eq!(draw(&title), blank, "New title starts above the viewport");
        let note_height = title.note.as_ref().unwrap().size.y;
        title.update(Ticks::from_f32((note_height + 4.0) / 2.0));
        let shown = draw(&title);
        let changed = shown
            .chunks_exact(4)
            .zip(blank.chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        assert!(changed > 100, "Real supplied title font must be visible");
        for (index, (a, b)) in shown.chunks_exact(4).zip(blank.chunks_exact(4)).enumerate() {
            if a != b {
                assert!(
                    index / 1920 < 96,
                    "Native title stays at the top of the screen"
                );
            }
        }
        let fills = shown
            .chunks_exact(4)
            .filter(|p| p[0] > 180 && p[1] > 245 && p[2] > 245)
            .count();
        assert!(
            fills > 20,
            "Native cyan-white glyph fill must survive its black outline"
        );
        title.set_enabled(false);
        assert_eq!(draw(&title), blank);
        title.set_enabled(true);
        assert_eq!(draw(&title), shown);
        title.show(server, "");
        assert_eq!(
            draw(&title),
            shown,
            "Empty display name must retain current title"
        );
        title.update(Ticks::from_f32(40.0));
        let animated = draw(&title);
        assert_ne!(
            animated, shown,
            "Native note bounce changes visible glyph pixels"
        );
        title.update(Ticks::from_f32(300.0));
        assert_eq!(draw(&title), blank);
        let escaped_name = scenario
            .info_tables()
            .bgm_info
            .iter()
            .find(|bgm| bgm.display_name.as_str().contains("@u229."))
            .unwrap()
            .display_name
            .as_str();
        assert_eq!(escaped_name, "百合の泪 -En liljas t@u229.rar-");
        let mut escaped = Self::new();
        escaped.show(server, escaped_name);
        assert!(
            escaped
                .title
                .as_ref()
                .unwrap()
                .glyphs
                .iter()
                .any(|glyph| glyph.codepoint == 'å')
        );
        let mut decoded = Self::new();
        decoded.show(server, "百合の泪 -En liljas tårar-");
        for overlay in [&mut escaped, &mut decoded] {
            let ticks = (overlay.note.as_ref().unwrap().size.y + 4.0) / 2.0;
            overlay.update(Ticks::from_f32(ticks));
        }
        let escaped_pixels = draw(&escaped);
        assert_ne!(escaped_pixels, blank, "Escaped music title must be visible");
        assert_eq!(
            escaped_pixels,
            draw(&decoded),
            "Authored Unicode escape must render identically to the native decoded title"
        );
        println!(
            "Authored BGM Unicode title {escaped_name:?}: full-frame equality with native decoded text passes"
        );
        println!(
            "Native BGM title{name:?}: {changed} visible pixels,{fills} filled pixels,note height{note_height}; setting suppression,note animation and expiry pass"
        );
    }
}
