//! Ger BustupMode: remembered selectors, real shared BUP/Picture layers and
//! asynchronous replacements. The Viewer retains the selected scene.
use super::{AppAction, MenuBgm, Viewport};
use crate::{
    adv::{
        character_view::CharacterTextures,
        gerokasu2::{
            bustup_gallery::{
                self, Action, Controller, Input, ViewerInput,
                catalog::{Catalog, Segment, Selection},
            },
            layer_factory::{BasicDrawable, LayerFactoryRequest, PreparedBasicDrawable},
            reconstruction_mode::{OverlayClock, overlay::Overlay},
        },
        input_area::{InputArea, InputAreaTarget, PointerSampler},
    },
    asset::{mask::MaskTexture, system::AssetServer},
    audio::SystemSoundPlayer,
    layer::{
        DrawableLayer, Layer, LayerGroup, LayerProperties, PropertyPolicy,
        render_params::TransformParams,
    },
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext},
};
use anyhow::{Context, Result};
use enum_map::{Enum, EnumMap};
use glam::{Mat4, Vec2, Vec4};
use shin_core::{
    format::{
        save::gerokasu2::SaveData,
        scenario::{
            gerokasu2::Gerokasu2Scenario,
            info::{BgmId, CharacterBoxSegment},
            instruction_elements::FromNumber,
        },
    },
    primitives::{color::UnormColor, update::FrameId},
    time::{Ticks, Tween},
    vm::command::types::{LayerProperty, LayerbankId, Volume},
};
use shin_input::ActionState;
use shin_render::{
    ColorBlendType, PassKind, RenderRequestBuilder, quad_vertices::QuadVertices,
    render_pass::RenderPass, render_texture::RenderTexture,
};
use std::sync::Arc;

struct Replacement {
    task: Option<shin_tasks::AsyncTask<Result<Vec<(u32, PreparedBasicDrawable)>>>>,
}
impl Drop for Replacement {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            #[cfg(not(target_arch = "wasm32"))]
            task.cancel_and_retire();
            #[cfg(target_arch = "wasm32")]
            drop(task);
        }
    }
}
pub(super) struct BustupGalleryHost {
    model: Controller,
    published_selection: Selection,
    scene: LayerGroup<BasicDrawable>,
    atlas: Arc<CharacterTextures>,
    mask: Arc<MaskTexture>,
    previous: Option<Overlay>,
    job: Option<Replacement>,
    assets: Arc<AssetServer>,
    scenario: Arc<Gerokasu2Scenario>,
    bgm: MenuBgm,
    sounds: SystemSoundPlayer,
    pointer: PointerSampler,
    area: InputArea,
    armed: bool,
    axes: Vec2,
    frame: FrameId,
    delta: Ticks,
}

