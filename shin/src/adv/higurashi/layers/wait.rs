//! Updated a350 captures coordinates; a560 polls current owners' raw busy bits.
//! Unlike the later engine, the live callback never forwards property tweens.
use super::{HigurashiLayers, HigurashiProperties, Target};
use anyhow::{Result, ensure};

pub(in crate::adv::higurashi) struct LayerWait {
    target: WaitTarget,
    properties: Vec<usize>,
}

enum WaitTarget {
    Root,
    Screen(u8),
    Page,
    Layers(Vec<Target>),
}

impl LayerWait {
    pub fn capture(layers: &HigurashiLayers, id: i32, properties: &[i32]) -> Result<Self> {
        // Native property indexing is unchecked. Reject malformed desktop
        // bytecode before installing the continuation or changing any owner.
        ensure!(
            properties.len() <= 32,
            "Higurashi LAYERWAIT list exceeds its 32-property command storage"
        );
        ensure!(
            properties.iter().all(|property| (0..65).contains(property)),
            "Higurashi LAYERWAIT property outside 0..65"
        );
        let target = match id {
            -1 => WaitTarget::Root,
            -2 => WaitTarget::Screen(layers.state.screen_back),
            -3 => WaitTarget::Page,
            _ => WaitTarget::Layers(layers.state.targets(id)?),
        };
        Ok(Self {
            target,
            properties: properties
                .iter()
                .map(|&property| property as usize)
                .collect(),
        })
    }

    pub fn complete(&self, layers: &HigurashiLayers) -> bool {
        let idle = |properties: &HigurashiProperties| {
            self.properties
                .iter()
                .all(|&index| !properties.tweener(index).unwrap().is_waiting())
        };
        match &self.target {
            WaitTarget::Root => idle(&layers.root),
            WaitTarget::Screen(bank) => idle(&layers.screen[usize::from(*bank != 0)]),
            WaitTarget::Page => idle(&layers.page),
            WaitTarget::Layers(targets) => targets.iter().all(|target| {
                // a560 resolves captured physical coordinates again every poll.
                // A removed owner completes; slot reuse checks the replacement.
                layers.owners[target.slot as usize][usize::from(target.bank != 0)]
                    .as_ref()
                    .is_none_or(|owner| idle(&owner.properties))
            }),
        }
    }
}

#[cfg(test)]
mod native_tests;
