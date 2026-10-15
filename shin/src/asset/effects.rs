use std::{
    collections::BTreeMap,
    ops::Deref,
    sync::{Arc, Weak},
};

use super::texture_archive::{TextureArchive, TextureArchiveBuilder};
use parking_lot::{ArcRwLockReadGuard, RawRwLock, RwLock};
use shin_render::gpu_texture::GpuTexture;

/// One complete named atlas, shared by all native effects consumers. Preserve
/// unknown names as well: Animation uses the exact-case optional name Sakura.
#[derive(Debug, Default)]
pub struct EffectTextures {
    textures: BTreeMap<String, GpuTexture>,
}
impl TextureArchive for EffectTextures {
    type Builder = Self;
}
impl TextureArchiveBuilder for EffectTextures {
    type Output = Self;
    fn new() -> Self {
        Self::default()
    }
    fn add_texture(&mut self, name: &str, texture: GpuTexture) {
        self.textures.insert(name.to_owned(), texture);
    }
    fn build(self) -> Self {
        self
    }
}
impl EffectTextures {
    pub(crate) fn validate_for_frame(&self) -> anyhow::Result<()> {
        // App frame preparation always binds breakup. Other names belong to
        // particular effect layers and need not be used by the current scene.
        anyhow::ensure!(
            self.get("breakup").is_some(),
            "Missing texture breakup in /effects.txa"
        );
        Ok(())
    }
    pub(crate) fn require_texture(&self, name: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.get(name).is_some(),
            "Missing texture {name} in /effects.txa"
        );
        Ok(())
    }
    pub fn get(&self, name: &str) -> Option<&GpuTexture> {
        self.textures.get(name)
    }
    pub fn required(&self, name: &str) -> &GpuTexture {
        self.get(name).unwrap_or_else(|| panic!(
            "Effect layer requires texture '{name}' in /effects.txa; the supplied atlas does not contain it"
        ))
    }
}

#[derive(Default)]
struct AtlasState {
    textures: Option<Arc<EffectTextures>>,
    // Requirements follow actual prepared/layer/copy borrowers, without
    // keeping an atlas alive after replacement. Drop of the last borrower
    // makes that conditional name optional again.
    requirements: BTreeMap<&'static str, Weak<()>>,
}
type AtlasCell = RwLock<AtlasState>;

/// Engine-owned singleton. Layers and prepared particle factories hold weak
/// access to this actual resource cell, never an atlas snapshot or texture owner.
#[derive(Default)]
pub(crate) struct EffectsAtlas {
    cell: Arc<AtlasCell>,
}
impl EffectsAtlas {
    pub(crate) fn try_publish(&self, textures: Arc<EffectTextures>) -> anyhow::Result<()> {
        let mut state = self.cell.write();
        state
            .requirements
            .retain(|_, token| token.strong_count() > 0);
        for name in state.requirements.keys() {
            textures.require_texture(name)?;
        }
        state.textures = Some(textures);
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn publish(&self, textures: Arc<EffectTextures>) {
        // Reference-only synthetic atlas injection intentionally also covers
        // missing-resource render diagnostics; production uses try_publish.
        self.cell.write().textures = Some(textures);
    }
    pub(crate) fn clear(&self) {
        self.cell.write().textures = None;
    }
    pub(crate) fn is_loaded(&self) -> bool {
        self.cell.read_recursive().textures.is_some()
    }
    pub(crate) fn borrower(&self) -> EffectsAtlasBorrow {
        EffectsAtlasBorrow {
            cell: Arc::downgrade(&self.cell),
            _requirement: None,
        }
    }
    pub(crate) fn borrower_for(&self, name: &'static str) -> anyhow::Result<EffectsAtlasBorrow> {
        // Name validation, requirement registration and publication serialize
        // under the same lock, so a replacement cannot invalidate a new owner.
        let mut state = self.cell.write();
        let textures = state
            .textures
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Effects atlas /effects.txa is not loaded"))?;
        textures.require_texture(name)?;
        let token = state
            .requirements
            .get(name)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| Arc::new(()));
        state.requirements.insert(name, Arc::downgrade(&token));
        Ok(EffectsAtlasBorrow {
            cell: Arc::downgrade(&self.cell),
            _requirement: Some(token),
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EffectsAtlasBorrow {
    cell: Weak<AtlasCell>,
    _requirement: Option<Arc<()>>,
}
impl EffectsAtlasBorrow {
    pub(crate) fn read(&self) -> EffectsAtlasRead {
        let cell = self
            .cell
            .upgrade()
            .expect("Effects atlas owner has retired");
        // A frame can borrow breakup while a particle draw borrows the same
        // atlas. Recursive reads also succeed if a publisher is waiting.
        let guard = cell.read_arc_recursive();
        assert!(guard.textures.is_some(), "Effects atlas has been cleared");
        EffectsAtlasRead(guard)
    }
    #[cfg(test)]
    pub(crate) fn reference_identity(&self) -> usize {
        Arc::as_ptr(self.read().0.textures.as_ref().unwrap()) as usize
    }
    #[cfg(test)]
    pub(crate) fn reference_weak(&self) -> Weak<EffectTextures> {
        Arc::downgrade(self.read().0.textures.as_ref().unwrap())
    }
    #[cfg(test)]
    pub(crate) fn reference_strong_count(&self) -> usize {
        Arc::strong_count(self.read().0.textures.as_ref().unwrap())
    }
}

/// Holds the actual cell's read lock while texture bindings are constructed.
/// This does not retain another atlas Arc; submitted wgpu work retains its own
/// backend resources until completion instead of using native NVN release jobs.
pub(crate) struct EffectsAtlasRead(ArcRwLockReadGuard<RawRwLock, AtlasState>);
impl Deref for EffectsAtlasRead {
    type Target = EffectTextures;
    fn deref(&self) -> &Self::Target {
        self.0.textures.as_ref().unwrap()
    }
}
