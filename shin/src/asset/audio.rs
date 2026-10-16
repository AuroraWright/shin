use std::sync::Arc;

use anyhow::{Context, Result};
use shin_core::format::audio::{AudioFile, read_audio, read_higurashi_audio};

use crate::asset::system::{Asset, AssetDataAccessor, AssetLoadContext};

impl Asset for shin_core::format::sysse::SysSe {
    type Args = ();

    async fn load(
        _context: &Arc<AssetLoadContext>,
        _args: (),
        _name: &str,
        data: AssetDataAccessor,
    ) -> Result<Self> {
        let data = data.read_all().await?;
        crate::asset::system::catch_asset_preparation_panic("SysSE", || {
            shin_core::format::sysse::read_sys_se(&data)
        })
        .context("Parsing system sound bank")
    }
}

impl Asset for AudioFile {
    type Args = ();

    async fn load(
        context: &Arc<AssetLoadContext>,
        _args: (),
        _name: &str,
        data: AssetDataAccessor,
    ) -> Result<Self> {
        let bytes = data.read_all().await?;
        match context.game {
            crate::cli::Game::Higurashi => read_higurashi_audio(&bytes),
            crate::cli::Game::Umineko | crate::cli::Game::Gerokasu2 | crate::cli::Game::Gerokasu => read_audio(&bytes),
        }
        .context("Parsing audio file")
    }
}
