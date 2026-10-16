//! Native boot splash owner `N4shin9FirstBootE` (constructor 5B720, update
//! 5BE40, draw 5C178), installed unconditionally by app init 252C8/5C2D0.
//!
//! The 0x14EBD0 table lists `{name, hold ticks}` entries from `/splash.txa`,
//! ending with a null name. For each entry, including the null end, the update
//! state machine captures the current frame (25990) and snaps the overlay
//! tween +A8 to one (986C4). It then fades the overlay to zero over 60 ticks
//! (98E10 flags 0, linear) and, while an entry is present, holds for its ticks
//! (flag 0x80, a relative zero). The draw clears to black, draws the entry
//! texture over 1920x1080, then the captured frame with color (1,1,1,overlay).
//! After the null entry finishes, lambda 5D0FC runs the DF9B0 boot gate
//! (TitleDemo or Title). FirstBoot reads no input.
use crate::{adv::character_view::CharacterTextures, asset::system::AssetServer};
use anyhow::Result;
use shin_core::{
    primitives::color::FloatColor4,
    time::{Easing, Ticks, Tweener},
};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
};
use std::sync::Arc;

const ARCHIVE: &str = "/splash.txa";
/// 0x14EBD0: caution 180, black 10, 07thExpansion 120, entergram 120.
pub(super) const STEPS: [(&str, f32); 4] = [
    ("caution", 180.0),
    ("black", 10.0),
    ("07thExpansion", 120.0),
    ("entergram", 120.0),
];
const CROSSFADE: f32 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Start,
    Fade,
    Hold,
    Done,
}

pub(super) struct FirstBoot {
    textures: Arc<CharacterTextures>,
    state: State,
    index: usize,
    /// The entry drawn now (+60); `None` past the table end.
    current: Option<&'static str>,
    /// The captured previous frame: the previous entry, or the black clear.
    previous: Option<&'static str>,
    overlay: Tweener,
    requested: bool,
}
impl FirstBoot {
    pub async fn load(assets: Arc<AssetServer>) -> Result<Self> {
        let textures: Arc<CharacterTextures> = assets.load(ARCHIVE).await?;
        textures.require_names(ARCHIVE, &STEPS.map(|(name, _)| name))?;
        Ok(Self {
            textures,
            state: State::Start,
            index: 0,
            current: None,
            previous: None,
            overlay: Tweener::new_gerokasu2(0.0),
            requested: false,
        })
    }

    /// 5BE40 after the archive is ready.
    pub fn update(&mut self, delta: Ticks) {
        if self.state == State::Start {
            self.previous = self.current;
            self.current = STEPS.get(self.index).map(|(name, _)| *name);
            self.overlay.fast_forward_to(1.0);
            self.overlay
                .gerokasu2_control(0.0, Ticks::from_f32(CROSSFADE), 0, Easing::Linear, false);
            self.state = State::Fade;
        }
        if self.state == State::Fade && self.overlay.is_idle() {
            if self.current.is_some() {
                let hold = STEPS[self.index].1;
                self.overlay
                    .gerokasu2_control(0.0, Ticks::from_f32(hold), 0x80, Easing::Linear, false);
            }
            self.state = State::Hold;
        }
        if self.state == State::Hold && self.overlay.is_idle() {
            self.state = if self.current.is_some() {
                self.index += 1;
                State::Start
            } else {
                State::Done
            };
        }
        self.overlay.update(delta);
    }

    /// One boot-gate publication per owner (5C06C..5C10C).
    pub fn take_finished(&mut self) -> bool {
        if self.state == State::Done && !self.requested {
            self.requested = true;
            return true;
        }
        false
    }

    /// (entry index, current entry, overlay)
    #[cfg(test)]
    pub(super) fn reference_state(&self) -> (usize, Option<&'static str>, f32) {
        (self.index, self.current, self.overlay.value())
    }

    pub fn render(&self, pass: &mut RenderPass) {
        let matrix = crate::render::top_left_projection_matrix();
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        let screen = QuadVertices::new().with_box(0.0, 0.0, 1920.0, 1080.0);
        if let Some(name) = self.current {
            screen.render_sprite(pass, builder, self.textures.get(name).as_source(), matrix);
        }
        let alpha = self.overlay.value();
        if alpha <= 0.0 {
            return;
        }
        match self.previous {
            Some(name) => screen
                .with_color(FloatColor4::from_rgba(1.0, 1.0, 1.0, alpha).into_unorm())
                .render_sprite(pass, builder, self.textures.get(name).as_source(), matrix),
            None => screen
                .with_color(FloatColor4::from_rgba(0.0, 0.0, 0.0, alpha).into_unorm())
                .fill(pass, builder, matrix),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The state machine without textures: crossfade 60, then each hold.
    #[test]
    fn steps_follow_native_table_timing() {
        let mut overlay = Tweener::new_gerokasu2(0.0);
        let mut ticks = 0.0;
        for (_, hold) in STEPS.iter().map(|(n, h)| (Some(*n), *h)).chain([(None, 0.0)]) {
            overlay.fast_forward_to(1.0);
            overlay.gerokasu2_control(0.0, Ticks::from_f32(CROSSFADE), 0, Easing::Linear, false);
            overlay.update(Ticks::from_f32(30.0));
            assert_eq!(overlay.value(), 0.5);
            overlay.update(Ticks::from_f32(30.0));
            assert!(overlay.is_idle());
            if hold > 0.0 {
                overlay.gerokasu2_control(0.0, Ticks::from_f32(hold), 0x80, Easing::Linear, false);
                assert!(!overlay.is_idle());
                overlay.update(Ticks::from_f32(hold));
                assert!(overlay.is_idle());
                assert_eq!(overlay.value(), 0.0);
            }
            ticks += CROSSFADE + hold;
        }
        assert_eq!(ticks, 730.0);
    }
}
