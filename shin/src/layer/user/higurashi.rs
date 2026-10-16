//! Older drawable owners. Shared assets/GPU blocks are retained, while property
//! clocks, parent flags, projection and header scaling use Higurashi semantics.
use std::sync::Arc;

use anyhow::{Context, bail, ensure};
use shin_core::{
    format::scenario::{higurashi::HigurashiScenario, instruction_elements::UntypedNumberArray},
    time::Ticks,
    vm::command::types::LayerType,
};
use shin_render::{PassKind, RenderRequestBuilder, render_pass::RenderPass};

use super::{
    bustup_layer::BustupLayerImpl,
    picture_layer::{PictureBlockParams, PictureLayerImpl},
};
use crate::{
    asset::{
        bustup::{BustupArgs, CharacterId, higurashi::HigurashiBustup},
        picture::Picture,
        system::AssetServer,
    },
    layer::{
        higurashi_properties::HigurashiProperties, render_params::higurashi::HigurashiTransform,
    },
};

#[derive(Debug, Clone)]
enum Drawable {
    Null,
    Tile(tile::Tile),
    Picture(PictureLayerImpl),
    Bustup(BustupLayerImpl),
    Effect(effect::Owner),
    Animation(animation::Owner),
    Movie(movie::LiveOwner),
    Temporary(temporary::Owner),
    CharacterSelection(crate::adv::higurashi::character_selection::view::View),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HigurashiLayerKind {
    Null,
    Tile,
    Picture,
    Bustup,
    Animation,
    Blur,
    Mosaic,
    Movie,
    FocusLine,
    Rain,
    Raster,
    CharacterSelection,
    Tips,
    Notification,
    TextNotification,
}
impl HigurashiLayerKind {
    pub fn native_id(self) -> i32 {
        match self {
            Self::Null => 0,
            Self::Tile => 1,
            Self::Picture => 2,
            Self::Bustup => 3,
            Self::Animation => 4,
            Self::Blur => 5,
            Self::Mosaic => 6,
            Self::Movie => 7,
            Self::FocusLine => 8,
            Self::Rain => 9,
            Self::Raster => 10,
            Self::CharacterSelection => 11,
            Self::Tips => 12,
            Self::Notification => 13,
            Self::TextNotification => 14,
        }
    }
    pub fn from_native(value: i32) -> Option<Self> {
        Some(match value {
            0 => Self::Null,
            1 => Self::Tile,
            2 => Self::Picture,
            3 => Self::Bustup,
            4 => Self::Animation,
            5 => Self::Blur,
            6 => Self::Mosaic,
            7 => Self::Movie,
            8 => Self::FocusLine,
            9 => Self::Rain,
            10 => Self::Raster,
            11 => Self::CharacterSelection,
            12 => Self::Tips,
            13 => Self::Notification,
            14 => Self::TextNotification,
            _ => return None,
        })
    }
}
impl PartialEq<LayerType> for HigurashiLayerKind {
    fn eq(&self, other: &LayerType) -> bool {
        self.native_id() == *other as i32
    }
}

pub(crate) mod animation;
pub(super) mod block;
pub(crate) mod effect;
pub(crate) mod effect_capture;
pub(crate) mod leaf_clip;
pub(crate) mod movie;
mod native_float;
pub(crate) mod temporary;
mod tile;
mod tile_gamma;

/// Originalbc8a4/bca14 replace every '?' with pc for zero, cs for nonzero.
pub(crate) fn selected_asset_name(name: &str, setting: u8) -> String {
    name.replace('?', if setting == 0 { "pc" } else { "cs" })
        .to_ascii_lowercase()
}

#[cfg(test)]
pub(crate) mod fragment_tests;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub struct HigurashiUserLayer {
    pub properties: HigurashiProperties,
    owner_id: i32,
    drawable: Drawable,
}

impl HigurashiUserLayer {
    /// Four fresh native entropy seeds belong to the owner, not the asset cache.
    /// The bridge must supply them; copying this owner retains existing seeds.
    pub fn null(seeds: [u32; 4]) -> Self {
        Self {
            properties: HigurashiProperties::from_seeds(seeds),
            owner_id: 0,
            drawable: Drawable::Null,
        }
    }

