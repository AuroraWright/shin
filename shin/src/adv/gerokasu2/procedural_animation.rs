//! Original Animation/Effect resource preparation and publication adapters.
//! Shared particle/capture owners retain per-game arithmetic and property rules.
use anyhow::Result;

use crate::{
    asset::system::AssetServer,
    layer::user::{EffectLayer, PreparedSakuraLayer, SakuraLayer, UserLayer},
};

pub enum PreparedProcedural {
    Animation(PreparedSakuraLayer),
    Effect,
}
impl PreparedProcedural {
    /// Preparation acquires only Sakura artwork. Mode checking precedes asset
    /// access; no particle warmup, property owner or RNG state is created here.
    /// Effect captures lower layers and requires no procedural texture.
    pub async fn prepare(
        assets: &AssetServer,
        kind: i32,
        params: [i32; 8],
    ) -> Result<Option<Self>> {
        match kind {
            4 => Ok(SakuraLayer::prepare_checked_gerokasu2(assets, params[0])
                .await?
                .map(Self::Animation)),
            5 => Ok(Some(Self::Effect)),
            _ => Ok(None),
        }
    }
    /// Each published bank constructs its own simulation/capture owner. The
    /// original getter returns no layer for invalid Animation modes; callers
    /// must retain that absence instead of substituting an invisible drawable.
    pub fn construct(&self) -> UserLayer {
        match self {
            Self::Animation(prepared) => prepared.construct().into(),
            Self::Effect => EffectLayer::new_gerokasu2().into(),
        }
    }
}

#[cfg(test)]
mod tests;
