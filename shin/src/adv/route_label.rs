//! Bottom-left chapter label shown by AUTOSAVE (native 0x7984/0x642e0).
use super::bgm_title::TitleText;
use crate::asset::{font::GpuFontLazy, system::AssetServer};
use glam::{Vec2, Vec4, vec2};
use shin_core::time::{Ticks, Tween, Tweener};
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

pub(super) struct RouteLabel {
    text: Option<TitleText>,
    motion: LabelMotion,
    enabled: bool,
}

#[cfg(test)]
impl RouteLabel {
    #[cfg(test)]
    pub(super) fn reference_motion(&self) -> (f32, f32, bool) {
        (
            self.motion.fade.value(),
            self.motion.opacity,
            self.motion.fade.is_idle(),
        )
    }
    pub(crate) fn assert_reference_render(
        mut label: Self,
        context: &mut crate::render::PreRenderContext,
        server: &AssetServer,
    ) {
        use shin_core::primitives::color::UnormColor;
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Native route label"),
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
        let mut draw = |label: &Self| {
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    None,
                    "Native route label",
                );
                pass.clear(Some(UnormColor::from_rgba(32, 64, 96, 255)), None, None);
                label.render(&mut pass);
            }
            crate::layer::user::readback_test_texture(context, &target)
        };
        let blank = draw(&Self::new());
        assert!(
            label.text.is_some(),
            "Live AUTOSAVE must install the chapter label"
        );
        assert_eq!(draw(&label), blank);
        label.update(Ticks::from_f32(10.0));
        let fading = draw(&label);
        label.update(Ticks::from_f32(30.0));
        let shown = draw(&label);
        let mut changed = 0;
        for (i, (p, b)) in shown.chunks_exact(4).zip(blank.chunks_exact(4)).enumerate() {
            if p != b {
                changed += 1;
                assert!(i / 1920 > 970, "Native route label stays at the bottom");
            }
        }
        assert!(
            changed > 1000,
            "Supplied system font must render the chapter label"
        );
        let white = shown
            .chunks_exact(4)
            .filter(|p| p[0] > 245 && p[1] > 245 && p[2] > 245)
            .count();
        assert!(white > 100, "Native route label uses white fill");
        assert_ne!(fading, shown, "Native fade must change real glyph pixels");
        label.set_enabled(false);
        assert_eq!(draw(&label), blank);
        label.update(Ticks::from_f32(10.0));
        label.set_enabled(true);
        assert_eq!(
            draw(&label),
            shown,
            "Setting disables drawing, not the hold timer"
        );
        label.show(server, "");
        label.update(Ticks::from_f32(40.0));
        assert_eq!(
            draw(&label),
            blank,
            "Empty chapter label replaces the prior label"
        );
        label.show(server, "10/4（土）8:00 新島空港到着");
        label.update(Ticks::from_f32(400.0));
        assert_eq!(draw(&label), blank);
        println!(
            "Native route label: {changed} bottom glyph pixels, {white} white pixels; fade, setting, empty replacement and expiry pass"
        );
    }
}
struct LabelMotion {
    fade: Tweener,
    opacity: f32,
    hidden: bool,
}
impl Default for LabelMotion {
    fn default() -> Self {
        Self {
            fade: Tweener::new(0.0),
            opacity: 1.0,
            hidden: false,
        }
    }
}
impl LabelMotion {
    fn start(&mut self) {
        self.fade = Tweener::new(0.0);
        let duration = Ticks::from_f32(1.0 / 0.03);
        self.fade.enqueue(1.0, Tween::linear(duration));
        self.fade
            .enqueue(1.0, Tween::linear(Ticks::from_f32(240.0)));
        self.fade.enqueue(0.0, Tween::linear(duration));
    }
    fn cancel(&mut self) {
        if !self.fade.is_idle() {
            let duration = Ticks::from_f32(self.fade.value().abs() / 0.03);
            self.fade.enqueue_now(0.0, Tween::linear(duration));
        }
    }
    fn update(&mut self, delta: Ticks) {
        let step = delta.as_f32() * 0.1;
        self.opacity = (self.opacity + if self.hidden { -step } else { step }).clamp(0.0, 1.0);
        if self.opacity >= 1.0 {
            self.fade.update(delta);
        }
    }
}
impl RouteLabel {
    pub fn new() -> Self {
        Self {
            text: None,
            motion: LabelMotion::default(),
            enabled: true,
        }
    }
    pub fn set_hidden(&mut self, hidden: bool) {
        self.motion.hidden = hidden;
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    pub fn show(&mut self, server: &AssetServer, text: &str) {
        let font: Arc<GpuFontLazy> = server.load_sync("/system.fnt").unwrap();
        self.text = Some(TitleText::load_sized(server, &font, text, 48.0));
        self.motion.start();
    }
    /// Native saved/SceneJump loads retain glyph ownership while replacing
    /// the current motion with its exit. A newly reconstructed label wins.
    pub(super) fn restore_previous_for_exit(&mut self, previous: &mut Self) {
        if self.text.is_none() {
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
        if !self.enabled || self.motion.fade.is_idle() {
            return;
        }
        if let Some(text) = &self.text {
            text.render_colored(
                pass,
                vec2(4.0, 1076.0),
                vec2(0.0, 1.0),
                Vec2::ONE,
                self.motion.fade.value() * self.motion.opacity,
                Vec4::ONE,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_exits_from_current_value_and_discards_hold_queue() {
        let mut motion = LabelMotion::default();
        motion.start();
        motion.update(Ticks::from_f32(10.0));
        let before = motion.fade.value();
        assert!(before > 0.0);
        motion.cancel();
        assert_eq!(motion.fade.value(), before);
        let duration = before / 0.03;
        motion.update(Ticks::from_f32(duration / 2.0));
        assert!((motion.fade.value() - before / 2.0).abs() < 0.00001);
        motion.cancel();
        motion.update(Ticks::from_f32(duration / 2.0 + 1.0));
        assert_eq!(motion.fade.value(), 0.0);
        assert!(motion.fade.is_idle());
        motion.cancel();
        motion.update(Ticks::from_f32(400.0));
        assert_eq!(motion.fade.value(), 0.0);
        assert!(motion.fade.is_idle());
        motion.start();
        motion.cancel();
        assert!(
            motion.fade.is_idle(),
            "cancel at zero must not enter the hold queue"
        );
        let mut previous = RouteLabel::new();
        previous.set_enabled(false);
        let mut restored = RouteLabel::new();
        restored.set_enabled(true);
        restored.restore_previous_for_exit(&mut previous);
        assert!(
            restored.enabled,
            "reconstructed settings must override retired display settings"
        );
    }

    #[test]
    fn native_route_label_fade_hold_replacement_and_hidden_pause() {
        let mut motion = LabelMotion::default();
        motion.start();
        motion.update(Ticks::from_f32(10.0));
        assert!((motion.fade.value() - 0.3).abs() < 0.00001);
        motion.hidden = true;
        motion.update(Ticks::from_f32(10.0));
        assert_eq!(motion.opacity, 0.0);
        assert!((motion.fade.value() - 0.3).abs() < 0.00001);
        motion.hidden = false;
        motion.update(Ticks::from_f32(5.0));
        assert_eq!(motion.opacity, 0.5);
        motion.update(Ticks::from_f32(5.0));
        assert!((motion.fade.value() - 0.45).abs() < 0.00001);
        motion.start();
        assert_eq!(motion.fade.value(), 0.0);
        motion.update(Ticks::from_f32(1.0 / 0.03 + 240.0));
        assert!((motion.fade.value() - 1.0).abs() < 0.00001);
        motion.update(Ticks::from_f32(10.0));
        assert!((motion.fade.value() - 0.7).abs() < 0.00001);
        motion.update(Ticks::from_f32(24.0));
        assert_eq!(motion.fade.value(), 0.0);
        assert!(motion.fade.is_idle());
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_route_label_trigger_font_setting_and_timing() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        assert_eq!(word(0x159a30), 1076.0f32.to_bits());
        assert_eq!(word(0x15983c), 0.03f32.to_bits());
        assert_eq!(word(0x159660), 240.0f32.to_bits());
        for (a, expected) in NATIVE_WORDS {
            assert_eq!(word(*a), *expected, "native route label {a:x}");
        }
    }
    const NATIVE_WORDS: &[(usize, u32)] = &[
        (0x64474, 0x94015d5f),
        (0x64478, 0x36000100),
        (0x64480, 0xbd483d01),
        (0x64488, 0x52802801),
        (0x64494, 0x94015d5b),
        (0x1aaf8, 0x9401265a),
        (0x271c, 0xf941f508),
        (0x2788, 0x52922109),
        (0x2790, 0x5292220a),
        (0x2794, 0xf82a6909),
        (0x78f8, 0x52922209),
        (0x78fc, 0xf8696913),
        (0x797c, 0x9107c2e0),
        (0x7984, 0x94017257),
        (0x64308, 0xf2e84809),
        (0x64368, 0x97feea96),
        (0x6438c, 0x4ea91d21),
        (0x643ac, 0x1e221000),
        (0x643c0, 0x9402735c),
        (0x643d4, 0xbd483d0a),
        (0x643dc, 0x321a03e1),
        (0x643e8, 0x94015d86),
        (0x643f8, 0x52821001),
        (0x64400, 0x94015d80),
        (0x64414, 0x94015d7b),
        (0x644c4, 0x340000e8),
        (0x64508, 0x54000045),
        (0x64514, 0x14015d4f),
        (0x64540, 0x3942f908),
        (0x64544, 0x340005c8),
        (0x64558, 0x36000520),
        (0x6459c, 0x1e21080a),
        (0x645a0, 0x4ea91d20),
        (0x645f8, 0x940272f6),
    ];
}
