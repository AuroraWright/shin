//! Retained native in-game SystemMenu, with caller-owned ADV modal scope.
use super::{AppAction, Viewport};
use crate::adv::gerokasu2::generic_text::Text;
use crate::{
    adv::{
        characters::view::CharacterTextures,
        gerokasu2::{
            config::dialog::GerDialog,
            system_menu::{
                self, Controller, Event, Input,
                artwork::{self, TextKind},
            },
        },
        input_area::{InputArea, InputAreaTarget, PointerSampler, desktop_gesture_vector},
    },
    asset::{font::gerokasu2::GpuGerokasu2Font, system::AssetServer},
    audio::SystemSoundPlayer,
    render::PreRenderContext,
};
use anyhow::Result;
use enum_map::EnumMap;
use glam::{Mat4, Vec2, Vec4, vec3, vec4};
use shin_core::{
    format::save::Settings,
    layout::{LayoutParams, gerokasu2::generic_layouter},
    primitives::color::UnormColor,
    time::Ticks,
    vm::command::types::Volume,
};
use shin_input::ActionState;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::BlendVertex},
};
use std::sync::Arc;
struct Label {
    text: Text,
    width: f32,
}
impl Label {
    fn load(assets: &AssetServer, font: &Arc<GpuGerokasu2Font>, text: &str) -> Result<Self> {
        let (commands, lines, extent) = generic_layouter(
            font.metrics(),
            LayoutParams {
                layout_width: artwork::TEXT_LAYOUT_WIDTH,
                text_size: artwork::TEXT_SIZE,
                rubi_size: 0.,
                line_padding_above: 2.,
                line_padding_below: 2.,
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
                artwork::TEXT_SIZE,
            )?,
            width: extent.x + 6.,
        })
    }
}
pub(super) struct Host {
    pub model: Controller,
    assets: Arc<AssetServer>,
    atlas: Arc<CharacterTextures>,
    font: Arc<GpuGerokasu2Font>,
    title: Option<Label>,
    clock: Option<Label>,
    clock_value: String,
    sounds: SystemSoundPlayer,
    pointer: PointerSampler,
    area: InputArea,
    armed: bool,
    dialog: Option<GerDialog>,
}
impl Host {
    pub async fn load(
        assets: Arc<AssetServer>,
        audio: Arc<shin_audio::AudioManager>,
        settings: Settings,
        title: String,
    ) -> Result<Self> {
        let (atlas, font) = futures::try_join!(
            assets.load::<CharacterTextures, _>(system_menu::ATLAS),
            assets.load::<GpuGerokasu2Font, _>(artwork::TEXT_FONT)
        )?;
        atlas.require_names(system_menu::ATLAS, system_menu::TEXTURES)?;
        let title = if title.is_empty() {
            None
        } else {
            Some(Label::load(&assets, &font, &title)?)
        };
        let mut sounds = SystemSoundPlayer::new(audio);
        sounds.set_settings_volume(settings.v3_sysvol);
        let mut model = Controller::new();
        model.take_events(); // parent acquired scope after capture
        Ok(Self {
            model,
            assets,
            atlas,
            font,
            title,
            clock: None,
            clock_value: String::new(),
            sounds,
            pointer: Default::default(),
            area: InputArea::canvas(),
            armed: false,
            dialog: None,
        })
    }
    pub fn cancel_input(&mut self) {
        self.pointer.cancel_contact(true);
        self.armed = false;
        self.area.disable(&mut Contact(&mut self.model));
        self.model.cancel();
        if let Some(d) = self.dialog.as_mut() {
            d.cancel_input();
        }
    }
    pub fn apply_settings(&mut self, s: &Settings) {
        self.sounds.set_settings_volume(s.v3_sysvol);
    }
    pub fn recover_child(&mut self) {
        self.model.recover_child();
        self.cancel_input();
    }
    pub fn child_returned(&mut self) {
        self.model.child_returned(1);
        self.cancel_input();
    }
    pub fn update(
        &mut self,
        delta: f32,
        input: &EnumMap<AppAction, ActionState>,
        point: Vec2,
        viewport: Viewport,
        seconds: f64,
        play_ticks: f64,
        permission: bool,
    ) -> Result<Vec<Event>> {
        self.sounds.update();
        let point = viewport.map(point).unwrap_or(Vec2::splat(-1.));
        let held = input[AppAction::PointerClick].is_held;
        if [
            AppAction::Enter,
            AppAction::Cancel,
            AppAction::SystemMenu,
            AppAction::Up,
            AppAction::AnyDown,
            AppAction::Left,
            AppAction::Right,
            AppAction::PointerClick,
        ]
        .into_iter()
        .all(|a| !input[a].is_held)
        {
            self.armed = true;
        }
        self.pointer.sample(point, held, seconds);
        let mut captured = false;
        if let Some(dialog) = self.dialog.as_mut() {
            if self.armed {
                dialog.pointer_state(point, held, seconds);
                dialog.keyboard(input);
            }
            let responses = dialog.update(Ticks::from_f32(delta));
            for cue in dialog.take_sounds() {
                self.sounds.play(&self.assets, cue, Volume(1.));
            }
            let done = responses.iter().any(|r| r.final_flag);
            for response in responses {
                self.model
                    .title_answer(i32::from(response.final_flag), response.button);
            }
            if done {
                self.dialog = None;
                self.cancel_input();
            }
            captured = true;
        } else if self.armed && permission && self.model.accepts_input() {
            captured = self.area.update_with_mouse_release(
                &mut Contact(&mut self.model),
                &self.pointer.frame(),
                self.pointer.mouse_release(),
            );
        }
        self.model.update(
            delta,
            permission && self.armed,
            captured,
            Input {
                cancel: input[AppAction::Cancel].is_clicked
                    || input[AppAction::SystemMenu].is_clicked,
                confirm: input[AppAction::Enter].is_clicked,
                up: input[AppAction::Up].is_clicked_or_repeated,
                down: input[AppAction::AnyDown].is_clicked_or_repeated,
                left: input[AppAction::Left].is_clicked_or_repeated,
                right: input[AppAction::Right].is_clicked_or_repeated,
            },
            play_ticks,
        );
        if self.clock_value != self.model.play_time {
            self.clock = Some(Label::load(
                &self.assets,
                &self.font,
                &self.font.localize(&self.model.play_time),
            )?);
            self.clock_value = self.model.play_time.clone();
        }
        let events = self.model.take_events();
        let mut outgoing = vec![];
        for event in events {
            if event == Event::ConfirmTitle {
                match GerDialog::load(
                    &self.assets,
                    system_menu::TITLE_QUESTION,
                    &["はい", "いいえ"],
                    1,
                    true,
                    held,
                ) {
                    Ok(dialog) => {
                        self.dialog = Some(dialog);
                        self.cancel_input();
                    }
                    Err(error) => {
                        tracing::error!(%error,"Ger Title confirmation preparation failed");
                        self.recover_child();
                    }
                }
            } else {
                outgoing.push(event);
            }
        }
        for cue in self.model.take_sounds() {
            self.sounds.play(&self.assets, cue, Volume(1.));
        }
        Ok(outgoing)
    }
    pub fn prepare(&mut self, ctx: &mut PreRenderContext) -> Result<()> {
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.prepare(ctx)?;
        }
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        for packet in artwork::packets(&self.model) {
            self.render_packet(pass, &packet);
        }
        let widths = [
            self.title.as_ref().map_or(0., |l| l.width),
            self.clock.as_ref().map_or(0., |l| l.width),
        ];
        for detail in artwork::details(&self.model, widths) {
            self.render_packet(pass, &detail.panel);
            let label = match detail.text.kind {
                TextKind::SceneTitle => self.title.as_ref(),
                TextKind::PlayTime => self.clock.as_ref(),
            };
            if let Some(label) = label {
                let alpha = (detail.text.color >> 24) as f32 / 255.;
                label.text.render(
                    pass,
                    RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                    Mat4::from_cols_array(&detail.text.transform),
                    vec4(1., 1., 1., alpha),
                )?;
            }
        }
        if let Some(dialog) = self.dialog.as_ref() {
            dialog.render(pass)?;
        }
        Ok(())
    }
    fn render_packet(&self, pass: &mut RenderPass, p: &artwork::Packet) {
        let bytes = p.color.to_le_bytes();
        let color = UnormColor::from_rgba(bytes[2], bytes[1], bytes[0], bytes[3]);
        let r = p.rect;
        let uv = p.uv;
        let transform = Mat4::from_cols_array(&p.transform);
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        if let Some(second) = p.texture2 {
            let vertices = [
                (r[0], r[1], uv[0], uv[1]),
                (r[2], r[1], uv[2], uv[1]),
                (r[0], r[3], uv[0], uv[3]),
                (r[2], r[3], uv[2], uv[3]),
            ]
            .map(|(x, y, u, v)| BlendVertex {
                position: vec3(x, y, 0.),
                color,
                texture_position: vec4(u, v, u, v),
            });
            pass.run(builder.build(
                RenderProgramWithArguments::Blend {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture1: self.atlas.get(p.texture).as_source(),
                    texture2: self.atlas.get(second).as_source(),
                    transform,
                    blend: Vec4::from_array(p.mix),
                    flash: Vec4::from_array(p.flash),
                },
                DrawPrimitive::TrianglesStrip,
            ));
        } else {
            QuadVertices::new()
                .with_box(r[0], r[1], r[2], r[3])
                .with_tex_box(uv[0], uv[1], uv[2], uv[3])
                .with_color(color)
                .render_sprite(
                    pass,
                    builder,
                    self.atlas.get(p.texture).as_source(),
                    transform,
                );
        }
    }
}
struct Contact<'a>(&'a mut Controller);
impl InputAreaTarget for Contact<'_> {
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, _: Vec2, _: Vec2) {
        self.0.cancel();
    }
    fn probe(&mut self, p: Vec2) -> bool {
        p.is_finite()
            && p.x >= 0.
            && p.y >= 0.
            && p.x < 1920.
            && p.y < 1080.
            && self.0.probe(p.to_array())
    }
    fn reset(&mut self) {
        self.0.reset();
    }
    fn press(&mut self, _: Vec2) {
        self.0.press();
    }
    fn release(&mut self, _: Vec2) {
        self.0.release();
    }
    fn cancel(&mut self) {
        self.0.cancel();
    }
    fn gesture(&mut self, v: Vec2) {
        self.0.gesture(v.to_array());
    }
    /// Like the root's overlay swipe, a slow mouse drag closes by displacement.
    fn mouse_gesture(&mut self, displacement: Vec2, velocity: Vec2) {
        self.gesture(desktop_gesture_vector(displacement, velocity));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::vec2;

    fn ready() -> Controller {
        let mut m = Controller::new();
        m.update(30., true, false, Default::default(), 0.);
        m.take_sounds();
        m
    }

    #[test]
    fn slow_left_mouse_drag_closes_like_a_touch_flick() {
        let mut m = ready();
        let phase = m.phase;
        Contact(&mut m).mouse_gesture(vec2(-128., 0.), vec2(-5., 0.));
        assert_eq!(m.phase, phase + 1);
        assert_eq!(m.take_sounds(), ["cancel"]);
        for (displacement, velocity) in [
            (vec2(128., 0.), vec2(5., 0.)),
            (vec2(0., -128.), vec2(0., -5.)),
            (vec2(-12., 0.), vec2(-5., 0.)),
        ] {
            let mut m = ready();
            let phase = m.phase;
            Contact(&mut m).mouse_gesture(displacement, velocity);
            assert_eq!(m.phase, phase, "{displacement:?}");
        }
    }
}
