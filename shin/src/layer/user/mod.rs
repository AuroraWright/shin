use derivative::Derivative;
use from_variants::FromVariants;
use glam::vec4;
use shin_audio::AudioManager;
use shin_core::{
    format::scenario::{
        Scenario,
        info::{BustupInfoItem, MovieInfoItem, PictureInfoItem},
        instruction_elements::{TypedNumberArray, UntypedNumberArray, lower_number_array},
    },
    primitives::color::FloatColor4,
    vm::command::types::{LayerType, Volume},
};
use shin_render::{PassKind, render_pass::RenderPass, shaders::types::RenderClone};
use std::sync::Arc;
use tracing::debug;

use crate::{
    asset::{
        bustup::{Bustup, BustupArgs, CharacterId},
        movie::Movie,
        picture::Picture,
        system::AssetServer,
    },
    layer::{DrawableLayer, Layer, LayerProperties, render_params::TransformParams},
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext},
};

mod bustup_blink;
#[expect(unused)]
mod bustup_layer;
#[cfg(test)]
mod bustup_voice_tests;
pub mod cake_layer;
pub mod higurashi;
pub mod last_layer;
#[expect(unused)]
mod movie_layer;
pub(crate) use movie_layer::MovieArgs;
#[cfg(test)]
mod movie_owner_tests;
#[cfg(test)]
mod native_metadata_tests;
#[cfg(test)]
mod native_movie_link_tests;
#[cfg(test)]
mod plain_owner_tests;
#[cfg(test)]
mod resource_clone_tests;
#[cfg(test)]
pub(crate) use movie_layer::{ReferencePreparationGate, reference_preparation_gate};
#[cfg(test)]
pub(crate) use murderer::parent_tests::reference_parent_properties_and_capture;
mod null_layer;
mod picture_layer;
#[cfg(test)]
pub(crate) use bustup_blink::BustupBlink;
pub(crate) use bustup_layer::BustupLayerImpl;
pub(crate) use picture_layer::{PictureBlockParams, PictureBlockPassKind, render_block};
mod effect_layer;
pub(crate) use effect_layer::EffectLayer;
mod focus_line_layer;
pub(crate) mod focus_line_model;
#[cfg(test)]
mod focus_owner_tests;
pub mod purple_text;
mod purple_view;
#[cfg(test)]
pub(crate) use purple_view::reference_transcript_colors_render;
#[cfg(test)]
mod effects_atlas_tests;
#[cfg(test)]
mod particle_artwork_failure_tests;
pub mod quiz_layer;
mod rain_layer;
#[cfg(test)]
mod rain_owner_tests;
pub(crate) mod rain_particles;
mod sakura_layer;
pub(crate) mod sakura_particles;
pub(crate) use focus_line_layer::FocusLineLayer;
pub(crate) use focus_line_model::Ray as FocusRay;
pub(crate) use rain_layer::RainLayer;
pub(crate) use rain_particles::Particle as RainParticle;
pub(crate) use sakura_layer::{PreparedSakuraLayer, SakuraLayer};
#[cfg(test)]
pub(crate) use sakura_particles::{SakuraArithmetic, SakuraParticles};
mod tile_layer;
pub mod murderer;

pub use self::{
    bustup_layer::BustupLayer, movie_layer::MovieLayer, null_layer::NullLayer,
    picture_layer::PictureLayer, tile_layer::TileLayer,
};

/// Native loaders distinguish worker-built prototypes from per-bank factories.
/// Preparing an atlas must not consume constructor RNG or create particle state.
pub(crate) enum PreparedUserLayer {
    Prototype(UserLayer),
    FocusLine(crate::asset::effects::EffectsAtlasBorrow, i32, i32),
    Rain(crate::asset::effects::EffectsAtlasBorrow, i32, i32, i32),
    Animation(sakura_layer::PreparedSakuraLayer),
    Quiz(Arc<quiz_layer::QuizTextures>, i32),
    Cake(Arc<cake_layer::CakeTextures>),
    Murderer(murderer::accusation::PreparedMurdererLayer),
    Last(Arc<last_layer::LastTextures>),
    InvalidInteractive(i32),
    InvalidAnimation(i32),
}

