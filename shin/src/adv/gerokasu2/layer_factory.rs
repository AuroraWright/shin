//! Later-engine layer resource requests (original factory1f8c0).
use std::sync::Arc;

use anyhow::{Result, bail};
use glam::Vec4;
use shin_core::{
    format::scenario::{
        gerokasu2::{Dialect, Gerokasu2Scenario},
        instruction_elements::UntypedNumberArray,
    },
    primitives::color::FloatColor4,
};

use super::card_select::draw::{CardLayer, CardTextures};
use crate::{
    asset::{
        bustup::{BustupArgs, CharacterId, gerokasu2::Gerokasu2Bustup},
        picture::Picture,
        system::AssetServer,
    },
    layer::user::{BustupLayer, NullLayer, PictureLayer, TileLayer, UserLayer},
    layer::{DrawableLayer as _, Layer as _},
    update::AdvUpdatable as _,
};

#[derive(Debug, Clone)]
pub enum LayerFactoryRequest {
    Null,
    Tile {
        color: FloatColor4,
        rect: Vec4,
    },
    Picture {
        path: String,
    },
    Bustup {
        path: String,
        expression: String,
        character_id: i32,
        /// Gerokasu metadata placement: (scale, x, y), drawn by the layer as
        /// `T(x, -y) * S(scale)` around the bustup origin (native 351c0).
        placement: Option<[f32; 3]>,
    },
    Animation {
        mode: i32,
    },
    Effect,
    Movie {
        path: String,
        still: Option<String>,
        volume_source: u8,
        transparency: u8,
        volume: f32,
        repeat: bool,
    },
    FocusLine {
        x: f32,
        y: f32,
    },
    Rain {
        mode: i32,
        min: i32,
        max: i32,
    },
    CardSelect {
        first: i32,
        second: i32,
        third: i32,
    },
}

impl LayerFactoryRequest {
    /// Invalid metadata IDs produce the native Null request. An unrecognized
    /// raw kind produces no factory, which is distinct from a Null drawable.
    pub fn resolve(
        scenario: &Gerokasu2Scenario,
        kind: i32,
        params: UntypedNumberArray,
    ) -> Option<Self> {
        let index = usize::try_from(params.0).ok();
        if scenario.dialect == Dialect::Gerokasu {
            match kind {
                // Native 1f92c: param 1 selects the `%TIME%` suffix.
                2 => {
                    return Some(match index.and_then(|i| scenario.picture_info.get(i)) {
                        Some(info) => Self::Picture {
                            path: Gerokasu2Scenario::gerokasu_picture_path(
                                info.name.as_str(),
                                params.1,
                            ),
                        },
                        None => Self::Null,
                    });
                }
                // Native 1f97c: params are (entry, dress, face number, face
                // variant); be760 builds the path, bec10 formats "%03d" plus
                // the variant suffix ("" or "a").
                3 => {
                    return Some(match index.and_then(|i| scenario.bustup_info.get(i)) {
                        Some(info) => {
                            let [scale, x, y] = info.raw_placement;
                            Self::Bustup {
                                path: Gerokasu2Scenario::gerokasu_bustup_path(
                                    info.name.as_str(),
                                    params.1,
                                ),
                                expression: format!(
                                    "{:03}{}",
                                    params.2,
                                    if params.3 == 1 { "a" } else { "" }
                                ),
                                character_id: i32::from(info.lipsync_character_id as i16),
                                placement: Some([
                                    f32::from(scale) * 0.01,
                                    f32::from(x),
                                    f32::from(y),
                                ]),
                            }
                        }
                        None => Self::Null,
                    });
                }
                _ => {}
            }
        }
        Some(match kind {
            0 => Self::Null,
            1 => Self::Tile {
                color: FloatColor4::from_4bpp_property(params.0),
                rect: Vec4::new(
                    params.1 as f32,
                    params.2 as f32,
                    params.3 as f32,
                    params.4 as f32,
                ),
            },
            2 => match index.and_then(|i| scenario.picture_info.get(i)) {
                Some(info) => Self::Picture {
                    path: format!("/picture/{}.pic", info.name.as_str()),
                },
                None => Self::Null,
            },
            3 => match index.and_then(|i| scenario.bustup_info.get(i)) {
                Some(info) => Self::Bustup {
                    path: format!(
                        "/bustup/{}/{}.bup",
                        if params.1 == 0 { "M" } else { "L" },
                        info.name.as_str()
                    ),
                    expression: info.emotion.as_str().to_owned(),
                    character_id: i32::from(info.lipsync_character_id as i16),
                    placement: None,
                },
                None => Self::Null,
            },
            4 => Self::Animation { mode: params.0 },
            5 => Self::Effect,
            6 => match index.and_then(|i| scenario.movie_info.get(i)) {
                Some(info) => Self::Movie {
                    path: format!("/movie/{}.mp4", info.name.as_str()),
                    still: usize::try_from(info.linked_picture_id)
                        .ok()
                        .and_then(|i| scenario.picture_info.get(i))
                        .map(|p| format!("/picture/{}.pic", p.name.as_str())),
                    volume_source: info.volume_source,
                    transparency: info.transparency,
                    volume: params.1 as f32 * f32::from_bits(0x3a83126f),
                    repeat: params.2 & 1 != 0,
                },
                None => Self::Null,
            },
            7 => Self::FocusLine {
                x: params.0 as f32,
                y: params.1 as f32,
            },
            8 => Self::Rain {
                mode: params.0,
                min: params.1,
                max: params.2,
            },
            9 => Self::CardSelect {
                first: params.0,
                second: params.1,
                third: params.2,
            },
            _ => return None,
        })
    }
}

