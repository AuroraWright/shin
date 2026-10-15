use std::sync::Arc;

use shin_core::format::scenario::Scenario;
use shin_core::format::scenario::higurashi::HigurashiScenario;

use crate::asset::system::{Asset, AssetDataAccessor, AssetLoadContext};

impl Asset for Scenario {
    type Args = ();

    async fn load(
        _context: &Arc<AssetLoadContext>,
        _args: (),
        _name: &str,
        data: AssetDataAccessor,
    ) -> anyhow::Result<Self> {
        Scenario::new(data.read_all().await?.into())
    }
}

impl Asset for HigurashiScenario {
    type Args = ();

    async fn load(
        context: &Arc<AssetLoadContext>,
        _args: (),
        _name: &str,
        data: AssetDataAccessor,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            context.game == crate::cli::Game::Higurashi,
            "Higurashi scenario requires a Higurashi asset server"
        );
        HigurashiScenario::new(data.read_all().await?.into())
    }
}