impl PreparedUserLayer {
    pub(crate) fn publish(
        &self,
        context: &mut shin_render::shaders::types::RenderCloneCtx,
    ) -> UserLayer {
        match self {
            Self::Prototype(layer) => layer.render_clone(context),
            Self::FocusLine(textures, x, y) => {
                focus_line_layer::FocusLineLayer::new(textures.clone(), *x, *y).into()
            }
            Self::Rain(textures, mode, min, max) => {
                rain_layer::RainLayer::new(textures.clone(), *mode, *min, *max).into()
            }
            Self::Animation(prepared) => prepared.construct().into(),
            Self::Quiz(textures, mask) => {
                quiz_layer::QuizLayer::new(textures.clone(), *mask).into()
            }
            Self::Cake(textures) => cake_layer::CakeLayer::new(textures.clone()).into(),
            Self::Murderer(prepared) => prepared.construct(false).into(),
            Self::Last(textures) => last_layer::LastLayer::new(textures.clone()).into(),
            Self::InvalidInteractive(kind) => panic!(
                "Invalid interactive layer mode {kind}: native factory returned no drawable layer"
            ),
            Self::InvalidAnimation(mode) => {
                panic!("Invalid Animation mode {mode}: native factory returned no drawable layer")
            }
        }
    }
}