/// Assets are acquired before construction. The publication/clone/controller
/// owner chooses when to construct and bind properties; no implicit command
/// acknowledgment or transition completion happens in this module.
pub enum PreparedBasicDrawable {
    Null,
    Tile { color: FloatColor4, rect: Vec4 },
    Picture(Arc<Picture>, String),
    Bustup(Arc<Gerokasu2Bustup>, String, Option<[f32; 3]>),
    CardSelect(Arc<CardTextures>, i32),
    Movie(parking_lot::Mutex<crate::layer::user::MovieLayer>),
    Procedural(super::procedural_animation::PreparedProcedural),
    Rain(crate::asset::effects::EffectsAtlasBorrow, i32, i32, i32),
    FocusLine(crate::asset::effects::EffectsAtlasBorrow, glam::Vec2),
    InvalidAnimation(i32),
}
impl PreparedBasicDrawable {
    pub fn construct_movie(
        &self,
        ctx: &mut shin_render::shaders::types::RenderCloneCtx,
    ) -> Option<BasicDrawable> {
        use shin_render::shaders::types::RenderClone;
        if let Self::Movie(layer) = self {
            Some(BasicDrawable {
                layer: GerDrawable::Shared(layer.lock().render_clone(ctx).into()),
                retained_bustup: None,
            })
        } else {
            None
        }
    }
    pub async fn load(assets: &AssetServer, request: &LayerFactoryRequest) -> Result<Self> {
        Ok(match request {
            LayerFactoryRequest::Null => Self::Null,
            LayerFactoryRequest::Tile { color, rect } => Self::Tile {
                color: *color,
                rect: *rect,
            },
            LayerFactoryRequest::Picture { path } => {
                Self::Picture(assets.load(path).await?, path.clone())
            }
            LayerFactoryRequest::Bustup {
                path,
                expression,
                character_id,
                placement,
            } => Self::Bustup(
                assets
                    .load_with_args(
                        path,
                        BustupArgs {
                            expression: expression.clone(),
                            character_id: CharacterId::new(*character_id),
                            disable_animations: false,
                        },
                    )
                    .await?,
                format!("{path}[{expression}]"),
                *placement,
            ),
            LayerFactoryRequest::CardSelect { first, second, .. } => {
                anyhow::ensure!(
                    *first == 0,
                    "Ger CardSelect resource mode {first} is not connected"
                );
                super::card_select::CardSelectModel::new(*second, 1)?;
                Self::CardSelect(assets.load("/cardselect.txa").await?, *second)
            }
            LayerFactoryRequest::Animation { mode } => {
                match super::procedural_animation::PreparedProcedural::prepare(
                    assets,
                    4,
                    [*mode, 0, 0, 0, 0, 0, 0, 0],
                )
                .await?
                {
                    Some(prepared) => Self::Procedural(prepared),
                    None => Self::InvalidAnimation(*mode),
                }
            }
            LayerFactoryRequest::Effect => {
                Self::Procedural(super::procedural_animation::PreparedProcedural::Effect)
            }
            LayerFactoryRequest::FocusLine { x, y } => Self::FocusLine(
                assets.effects_atlas_for("focusline").await?,
                glam::vec2(*x, *y),
            ),
            LayerFactoryRequest::Rain { mode, min, max } => Self::Rain(
                assets
                    .effects_atlas_for(crate::layer::user::RainLayer::texture_name(*mode))
                    .await?,
                *mode,
                *min,
                *max,
            ),
            _ => bail!("Gerokasu2 drawable constructor is not connected for {request:?}"),
        })
    }

