//! Contains functionality pertaining to the asset system itself.

pub mod cache;
mod locate;
pub mod resource_cache;
mod server;

/// Catch inside the detached compute job: catching its awaiting future cannot
/// prevent Rayon from aborting on an escaped decoder or upload panic. Failed
/// preparation never publishes the partially constructed asset to its cache.
pub(crate) fn catch_asset_preparation_panic<T>(
    kind: &str,
    prepare: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(prepare)).map_err(|panic| {
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("unknown panic");
        anyhow::anyhow!("{kind} worker failed: {message}")
    })?
}

pub use self::{
    locate::locate_assets_for_game,
    server::{
        Asset, AssetDataAccessor, AssetDataCursor, AssetLoadContext, AssetServer, LayeredAssetIo,
    },
};

#[cfg(test)]
pub use self::locate::locate_assets;

#[cfg(all(test, target_os = "macos"))]
pub(crate) use server::accessor_io_reference_tests::verify_asset_load_failures;
#[cfg(all(test, target_os = "macos"))]
pub(crate) use server::accessor_io_reference_tests::{
    IoFault, supplied_payload, supplied_portrait,
};

pub(crate) use server::AssetIo;
