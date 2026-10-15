//! Switch TROPHY completion and live/replay gallery UNLOCK behavior.
use anyhow::{Result, ensure};
use shin_core::format::{
    save::index_set::IndexSet,
    scenario::gerokasu2::{BgmInfoItem, Gerokasu2Scenario, MovieInfoItem, PictureInfoItem},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalleryUnlock {
    Picture(u32),
    Music(u32),
    Movie(u32),
}

/// The original callback reads only these three scenario metadata tables.
#[derive(Clone, Copy)]
pub struct GalleryMetadata<'a> {
    pub pictures: &'a [PictureInfoItem],
    pub bgms: &'a [BgmInfoItem],
    pub movies: &'a [MovieInfoItem],
}
impl<'a> From<&'a Gerokasu2Scenario> for GalleryMetadata<'a> {
    fn from(scenario: &'a Gerokasu2Scenario) -> Self {
        Self {
            pictures: &scenario.picture_info,
            bgms: &scenario.bgm_info,
            movies: &scenario.movie_info,
        }
    }
}

/// Original1F780: successful PIC and movie publication update global gallery
/// bits. BUP and every other kind have no automatic effect. A movie's linked
/// picture's linked CG goes to the music set, matching1F868..1F878.
pub fn loaded_layer_unlocks(info: GalleryMetadata<'_>, kind: i32, id: i32) -> Vec<GalleryUnlock> {
    let Ok(index) = usize::try_from(id) else {
        return Vec::new();
    };
    match kind {
        2 => {
            let Some(item) = info.pictures.get(index) else {
                return Vec::new();
            };
            let mut result = vec![GalleryUnlock::Picture(id as u32)];
            if let Ok(link) = u32::try_from(item.linked_cg_id) {
                result.push(GalleryUnlock::Picture(link));
            }
            result
        }
        6 => {
            let Some(item) = info.movies.get(index) else {
                return Vec::new();
            };
            let mut result = Vec::new();
            if let Ok(bgm) = u32::try_from(item.linked_bgm_id) {
                if let Some(link) = info
                    .bgms
                    .get(bgm as usize)
                    .and_then(|b| u32::try_from(b.linked_bgm_id).ok())
                {
                    result.push(GalleryUnlock::Music(link));
                }
                // Native records the direct link even without its metadata.
                result.push(GalleryUnlock::Music(bgm));
            }
            if let Ok(picture) = u32::try_from(item.linked_picture_id) {
                if let Some(link) = info
                    .pictures
                    .get(picture as usize)
                    .and_then(|p| u32::try_from(p.linked_cg_id).ok())
                {
                    result.push(GalleryUnlock::Music(link));
                }
                result.push(GalleryUnlock::Picture(picture));
            }
            result.push(GalleryUnlock::Movie(id as u32));
            result
        }
        _ => Vec::new(),
    }
}

#[derive(Clone)]
pub struct GalleryUnlocks {
    pub cgs: IndexSet,
    pub bgms: IndexSet,
    pub movies: IndexSet,
}
impl Default for GalleryUnlocks {
    fn default() -> Self {
        Self {
            cgs: IndexSet::new(),
            bgms: IndexSet::new(),
            movies: IndexSet::new(),
        }
    }
}
impl GalleryUnlocks {
    /// Return whether any new persistent bit was set. Silent command replay
    /// suppresses commits; native resource hydration is a publication producer.
    pub fn apply(
        &mut self,
        unlocks: impl IntoIterator<Item = GalleryUnlock>,
        replay: bool,
    ) -> bool {
        if replay {
            return false;
        }
        let mut changed = false;
        for unlock in unlocks {
            let (set, index) = match unlock {
                GalleryUnlock::Picture(id) => (&mut self.cgs, id),
                GalleryUnlock::Music(id) => (&mut self.bgms, id),
                GalleryUnlock::Movie(id) => (&mut self.movies, id),
            };
            changed |= set.add(index as usize);
        }
        changed
    }
    /// Consume a backend publication notification using its original eight
    /// parameter words; the native callback reads only params[0].
    pub fn loaded_layer(
        &mut self,
        scenario: &Gerokasu2Scenario,
        kind: i32,
        params: [i32; 8],
        replay: bool,
    ) -> bool {
        if replay {
            return false;
        }
        self.apply(
            loaded_layer_unlocks(scenario.into(), kind, params[0]),
            false,
        )
    }
    /// Native replay uses the no-op RunState. Live poll selects only types0..2.
    /// Validate the complete index list before changing the global bitset.
    pub fn unlock(&mut self, kind: u8, indices: &[i32], replay: bool) -> Result<()> {
        if replay || kind > 2 {
            return Ok(());
        }
        ensure!(
            indices.iter().all(|&i| i >= 0),
            "Negative Ger gallery unlock index"
        );
        let set = match kind {
            0 => &mut self.cgs,
            1 => &mut self.bgms,
            2 => &mut self.movies,
            _ => unreachable!(),
        };
        for &index in indices {
            set.add(index as usize);
        }
        Ok(())
    }
}
pub trait GalleryServices {
    fn unlock_gallery(&mut self, kind: u8, indices: &[i32]) -> Result<()>;
}
/// Native78ac/e29a0 ignores every trophy ID and only locks/unlocks its mutex.
/// This supplied Switch dialect has no acquisition or persistent state effect.
pub fn trophy(_: i32) {}

#[cfg(test)]
mod tests;