    /// A native Animation getter can return a nil drawable, distinct from Null.
    pub(crate) fn has_drawable(&self) -> bool {
        !matches!(self, Self::InvalidAnimation(_))
    }
    pub(crate) fn try_construct(&self) -> Result<BasicDrawable> {
        if let Self::InvalidAnimation(mode) = self {
            bail!("Ger Animation mode {mode} returned no drawable at publication");
        }
        Ok(self.construct())
    }

    pub fn construct(&self) -> BasicDrawable {
        if let Self::Movie(_) = self {
            unreachable!("Movie resources require render_clone construction");
        }
        if let Self::CardSelect(textures, mask) = self {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos();
            return BasicDrawable {
                layer: GerDrawable::Card(
                    super::card_select::draw::construct(textures.clone(), *mask, seed)
                        .expect("valid authored card mask"),
                ),
                retained_bustup: None,
            };
        }
        let mut retained_bustup = None;
        let mut layer: UserLayer = match self {
            Self::Null => NullLayer::new().into(),
            Self::Tile { color, rect } => TileLayer::new(*color, *rect).into(),
            Self::Picture(picture, name) => {
                PictureLayer::new(picture.clone(), Some(name.clone())).into()
            }
            Self::Bustup(bustup, name, placement) => {
                retained_bustup = Some(bustup.clone());
                let mut layer = BustupLayer::new_gerokasu2(bustup.0.clone(), name.clone());
                if let Some(placement) = placement {
                    layer.inner_mut().set_placement(*placement);
                }
                layer.into()
            }
            Self::Procedural(prepared) => prepared.construct(),
            Self::Rain(textures, mode, min, max) => {
                crate::layer::user::RainLayer::new_gerokasu2(textures.clone(), *mode, *min, *max)
                    .into()
            }
            Self::FocusLine(textures, offset) => {
                crate::layer::user::FocusLineLayer::new_gerokasu2(textures.clone(), *offset).into()
            }
            Self::InvalidAnimation(mode) => {
                panic!("Ger Animation mode {mode} returned no drawable at publication")
            }
            Self::CardSelect(..) | Self::Movie(..) => unreachable!(),
        };
        if matches!(
            self,
            Self::Procedural(_) | Self::Rain(..) | Self::FocusLine(..)
        ) && layer.properties().policy() != crate::layer::PropertyPolicy::Gerokasu2
        {
            layer.set_properties(crate::layer::LayerProperties::with_policy(
                crate::layer::PropertyPolicy::Gerokasu2,
            ));
        }
        BasicDrawable {
            layer: GerDrawable::Shared(layer),
            retained_bustup,
        }
    }
}