    pub fn picture(picture: Arc<Picture>, name: Option<String>, seeds: [u32; 4]) -> Self {
        Self {
            properties: HigurashiProperties::from_seeds(seeds),
            owner_id: 0,
            drawable: Drawable::Picture(PictureLayerImpl::new(picture, name)),
        }
    }

    pub fn tile(
        color: shin_core::primitives::color::FloatColor4,
        rect: glam::Vec4,
        seeds: [u32; 4],
    ) -> Self {
        Self {
            properties: HigurashiProperties::from_seeds(seeds),
            owner_id: 0,
            drawable: Drawable::Tile(tile::Tile { color, rect }),
        }
    }

    pub fn bustup(bustup: Arc<HigurashiBustup>, name: Option<String>, seeds: [u32; 4]) -> Self {
        Self {
            properties: HigurashiProperties::from_seeds(seeds),
            owner_id: 0,
            drawable: Drawable::Bustup(BustupLayerImpl::new_higurashi(bustup, name)),
        }
    }

    pub fn character_selection(
        view: crate::adv::higurashi::character_selection::view::View,
        seeds: [u32; 4],
    ) -> Self {
        Self {
            properties: HigurashiProperties::from_seeds(seeds),
            owner_id: 0,
            drawable: Drawable::CharacterSelection(view),
        }
    }
    pub fn character_view(
        &self,
    ) -> Option<&crate::adv::higurashi::character_selection::view::View> {
        match &self.drawable {
            Drawable::CharacterSelection(view) => Some(view),
            Drawable::Temporary(owner) => owner.character_view(),
            _ => None,
        }
    }
    pub fn character_view_mut(
        &mut self,
    ) -> Option<&mut crate::adv::higurashi::character_selection::view::View> {
        match &mut self.drawable {
            Drawable::CharacterSelection(view) => Some(view),
            Drawable::Temporary(owner) => owner.character_view_mut(),
            _ => None,
        }
    }
    pub fn layer_type(&self) -> HigurashiLayerKind {
        match self.drawable {
            Drawable::Null => HigurashiLayerKind::Null,
            Drawable::Tile(_) => HigurashiLayerKind::Tile,
            Drawable::Picture(_) => HigurashiLayerKind::Picture,
            Drawable::Bustup(_) => HigurashiLayerKind::Bustup,
            Drawable::Movie(_) => HigurashiLayerKind::Movie,
            Drawable::Effect(ref effect) => HigurashiLayerKind::from_native(effect.kind()).unwrap(),
            Drawable::Animation(ref animation) => {
                HigurashiLayerKind::from_native(animation.native_id()).unwrap()
            }
            Drawable::Temporary(ref owner) => {
                HigurashiLayerKind::from_native(owner.native_id()).unwrap()
            }
            Drawable::CharacterSelection(_) => HigurashiLayerKind::CharacterSelection,
        }
    }
    /// Original virtual38 may return a NULL child (notably live Movie7).
    pub(crate) fn native_copy(&self) -> Option<Self> {
        if self.layer_type() == HigurashiLayerKind::Movie {
            None
        } else {
            Some(self.clone())
        }
    }
    pub(crate) fn owner_id(&self) -> i32 {
        self.owner_id
    }
    /// Original67c48 is independent of the VM's retained integer record.
    pub(crate) fn set_owner_id(&mut self, value: i32) {
        self.owner_id = value;
    }
    #[cfg(test)]
    pub(crate) fn reference_animation(&self) -> Option<&animation::Owner> {
        if let Drawable::Animation(owner) = &self.drawable {
            Some(owner)
        } else {
            None
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_effect(&self) -> Option<&effect::Owner> {
        if let Drawable::Effect(owner) = &self.drawable {
            Some(owner)
        } else {
            None
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_movie(&self) -> Option<&movie::LiveOwner> {
        if let Drawable::Movie(owner) = &self.drawable {
            Some(owner)
        } else {
            None
        }
    }

    pub fn advance(&mut self, delta: Ticks, voice: Option<&dyn Fn(i32, f32) -> Option<u8>>) {
        if let Drawable::Temporary(owner) = &mut self.drawable {
            owner.advance(delta);
            return;
        }
        if let Drawable::CharacterSelection(view) = &mut self.drawable {
            view.advance(delta);
            return;
        }
        if let Drawable::Animation(animation) = &mut self.drawable {
            animation.advance(delta, &mut self.properties);
            return;
        }
        self.properties.advance(delta);
        if let Drawable::Movie(movie) = &self.drawable {
            movie.advance();
        }
        if let Drawable::Effect(effect) = &mut self.drawable {
            effect.advance(delta, &self.properties);
        }
        if let Drawable::Bustup(bustup) = &mut self.drawable {
            bustup.update_higurashi_voice(&self.properties, voice);
        }
    }

    pub fn fast_forward(&mut self) {
        if let Drawable::Temporary(owner) = &mut self.drawable {
            owner.fast_forward();
            return;
        }
        if let Drawable::CharacterSelection(view) = &mut self.drawable {
            view.fast_forward();
            return;
        }
        self.properties.fast_forward();
    }

    /// Direct native PIC/BUP submission. Clipped draws need their older state
    /// preparation path before they can render here.
    pub fn render(
        &self,
        pass: &mut RenderPass,
        parent: &HigurashiTransform,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) -> anyhow::Result<()> {
        if let Drawable::Temporary(owner) = &self.drawable {
            return owner.render(pass, &self.properties, parent, stencil_ref, pass_kind);
        }
        if let Drawable::Effect(effect) = &self.drawable {
            return effect.render(pass, &self.properties, stencil_ref, pass_kind);
        }
        if let Drawable::Animation(animation) = &self.drawable {
            return animation.render(pass, &self.properties, parent, stencil_ref, pass_kind);
        }
        if let Drawable::CharacterSelection(view) = &self.drawable {
            return view.render(pass, &self.properties, parent, stencil_ref, pass_kind);
        }
        if let Drawable::Movie(movie) = &self.drawable {
            return movie.render(pass, &self.properties, parent, stencil_ref, pass_kind);
        }
        if matches!(self.drawable, Drawable::Null) || !self.properties.is_visible() {
            return Ok(());
        }
        let drawable = self.properties.drawable_params();
        let params = if let Drawable::Tile(tile) = &self.drawable {
            if pass_kind != tile.pass_kind(&drawable) {
                return Ok(());
            }
            None
        } else {
            let Some(params) = PictureBlockParams::setup(pass_kind, &drawable) else {
                return Ok(());
            };
            Some(params)
        };
        let transform = self.properties.composed_transform(parent);
        if let Drawable::Tile(tile) = &self.drawable {
            if drawable.fragment_shader.simplify(drawable.shader_param)
                == shin_render::LayerFragmentShader::Gamma
            {
                let values = self.properties.values();
                let endpoints = [values[44], values[45], values[46], values[47]];
                // Validate all material/geometry before the clip mutates depth.
                // Finite active masks reuse the original leaf clip transport.
                let clip = leaf_clip::Clip::from_endpoints(endpoints);
                if clip.active() {
                    let gamma = tile_gamma::ActiveClip::new(tile, &drawable, endpoints)?;
                    gamma.render(
                        pass,
                        transform.compute_tile_transform(),
                        stencil_ref,
                        pass_kind,
                    );
                } else {
                    let gamma = tile_gamma::NoMask::new(tile, &drawable, endpoints, clip)?;
                    gamma.render(
                        pass,
                        transform.compute_tile_transform(),
                        stencil_ref,
                        pass_kind,
                    );
                }
                return Ok(());
            }
            let clipped = leaf_clip::Clip::from_properties(&self.properties).prepare(pass);
            tile.render_clipped(
                pass,
                &drawable,
                transform.compute_tile_transform(),
                stencil_ref,
                pass_kind,
                clipped,
            );
            return Ok(());
        }
        let params = params.expect("Picture/Bustup pass parameters");
        let clipped = leaf_clip::Clip::from_properties(&self.properties).prepare(pass);
        let builder =
            RenderRequestBuilder::new().depth_stencil_shorthand(stencil_ref, false, clipped);
        match &self.drawable {
            Drawable::Null => {}
            Drawable::Tile(_) => unreachable!(),
            Drawable::Picture(picture) => picture.render_higurashi_blocks_with_asset_transform(
                pass,
                builder,
                params,
                transform.compute_native_asset_transform(picture.origin()),
            ),
            Drawable::Bustup(bustup) => bustup.render_higurashi_blocks_with_asset_transform(
                pass,
                builder,
                params,
                transform.compute_native_asset_transform(bustup.origin()),
            ),
            Drawable::Effect(_) => unreachable!(),
            Drawable::Animation(_) => unreachable!(),
            Drawable::Movie(_) => unreachable!(),
            Drawable::Temporary(_) => unreachable!(),
            Drawable::CharacterSelection(_) => unreachable!(),
        }
        Ok(())
    }

    /// Render an existing PIC with its single native properties owner. Gallery
    /// viewing does not construct a user layer, copy clocks or sample seeds.
    pub(crate) fn render_picture_with_properties(
        picture: &Arc<crate::asset::picture::Picture>,
        properties: &HigurashiProperties,
        pass: &mut RenderPass,
        parent: &HigurashiTransform,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) -> anyhow::Result<()> {
        if !properties.is_visible() {
            return Ok(());
        }
        let drawable = properties.drawable_params();
        let Some(params) = PictureBlockParams::setup(pass_kind, &drawable) else {
            return Ok(());
        };
        let transform = properties.composed_transform(parent);
        let clipped = leaf_clip::Clip::from_properties(properties).prepare(pass);
        let builder =
            RenderRequestBuilder::new().depth_stencil_shorthand(stencil_ref, false, clipped);
        PictureLayerImpl::render_higurashi_asset_with_transform(
            picture,
            &picture.label,
            pass,
            builder,
            params,
            transform
                .compute_native_asset_transform((picture.origin_x as i16, picture.origin_y as i16)),
        );
        Ok(())
    }

    #[cfg(test)]
    pub async fn load(
        asset_server: &AssetServer,
        scenario: &HigurashiScenario,
        layer_ty: LayerType,
        params: UntypedNumberArray,
        seeds: [u32; 4],
    ) -> anyhow::Result<Self> {
        Self::load_with_setting(asset_server, scenario, layer_ty, params, seeds, 0).await
    }
    /// Original20df0 samples settings+100 when preparing PIC/BUP factories.
    pub async fn load_with_setting(
        asset_server: &AssetServer,
        scenario: &HigurashiScenario,
        layer_ty: LayerType,
        params: UntypedNumberArray,
        seeds: [u32; 4],
        setting: u8,
    ) -> anyhow::Result<Self> {
        ensure!(
            asset_server.context().game == crate::cli::Game::Higurashi,
            "Higurashi layer requires a Higurashi asset server"
        );
        match layer_ty {
            LayerType::Null => Ok(Self::null(seeds)),
            LayerType::Tile => {
                let (color, x, y, width, height, ..) = params;
                Ok(Self::tile(
                    shin_core::primitives::color::FloatColor4::from_4bpp_property(color),
                    glam::vec4(x as f32, y as f32, width as f32, height as f32),
                    seeds,
                ))
            }
            LayerType::Picture => {
                // Native 20df0 constructs the Null layer (6fafc) when bc3d4 finds no record,
                // including negative IDs; Origin Tsumihoroboshi requests IDs past the table.
                let Some(info) = usize::try_from(params.0)
                    .ok()
                    .and_then(|id| scenario.picture_info.get(id))
                else {
                    return Ok(Self::null(seeds));
                };
                let name = selected_asset_name(info.name.as_str(), setting);
                let picture = asset_server
                    .load::<Picture, _>(format!("/picture/{name}.pic"))
                    .await?;
                Ok(Self::picture(picture, Some(name), seeds))
            }
            LayerType::Bustup => {
                // Native bc3fc misses fall back to the same Null layer.
                let Some(info) = usize::try_from(params.0)
                    .ok()
                    .and_then(|id| scenario.bustup_info.get(id))
                else {
                    return Ok(Self::null(seeds));
                };
                let name = selected_asset_name(info.name.as_str(), setting);
                let bustup = asset_server
                    .load_with_args::<HigurashiBustup, _>(
                        format!("/bustup/{name}.bup"),
                        BustupArgs {
                            expression: info.emotion.as_str().to_owned(),
                            character_id: CharacterId::from_metadata(info.lipsync_character_id),
                            disable_animations: false,
                        },
                    )
                    .await?;
                Ok(Self::bustup(bustup, Some(name), seeds))
            }
            _ => bail!("Higurashi layer type {layer_ty:?} is not implemented yet"),
        }
    }

    pub(crate) fn prepare_lower(
        &self,
        context: &mut crate::render::PreRenderContext,
        depth: &effect_capture::SharedDepth,
        lower: &[crate::adv::higurashi::group::DrawableChild<'_>],
        parent: &HigurashiTransform,
    ) -> anyhow::Result<bool> {
        if let Drawable::Effect(effect) = &self.drawable {
            effect.prepare(context, &self.properties, depth, lower, parent)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub(crate) fn reconstruction_refresh(&mut self) -> anyhow::Result<()> {
        if let Drawable::Temporary(owner) = &mut self.drawable {
            owner.refresh();
        }
        if let Drawable::Animation(animation) = &mut self.drawable {
            animation.warm_up(&self.properties)?;
        }
        Ok(())
    }
    pub(crate) fn movie_status(&self) -> Option<u32> {
        if let Drawable::Movie(movie) = &self.drawable {
            Some(movie.status())
        } else {
            None
        }
    }
    pub(crate) fn movie_fault(&self) -> Option<String> {
        if let Drawable::Movie(movie) = &self.drawable {
            movie.fault()
        } else {
            None
        }
    }
    pub(crate) fn prepare_movie(
        &self,
        context: &mut crate::render::PreRenderContext,
    ) -> anyhow::Result<()> {
        if let Drawable::Movie(movie) = &self.drawable {
            movie.prepare(context)?;
        }
        Ok(())
    }
    pub(crate) fn set_route_navigation(&mut self, value: bool) {
        match &mut self.drawable {
            Drawable::Temporary(owner) => owner.set_route_navigation(value),
            Drawable::CharacterSelection(view) => view.route_navigation = value,
            _ => {}
        }
    }
    pub(crate) fn temporary_owner(&self) -> Option<&temporary::Owner> {
        if let Drawable::Temporary(owner) = &self.drawable {
            Some(owner)
        } else {
            None
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_temporary(&self) -> Option<&temporary::Owner> {
        self.temporary_owner()
    }
}

/// Ready asset descriptors are separated from their live constructors. Native
/// loader+28 observes the outgoing owner at publication, after any load wait.
pub(crate) enum Prepared {
    Basic(HigurashiUserLayer),
    Effect(HigurashiLayerKind),
    Animation(animation::Prepared),
    Movie(Option<movie::Prepared>),
    Temporary(temporary::Prepared),
}
impl Prepared {
    pub(crate) fn requires_settings(&self) -> bool {
        matches!(self, Self::Temporary(prepared) if prepared.requires_settings())
            || matches!(self, Self::Movie(Some(_)))
    }
    pub(crate) fn requires_media(&self) -> bool {
        matches!(self, Self::Movie(Some(_)))
    }
    pub fn inherits_base(&self) -> bool {
        !matches!(self, Self::Effect(HigurashiLayerKind::Mosaic))
    }
    pub fn present(&self) -> bool {
        match self {
            Self::Animation(prepared) => prepared.present(),
            _ => true,
        }
    }
    pub async fn load(
        assets: &AssetServer,
        scenario: &HigurashiScenario,
        kind: HigurashiLayerKind,
        params: UntypedNumberArray,
        setting: u8,
    ) -> anyhow::Result<Self> {
        ensure!(
            assets.context().game == crate::cli::Game::Higurashi,
            "Higurashi layer requires a Higurashi asset server"
        );
        let basic = match kind {
            HigurashiLayerKind::Null => LayerType::Null,
            HigurashiLayerKind::Tile => LayerType::Tile,
            HigurashiLayerKind::Picture => LayerType::Picture,
            HigurashiLayerKind::Bustup => LayerType::Bustup,
            HigurashiLayerKind::Blur | HigurashiLayerKind::Mosaic | HigurashiLayerKind::Raster => {
                return Ok(Self::Effect(kind));
            }
            HigurashiLayerKind::Movie => {
                return Ok(Self::Movie(
                    movie::Prepared::load(
                        assets,
                        scenario,
                        &[
                            params.0, params.1, params.2, params.3, params.4, params.5, params.6,
                            params.7,
                        ],
                        setting,
                    )
                    .await?,
                ));
            }
            HigurashiLayerKind::Animation
            | HigurashiLayerKind::FocusLine
            | HigurashiLayerKind::Rain => {
                return Ok(Self::Animation(
                    animation::Prepared::load(
                        assets,
                        kind.native_id(),
                        &[
                            params.0, params.1, params.2, params.3, params.4, params.5, params.6,
                            params.7,
                        ],
                    )
                    .await?,
                ));
            }
            HigurashiLayerKind::CharacterSelection
            | HigurashiLayerKind::Tips
            | HigurashiLayerKind::Notification
            | HigurashiLayerKind::TextNotification => {
                return Ok(Self::Temporary(
                    temporary::Prepared::load(
                        assets,
                        scenario,
                        kind.native_id(),
                        &[
                            params.0, params.1, params.2, params.3, params.4, params.5, params.6,
                            params.7,
                        ],
                    )
                    .await?,
                ));
            }
        };
        Ok(Self::Basic(
            HigurashiUserLayer::load_with_setting(assets, scenario, basic, params, [0; 4], setting)
                .await?,
        ))
    }
    #[cfg(test)]
    pub fn construct(
        self,
        old: Option<&HigurashiUserLayer>,
        seeds: Option<[u32; 4]>,
    ) -> anyhow::Result<Option<HigurashiUserLayer>> {
        self.construct_with_settings(old, seeds, None)
    }
    pub(crate) fn construct_with_settings(
        self,
        old: Option<&HigurashiUserLayer>,
        seeds: Option<[u32; 4]>,
        settings: Option<&mut shin_core::format::save::higurashi::SaveData>,
    ) -> anyhow::Result<Option<HigurashiUserLayer>> {
        self.construct_with_services(old, seeds, settings, None)
    }
    pub(crate) fn construct_with_services(
        self,
        old: Option<&HigurashiUserLayer>,
        seeds: Option<[u32; 4]>,
        settings: Option<&mut shin_core::format::save::higurashi::SaveData>,
        media: Option<&movie::Services>,
    ) -> anyhow::Result<Option<HigurashiUserLayer>> {
        ensure!(
            !self.requires_settings() || settings.is_some(),
            "Live Higurashi derived layer requires the actual settings bank"
        );
        ensure!(
            !self.requires_media() || media.is_some(),
            "Live Higurashi Movie requires configured media services"
        );
        if !self.present() {
            return Ok(None);
        }
        // Loader706b0 passes NULL even when its caller supplied an old owner.
        let old = if matches!(self, Self::Effect(HigurashiLayerKind::Mosaic)) {
            None
        } else {
            old
        };
        let mut properties = if let Some(old) = old {
            old.properties.clone()
        } else {
            HigurashiProperties::from_seeds(
                seeds.context("Fresh Higurashi layer needs four OS seeds")?,
            )
        };
        let owner_id = old.map_or(0, HigurashiUserLayer::owner_id);
        Ok(Some(match self {
            Self::Basic(mut owner) => {
                owner.properties = properties;
                owner.owner_id = owner_id;
                owner
            }
            Self::Effect(kind) => HigurashiUserLayer {
                properties,
                owner_id,
                drawable: Drawable::Effect(effect::Owner::replacement(
                    kind.native_id(),
                    old.and_then(|old| {
                        if let Drawable::Effect(effect) = &old.drawable {
                            Some(effect)
                        } else {
                            None
                        }
                    }),
                )?),
            },
            Self::Animation(prepared) => {
                let elapsed = old.and_then(|old| {
                    if let Drawable::Animation(animation) = &old.drawable {
                        animation.elapsed()
                    } else {
                        None
                    }
                });
                let owner = prepared
                    .construct(&mut properties, elapsed, crate::engine_random::next_seed())?
                    .context("Present Higurashi animation constructor returned NULL")?;
                HigurashiUserLayer {
                    properties,
                    owner_id,
                    drawable: Drawable::Animation(owner),
                }
            }
            Self::Movie(prepared) => {
                let drawable = if let Some(prepared) = prepared {
                    let media = media.unwrap();
                    let preferences = movie::percentages(settings.as_deref().unwrap());
                    media.set_preferences(preferences);
                    let owner = prepared.construct(
                        media.assets().context().wgpu_device.clone(),
                        media.audio().clone(),
                        preferences,
                        media.active_clock(),
                        media.assets(),
                    )?;
                    Drawable::Movie(media.retain_owner(owner))
                } else {
                    Drawable::Null
                };
                HigurashiUserLayer {
                    properties,
                    owner_id,
                    drawable,
                }
            }
            Self::Temporary(prepared) => {
                let owner = if let Some(settings) = settings {
                    let route = settings.settings.common.v14_showroutenavi;
                    prepared.construct(settings, route)?
                } else {
                    prepared.construct(&mut UnavailableAcquiredBits, false)?
                };
                HigurashiUserLayer {
                    properties,
                    owner_id,
                    drawable: Drawable::Temporary(owner),
                }
            }
        }))
    }
}

struct UnavailableAcquiredBits;
impl temporary::AcquiredBits for UnavailableAcquiredBits {
    fn acquired(&self, _: usize) -> bool {
        unreachable!("Non-TIPS constructors do not read acquired bits")
    }
    fn acquire(&mut self, _: usize) -> anyhow::Result<()> {
        bail!("Missing live acquired-bit service")
    }
}
impl temporary::AcquiredBits for shin_core::format::save::higurashi::SaveData {
    fn acquired(&self, id: usize) -> bool {
        id.checked_mul(2).is_some_and(|bit| {
            self.save_vectors
                .extra_words_0
                .get(bit / 32)
                .is_some_and(|w| w & (1 << (bit % 32)) != 0)
        })
    }
    fn acquire(&mut self, id: usize) -> anyhow::Result<()> {
        let bit = id.checked_mul(2).context("TIPS acquired index overflow")?;
        let word = bit / 32;
        let bank = &mut self.save_vectors.extra_words_0;
        if word >= bank.len() {
            let count = word
                .checked_add(1)
                .and_then(|n| n.checked_next_multiple_of(256))
                .context("TIPS acquired bank overflow")?;
            bank.try_reserve(count - bank.len())?;
            bank.resize(count, 0);
        }
        bank[word] |= 1 << (bit % 32);
        Ok(())
    }
}
