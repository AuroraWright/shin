//! Native9678 constructor captures physical coordinates and edits VM records;
//! 98e4 later removes the then-current keyed drawable, without waiting for it.
use super::{HigurashiLayers, LayerBanks, Target};
use anyhow::Result;

pub(crate) struct LayerUnload {
    targets: Vec<Target>,
    delay: i32,
    completed: bool,
}
impl LayerUnload {
    pub(crate) fn capture(state: &mut LayerBanks, id: i32, delay: i32) -> Result<Self> {
        Ok(Self {
            targets: state.unload(id)?,
            delay,
            completed: false,
        })
    }
    pub(crate) fn complete(&mut self, layers: &mut HigurashiLayers) -> Result<()> {
        if self.completed {
            return Ok(());
        }
        for target in &self.targets {
            let bank = usize::from(target.bank != 0);
            if let Some(owner) = layers.owners[target.slot as usize][bank].take() {
                if self.delay > 0 {
                    layers.retiring[bank].prepend_retiring(owner, self.delay as f32);
                }
            }
        }
        self.completed = true;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn reference_targets(&self) -> &[Target] {
        &self.targets
    }
}
