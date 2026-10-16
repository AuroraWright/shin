//! ADV root UI: auto/skip indicators, the touch navi toolbar and the event
//! hint (draw `11D20..12618`, enable `10760..10A14`/`114F0`, hit `152DC`).
use super::cg_gallery::motion::Motion;
use crate::adv::character_view::CharacterTextures;
use glam::{Vec4, vec4};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
};

pub const ATLAS_PATH: &str = "/adv.txa";
pub const TEXTURE_NAMES: [&str; 3] = ["adv", "navi", "skip"];
/// BSS `1B8DE0` (initialized by `19480`), indexed by button ID: Auto, Skip,
/// Backlog, Minus, Plus. Hits are 192x120; quads are drawn 208x128.
pub const NAVI: [(f32, f32); 5] = [
    (1332., 160.),
    (1524., 160.),
    (1716., 160.),
    (1428., 292.),
    (1620., 292.),
];
const EPSILON: f32 = f32::from_bits(0x3400_0000);

pub struct Button {
    pub enabled: bool,
    /// `+8` (`+5C0`): 0.5 + 0.5 * value greys a disabled button.
    pub shade: Motion,
    /// `+0x60` (`+618`): white flash amount.
    pub press: Motion,
}
pub struct RootUi {
    /// `+4F8`, in seconds.
    pub phase: f32,
    /// `+558`.
    pub navi_shown: bool,
    /// `+560`.
    pub slide: Motion,
    pub buttons: [Button; 5],
    /// `+500`.
    pub hint: Motion,
    /// `+4FC`: tapped (touch row) rather than pressed.
    pub hint_touch: bool,
}
impl Default for RootUi {
    fn default() -> Self {
        Self {
            phase: 0.,
            navi_shown: false,
            slide: Motion::new(0.),
            buttons: std::array::from_fn(|_| Button {
                enabled: false,
                shade: Motion::new(0.),
                press: Motion::new(0.),
            }),
            hint: Motion::new(0.),
            hint_touch: false,
        }
    }
}
/// Inputs to `10760..10A14`.
pub struct Availability {
    /// ADV inhibit bytes, pause/conceal counts, modal and overlay idle.
    pub gate: bool,
    /// VM `92B0`.
    pub system_controls: bool,
    /// The interactive overlay's `+0x58` permits playback modes.
    pub modes_allowed: bool,
    /// VM `2DE4`.
    pub event: i32,
}
pub struct Indicators {
    pub auto: bool,
    pub skip: bool,
    pub auto_active: bool,
    pub skip_active: bool,
    /// Settings `+0xBB`.
    pub disallow_skip_unread: bool,
}
impl RootUi {
    /// `114F0`.
    pub fn set_enabled(&mut self, id: usize, enabled: bool) {
        let button = &mut self.buttons[id];
        if button.enabled == enabled {
            return;
        }
        button.enabled = enabled;
        if enabled {
            button.shade.target(1., 0.25, 0, true);
        } else {
            button.shade.target(0., 0.1, 0, true);
            button.press.target(0., 0.2, 0, true);
        }
    }
    pub fn update_enabled(&mut self, a: Availability) {
        let modes = a.system_controls && a.modes_allowed;
        self.set_enabled(0, modes);
        self.set_enabled(1, modes);
        let others = if a.gate {
            self.set_enabled(2, a.system_controls && a.event < 0);
            a.system_controls
        } else {
            self.set_enabled(2, false);
            false
        };
        self.set_enabled(3, others);
        self.set_enabled(4, others);
    }
    /// Root update `10DCC`: tweens, then the indicator phase.
    pub fn advance(&mut self, delta: f32) {
        self.slide.update(delta);
        for button in &mut self.buttons {
            button.shade.update(delta);
            button.press.update(delta);
        }
        self.hint.update(delta);
        self.phase += delta * f32::from_bits(0x3c88_8889);
    }
    pub fn navi_ready(&self) -> bool {
        self.navi_shown && (self.slide.value() - 1.).abs() < EPSILON
    }
    pub fn show_navi(&mut self) {
        self.slide.target(1., 0.1, 0, true);
        self.navi_shown = true;
    }
    pub fn hide_navi(&mut self) {
        self.slide.target(0., 0.1, 0, true);
        self.navi_shown = false;
    }
    /// `152DC`: ID + 1 for an enabled button, 0 for a disabled one, -1 for none.
    pub fn hit(&self, p: glam::Vec2) -> i32 {
        if !self.navi_ready() {
            return -1;
        }
        for (id, &(x, y)) in NAVI.iter().enumerate() {
            if p.x >= x && p.x < x + 192. && p.y >= y && p.y < y + 120. {
                return if self.buttons[id].enabled {
                    id as i32 + 1
                } else {
                    0
                };
            }
        }
        -1
    }
    pub fn press(&mut self, id: usize) {
        self.buttons[id].press.target(0.7, 0.2, 0, true);
    }
    pub fn release(&mut self, id: usize) {
        let press = &mut self.buttons[id].press;
        press.target(0.7, 0.2, 0, true);
        press.enqueue(0., 0.2, 0, true);
    }
    pub fn cancel(&mut self, id: usize) {
        self.buttons[id].press.target(0., 0.2, 0, true);
    }
    /// `118A0` (A) and `15C1C` (tap) during an event: fade in over 32
    /// ticks, hold 120, fade out over 32. A running banner is not restarted.
    pub fn show_hint(&mut self, touch: bool) -> bool {
        if self.hint.busy() {
            return false;
        }
        self.hint_touch = touch;
        self.hint.snap(0.);
        self.hint.target(1., 32., 0, false);
        self.hint.enqueue(1., 120., 0, false);
        self.hint.enqueue(0., 32., 0, false);
        true
    }
    pub fn render(&self, atlas: &CharacterTextures, pass: &mut RenderPass, i: Indicators) {
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        let matrix = crate::render::top_left_projection_matrix();
        let quad = |pass: &mut RenderPass, name: &str, rect: Vec4, uv: Vec4, color: Vec4| {
            QuadVertices::new()
                .with_box(rect.x, rect.y, rect.z, rect.w)
                .with_tex_box(uv.x, uv.y, uv.z, uv.w)
                .with_color(FloatColor4::from_vec4(color).into_unorm())
                .render_sprite(pass, builder, atlas.get(name).as_source(), matrix);
        };
        let pulse = (self.phase * f32::from_bits(0x40c9_0fdb)).sin().mul_add(0.25, 0.75);
        let grey = vec4(0.25, 0.25, 0.25, 1.);
        let black = vec4(0., 0., 0., 1.);
        if i.skip {
            let rect = vec4(1824., 24., 1896., 96.);
            quad(pass, "adv", rect, vec4(0.5, 0.5, 1., 1.), black);
            let color = match (i.skip_active, i.disallow_skip_unread) {
                (false, _) => grey,
                (true, true) => vec4(pulse, pulse, 1., 1.),
                (true, false) => vec4(1., pulse, pulse, 1.),
            };
            quad(pass, "adv", rect, vec4(0., 0.5, 0.5, 1.), color);
        }
        if i.auto {
            let x = if i.skip { 1744. } else { 1824. };
            let rect = vec4(x, 24., x + 72., 96.);
            quad(pass, "adv", rect, vec4(0.5, 0., 1., 0.5), black);
            let color = if i.auto_active {
                vec4(pulse, 1., pulse, 1.)
            } else {
                grey
            };
            quad(pass, "adv", rect, vec4(0., 0., 0.5, 0.5), color);
        }
        let slide = self.slide.value();
        if slide > 0. {
            let offset = (1. - slide) * 588.;
            for (id, button) in self.buttons.iter().enumerate() {
                let (x, y) = NAVI[id];
                let shade = button.shade.value().mul_add(0.5, 0.5);
                let v = (id as i32 * 128) as f32 / 640.;
                QuadVertices::new()
                    .with_box(x + offset, y, x + offset + 208., y + 128.)
                    .with_tex_box(0., v, 0.5, v + 0.2)
                    .with_color(FloatColor4::from_rgba(shade, shade, shade, 1.).into_unorm())
                    .render_button(
                        pass,
                        builder,
                        atlas.get("navi").as_source(),
                        matrix,
                        Vec4::ZERO,
                        vec4(1., 1., 1., button.press.value()),
                    );
            }
        }
        let hint = self.hint.value();
        if hint > 0. {
            // ce610 returns 0: controller presses use the first row.
            let v = if self.hint_touch { 0.5 } else { 0. };
            quad(
                pass,
                "skip",
                vec4(16., 1000., 656., 1064.),
                vec4(0., v, 1., v + 0.25),
                vec4(1., 1., 1., hint),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navi_enable_slide_hit_and_hint_follow_native_tweens() {
        let mut ui = RootUi::default();
        ui.update_enabled(Availability {
            gate: true,
            system_controls: true,
            modes_allowed: true,
            event: -1,
        });
        assert!(ui.buttons.iter().all(|b| b.enabled));
        // Rate 0.25: enabled shade reaches 1 after four ticks.
        ui.advance(4.);
        assert_eq!(ui.buttons[0].shade.value(), 1.);
        let p = glam::Vec2::new(1332. + 10., 170.);
        assert_eq!(ui.hit(p), -1, "hidden toolbar has no hits");
        ui.show_navi();
        ui.advance(5.);
        assert_eq!(ui.hit(p), -1, "sliding toolbar has no hits");
        ui.advance(5.);
        assert!(ui.navi_ready());
        assert_eq!(ui.hit(p), 1);
        assert_eq!(ui.hit(glam::Vec2::new(1620. + 191., 292. + 119.)), 5);
        assert_eq!(ui.hit(glam::Vec2::new(1620. + 192., 292.)), -1);
        // In an event the backlog is disabled but still swallows a hit.
        ui.update_enabled(Availability {
            gate: true,
            system_controls: true,
            modes_allowed: true,
            event: 3,
        });
        assert_eq!(ui.hit(glam::Vec2::new(1720., 170.)), 0);
        // Without the gate Minus/Plus disable; modes stay with permission.
        ui.update_enabled(Availability {
            gate: false,
            system_controls: true,
            modes_allowed: true,
            event: -1,
        });
        assert!(ui.buttons[0].enabled && ui.buttons[1].enabled);
        assert!(!ui.buttons[3].enabled && !ui.buttons[4].enabled);
        // Release flashes to 0.7 and back at rate 0.2.
        ui.release(0);
        ui.advance(3.5);
        assert!((ui.buttons[0].press.value() - 0.7).abs() < 1e-5);
        ui.advance(3.5);
        assert!(ui.buttons[0].press.value().abs() < 1e-5 && !ui.buttons[0].press.busy());
        assert!(ui.show_hint(true));
        assert!(!ui.show_hint(false), "a running banner is kept");
        ui.advance(32.);
        assert_eq!(ui.hint.value(), 1.);
        ui.advance(120.);
        assert_eq!(ui.hint.value(), 1.);
        ui.advance(32.);
        assert_eq!(ui.hint.value(), 0.);
        assert!(!ui.hint.busy() && ui.hint_touch);
        ui.hide_navi();
        assert_eq!(ui.hit(p), -1);
    }
}
