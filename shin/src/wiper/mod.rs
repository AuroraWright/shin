mod default;
pub mod higurashi;
pub(crate) mod glass;
mod mask;
mod ripple;
mod scanline;
mod scroll;
mod timed;
mod turn;
mod wave;
mod whirl;
mod zoom;

#[cfg(test)]
pub(crate) mod native_batch_tests;

use from_variants::FromVariants;
use shin_core::{
    format::scenario::{
        Scenario,
        instruction_elements::{TypedNumberArray, UntypedNumberArray, lower_number_array},
    },
    time::Ticks,
    vm::command::types::{MaskFlags, MaskParam, WiperType},
};
use shin_derive::RenderClone;
use shin_render::{
    RenderRequestBuilder, render_pass::RenderPass, shaders::types::texture::TextureSource,
};

pub use self::default::DefaultWiper;
use crate::{
    asset::{mask::MaskTexture, system::AssetServer},
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::mask::MaskWiper,
};

pub trait Wiper: AdvUpdatable {
    fn is_running(&self) -> bool;
    fn fast_forward(&mut self);
    fn render(
        &self,
        pass: &mut RenderPass,
        render_request_builder: RenderRequestBuilder,
        texture_target: TextureSource,
        texture_source: TextureSource,
    );
}

// Native prepared batches retain the original slot's mutable wipe, rather than
// copying it. Slot updates and batch rendering observe the same clock and frame
// guard; explicit RenderClone still invokes the actual wipe copy body.
impl<T: Wiper> Wiper for crate::layer::owner::LayerOwner<T> {
    fn is_running(&self) -> bool {
        self.read().is_running()
    }
    fn fast_forward(&mut self) {
        self.write().fast_forward();
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        target: TextureSource,
        source: TextureSource,
    ) {
        self.read().render(pass, builder, target, source);
    }
}

#[derive(Debug, RenderClone, FromVariants)]
pub enum AnyWiper {
    Default(DefaultWiper),
    Scroll(scroll::ScrollWiper),
    Zoom(zoom::ZoomWiper),
    Turn(turn::TurnWiper),
    Wave(wave::WaveWiper),
    Ripple(ripple::RippleWiper),
    Scanline(scanline::ScanlineWiper),
    Mask(MaskWiper),
    Whirl(whirl::WhirlWiper),
    Glass(glass::GlassWiper),
}

impl AnyWiper {
    pub(crate) fn can_batch_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Default(a), Self::Default(b)) => a.can_batch_with(b),
            (Self::Scroll(a), Self::Scroll(b)) => a.can_batch_with(b),
            (Self::Zoom(a), Self::Zoom(b)) => a.can_batch_with(b),
            (Self::Turn(a), Self::Turn(b)) => a.can_batch_with(b),
            (Self::Wave(a), Self::Wave(b)) => a.can_batch_with(b),
            (Self::Ripple(a), Self::Ripple(b)) => a.can_batch_with(b),
            (Self::Scanline(a), Self::Scanline(b)) => a.can_batch_with(b),
            (Self::Mask(a), Self::Mask(b)) => a.can_batch_with(b),
            (Self::Whirl(a), Self::Whirl(b)) => a.can_batch_with(b),
            (Self::Glass(a), Self::Glass(b)) => a.can_batch_with(b),
            _ => false,
        }
    }

    /// The native factory returns no loader for unsigned values outside0..9.
    pub async fn load_raw(
        asset_server: &AssetServer,
        scenario: &Scenario,
        ty: i32,
        duration: Ticks,
        params: UntypedNumberArray,
    ) -> Option<AnyWiper> {
        let ty = num_traits::FromPrimitive::from_i32(ty)?;
        Some(Self::load(asset_server, scenario, ty, duration, params).await)
    }

    pub(crate) async fn load_raw_checked(
        asset_server: &AssetServer,
        scenario: &Scenario,
        ty: i32,
        duration: Ticks,
        params: UntypedNumberArray,
    ) -> anyhow::Result<Option<AnyWiper>> {
        let Some(ty) = num_traits::FromPrimitive::from_i32(ty) else {
            return Ok(None);
        };
        Self::load_checked(asset_server, scenario, ty, duration, params)
            .await
            .map(Some)
    }

    pub async fn load(
        asset_server: &AssetServer,
        scenario: &Scenario,
        ty: WiperType,
        duration: Ticks,
        params: UntypedNumberArray,
    ) -> AnyWiper {
        Self::load_checked(asset_server, scenario, ty, duration, params)
            .await
            .expect("Failed to load wipe")
    }

    async fn load_checked(
        asset_server: &AssetServer,
        scenario: &Scenario,
        ty: WiperType,
        duration: Ticks,
        params: UntypedNumberArray,
    ) -> anyhow::Result<AnyWiper> {
        Ok(match ty {
            WiperType::Default => AnyWiper::Default(DefaultWiper::new(duration)),
            WiperType::Mask => {
                // Native21c14 queries the raw signed ID before reading the
                // remaining parameters. Invalid metadata selects Default.
                let mask_info = usize::try_from(params.0)
                    .ok()
                    .and_then(|index| scenario.info_tables().mask_info.get(index));
                let Some(mask_info) = mask_info else {
                    return Ok(AnyWiper::Default(DefaultWiper::new(duration)));
                };
                let (_, param2, flags, _, _, _, _, _): TypedNumberArray<i32, MaskParam, MaskFlags> =
                    lower_number_array(params);

                let mask_path = mask_info.path();

                let mask: std::sync::Arc<MaskTexture> = asset_server.load(&mask_path).await?;

                AnyWiper::Mask(MaskWiper::new(duration, mask, param2, flags))
            }
            WiperType::Scroll => {
                AnyWiper::Scroll(scroll::ScrollWiper::new(duration, params.0, params.1))
            }
            WiperType::Scanline => {
                AnyWiper::Scanline(scanline::ScanlineWiper::new(duration, params.0))
            }
            WiperType::Ripple => AnyWiper::Ripple(ripple::RippleWiper::new(duration)),
            WiperType::Wave => AnyWiper::Wave(wave::WaveWiper::new(duration)),
            WiperType::Turn => AnyWiper::Turn(turn::TurnWiper::new(duration, params.0)),
            WiperType::Zoom => AnyWiper::Zoom(zoom::ZoomWiper::new(duration, params.0, params.1)),
            WiperType::Glass => AnyWiper::Glass(glass::GlassWiper::new(duration)),
            WiperType::Whirl => {
                let (direction, _, _, _, _, _, _, _): TypedNumberArray<i32> =
                    lower_number_array(params);
                AnyWiper::Whirl(whirl::WhirlWiper::new(duration, direction, 1.0))
            }
        })
    }
}