impl BustupGalleryHost {
    pub async fn load(
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        audio: Arc<shin_audio::AudioManager>,
        data: &SaveData,
        bgm: MenuBgm,
    ) -> Result<Self> {
        let catalog =
            Catalog::from_segments(scenario.character_box_info.iter().map(|s| match *s {
                CharacterBoxSegment::Background {
                    primary_picture_id,
                    secondary_picture_id_offset,
                } => Segment::Background {
                    primary: primary_picture_id.into_i32() as u16,
                    secondary_offset: secondary_picture_id_offset,
                },
                CharacterBoxSegment::Bustup { bustup_id } => {
                    Segment::Bustup(bustup_id.into_i32() as u16)
                }
                CharacterBoxSegment::EndExpressionGroup => Segment::EndExpressionGroup,
                CharacterBoxSegment::EndPoseGroup => Segment::EndPoseGroup,
                CharacterBoxSegment::EndDefinition => Segment::EndDefinition,
            }))
            .map_err(anyhow::Error::msg)?;
        let mut model = Controller::new(catalog);
        let atlas: Arc<CharacterTextures> = assets.load(bustup_gallery::ATLAS).await?;
        atlas.require_names(bustup_gallery::ATLAS, bustup_gallery::TEXTURES)?;
        let mask = assets.load(bustup_gallery::MASK).await?;
        let (primary, secondary) = model.selected_background();
        let (background, bustup) = futures::try_join!(
            prepare(
                &assets,
                &scenario,
                Action::LoadBackground { primary, secondary }
            ),
            prepare(
                &assets,
                &scenario,
                Action::LoadBustup {
                    id: model.selected_bustup()
                }
            ),
        )?;
        let mut scene = LayerGroup::with_property_policy(
            Some("Ger Bustup selected scene".into()),
            PropertyPolicy::Gerokasu2,
        );
        install(&mut scene, background);
        install(&mut scene, bustup);
        model.assets_ready();
        let mut sounds = SystemSoundPlayer::new(audio);
        sounds.set_settings_volume(data.settings.v3_sysvol);
        bgm.lock().set_settings_volume(data.settings.v0_bgmvol);
        Ok(Self {
            published_selection: model.selection.clone(),
            model,
            scene,
            atlas,
            mask,
            previous: None,
            job: None,
            assets,
            scenario,
            bgm,
            sounds,
            pointer: Default::default(),
            area: InputArea::canvas(),
            armed: false,
            axes: Vec2::ZERO,
            frame: FrameId::default(),
            delta: Ticks::ZERO,
        })
    }
    pub fn set_previous(&mut self, snapshot: Option<RenderTexture>) {
        self.previous = snapshot.map(|snapshot| {
            let mut clock = OverlayClock::captured(true);
            clock.masked(60, 0.3, 0, true);
            Overlay {
                clock,
                snapshot,
                mask: Some(self.mask.clone()),
            }
        });
    }
    pub fn set_viewer_axes(&mut self, axes: Vec2) {
        self.axes = if axes.is_finite() {
            axes.clamp(Vec2::splat(-1.), Vec2::ONE)
        } else {
            Vec2::ZERO
        };
    }
    pub fn cancel_input(&mut self) {
        self.area.disable(&mut Target(&mut self.model));
        self.pointer.cancel_contact(true);
        self.armed = false;
        self.axes = Vec2::ZERO;
    }
    pub fn recover_return(&mut self) {
        self.model.recover_return();
        self.cancel_input();
    }
    pub fn update(
        &mut self,
        delta: f32,
        input: &EnumMap<AppAction, ActionState>,
        point: Vec2,
        viewport: Viewport,
        seconds: f64,
    ) -> Result<Option<i32>> {
        self.frame.advance();
        self.delta = Ticks::from_f32(delta);
        self.sounds.update();
        let mut published = false;
        if let Some(result) = self
            .job
            .as_mut()
            .and_then(|j| j.task.as_mut())
            .and_then(|t| t.poll_naive())
        {
            self.job.as_mut().unwrap().task = None;
            self.job = None;
            match result {
                Ok(prepared) => {
                    install(&mut self.scene, prepared);
                    self.published_selection = self.model.selection.clone();
                }
                Err(error) => {
                    // Explicit desktop IO failure boundary: keep the real old
                    // scene and its remembered selector, then require release.
                    tracing::error!(%error, "Ger Bustup replacement failed; retaining selected scene");
                    self.model.selection = self.published_selection.clone();
                    self.cancel_input();
                }
            }
            published = true;
        }
        if [
            AppAction::PointerClick,
            AppAction::Cancel,
            AppAction::Enter,
            AppAction::Up,
            AppAction::AnyDown,
            AppAction::Left,
            AppAction::Right,
            AppAction::PageUp,
            AppAction::PageDown,
        ]
        .into_iter()
        .all(|a| !input[a].is_held)
        {
            self.armed = true;
        }
        self.pointer.sample(
            viewport.map(point).unwrap_or(Vec2::splat(-1.)),
            input[AppAction::PointerClick].is_held,
            seconds,
        );
        let enabled = self.armed && self.model.accepts_input();
        let captured = if enabled {
            self.area.update_with_mouse_release(
                &mut Target(&mut self.model),
                &self.pointer.frame(),
                self.pointer.mouse_release(),
            )
        } else {
            self.area.disable(&mut Target(&mut self.model));
            false
        };
        self.model.update(
            delta,
            self.armed,
            captured,
            Input {
                cancel: input[AppAction::Cancel].is_clicked,
                confirm: input[AppAction::Enter].is_clicked,
                up: input[AppAction::Up].is_clicked_or_repeated,
                down: input[AppAction::AnyDown].is_clicked_or_repeated,
                left: input[AppAction::Left].is_clicked_or_repeated,
                right: input[AppAction::Right].is_clicked_or_repeated,
                previous: input[AppAction::PageUp].is_clicked_or_repeated,
                next: input[AppAction::PageDown].is_clicked_or_repeated,
            },
            ViewerInput {
                cancel: input[AppAction::Cancel].is_clicked,
                confirm: input[AppAction::Enter].is_clicked,
                up: input[AppAction::Up].is_held,
                down: input[AppAction::AnyDown].is_held,
                left: input[AppAction::Left].is_held,
                right: input[AppAction::Right].is_held,
                axes: self.axes.to_array(),
            },
        );
        // Native queued task denies input on its publication snapshot. The
        // next snapshot observes readiness, even if the ten-tick wipe runs on.
        if published {
            self.model.replacement_ready();
        }
        let mut result = None;
        for action in self.model.take_actions() {
            match action {
                Action::LoadBustup { .. } | Action::LoadBackground { .. } => {
                    let (assets, scenario) = (self.assets.clone(), self.scenario.clone());
                    self.job = Some(Replacement {
                        task: Some(shin_tasks::async_io::spawn_loader(async move {
                            prepare(&assets, &scenario, action).await
                        })),
                    });
                }
                Action::StopBgm(ticks) => {
                    self.bgm.lock().stop(Tween::linear(Ticks::from_u32(ticks)))
                }
                Action::PlayBgm { id, fade } => {
                    let info = self
                        .scenario
                        .bgm_info
                        .get(id as usize)
                        .context("Ger Bustup menu BGM missing")?;
                    self.bgm.lock().request(
                        self.assets.clone(),
                        format!("/bgm/{}.nxa", info.name.as_str()),
                        BgmId::from_number(id),
                        true,
                        Volume(1.),
                        Tween::linear(Ticks::from_u32(fade)),
                    );
                }
                Action::ReturnTitle { requested_id } => result = Some(requested_id),
            }
        }
        for cue in self.model.take_sounds() {
            self.sounds.play(&self.assets, cue, Volume(1.));
        }
        if let Some(previous) = &mut self.previous {
            previous.clock.elapsed = self.model.capture_elapsed;
            if !previous.clock.busy() {
                self.previous = None;
            }
        }
        Ok(result)
    }
    pub fn prepare(&mut self, ctx: &mut PreRenderContext) -> Result<()> {
        // Shared group computes child order, perspective and native stencil
        // ownership. Screen X and plane-camera values come from the controller.
        let props = self.scene.properties_mut();
        for (p, value) in [
            (LayerProperty::TranslateX, self.model.scene_x.value()),
            (LayerProperty::CameraPositionX, self.model.camera[0].value()),
            (LayerProperty::CameraPositionY, self.model.camera[1].value()),
        ] {
            props.property_tweener_mut(p).fast_forward_to(value);
        }
        self.scene.update(&AdvUpdateContext {
            frame_id: self.frame,
            delta_ticks: self.delta,
            asset_server: &self.assets,
            device: ctx.device,
            queue: ctx.queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        });
        self.scene.pre_render(ctx, &TransformParams::default());
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        for kind in [PassKind::Opaque, PassKind::Transparent] {
            self.scene
                .render(pass, &TransformParams::default(), 0, kind);
        }
        for packet in bustup_gallery::artwork::packets(&self.model) {
            let [b, g, r, a] = packet.color.to_le_bytes();
            let vertices = QuadVertices::new()
                .with_box(
                    packet.rect[0],
                    packet.rect[1],
                    packet.rect[2],
                    packet.rect[3],
                )
                .with_tex_box(packet.uv[0], packet.uv[1], packet.uv[2], packet.uv[3])
                .with_color(UnormColor::from_rgba(r, g, b, a));
            let builder = RenderRequestBuilder::new().color_blend_type(if packet.blend == 3 {
                ColorBlendType::Layer2
            } else {
                ColorBlendType::Layer1
            });
            let matrix =
                Mat4::from_cols_array(&crate::adv::gerokasu2::cg_gallery::artwork::projection());
            if let Some(flash) = packet.flash.filter(|f| *f > 0.) {
                vertices.render_button(
                    pass,
                    builder,
                    self.atlas.get(&packet.texture).as_source(),
                    matrix,
                    Vec4::ZERO,
                    Vec4::new(1., 1., 1., flash),
                );
            } else {
                vertices.render_sprite(
                    pass,
                    builder,
                    self.atlas.get(&packet.texture).as_source(),
                    matrix,
                );
            }
        }
        if let Some(previous) = &self.previous {
            previous.render(pass);
        }
        Ok(())
    }
}

