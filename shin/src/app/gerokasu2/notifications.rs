//! ADV-owned scene-title (ADV+250, 5A790..5AC44) and BGM-title (ADV+340,
//! 7B2A4..7B8F8) notifications. AUTOSAVE7604 shows the captured scene label;
//! an accepted BGMPLAY (6C04..6C18) shows the track's display name. Both use
//! GenericText with `/system.fnt` and are drawn after the ADV root/modal dim
//! (11E58..11E64).
use std::sync::Arc;

use anyhow::Result;
use glam::{Mat4, Vec2, Vec4, vec2, vec3};
use shin_core::{
    layout::{LayoutParams, gerokasu2::generic_layouter},
    time::{Easing, Ticks, Tweener},
};
use shin_render::{ColorBlendType, RenderRequestBuilder, render_pass::RenderPass};

use super::load_menu::text::Text;
use crate::asset::{font::gerokasu2::GpuGerokasu2Font, system::AssetServer};

/// Native 98E10 flag words used by both owners: bit6 speed duration, bit7
/// relative target, bit8 replace queue, bit12 protected from skip.
const SPEED: u32 = 0x40;
const HOLD: u32 = 0x1080;
const CLOSE: u32 = 0x140;

struct Label {
    text: Text,
    /// GenericText box: layout extent plus the fixed 3px border on both sides.
    size: Vec2,
}
impl Label {
    /// Caller request {width1920,size,ruby0,align1,flags0,padding0}.
    fn load(assets: &AssetServer, font: &Arc<GpuGerokasu2Font>, text: &str, size: f32) -> Result<Self> {
        let (commands, lines, extent) = generic_layouter(
            font.metrics(),
            LayoutParams {
                layout_width: 1914.0,
                text_size: size,
                rubi_size: 0.0,
                perform_soft_breaks: false,
                always_leave_space_for_rubi: false,
                ..Default::default()
            },
        )
        .parse(text);
        Ok(Self {
            text: Text::prepare(
                &assets.context().wgpu_device,
                &assets.context().wgpu_queue,
                font,
                commands,
                lines,
                size,
            )?,
            size: extent + Vec2::splat(6.0),
        })
    }
    /// D4AE0: parent * T(position) * S(scale) * T(-size * anchor).
    fn render(
        &self,
        pass: &mut RenderPass,
        position: Vec2,
        anchor: Vec2,
        scale: Vec2,
        fill: Vec4,
    ) -> Result<()> {
        let transform = crate::render::top_left_projection_matrix()
            * Mat4::from_translation(position.extend(0.0))
            * Mat4::from_scale(vec3(scale.x, scale.y, 1.0))
            * Mat4::from_translation((-self.size * anchor).extend(0.0));
        self.text.render_colored(
            pass,
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
            transform,
            fill,
        )
    }
}

fn control(tween: &mut Tweener, value: f32, time: f32, flags: u32) {
    tween.gerokasu2_control(value, Ticks::from_f32(time), flags, Easing::Linear, false);
}

/// Shared 5AAEC/7B634 conceal fade. The motion only advances while fully
/// visible; concealment freezes it at the current value.
fn conceal_fade(visibility: &mut f32, concealed: bool, delta: f32) -> bool {
    let step = if concealed {
        f32::from_bits(0xbdcccccd)
    } else {
        f32::from_bits(0x3dcccccd)
    };
    *visibility = delta.mul_add(step, *visibility);
    if concealed && *visibility < 0.0 {
        *visibility = 0.0;
        return false;
    }
    if *visibility > 1.0 {
        *visibility = 1.0;
    }
    *visibility >= 1.0
}

