//! Original20d20 commits requested resource metadata after front publication.
use super::*;
use shin_core::format::save::higurashi::SaveData;

pub(in crate::adv::higurashi) fn unlock(words: &mut Vec<u32>, id: usize) -> Result<()> {
    let at = id / 32;
    if at >= words.len() {
        let count = at.checked_add(1).and_then(|n| n.checked_next_multiple_of(256))
            .context("Higurashi gallery bank overflow")?;
        words.try_reserve(count - words.len())?;
        words.resize(count, 0);
    }
    words[at] |= 1 << (id % 32);
    Ok(())
}

pub(super) fn commit(scenario: &HigurashiScenario, kind: i32, id: i32,
    settings: Option<&mut SaveData>) -> Result<()> {
    let Some(id) = usize::try_from(id).ok() else { return Ok(()); };
    match kind {
        2 => {
            let Some(info) = scenario.picture_info.get(id) else { return Ok(()); };
            let data = settings.context("Higurashi front picture metadata requires actual settings")?;
            unlock(&mut data.save_vectors.unlocked_cgs, id)?;
            if let Ok(linked) = usize::try_from(info.linked_cg_id) { unlock(&mut data.save_vectors.unlocked_cgs, linked)?; }
        }
        7 => {
            let Some(info) = scenario.movie_info.get(id) else { return Ok(()); };
            let data = settings.context("Higurashi front Movie metadata requires actual settings")?;
            unlock(&mut data.save_vectors.unlocked_movies, id)?;
            if let Ok(bgm) = usize::try_from(info.raw_fields[0]) { unlock(&mut data.save_vectors.unlocked_bgms, bgm)?; }
            if let Ok(cg) = usize::try_from(info.raw_fields[1]) { unlock(&mut data.save_vectors.unlocked_cgs, cg)?; }
        }
        _ => {}
    }
    Ok(())
}

impl HigurashiLayers {
    pub(crate) fn commit_pending_metadata(&mut self, scenario: &HigurashiScenario,
        mut settings: Option<&mut SaveData>) -> Result<()> {
        while let Some(&(kind, id)) = self.pending_metadata.first() {
            gallery::commit(scenario, kind, id, settings.as_deref_mut())?;
            self.pending_metadata.remove(0);
        }
        Ok(())
    }
}