pub enum GerDrawable {
    Shared(UserLayer),
    Card(CardLayer),
}
impl std::ops::Deref for GerDrawable {
    type Target = dyn crate::layer::DrawableLayer;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Shared(v) => v,
            Self::Card(v) => v,
        }
    }
}
impl std::ops::DerefMut for GerDrawable {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Shared(v) => v,
            Self::Card(v) => v,
        }
    }
}
impl shin_render::shaders::types::RenderClone for GerDrawable {
    fn render_clone(&self, ctx: &mut shin_render::shaders::types::RenderCloneCtx) -> Self {
        match self {
            Self::Shared(v) => Self::Shared(v.render_clone(ctx)),
            Self::Card(v) => Self::Card(v.render_clone(ctx)),
        }
    }
}
pub struct BasicDrawable {
    pub layer: GerDrawable,
    // Keep the typed asset-registry owner alive with its drawable.
    retained_bustup: Option<Arc<Gerokasu2Bustup>>,
}
impl BasicDrawable {
    pub fn retained_bustup(&self) -> Option<&Arc<Gerokasu2Bustup>> {
        self.retained_bustup.as_ref()
    }
    pub fn render_clone(&self, context: &mut shin_render::shaders::types::RenderCloneCtx) -> Self {
        use shin_render::shaders::types::RenderClone;
        Self {
            layer: self.layer.render_clone(context),
            retained_bustup: self.retained_bustup.clone(),
        }
    }
}

// Keep the Ger registry owner in the shared group/replacement graph, including
// retained source sides and render clones, rather than in a parallel bank map.
impl shin_render::shaders::types::RenderClone for BasicDrawable {
    fn render_clone(&self, context: &mut shin_render::shaders::types::RenderCloneCtx) -> Self {
        BasicDrawable::render_clone(self, context)
    }
}
impl crate::update::AdvUpdatable for BasicDrawable {
    fn update(&mut self, context: &crate::update::AdvUpdateContext) {
        self.layer.update(context);
    }
}
impl crate::layer::Layer for BasicDrawable {
    fn fast_forward(&mut self) {
        self.layer.fast_forward();
    }
    fn get_stencil_bump(&self) -> u8 {
        self.layer.get_stencil_bump()
    }
    fn needs_lower_capture(&self) -> bool {
        self.layer.needs_lower_capture()
    }
    fn pre_render(
        &mut self,
        context: &mut crate::render::PreRenderContext,
        transform: &crate::layer::render_params::TransformParams,
    ) {
        self.layer.pre_render(context, transform);
    }
    fn pre_render_with_lower(
        &mut self,
        context: &mut crate::render::PreRenderContext,
        transform: &crate::layer::render_params::TransformParams,
        lower: &[&dyn crate::layer::Layer],
    ) -> bool {
        self.layer.pre_render_with_lower(context, transform, lower)
    }
    fn pre_render_with_owned_lower(
        &mut self,
        context: &mut crate::render::PreRenderContext,
        transform: &crate::layer::render_params::TransformParams,
        lower: &[crate::layer::owner::RetainedLayer],
    ) -> bool {
        self.layer
            .pre_render_with_owned_lower(context, transform, lower)
    }
    fn render(
        &self,
        pass: &mut shin_render::render_pass::RenderPass,
        transform: &crate::layer::render_params::TransformParams,
        stencil_ref: u8,
        kind: shin_render::PassKind,
    ) {
        self.layer.render(pass, transform, stencil_ref, kind);
    }
}
impl crate::layer::DrawableLayer for BasicDrawable {
    fn properties(&self) -> &crate::layer::LayerProperties {
        self.layer.properties()
    }
    fn properties_mut(&mut self) -> &mut crate::layer::LayerProperties {
        self.layer.properties_mut()
    }
    fn set_properties(&mut self, properties: crate::layer::LayerProperties) {
        self.layer.set_properties(properties);
    }
}

#[cfg(test)]
mod tests;
