//! Ger movie resource acquisition and captured MOVIEWAIT bank queries.
use super::layers::Gerokasu2Layers;
use crate::{
    asset::{movie::Movie, picture::Picture, system::AssetServer},
    audio::MovieVolumeSettings,
    layer::user::{MovieArgs, MovieLayer},
};
use anyhow::{Result, bail};
use shin_audio::AudioManager;
use shin_core::{
    format::scenario::info::{MovieTransparencyMode, MovieVolumeSource},
    vm::command::types::{LayerbankId, PlaneId},
    vm::gerokasu2::Gerokasu2RuntimeCommand,
};
use std::sync::Arc;
pub async fn load(
    assets: &AssetServer,
    audio: &Arc<AudioManager>,
    request: &super::layer_factory::LayerFactoryRequest,
    settings: MovieVolumeSettings,
) -> Result<MovieLayer> {
    let super::layer_factory::LayerFactoryRequest::Movie {
        path,
        still,
        volume_source,
        transparency,
        volume,
        repeat,
    } = request
    else {
        bail!("Expected Ger movie request")
    };
    let source = match *volume_source {
        0 => MovieVolumeSource::Ignore,
        1 => MovieVolumeSource::Bgm,
        2 => MovieVolumeSource::Se,
        3 => MovieVolumeSource::Voice,
        _ => MovieVolumeSource::Ignore,
    };
    let movie: Arc<Movie> = assets.load(path).await?;
    let still: Option<Arc<Picture>> = if let Some(path) = still {
        Some(assets.load(path).await?)
    } else {
        None
    };
    Ok(MovieLayer::new(
        &assets.context().wgpu_device,
        audio,
        movie,
        MovieArgs {
            volume_source: source,
            transparency: if transparency & 1 == 0 {
                MovieTransparencyMode::Opaque
            } else {
                MovieTransparencyMode::Transparent
            },
            local_volume: shin_core::vm::command::types::Volume(volume.clamp(0.0, 1.0)),
            repeat: *repeat,
        },
        still,
        settings,
    )
    .await)
}
pub trait MovieServices {
    fn movie_status(&mut self, plane: PlaneId, bank: LayerbankId) -> Result<u32>;
}
pub struct MovieWait {
    plane: PlaneId,
    banks: Vec<LayerbankId>,
    unwanted: u32,
}
impl MovieWait {
    pub fn new(layers: &mut Gerokasu2Layers, command: &Gerokasu2RuntimeCommand) -> Result<Self> {
        let Gerokasu2RuntimeCommand::MovieWait { layer_id, statuses } = command else {
            bail!("Expected Ger MOVIEWAIT")
        };
        let banks = if (-5..=-1).contains(layer_id) {
            Vec::new()
        } else {
            layers
                .targets(*layer_id)?
                .iter()
                .map(|t| t.layerbank)
                .collect()
        };
        Ok(Self {
            plane: layers.current_plane(),
            banks,
            unwanted: *statuses as u32,
        })
    }
    pub fn poll(&self, services: &mut dyn MovieServices) -> Result<bool> {
        for bank in &self.banks {
            if services.movie_status(self.plane, *bank)? & self.unwanted & 3 != 0 {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
