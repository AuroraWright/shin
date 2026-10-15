//! Updated102f8 live-only gallery acquisition; replay/forward only acknowledge.
use anyhow::{Context, Result, ensure};
use shin_core::format::save::higurashi::SaveData;

pub(super) fn poll(kind: u8, ids: &[i32], settings: Option<&mut SaveData>) -> Result<()> {
    // Native wrappers copy kind/count plus exactly sixteen decoded ID words.
    ensure!(
        ids.len() <= 16,
        "Higurashi UNLOCK exceeds sixteen-ID command storage"
    );
    if kind > 2 || ids.is_empty() {
        return Ok(());
    }
    // Native unsigned indexing can request enormous banks for negative IDs.
    // Reject malformed desktop input before any partial settings mutation.
    ensure!(
        ids.iter().all(|id| *id >= 0),
        "Higurashi UNLOCK has a negative gallery ID"
    );
    let settings = settings.context("Higurashi UNLOCK requires the actual settings bank")?;
    let words = match kind {
        0 => &mut settings.save_vectors.unlocked_cgs,
        1 => &mut settings.save_vectors.unlocked_bgms,
        2 => &mut settings.save_vectors.unlocked_movies,
        _ => unreachable!(),
    };
    for &id in ids {
        super::layers::unlock_gallery_bit(words, id as usize)?;
    }
    Ok(())
}