struct SceneTitle {
    label: Option<Label>,
    motion: Tweener,
    visibility: f32,
    concealed: bool,
}
impl SceneTitle {
    fn new() -> Self {
        Self {
            label: None,
            motion: Tweener::new_gerokasu2(0.0),
            visibility: 1.0,
            concealed: false,
        }
    }
    /// 5A8A0: bottom-left at (4,1076); fade in, hold240 ticks, fade out.
    fn show(&mut self, label: Label) {
        self.label = Some(label);
        self.motion.fast_forward_to(0.0);
        let rate = f32::from_bits(0x3cf5c28f);
        control(&mut self.motion, 1.0, rate, SPEED);
        control(&mut self.motion, 0.0, 240.0, HOLD);
        control(&mut self.motion, 0.0, rate, SPEED);
    }
    fn close(&mut self) {
        if !self.motion.is_idle() {
            control(&mut self.motion, 0.0, f32::from_bits(0x3cf5c28f), CLOSE);
        }
    }
    fn skip(&mut self) {
        if !self.concealed {
            self.motion.fast_forward_unless_prohibited();
            self.visibility = 1.0;
        }
    }
    fn update(&mut self, delta: f32) {
        if conceal_fade(&mut self.visibility, self.concealed, delta) {
            self.motion.update(Ticks::from_f32(delta));
        }
    }
    fn render(&self, pass: &mut RenderPass) -> Result<()> {
        let Some(label) = self.label.as_ref().filter(|_| !self.motion.is_idle()) else {
            return Ok(());
        };
        let alpha = self.motion.value() * self.visibility;
        label.render(
            pass,
            vec2(4.0, f32::from_bits(0x44868000)),
            vec2(0.0, 1.0),
            Vec2::ONE,
            Vec4::new(1.0, 1.0, 1.0, alpha),
        )
    }
}

struct BgmTitle {
    labels: Option<(Label, Label)>,
    motion: Tweener,
    visibility: f32,
    concealed: bool,
    elapsed: f32,
}
impl BgmTitle {
    fn new() -> Self {
        Self {
            labels: None,
            motion: Tweener::new_gerokasu2(0.0),
            visibility: 1.0,
            concealed: false,
            elapsed: 0.0,
        }
    }
    /// 7B3B0: slide down by the note height+4 at 2px/tick, hold180 ticks.
    fn show(&mut self, title: Label, note: Label) {
        let extent = note.size.y + 4.0;
        self.labels = Some((title, note));
        self.motion.fast_forward_to(0.0);
        control(&mut self.motion, extent, 2.0, SPEED);
        control(&mut self.motion, 0.0, 180.0, HOLD);
        control(&mut self.motion, 0.0, 2.0, SPEED);
        self.elapsed = 0.0;
    }
    fn close(&mut self) {
        if !self.motion.is_idle() {
            control(&mut self.motion, 0.0, 2.0, CLOSE);
        }
    }
    fn skip(&mut self) {
        self.motion.fast_forward_unless_prohibited();
    }
    fn update(&mut self, delta: f32) {
        if conceal_fade(&mut self.visibility, self.concealed, delta) {
            self.motion.update(Ticks::from_f32(delta));
            self.elapsed += delta;
        }
    }
    fn render(&self, pass: &mut RenderPass) -> Result<()> {
        if !(self.visibility > 0.0) || self.motion.is_idle() {
            return Ok(());
        }
        let Some((title, note)) = &self.labels else {
            return Ok(());
        };
        let y = self.motion.value();
        let width = note.size.x;
        let fill = Vec4::new(0.75, 1.0, 1.0, self.visibility);
        title.render(pass, vec2(width + 4.0, y), vec2(0.0, 1.0), Vec2::ONE, fill)?;
        // 7B794..7B7F4: the note hops while the sine is positive and squashes
        // while it is negative, using fused offsets from its resting pose.
        let phase = (self.elapsed / 40.0 * f32::from_bits(0x40490fd0)).sin();
        let (scale, note_y) = if phase < 0.0 {
            (
                vec2(
                    phase.mul_add(f32::from_bits(0xbecccccd), 1.0),
                    phase.mul_add(f32::from_bits(0x3e99999a), 1.0),
                ),
                y,
            )
        } else {
            (Vec2::ONE, phase.mul_add(-8.0, y))
        };
        note.render(
            pass,
            vec2(width.mul_add(0.5, 4.0), note_y),
            vec2(0.5, 1.0),
            scale,
            fill,
        )
    }
}