#[derive(Derivative, RenderClone, FromVariants)]
#[derivative(Debug)]
pub enum UserLayer {
    #[derivative(Debug = "transparent")]
    Null(NullLayer),
    #[derivative(Debug = "transparent")]
    Picture(#[render_clone(needs_render)] PictureLayer),
    #[derivative(Debug = "transparent")]
    Bustup(#[render_clone(needs_render)] BustupLayer),
    #[derivative(Debug = "transparent")]
    Tile(#[render_clone(needs_render)] TileLayer),
    #[derivative(Debug = "transparent")]
    Movie(#[render_clone(needs_render)] MovieLayer),
    #[derivative(Debug = "transparent")]
    Quiz(#[render_clone(needs_render)] quiz_layer::QuizLayer),
    #[derivative(Debug = "transparent")]
    Cake(#[render_clone(needs_render)] cake_layer::CakeLayer),
    #[derivative(Debug = "transparent")]
    Murderer(#[render_clone(needs_render)] murderer::accusation::MurdererAccusationView),
    #[derivative(Debug = "transparent")]
    Rain(#[render_clone(needs_render)] rain_layer::RainLayer),
    #[derivative(Debug = "transparent")]
    Animation(#[render_clone(needs_render)] sakura_layer::SakuraLayer),
    #[derivative(Debug = "transparent")]
    Effect(#[render_clone(needs_render)] effect_layer::EffectLayer),
    #[derivative(Debug = "transparent")]
    FocusLine(#[render_clone(needs_render)] focus_line_layer::FocusLineLayer),
    #[derivative(Debug = "transparent")]
    Last(#[render_clone(needs_render)] last_layer::LastLayer),
}

/// A direct view borrows an already-scoped layer; an owned view retains the
/// actual UserLayer cell and locks it only for each callback. Neither path copies
/// controllers, drawable state or resources, or keeps a lock across frame updates.
pub enum InteractiveView<'a, T> {
    Borrowed(&'a T),
    Owned {
        owner: crate::layer::owner::LayerOwner<UserLayer>,
        project: fn(&UserLayer) -> &T,
    },
}
impl<T> InteractiveView<'_, T> {
    fn with<R>(&self, callback: impl FnOnce(&T) -> R) -> R {
        match self {
            Self::Borrowed(layer) => callback(layer),
            Self::Owned { owner, project } => callback(project(&owner.read())),
        }
    }
}
pub enum InteractiveLayerRef<'a> {
    Quiz(InteractiveView<'a, quiz_layer::QuizLayer>),
    Cake(InteractiveView<'a, cake_layer::CakeLayer>),
    Murderer(InteractiveView<'a, murderer::accusation::MurdererAccusationView>),
    Last(InteractiveView<'a, last_layer::LastLayer>),
}
impl InteractiveLayerRef<'static> {
    pub(crate) fn from_owned(owner: crate::layer::owner::LayerOwner<UserLayer>) -> Option<Self> {
        let kind = match &*owner.read() {
            UserLayer::Quiz(_) => 0,
            UserLayer::Cake(_) => 1,
            UserLayer::Murderer(_) => 2,
            UserLayer::Last(_) => 3,
            _ => return None,
        };
        Some(match kind {
            0 => Self::Quiz(InteractiveView::Owned {
                owner,
                project: |layer| {
                    let UserLayer::Quiz(layer) = layer else {
                        unreachable!()
                    };
                    layer
                },
            }),
            1 => Self::Cake(InteractiveView::Owned {
                owner,
                project: |layer| {
                    let UserLayer::Cake(layer) = layer else {
                        unreachable!()
                    };
                    layer
                },
            }),
            2 => Self::Murderer(InteractiveView::Owned {
                owner,
                project: |layer| {
                    let UserLayer::Murderer(layer) = layer else {
                        unreachable!()
                    };
                    layer
                },
            }),
            3 => Self::Last(InteractiveView::Owned {
                owner,
                project: |layer| {
                    let UserLayer::Last(layer) = layer else {
                        unreachable!()
                    };
                    layer
                },
            }),
            _ => unreachable!(),
        })
    }
}
impl InteractiveLayerRef<'_> {
    pub fn handle_controller_input(
        &self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
        scroll_axis: f32,
        axes: glam::Vec2,
    ) -> bool {
        match self {
            Self::Murderer(layer) => layer.handle_controller_input_with_scroll(state, scroll_axis),
            Self::Quiz(layer) => {
                layer.handle_controller_input(state);
                true
            }
            Self::Cake(layer) => {
                layer.handle_controller_input(state, axes);
                true
            }
            Self::Last(layer) => {
                layer.handle_controller_input(state);
                true
            }
        }
    }
    pub fn allows_playback_mode_changes(&self) -> bool {
        match self {
            Self::Murderer(layer) => layer.allows_playback_mode_changes(),
            // Native Cake/Quiz/Last overlay +0x58 returns true. Their +0x70
            // still blocks execution while the interactive command owns input.
            _ => true,
        }
    }
    pub fn pointer(&self, position: glam::Vec2, clicked: bool) {
        match self {
            Self::Quiz(layer) => layer.pointer(position, clicked),
            Self::Cake(layer) => layer.pointer(position, clicked),
            Self::Murderer(layer) => layer.pointer(position, clicked),
            Self::Last(layer) => layer.pointer(position, clicked),
        }
    }
    pub(crate) fn pointer_frame(
        &self,
        frame: &crate::adv::input_area::PointerFrame<'_>,
        extent: (u16, u16),
        canvas_position: glam::Vec2,
    ) -> bool {
        match self {
            Self::Quiz(layer) => {
                layer.pointer_frame(frame, extent, canvas_position);
                true
            }
            Self::Cake(layer) => {
                layer.pointer_frame(frame, extent);
                true
            }
            Self::Last(layer) => {
                layer.pointer_frame(frame, extent);
                true
            }
            _ => false,
        }
    }
    pub fn pointer_state(&self, position: glam::Vec2, clicked: bool, held: bool, seconds: f64) {
        match self {
            Self::Murderer(layer) => layer.pointer_state_with_time(position, held, seconds),
            _ => self.pointer(position, clicked),
        }
    }
    pub fn ignore_pointer_state(&self, held: bool) {
        match self {
            Self::Murderer(layer) => layer.ignore_pointer_state(held),
            Self::Quiz(layer) => layer.ignore_pointer_state(),
            Self::Cake(layer) => layer.ignore_pointer_state(),
            Self::Last(layer) => layer.ignore_pointer_state(),
        }
    }
    pub fn navigate(&self, delta: i32) {
        match self {
            Self::Quiz(layer) => layer.navigate(delta),
            Self::Cake(layer) => layer.navigate(delta),
            Self::Murderer(layer) => layer.navigate(delta),
            Self::Last(layer) => layer.navigate(delta),
        }
    }
    pub fn confirm(&self) {
        match self {
            Self::Quiz(layer) => layer.confirm(),
            Self::Cake(layer) => layer.confirm(),
            Self::Murderer(layer) => layer.confirm(),
            Self::Last(layer) => layer.confirm(),
        }
    }
    pub fn take_sounds(&self) -> Vec<&'static str> {
        match self {
            Self::Last(layer) => layer.take_sounds(),
            Self::Quiz(layer) => layer.take_sounds(),
            Self::Cake(layer) => layer.take_sounds(),
            Self::Murderer(layer) => layer.take_sounds(),
        }
    }
    pub fn result(&self) -> Option<i32> {
        match self {
            Self::Quiz(layer) => layer.result(),
            Self::Cake(layer) => layer.result(),
            Self::Murderer(layer) => layer.result(),
            Self::Last(layer) => layer.result(),
        }
    }
    pub fn back(&self) {
        match self {
            Self::Murderer(layer) => layer.back(),
            Self::Last(layer) => layer.back(),
            _ => {}
        }
    }
    pub fn navigate_horizontal(&self, delta: i32) {
        if let Self::Last(layer) = self {
            layer.navigate(delta);
        }
    }
    pub fn scroll_text(&self, delta: f32) {
        if let Self::Murderer(layer) = self {
            layer.scroll_text(delta);
        }
    }
}

impl UserLayer {
    pub(crate) async fn prepare_checked(
        device: &wgpu::Device,
        asset_server: &Arc<AssetServer>,
        audio_manager: &Arc<shin_audio::AudioManager>,
        scenario: &Arc<Scenario>,
        movie_volume_settings: crate::audio::MovieVolumeSettings,
        layer_ty: LayerType,
        params: UntypedNumberArray,
    ) -> anyhow::Result<PreparedUserLayer> {
        Ok(match layer_ty {
            LayerType::FocusLine => {
                let (x, y, ..): TypedNumberArray = lower_number_array(params);
                PreparedUserLayer::FocusLine(
                    asset_server.effects_atlas_for("focusline").await?,
                    x,
                    y,
                )
            }
            LayerType::Rain => {
                let (mode, min, max, ..): TypedNumberArray = lower_number_array(params);
                PreparedUserLayer::Rain(
                    asset_server
                        .effects_atlas_for(rain_layer::RainLayer::texture_name(mode))
                        .await?,
                    mode,
                    min,
                    max,
                )
            }
            LayerType::Animation => {
                let (mode, ..): TypedNumberArray = lower_number_array(params);
                // Native8489c retains the possibly-null factory result until
                // LAYERLOAD publication. Preparation must not fail ahead of
                // its independent transition readiness gate.
                match sakura_layer::SakuraLayer::prepare_checked(asset_server, mode).await? {
                    Some(prepared) => PreparedUserLayer::Animation(prepared),
                    None => PreparedUserLayer::InvalidAnimation(mode),
                }
            }
            LayerType::Quiz => {
                let (kind, mask, ..): TypedNumberArray = lower_number_array(params);
                match kind {
                    0 => PreparedUserLayer::Quiz(asset_server.load("/quiz.txa").await?, mask),
                    1 => PreparedUserLayer::Cake(asset_server.load("/cake.txa").await?),
                    2 => PreparedUserLayer::Murderer(
                        murderer::accusation::MurdererAccusationView::prepare(asset_server, mask)
                            .await,
                    ),
                    3 => PreparedUserLayer::Last(asset_server.load("/last.txa").await?),
                    // Native849bc rejects unsigned kind>3, including negatives.
                    // Keep its failed factory result until publication readiness.
                    _ => PreparedUserLayer::InvalidInteractive(kind),
                }
            }
            _ => PreparedUserLayer::Prototype(
                Self::load_checked(
                    device,
                    asset_server,
                    audio_manager,
                    scenario,
                    movie_volume_settings,
                    layer_ty,
                    params,
                )
                .await?,
            ),
        })
    }

    pub fn interactive(&self) -> Option<InteractiveLayerRef<'_>> {
        match self {
            Self::Quiz(layer) => Some(InteractiveLayerRef::Quiz(InteractiveView::Borrowed(layer))),
            Self::Cake(layer) => Some(InteractiveLayerRef::Cake(InteractiveView::Borrowed(layer))),
            Self::Murderer(layer) => Some(InteractiveLayerRef::Murderer(
                InteractiveView::Borrowed(layer),
            )),
            Self::Last(layer) => Some(InteractiveLayerRef::Last(InteractiveView::Borrowed(layer))),
            _ => None,
        }
    }

    pub(crate) fn is_resource_free(layer_ty: LayerType) -> bool {
        matches!(
            layer_ty,
            LayerType::Null | LayerType::Tile | LayerType::Effect
        )
    }

    /// Native immediate loaders construct on the publication thread, per bank.
    pub(crate) fn load_resource_free(
        layer_ty: LayerType,
        params: UntypedNumberArray,
    ) -> Option<Self> {
        match layer_ty {
            LayerType::Null => Some(NullLayer::new().into()),
            LayerType::Tile => {
                let (color, offset_x, offset_y, width, height, ..): TypedNumberArray =
                    lower_number_array(params);
                let color = FloatColor4::from_4bpp_property(color);
                let rect = vec4(
                    offset_x as f32,
                    offset_y as f32,
                    width as f32,
                    height as f32,
                );

                Some(TileLayer::new(color, rect).into())
            }
            LayerType::Effect => Some(effect_layer::EffectLayer::new().into()),
            _ => None,
        }
    }

    pub async fn load(
        device: &wgpu::Device,
        asset_server: &AssetServer,
        audio_manager: &Arc<AudioManager>,
        scenario: &Scenario,
        movie_volume_settings: crate::audio::MovieVolumeSettings,
        layer_ty: LayerType,
        params: UntypedNumberArray,
    ) -> Self {
        Self::load_checked(
            device,
            asset_server,
            audio_manager,
            scenario,
            movie_volume_settings,
            layer_ty,
            params,
        )
        .await
        .expect("Failed to load user layer")
    }

    /// Saved-scene preparation must report resource IO/decode errors without
    /// panicking out of its asynchronous producer.
    pub(crate) async fn load_checked(
        device: &wgpu::Device,
        asset_server: &AssetServer,
        audio_manager: &Arc<shin_audio::AudioManager>,
        scenario: &Scenario,
        movie_volume_settings: crate::audio::MovieVolumeSettings,
        layer_ty: LayerType,
        params: UntypedNumberArray,
    ) -> anyhow::Result<Self> {
        Ok(match layer_ty {
            LayerType::Null | LayerType::Tile | LayerType::Effect => {
                Self::load_resource_free(layer_ty, params).expect("Resource-free factory")
            }
            LayerType::Picture => {
                let pic_id = params.0;
                let Some(pic_info @ PictureInfoItem { name, linked_cg_id }) =
                    scenario.info_tables().picture_info.get(pic_id as usize)
                else {
                    // Native factory207a8/20c50 requests a Null drawable when
                    // the metadata getter rejects a negative/out-of-range ID.
                    return Ok(NullLayer::new().into());
                };
                debug!("Load picture: {} -> {} {:?}", pic_id, name, linked_cg_id);
                let pic = asset_server.load::<Picture, _>(pic_info.path()).await?;
                PictureLayer::new(pic, Some(name.to_string())).into()
            }
            LayerType::Bustup => {
                let bup_id = params.0;
                let Some(
                    bup_info @ BustupInfoItem {
                        name,
                        emotion,
                        lipsync_character_id,
                    },
                ) = scenario.info_tables().bustup_info.get(bup_id as usize)
                else {
                    // Native207fc/20cb4 uses the same Null request fallback.
                    return Ok(NullLayer::new().into());
                };
                debug!(
                    "Load bustup: {} -> {} {} {}",
                    bup_id, name, emotion, lipsync_character_id
                );
                let bup = asset_server
                    .load_with_args::<Bustup, _>(
                        bup_info.path(),
                        BustupArgs {
                            expression: emotion.to_string(),
                            character_id: CharacterId::from_metadata(*lipsync_character_id),
                            disable_animations: false,
                        },
                    )
                    .await?;

                BustupLayer::new(bup, Some(name.to_string())).into()
            }
            LayerType::Movie => {
                let (movie_id, volume, repeat, ..): TypedNumberArray<i32, Volume> =
                    lower_number_array(params);
                let Some(
                    movie_info @ &MovieInfoItem {
                        ref name,
                        linked_picture_id,
                        volume_source,
                        transparency,
                        linked_bgm_id,
                    },
                ) = scenario.info_tables().movie_info.get(movie_id as usize)
                else {
                    // Native20b38/20d18 uses the same Null request fallback.
                    return Ok(NullLayer::new().into());
                };
                let still_info = scenario
                    .info_tables()
                    .picture_info
                    .get(linked_picture_id.into_i32() as usize);
                debug!(
                    "Load movie: {movie_id} -> {name} {linked_picture_id} {volume_source:?} {transparency:?} {linked_bgm_id:?}"
                );
                let movie = asset_server.load::<Movie, _>(movie_info.path()).await?;

                // Native21050/21058 leaves the still filename empty on a
                // rejected signed16-bit cross-reference. Movie worker839a0
                // then skips its Picture factory and still constructs Movie.
                let still_picture = if let Some(info) = still_info {
                    Some(asset_server.load::<Picture, _>(info.path()).await?)
                } else {
                    None
                };

                let args = MovieArgs {
                    volume_source,
                    transparency,
                    local_volume: volume,
                    repeat: repeat & 1 != 0,
                };

                MovieLayer::new(
                    device,
                    audio_manager,
                    movie,
                    args,
                    still_picture,
                    movie_volume_settings,
                )
                .await
                .into()
            }
            LayerType::Quiz => {
                let (kind, mask, ..): TypedNumberArray = lower_number_array(params);
                match kind {
                    0 => quiz_layer::QuizLayer::load(asset_server, mask).await.into(),
                    1 => cake_layer::CakeLayer::load(asset_server).await.into(),
                    2 => murderer::accusation::MurdererAccusationView::load_menu(asset_server, mask)
                        .await
                        .into(),
                    3 => last_layer::LastLayer::load(asset_server).await.into(),
                    _ => panic!(
                        "Invalid interactive layer mode {kind}: native factory returned no drawable layer"
                    ),
                }
            }
            LayerType::Animation => {
                let (mode, ..): TypedNumberArray = lower_number_array(params);
                sakura_layer::SakuraLayer::prepare_checked(asset_server, mode)
                    .await?
                    .expect("Invalid Animation mode: native constructor returns a null layer")
                    .construct()
                    .into()
            }
            LayerType::FocusLine => {
                let (x, y, ..): TypedNumberArray = lower_number_array(params);
                focus_line_layer::FocusLineLayer::new(
                    asset_server.effects_atlas_for("focusline").await?,
                    x,
                    y,
                )
                .into()
            }
            LayerType::Rain => {
                let (mode, min_distance, max_distance, ..): TypedNumberArray =
                    lower_number_array(params);
                rain_layer::RainLayer::new(
                    asset_server
                        .effects_atlas_for(rain_layer::RainLayer::texture_name(mode))
                        .await?,
                    mode,
                    min_distance,
                    max_distance,
                )
                .into()
            }
        })
    }
}

impl AdvUpdatable for UserLayer {
    fn update(&mut self, context: &AdvUpdateContext) {
        match self {
            Self::Null(layer) => layer.update(context),
            Self::Picture(layer) => layer.update(context),
            Self::Bustup(layer) => layer.update(context),
            Self::Tile(layer) => layer.update(context),
            Self::Rain(layer) => layer.update(context),
            Self::Animation(layer) => layer.update(context),
            Self::Effect(layer) => layer.update(context),
            Self::FocusLine(layer) => layer.update(context),
            Self::Movie(layer) => layer.update(context),
            Self::Quiz(layer) => layer.update(context),
            Self::Cake(layer) => layer.update(context),
            Self::Murderer(layer) => layer.update(context),
            Self::Last(layer) => layer.update(context),
        }
    }
}

impl DrawableLayer for UserLayer {
    fn set_properties(&mut self, properties: LayerProperties) {
        match self {
            Self::Murderer(layer) => layer.set_properties(properties),
            _ => *self.properties_mut() = properties,
        }
    }
    fn properties(&self) -> &LayerProperties {
        match self {
            Self::Null(layer) => layer.properties(),
            Self::Picture(layer) => layer.properties(),
            Self::Bustup(layer) => layer.properties(),
            Self::Tile(layer) => layer.properties(),
            Self::Rain(layer) => layer.properties(),
            Self::Animation(layer) => layer.properties(),
            Self::Effect(layer) => layer.properties(),
            Self::FocusLine(layer) => layer.properties(),
            Self::Movie(layer) => layer.properties(),
            Self::Quiz(layer) => layer.properties(),
            Self::Cake(layer) => layer.properties(),
            Self::Murderer(layer) => layer.properties(),
            Self::Last(layer) => layer.properties(),
        }
    }

    fn properties_mut(&mut self) -> &mut LayerProperties {
        match self {
            Self::Null(layer) => layer.properties_mut(),
            Self::Picture(layer) => layer.properties_mut(),
            Self::Bustup(layer) => layer.properties_mut(),
            Self::Tile(layer) => layer.properties_mut(),
            Self::Rain(layer) => layer.properties_mut(),
            Self::Animation(layer) => layer.properties_mut(),
            Self::Effect(layer) => layer.properties_mut(),
            Self::FocusLine(layer) => layer.properties_mut(),
            Self::Movie(layer) => layer.properties_mut(),
            Self::Quiz(layer) => layer.properties_mut(),
            Self::Cake(layer) => layer.properties_mut(),
            Self::Murderer(layer) => layer.properties_mut(),
            Self::Last(layer) => layer.properties_mut(),
        }
    }
}

impl Layer for UserLayer {
    fn pre_render_with_owned_lower(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[crate::layer::owner::RetainedLayer],
    ) -> bool {
        if let Self::Effect(layer) = self {
            layer.pre_render_with_owned_lower(context, transform, lower)
        } else {
            self.pre_render(context, transform);
            false
        }
    }
    fn needs_lower_capture(&self) -> bool {
        matches!(self, Self::Effect(_))
    }
    fn pre_render_with_lower(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[&dyn Layer],
    ) -> bool {
        if let Self::Effect(layer) = self {
            layer.pre_render_with_lower(context, transform, lower)
        } else {
            self.pre_render(context, transform);
            false
        }
    }
    fn fast_forward(&mut self) {
        match self {
            Self::Null(layer) => layer.fast_forward(),
            Self::Picture(layer) => layer.fast_forward(),
            Self::Bustup(layer) => layer.fast_forward(),
            Self::Tile(layer) => layer.fast_forward(),
            Self::Rain(layer) => layer.fast_forward(),
            Self::Animation(layer) => layer.fast_forward(),
            Self::Effect(layer) => layer.fast_forward(),
            Self::FocusLine(layer) => layer.fast_forward(),
            Self::Movie(layer) => layer.fast_forward(),
            Self::Quiz(layer) => layer.fast_forward(),
            Self::Cake(layer) => layer.fast_forward(),
            Self::Murderer(layer) => layer.fast_forward(),
            Self::Last(layer) => layer.fast_forward(),
        }
    }

    fn get_stencil_bump(&self) -> u8 {
        match self {
            Self::Null(layer) => layer.get_stencil_bump(),
            Self::Picture(layer) => layer.get_stencil_bump(),
            Self::Bustup(layer) => layer.get_stencil_bump(),
            Self::Tile(layer) => layer.get_stencil_bump(),
            Self::Rain(layer) => layer.get_stencil_bump(),
            Self::Animation(layer) => layer.get_stencil_bump(),
            Self::Effect(layer) => layer.get_stencil_bump(),
            Self::FocusLine(layer) => layer.get_stencil_bump(),
            Self::Movie(layer) => layer.get_stencil_bump(),
            Self::Quiz(layer) => layer.get_stencil_bump(),
            Self::Cake(layer) => layer.get_stencil_bump(),
            Self::Murderer(layer) => layer.get_stencil_bump(),
            Self::Last(layer) => layer.get_stencil_bump(),
        }
    }

    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        match self {
            Self::Null(layer) => layer.pre_render(context, transform),
            Self::Picture(layer) => layer.pre_render(context, transform),
            Self::Bustup(layer) => layer.pre_render(context, transform),
            Self::Tile(layer) => layer.pre_render(context, transform),
            Self::Rain(layer) => layer.pre_render(context, transform),
            Self::Animation(layer) => layer.pre_render(context, transform),
            Self::Effect(layer) => layer.pre_render(context, transform),
            Self::FocusLine(layer) => layer.pre_render(context, transform),
            Self::Movie(layer) => layer.pre_render(context, transform),
            Self::Quiz(layer) => layer.pre_render(context, transform),
            Self::Cake(layer) => layer.pre_render(context, transform),
            Self::Murderer(layer) => layer.pre_render(context, transform),
            Self::Last(layer) => layer.pre_render(context, transform),
        }
    }

    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        match self {
            Self::Null(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Picture(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Bustup(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Tile(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Rain(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Animation(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Effect(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::FocusLine(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Movie(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Quiz(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Cake(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Murderer(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Last(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
        }
    }
}

#[cfg(test)]
pub(crate) use movie_layer::ReferenceMovieOwner;
#[cfg(test)]
pub(crate) use movie_layer::readback_test_texture;