async fn prepare(
    assets: &AssetServer,
    scenario: &Gerokasu2Scenario,
    action: Action,
) -> Result<Vec<(u32, PreparedBasicDrawable)>> {
    let requests = match action {
        Action::LoadBustup { id } => vec![(10, 3, id)],
        Action::LoadBackground { primary, secondary } => {
            vec![(0, 2, primary), (1, 2, secondary.unwrap_or(u16::MAX))]
        }
        _ => anyhow::bail!("Ger Bustup action has no resource request"),
    };
    let mut prepared = Vec::new();
    for (slot, kind, id) in requests {
        let request =
            LayerFactoryRequest::resolve(scenario, kind, (i32::from(id), 0, 0, 0, 0, 0, 0, 0))
                .context("Missing Ger Bustup factory")?;
        prepared.push((slot, PreparedBasicDrawable::load(assets, &request).await?));
    }
    Ok(prepared)
}
fn install(scene: &mut LayerGroup<BasicDrawable>, prepared: Vec<(u32, PreparedBasicDrawable)>) {
    for (slot, prepared) in prepared {
        let mut layer = prepared.construct();
        let bank = LayerbankId::new_unchecked(slot as u8);
        let previous = scene.get_layer(bank).map(|old| old.properties().clone());
        let replacing = previous.is_some();
        if let Some(properties) = previous {
            layer.set_properties(properties);
        } else {
            layer.set_properties(LayerProperties::with_policy(PropertyPolicy::Gerokasu2));
            let bg = bustup_gallery::background_properties(slot as u8);
            let values = if slot == u32::from(bustup_gallery::BUSTUP_SLOT) {
                bustup_gallery::BUSTUP_PROPERTIES
            } else {
                &bg
            };
            for &(p, value) in values {
                layer
                    .properties_mut()
                    .property_tweener_mut(LayerProperty::from_usize(p as usize))
                    .fast_forward_to(value);
            }
        }
        scene.add_layer_with_wiper(
            bank,
            layer,
            replacing.then(|| {
                crate::wiper::DefaultWiper::new(Ticks::from_u32(bustup_gallery::REPLACEMENT_TICKS))
                    .into()
            }),
        );
    }
}
struct Target<'a>(&'a mut Controller);
impl InputAreaTarget for Target<'_> {
    fn probe(&mut self, p: Vec2) -> bool {
        if !p.is_finite() || p.x < 0. || p.y < 0. || p.x >= 1920. || p.y >= 1080. {
            return false;
        }
        if self.0.in_viewer() {
            self.0.viewer_probe();
            true
        } else {
            self.0.probe(p.to_array())
        }
    }
    fn reset(&mut self) {
        self.0.reset();
    }
    fn press(&mut self, _: Vec2) {
        if !self.0.in_viewer() {
            self.0.press();
        }
    }
    fn release(&mut self, _: Vec2) {
        if self.0.in_viewer() {
            self.0.viewer_release();
        } else {
            self.0.release();
        }
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, old: Vec2, new: Vec2) {
        if self.0.in_viewer() {
            self.0.viewer_drag(old.to_array(), new.to_array());
        } else {
            self.0.cancel();
        }
    }
    fn gesture(&mut self, velocity: Vec2) {
        if self.0.in_viewer() {
            self.0.viewer_gesture(velocity.to_array());
        }
    }
    fn mouse_gesture(&mut self, delta: Vec2, velocity: Vec2) {
        self.gesture(crate::adv::input_area::desktop_gesture_vector(
            delta, velocity,
        ));
    }
    fn cancel(&mut self) {
        if !self.0.in_viewer() {
            self.0.cancel();
        }
    }
}
