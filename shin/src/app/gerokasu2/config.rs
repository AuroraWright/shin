//! Ger Title's native Config child; persistence is acknowledged by the caller.
use super::{AppAction, Viewport};
use crate::{
    adv::{
        character_view::CharacterTextures,
        config_dialog::ConfigDialogKind,
        gerokasu2::config::{self, ConfigController, Input, Phase, Request},
        input_area::{InputArea, InputAreaTarget, PointerSampler},
    },
    asset::system::AssetServer,
    audio::SystemSoundPlayer,
    render::PreRenderContext,
};
use anyhow::Result;
use enum_map::EnumMap;
use glam::{Mat4, Vec2, Vec4};
use shin_core::{
    format::save::Settings, primitives::color::FloatColor4, time::Ticks, vm::command::types::Volume,
};
use shin_input::ActionState;
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
};
use std::sync::Arc;

pub(super) enum ConfigEvent {
    SettingsChanged(Settings),
    SaveRequested,
    Close,
}
pub(super) struct ConfigHost {
    pub model: ConfigController,
    atlas: Arc<CharacterTextures>,
    system: Arc<CharacterTextures>,
    server: Arc<AssetServer>,
    sounds: SystemSoundPlayer,
    dialog: Option<(ConfigDialogKind, config::dialog::GerDialog)>,
    pointer: PointerSampler,
    area: InputArea,
    armed: bool,
    axis: f32,
}
impl ConfigHost {
    pub async fn load(
        server: Arc<AssetServer>,
        audio: Arc<shin_audio::AudioManager>,
        settings: Settings,
    ) -> Result<Self> {
        let (atlas, system) = futures::try_join!(
            server.load::<CharacterTextures, _>(config::ATLAS),
            server.load::<CharacterTextures, _>("/systex.txa")
        )?;
        atlas.require_names(config::ATLAS, config::TEXTURES)?;
        system.require_names("/systex.txa", &["scroll"])?;
        let mut sounds = SystemSoundPlayer::new(audio);
        sounds.set_settings_volume(settings.v3_sysvol);
        Ok(Self {
            model: ConfigController::new(settings),
            atlas,
            system,
            server,
            sounds,
            dialog: None,
            pointer: Default::default(),
            area: InputArea::canvas(),
            armed: false,
            axis: 0.,
        })
    }
    pub fn set_scroll_axis(&mut self, axis: f32) {
        self.axis = axis;
    }
    pub fn cancel_input(&mut self) {
        self.area.disable(&mut self.model);
        self.pointer.cancel_contact(true);
        if let Some((_, dialog)) = &mut self.dialog {
            dialog.cancel_input();
        }
        self.armed = false;
    }
    pub fn save_finished(&mut self) {
        self.model.save_finished();
    }
    pub fn save_failed(&mut self) {
        self.model.save_failed();
        self.cancel_input();
    }
    pub fn update(
        &mut self,
        delta: f32,
        input: &EnumMap<AppAction, ActionState>,
        point: Vec2,
        viewport: Viewport,
        seconds: f64,
    ) -> Result<Vec<ConfigEvent>> {
        let before = self.model.settings.clone();
        let point = viewport.map(point).unwrap_or(Vec2::splat(-1.));
        let held = input[AppAction::PointerClick].is_held;
        if [
            AppAction::PointerClick,
            AppAction::Cancel,
            AppAction::ConfigReset,
            AppAction::Enter,
            AppAction::Left,
            AppAction::Right,
            AppAction::Up,
            AppAction::AnyDown,
        ]
        .into_iter()
        .all(|a| !input[a].is_held)
        {
            self.armed = true;
        }
        self.pointer.sample(point, held, seconds);
        self.sounds.update();
        if let Some((kind, dialog)) = &mut self.dialog {
            self.area.disable(&mut self.model);
            if self.armed {
                dialog.pointer_state(point, held, seconds);
                dialog.keyboard(input);
            }
            let kind = *kind;
            let responses = dialog.update(Ticks::from_f32(delta));
            let mut finished = false;
            for response in responses {
                match kind {
                    ConfigDialogKind::Reset => self
                        .model
                        .reset_response(response.final_flag, response.button),
                    ConfigDialogKind::AutoSleep => {
                        if response.final_flag {
                            self.model.notice_finished();
                        }
                    }
                }
                finished |= response.final_flag;
            }
            for cue in dialog.take_sounds() {
                self.sounds.play(&self.server, cue, Volume(1.));
            }
            if finished {
                self.dialog = None;
                self.cancel_input();
            }
        } else if self.armed && self.model.phase == Phase::Active {
            let captured = self.area.update(&mut self.model, &self.pointer.frame());
            if !captured {
                self.model.input(Input {
                    up: input[AppAction::Up].is_clicked_or_repeated,
                    down: input[AppAction::AnyDown].is_clicked_or_repeated,
                    left: input[AppAction::Left].is_clicked_or_repeated,
                    right: input[AppAction::Right].is_clicked_or_repeated,
                    reset: input[AppAction::ConfigReset].is_clicked,
                    close: input[AppAction::Cancel].is_clicked,
                    axis: self.axis,
                });
            }
        } else {
            self.area.disable(&mut self.model);
        }
        self.model.update(delta);
        let mut events = vec![];
        for request in self.model.take_requests() {
            match request {
                Request::ResetConfirmation | Request::AutoSleepNotice => {
                    let kind = if request == Request::ResetConfirmation {
                        ConfigDialogKind::Reset
                    } else {
                        ConfigDialogKind::AutoSleep
                    };
                    match config::dialog::GerDialog::load(
                        &self.server,
                        kind.prompt(),
                        kind.labels(),
                        if kind == ConfigDialogKind::Reset {
                            1
                        } else {
                            0
                        },
                        kind == ConfigDialogKind::Reset,
                        held,
                    ) {
                        Ok(dialog) => self.dialog = Some((kind, dialog)),
                        Err(error) => {
                            tracing::error!(%error, "Ger Config question preparation failed");
                            match kind {
                                ConfigDialogKind::Reset => self.model.reset_response(true, -1),
                                ConfigDialogKind::AutoSleep => self.model.notice_finished(),
                            }
                        }
                    }
                    self.cancel_input();
                }
                Request::Save => events.push(ConfigEvent::SaveRequested),
                Request::Close => events.push(ConfigEvent::Close),
            }
        }
        self.sounds
            .set_settings_volume(self.model.settings.v3_sysvol);
        for cue in self.model.take_sounds() {
            self.sounds.play(&self.server, cue, Volume(1.));
        }
        if !before.serialized_eq(&self.model.settings) {
            events.insert(0, ConfigEvent::SettingsChanged(self.model.settings.clone()));
        }
        Ok(events)
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) -> Result<()> {
        if let Some((_, dialog)) = &mut self.dialog {
            dialog.prepare(context)?;
        }
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) {
        if self.model.phase == Phase::Closed {
            return;
        }
        let matrix = Mat4::from_cols_array(&[
            2. / 1920.,
            0.,
            0.,
            0.,
            0.,
            -2. / 1080.,
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
        ]);
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        for q in config::artwork::quads(&self.model) {
            let offset = config::artwork::offset(&self.model, q.space);
            let mut rect = [
                q.rect[0] + offset[0],
                q.rect[1] + offset[1],
                q.rect[2] + offset[0],
                q.rect[3] + offset[1],
            ];
            let mut uv = q.uv;
            if let Some(mut clip) = config::artwork::clip(q.space) {
                let x = self.model.progress.mul_add(-1920., 1920.);
                clip[0] += x;
                clip[2] += x;
                if !clip_quad(&mut rect, &mut uv, clip) {
                    continue;
                }
            }
            let quad = QuadVertices::new()
                .with_box(rect[0], rect[1], rect[2], rect[3])
                .with_tex_box(uv[0], uv[1], uv[2], uv[3])
                .with_color(FloatColor4::WHITE.with_alpha(q.alpha).into_unorm());
            if q.flash > 0. {
                quad.render_button(
                    pass,
                    builder,
                    self.atlas.get(q.texture).as_source(),
                    matrix,
                    Vec4::new(1., 1., 1., q.flash),
                    Vec4::new(1., 1., 1., 0.),
                );
            } else {
                // 54054..54068: `top` uses native blend mode 3 (additive), as
                // in every other Ger menu header; the rest use mode 2.
                let builder = if q.texture == "top" {
                    builder.color_blend_type(ColorBlendType::Layer2)
                } else {
                    builder
                };
                quad.render_sprite(pass, builder, self.atlas.get(q.texture).as_source(), matrix);
            }
        }
        let x = self.model.progress.mul_add(-1920., 1920.);
        crate::adv::scrollbar::render(
            &self.system,
            pass,
            matrix,
            crate::adv::scrollbar::slices(
                config::HEIGHT,
                self.model.scroll(),
                1770. + x,
                178.,
                config::VISIBLE,
            ),
        );
        if let Some((_, dialog)) = &self.dialog {
            if let Err(error) = dialog.render(pass) {
                tracing::error!(%error, "Ger Config dialog drawing failed");
            }
        }
    }
}
fn clip_quad(r: &mut [f32; 4], uv: &mut [f32; 4], clip: [f32; 4]) -> bool {
    let original = *r;
    let old_uv = *uv;
    r[0] = r[0].max(clip[0]);
    r[1] = r[1].max(clip[1]);
    r[2] = r[2].min(clip[2]);
    r[3] = r[3].min(clip[3]);
    if r[0] >= r[2] || r[1] >= r[3] {
        return false;
    }
    for (i, axis) in [(0, 0), (1, 1), (2, 0), (3, 1)] {
        uv[i] = ((r[i] - original[axis]) / (original[axis + 2] - original[axis]))
            .mul_add(old_uv[axis + 2] - old_uv[axis], old_uv[axis]);
    }
    true
}
impl InputAreaTarget for ConfigController {
    fn probe(&mut self, p: Vec2) -> bool {
        self.phase == Phase::Active && ConfigController::probe(self, p.x, p.y)
    }
    fn reset(&mut self) {
        self.cancel_pointer();
    }
    fn press(&mut self, p: Vec2) {
        ConfigController::press(self, p.x, p.y);
    }
    fn release(&mut self, _: Vec2) {
        ConfigController::release(self);
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, a: Vec2, b: Vec2) {
        ConfigController::drag(self, b.x - a.x, b.y - a.y);
    }
    fn gesture(&mut self, v: Vec2) {
        ConfigController::gesture(self, v.y);
    }
    fn cancel(&mut self) {
        self.cancel_pointer();
    }
}