pub(super) struct Notifications {
    font: Option<Arc<GpuGerokasu2Font>>,
    scene: SceneTitle,
    bgm: BgmTitle,
}
impl Notifications {
    pub fn new() -> Self {
        Self {
            font: None,
            scene: SceneTitle::new(),
            bgm: BgmTitle::new(),
        }
    }
    fn font(&mut self, assets: &AssetServer) -> Result<Arc<GpuGerokasu2Font>> {
        if let Some(font) = &self.font {
            return Ok(font.clone());
        }
        let font = assets.load_sync::<GpuGerokasu2Font>("/system.fnt")?;
        self.font = Some(font.clone());
        Ok(font)
    }
    pub fn show_scene_title(&mut self, assets: &AssetServer, title: &str) -> Result<()> {
        let font = self.font(assets)?;
        self.scene.show(Label::load(assets, &font, title, 48.0)?);
        Ok(())
    }
    /// An empty display name leaves the current banner untouched.
    pub fn show_bgm_title(&mut self, assets: &AssetServer, name: &str) -> Result<()> {
        if name.is_empty() {
            return Ok(());
        }
        let font = self.font(assets)?;
        let title = Label::load(assets, &font, name, 36.0)?;
        let note = Label::load(assets, &font, "♪", 36.0)?;
        self.bgm.show(title, note);
        Ok(())
    }
    /// 5A890/7B3A4 through the counted modal conceal scope.
    pub fn conceal_scene_title(&mut self, concealed: bool) {
        self.scene.concealed = concealed;
    }
    pub fn conceal_bgm_title(&mut self, concealed: bool) {
        self.bgm.concealed = concealed;
    }
    /// 5AA9C/7B5EC, used by script EXIT before its gates.
    pub fn close(&mut self) {
        self.scene.close();
        self.bgm.close();
    }
    pub fn update(&mut self, delta: f32, skipping: bool) {
        self.scene.update(delta);
        self.bgm.update(delta);
        if skipping {
            self.scene.skip();
            self.bgm.skip();
        }
    }
    /// The draw routines read Config's scene/music-title switches each frame.
    pub fn render(&self, pass: &mut RenderPass, scene_title: bool, song_title: bool) -> Result<()> {
        if scene_title {
            self.scene.render(pass)?;
        }
        if song_title {
            self.bgm.render(pass)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_title_fades_in_holds_and_fades_out_at_native_rates() {
        let mut motion = SceneTitle::new();
        motion.motion.fast_forward_to(0.0);
        let rate = f32::from_bits(0x3cf5c28f);
        control(&mut motion.motion, 1.0, rate, SPEED);
        control(&mut motion.motion, 0.0, 240.0, HOLD);
        control(&mut motion.motion, 0.0, rate, SPEED);
        let mut frames = 0;
        while motion.motion.value() < 1.0 {
            motion.update(1.0);
            frames += 1;
        }
        // 1/0.03 rounded up: the fade completes on its 34th frame.
        assert_eq!(frames, 34);
        for _ in 0..240 {
            motion.update(1.0);
        }
        assert!(!motion.motion.is_idle());
        assert!(motion.motion.value() < 1.0);
        for _ in 0..40 {
            motion.update(1.0);
        }
        assert!(motion.motion.is_idle());
    }

    #[test]
    fn concealment_freezes_motion_until_visibility_returns() {
        let mut motion = BgmTitle::new();
        control(&mut motion.motion, 40.0, 2.0, SPEED);
        motion.update(5.0);
        let value = motion.motion.value();
        motion.concealed = true;
        for _ in 0..20 {
            motion.update(1.0);
        }
        assert_eq!(motion.visibility, 0.0);
        assert_eq!(motion.motion.value(), value);
        motion.concealed = false;
        for _ in 0..9 {
            motion.update(1.0);
        }
        assert_eq!(motion.motion.value(), value);
        motion.update(1.0);
        motion.update(1.0);
        assert!(motion.motion.value() > value);
    }

    #[test]
    fn skipping_stops_at_the_protected_hold() {
        let mut motion = SceneTitle::new();
        let rate = f32::from_bits(0x3cf5c28f);
        control(&mut motion.motion, 1.0, rate, SPEED);
        control(&mut motion.motion, 0.0, 240.0, HOLD);
        control(&mut motion.motion, 0.0, rate, SPEED);
        motion.skip();
        assert_eq!(motion.motion.value(), 1.0);
        assert!(!motion.motion.is_idle());
        motion.close();
        motion.skip();
        assert!(motion.motion.is_idle());
    }
}
