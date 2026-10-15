//! Desktop writer boundary for the older codec. Complete runtime rows stay
//! immutable here until the worker projects them to the disk representation.
use super::{load_game::Storage, runtime_saves::Slots};
use anyhow::Result;
use shin_core::format::save::higurashi::SaveData;
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct FrozenProgress {
    pub data: SaveData,
    pub slots: Option<Slots>,
    pub storage: Arc<dyn Storage>,
}
impl FrozenProgress {
    pub fn write(&self) -> Result<()> {
        let data = self
            .slots
            .as_ref()
            .map_or_else(|| self.data.clone(), |slots| slots.disk(&self.data));
        let bytes = data.encode()?;
        if tracing::enabled!(tracing::Level::DEBUG) {
            let fingerprint = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
            });
            tracing::debug!(
                length = bytes.len(),
                buffer_fnv = fingerprint,
                "HIGURASHI_CLOSE_WRITE"
            );
        }
        self.storage.write(&bytes)
    }
}
