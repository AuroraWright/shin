//! Updated e0dc/e4c8 OTSUGET and f06c/f4bc KAKERAGET command owners.
//! Their VOID callbacks are independent of TIPSGET/Choice checkpoint data.
use super::layers::{HigurashiLayers, LayerBanks, Target};
use anyhow::{Result, bail, ensure};
use shin_core::vm::higurashi::command::HigurashiRuntimeCommand as Cmd;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    Otsu {
        argument: i32,
    },
    /// The argument is a positional bitmask over this original ordered list.
    Kakera {
        argument: u32,
        ids: Vec<i32>,
    },
}
impl Request {
    pub fn kind(&self) -> i32 {
        match self {
            Self::Otsu { .. } => 13,
            Self::Kakera { .. } => 14,
        }
    }
    pub fn producer_params(&self) -> [i32; 8] {
        let mut params = [0; 8];
        match self {
            Self::Otsu { argument } => params[0] = *argument,
            Self::Kakera { argument, ids } => {
                for (index, id) in ids.iter().enumerate() {
                    if argument & (1 << index) != 0 {
                        let next = params[0] as usize + 1;
                        params[next] = *id;
                        params[0] += 1;
                        if params[0] == 7 {
                            break;
                        }
                    }
                }
            }
        }
        params
    }
    fn skipped(&self) -> bool {
        match self {
            Self::Otsu { .. } => false,
            Self::Kakera { argument, ids } => {
                // f0d0 variable LSL masks to32bits; checked source count<=16.
                argument & !u32::MAX.wrapping_shl(ids.len() as u32) == 0
            }
        }
    }
}

pub(crate) trait Services {
    /// Retain one producer until ready. On the main poll construct with NULL
    /// oldowner, set drawable22/5=1, create the separate4OS Shared wrapper,
    /// set wrapper5=1, publish front drawable THEN root optional child0.
    fn prepare(
        &mut self,
        request: &Request,
        target: Target,
        layers: &mut HigurashiLayers,
    ) -> Result<bool>;
    /// Read the SAME Shared retained by the app Job and Overlay. True means
    /// Completion::Notification's actual VOID callback, not an integer0.
    fn poll(&mut self) -> Result<bool>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Loading,
    Waiting,
    Complete,
}

#[derive(Debug)]
pub(crate) struct NotificationCommand {
    request: Request,
    reservation: Option<Target>,
    skipped: bool,
    phase: Phase,
}
impl NotificationCommand {
    /// Native constructors preserve text/read/boundary/save data. KAKERAGET's
    /// empty-mask gate precedes reservation; OTSUGET always reserves.
    pub(in crate::adv::higurashi) fn new(command: &Cmd, layers: &mut LayerBanks) -> Result<Self> {
        let request = match command {
            Cmd::OTSUGET { argument } => Request::Otsu {
                argument: *argument,
            },
            Cmd::KAKERAGET { argument, ids } => {
                // f090..f0d8 copies a fixed0x48-byte argument/count/16-ID block.
                ensure!(
                    ids.len() <= 16,
                    "KAKERAGET exceeds original sixteen-ID command storage"
                );
                Request::Kakera {
                    argument: *argument as u32,
                    ids: ids.to_vec(),
                }
            }
            _ => bail!("Notification owner requires OTSUGET or KAKERAGET"),
        };
        let skipped = request.skipped();
        let reservation = if skipped {
            None
        } else {
            Some(layers.reserve_temporary()?)
        };
        Ok(Self {
            request,
            reservation,
            skipped,
            phase: Phase::Loading,
        })
    }
    pub fn request(&self) -> &Request {
        &self.request
    }
    pub fn target(&self) -> Option<Target> {
        self.reservation
    }
    pub fn skipped(&self) -> bool {
        self.skipped
    }
    pub fn callback_received(&self) -> bool {
        self.phase == Phase::Complete
    }
    /// Vtable+10 e4b8/f4ac literallyreturnstrue. No boundary/response/date read.
    pub fn poll_replay(&self) -> bool {
        true
    }
    /// Vtable+18 e4c0/f4b4 literallyreturnsfalse, including skipped KAKERAGET.
    pub fn forward_complete(&self) -> bool {
        false
    }
    pub fn poll_live(
        &mut self,
        layers: &mut HigurashiLayers,
        services: &mut impl Services,
    ) -> Result<bool> {
        if self.skipped || self.phase == Phase::Complete {
            return Ok(true);
        }
        if self.phase == Phase::Loading {
            if !services.prepare(
                &self.request,
                self.reservation.expect("Retained notification reservation"),
                layers,
            )? {
                return Ok(false);
            }
            self.phase = Phase::Waiting;
        }
        if services.poll()? {
            self.phase = Phase::Complete;
        }
        Ok(self.phase == Phase::Complete)
    }
    /// Parent first retires root wrapper and original front drawable by their
    /// separate identities, then releases this mapping. New-scene banks must
    /// not receive an old scene's reservation cleanup.
    pub(in crate::adv::higurashi) fn release(&mut self, layers: &mut LayerBanks) {
        if let Some(target) = self.reservation.take() {
            layers.release_temporary(target);
        }
    }
}

#[cfg(test)]
mod tests;