impl AdvUpdatable for AnyWiper {
    fn update(&mut self, context: &AdvUpdateContext) {
        match self {
            AnyWiper::Default(wiper) => wiper.update(context),
            AnyWiper::Scroll(wiper) => wiper.update(context),
            AnyWiper::Zoom(wiper) => wiper.update(context),
            AnyWiper::Turn(wiper) => wiper.update(context),
            AnyWiper::Wave(wiper) => wiper.update(context),
            AnyWiper::Ripple(wiper) => wiper.update(context),
            AnyWiper::Scanline(wiper) => wiper.update(context),
            AnyWiper::Mask(wiper) => wiper.update(context),
            AnyWiper::Whirl(wiper) => wiper.update(context),
            AnyWiper::Glass(wiper) => wiper.update(context),
        }
    }
}

impl Wiper for AnyWiper {
    fn is_running(&self) -> bool {
        match self {
            AnyWiper::Default(wiper) => wiper.is_running(),
            AnyWiper::Scroll(wiper) => wiper.is_running(),
            AnyWiper::Zoom(wiper) => wiper.is_running(),
            AnyWiper::Turn(wiper) => wiper.is_running(),
            AnyWiper::Wave(wiper) => wiper.is_running(),
            AnyWiper::Ripple(wiper) => wiper.is_running(),
            AnyWiper::Scanline(wiper) => wiper.is_running(),
            AnyWiper::Mask(wiper) => wiper.is_running(),
            AnyWiper::Whirl(wiper) => wiper.is_running(),
            AnyWiper::Glass(wiper) => wiper.is_running(),
        }
    }

    fn fast_forward(&mut self) {
        match self {
            AnyWiper::Default(wiper) => wiper.fast_forward(),
            AnyWiper::Scroll(wiper) => wiper.fast_forward(),
            AnyWiper::Zoom(wiper) => wiper.fast_forward(),
            AnyWiper::Turn(wiper) => wiper.fast_forward(),
            AnyWiper::Wave(wiper) => wiper.fast_forward(),
            AnyWiper::Ripple(wiper) => wiper.fast_forward(),
            AnyWiper::Scanline(wiper) => wiper.fast_forward(),
            AnyWiper::Mask(wiper) => wiper.fast_forward(),
            AnyWiper::Whirl(wiper) => wiper.fast_forward(),
            AnyWiper::Glass(wiper) => wiper.fast_forward(),
        }
    }

    fn render(
        &self,
        pass: &mut RenderPass,
        render_request_builder: RenderRequestBuilder,
        from_texture: TextureSource,
        to_texture: TextureSource,
    ) {
        match self {
            AnyWiper::Default(wiper) => {
                wiper.render(pass, render_request_builder, from_texture, to_texture)
            }
            AnyWiper::Glass(wiper) => {
                wiper.render(pass, render_request_builder, from_texture, to_texture)
            }
            AnyWiper::Whirl(wiper) => {
                wiper.render(pass, render_request_builder, from_texture, to_texture)
            }
            AnyWiper::Scroll(wiper) => {
                wiper.render(pass, render_request_builder, from_texture, to_texture);
            }
            AnyWiper::Zoom(wiper) => {
                wiper.render(pass, render_request_builder, from_texture, to_texture);
            }
            AnyWiper::Turn(wiper) => {
                wiper.render(pass, render_request_builder, from_texture, to_texture);
            }
            AnyWiper::Wave(wiper) => {
                wiper.render(pass, render_request_builder, from_texture, to_texture);
            }
            AnyWiper::Scanline(wiper) => {
                wiper.render(pass, render_request_builder, from_texture, to_texture);
            }
            AnyWiper::Ripple(wiper) => {
                wiper.render(pass, render_request_builder, from_texture, to_texture);
            }
            AnyWiper::Mask(wiper) => {
                wiper.render(pass, render_request_builder, from_texture, to_texture)
            }
        }
    }
}
