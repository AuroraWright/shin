//! Captured plane/bank edits and immediate native command completion.
use super::layers::Gerokasu2Layers;
use crate::adv::vm_state::layers::LayerOperationTarget;
use anyhow::{Result, bail};
use shin_core::vm::{
    command::{
        RuntimeCommand,
        types::{LayerId, PlaneId},
    },
    gerokasu2::Gerokasu2RuntimeCommand as Cmd,
};

#[derive(Debug)]
pub enum LayerEdit {
    Unload {
        plane: PlaneId,
        targets: Vec<LayerOperationTarget>,
        delay: i32,
    },
    Swap {
        plane: PlaneId,
        targets: Vec<LayerOperationTarget>,
    },
    Clear {
        plane: PlaneId,
    },
}
pub trait LayerEditServices {
    /// Validate before changing owners. Capture the previous root even when
    /// there are no targets; a service error leaves the command retryable.
    fn edit_layers(&mut self, edit: &LayerEdit) -> Result<()>;
}
impl LayerEdit {
    /// Apply integer effects to a caller-owned copy, committed only after the
    /// drawable service succeeds. Bank targets retain the command's plane.
    pub fn prepare(layers: &mut Gerokasu2Layers, command: &Cmd) -> Result<Self> {
        let plane = layers.current_plane();
        Ok(match command {
            Cmd::LayerUnload {
                layer_id,
                delay_time,
            } => Self::Unload {
                plane,
                targets: layers.unload(*layer_id)?.iter().copied().collect(),
                delay: *delay_time,
            },
            Cmd::Shared(RuntimeCommand::LAYERSWAP(c)) => {
                layers.swap(c.arg1, c.arg2);
                let mut targets = Vec::new();
                if (0..256).contains(&c.arg1) && (0..256).contains(&c.arg2) && c.arg1 != c.arg2 {
                    for id in [c.arg1, c.arg2] {
                        if let Some(target) = layers.target(LayerId::new(id as u16)) {
                            targets.push(target);
                        }
                    }
                }
                Self::Swap { plane, targets }
            }
            Cmd::Shared(RuntimeCommand::PLANECLEAR(_)) => {
                layers.clear_plane();
                Self::Clear { plane }
            }
            _ => bail!("Expected Gerokasu2 unload/swap/plane-clear command"),
        })
    }
    pub fn plane(&self) -> PlaneId {
        match *self {
            Self::Unload { plane, .. } | Self::Swap { plane, .. } | Self::Clear { plane } => plane,
        }
    }
}

#[cfg(test)]
mod tests;
