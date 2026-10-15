//! Updated older-engine variable queues and the response stream at VM2dd8.
//! Memory state only: disk settings and full ADV reconstruction have separate owners.
use anyhow::{Result, ensure};
use shin_core::{
    format::save::PersistData,
    primitives::bit_utils::BitBuffer,
    vm::{command::CommandResult, higurashi::command::HigurashiRuntimeCommand as Cmd},
};

#[derive(Clone, Copy, Debug)]
pub enum Execution {
    Live,
    /// Original reconstruction invokes the replay callbacks until its target.
    Replay {
        boundary: u32,
        target: u32,
    },
}

#[derive(Clone, Debug)]
pub struct HigurashiVariables {
    persistent: PersistData,
    bytes: Vec<u8>,
    pending_persistent: Vec<(u32, i32)>,
    pending_bytes: Vec<(u32, i32)>,
    route: Vec<u8>,
    position: usize,
}
impl HigurashiVariables {
    /// Explicitly supplied settings and GameData+80 values; an empty store is a
    /// fresh memory session, not a claim that saved user settings were loaded.
    pub fn new(persistent: PersistData, bytes: Vec<u8>) -> Self {
        Self {
            persistent,
            bytes,
            pending_persistent: Vec::new(),
            pending_bytes: Vec::new(),
            route: Vec::new(),
            position: 0,
        }
    }
    pub fn persistent(&self) -> &PersistData {
        &self.persistent
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn pending_persistent(&self) -> &[(u32, i32)] {
        &self.pending_persistent
    }
    pub fn pending_bytes(&self) -> &[(u32, i32)] {
        &self.pending_bytes
    }
    pub fn route(&self) -> (&[u8], usize) {
        (&self.route, self.position)
    }

    /// The browser owns a standalone copy of the same four-bit response stream.
    pub(crate) fn response_choice(&mut self, replay: bool, value: i32) -> Result<i32> {
        let mut data = std::mem::take(&mut self.route);
        let result = self.choice_in_progress(&mut data, replay, value);
        self.route = data;
        result
    }

    /// Native33168 binds the bit cursor to a progress entry's vector owner.
    /// Borrow that vector for this dispatch without cloning response bytes.
    /// The session owns the binding; standalone recorded data cannot be lost.
    pub(super) fn dispatch_in_progress(
        &mut self,
        data: &mut Vec<u8>,
        command: &Cmd,
        execution: Execution,
    ) -> Result<Option<CommandResult>> {
        ensure!(self.route.is_empty(), "Standalone Higurashi responses cannot be rebound");
        self.route = std::mem::take(data);
        let result = self.dispatch(command, execution);
        *data = std::mem::take(&mut self.route);
        result
    }
    pub(super) fn restore_progress_cursor(&mut self, data: &[u8], position: usize) -> Result<()> {
        ensure!(position <= data.len().saturating_mul(8), "Higurashi progress cursor exceeds stored responses");
        self.position = position;
        Ok(())
    }

    /// Native335d4 restores a snapshot cursor without consulting vector length.
    /// Subsequent native response reads/writes grow the bound vector as needed.
    pub(super) fn restore_checkpoint_cursor(&mut self, position: usize) {
        self.position = position;
    }

    /// Native347c4/347d0 records unsigned four-bit choice responses. The older
    /// partial-byte overwrite rule and loaded-vector >= growth apply here too.
    pub(super) fn choice_in_progress(&mut self, data: &mut Vec<u8>, replay: bool, value: i32) -> Result<i32> {
        ensure!(self.route.is_empty(), "Standalone Higurashi responses cannot be rebound");
        let end = self.position.checked_add(4).ok_or_else(|| anyhow::anyhow!("Higurashi choice cursor overflow"))?;
        let required = end.checked_add(7).ok_or_else(|| anyhow::anyhow!("Higurashi choice length overflow"))? / 8;
        if required >= data.len() {
            let length = required.checked_add(1023).ok_or_else(|| anyhow::anyhow!("Higurashi choice growth overflow"))? & !1023;
            data.try_reserve(length - data.len())?;
            data.resize(length, 0);
        }
        let mut buffer = BitBuffer::with_data(std::mem::take(data), self.position);
        let result = if replay { buffer.read_int::<u8>(4).map(i32::from).map_err(anyhow::Error::from) } else {
            buffer.write_int(4, value as u8 & 15); Ok(value)
        };
        let (mut bytes, position) = buffer.into_data();
        if !replay && position & 7 != 0 { bytes[position >> 3] &= 0xff << (8 - (position & 7)); }
        *data = bytes;
        if result.is_ok() { self.position = position; }
        result
    }

    /// Native snapshots retain the bit cursor separately from the response data.
    /// Existing trailing bytes are kept so replay can resume recording in place.
    pub fn restore_route(&mut self, data: Vec<u8>, position: usize) -> Result<()> {
        ensure!(
            position <= data.len().saturating_mul(8),
            "Higurashi route cursor exceeds stored data"
        );
        self.route = data;
        self.position = position;
        Ok(())
    }
    fn queue(queue: &mut Vec<(u32, i32)>, slot: u32, value: i32) {
        if let Some((_, pending)) = queue.iter_mut().find(|(id, _)| *id == slot) {
            *pending = value;
        } else {
            queue.push((slot, value));
        }
    }
    fn pending(queue: &[(u32, i32)], slot: u32) -> Option<i32> {
        queue
            .iter()
            .find(|(id, _)| *id == slot)
            .map(|(_, value)| *value)
    }
    pub(crate) fn response(&mut self, byte: bool, replay: bool, value: i32) -> Result<i32> {
        let start = self.position;
        {
            // Native33170/333a0/33504 round when required >= length, including
            // the last byte of a loaded vector that is not already block-sized.
            // Choose the actual2/17-bit form, so a short value at a block edge
            // does not allocate the long-form span. Shared Umineko is unchanged.
            let short = self
                .route
                .get(start >> 3)
                .is_some_and(|b| b & (0x80 >> (start & 7)) != 0);
            let bits = if byte {
                8
            } else if (replay && short) || (!replay && matches!(value, 0 | 1)) {
                2
            } else {
                17
            };
            let end = start
                .checked_add(bits)
                .ok_or_else(|| anyhow::anyhow!("Higurashi route cursor overflow"))?;
            let required = end
                .checked_add(7)
                .ok_or_else(|| anyhow::anyhow!("Higurashi route length overflow"))?
                / 8;
            if required >= self.route.len() {
                let length = required
                    .checked_add(1023)
                    .ok_or_else(|| anyhow::anyhow!("Higurashi route growth overflow"))?
                    & !1023;
                self.route.try_reserve(length - self.route.len())?;
                self.route.resize(length, 0);
            }
        }
        let mut buffer = BitBuffer::with_data(std::mem::take(&mut self.route), start);
        let result: Result<i32> = (|| {
            if replay {
                if byte {
                    Ok(buffer.read_int::<u8>(8)? as i32)
                } else if buffer.read_bool()? {
                    Ok(i32::from(buffer.read_bool()?))
                } else {
                    Ok(buffer.read_int::<u16>(16)? as i16 as i32)
                }
            } else {
                if byte {
                    buffer.write_int(8, value as u8);
                } else if matches!(value, 0 | 1) {
                    buffer.write_bool(true);
                    buffer.write_bool(value != 0);
                } else {
                    buffer.write_bool(false);
                    buffer.write_int(16, value as u16);
                }
                Ok(value)
            }
        })();
        let (mut data, position) = buffer.into_data();
        // Native33504 uses a prefix mask for false, and333a0 uses the same
        // mask for an integer's final partial byte. Two true bits (value1)
        // only OR into existing data. Shared BitBuffer preserves trailing bits;
        // apply the older overwrite rule here without changing Umineko.
        let partial = position & 7;
        if !replay && partial != 0 && (byte || value != 1) {
            data[position >> 3] &= 0xff << (8 - partial);
        }
        self.route = data;
        self.position = if result.is_ok() { position } else { start };
        result
    }
    /// SGET/DA live polls read pending full32 values before signed16/unsigned8
    /// committed storage, then record the truncated replay response. Replay
    /// reads only the stream and skips setters before its unsigned target.
    pub fn dispatch(
        &mut self,
        command: &Cmd,
        execution: Execution,
    ) -> Result<Option<CommandResult>> {
        let replay = matches!(execution, Execution::Replay { .. });
        let skip_set =
            matches!(execution, Execution::Replay { boundary, target } if boundary < target);
        let (dest, slot, byte) = match command {
            Cmd::SGET { dest, slot } => (*dest, *slot as u32, false),
            Cmd::OpcodeDA { dest, argument } => (*dest, *argument as u32, true),
            Cmd::SSET {
                args: [slot, value],
            }
            | Cmd::OpcodeDB {
                args: [slot, value],
            } => {
                if !skip_set {
                    let queue = if matches!(command, Cmd::SSET { .. }) {
                        &mut self.pending_persistent
                    } else {
                        &mut self.pending_bytes
                    };
                    Self::queue(queue, *slot as u32, *value);
                }
                return Ok(Some(CommandResult::None));
            }
            _ => return Ok(None),
        };
        let value = if replay {
            0
        } else if byte {
            Self::pending(&self.pending_bytes, slot)
                .unwrap_or_else(|| i32::from(self.bytes.get(slot as usize).copied().unwrap_or(0)))
        } else {
            Self::pending(&self.pending_persistent, slot)
                .unwrap_or_else(|| self.persistent.get(slot as usize))
        };
        Ok(Some(CommandResult::WriteMemory(
            dest,
            self.response(byte, replay, value)?,
        )))
    }

    /// Native1185c/11a94 commit persistent pairs first, then byte pairs, before
    /// the sparse capture decision. Disabled commits leave both queues intact.
    /// Successfully committed prefixes are acknowledged even if allocation fails.
    pub fn commit_pending(&mut self, enabled: bool) -> Result<()> {
        if !enabled {
            return Ok(());
        }
        let mut done = 0;
        let result: Result<()> = (|| {
            for &(slot, value) in &self.pending_persistent {
                let length = slot
                    .checked_add(64)
                    .ok_or_else(|| anyhow::anyhow!("Native persistent index overflows growth"))?
                    & !63;
                let length = length as usize;
                if length > self.persistent.0.len() {
                    self.persistent
                        .0
                        .try_reserve(length - self.persistent.0.len())?;
                }
                self.persistent.set(slot as usize, value);
                done += 1;
            }
            Ok(())
        })();
        self.pending_persistent.drain(..done);
        result?;
        let mut done = 0;
        let result: Result<()> = (|| {
            for &(slot, value) in &self.pending_bytes {
                let slot = slot as usize;
                // Original5e780 does not grow for an out-of-range low-byte zero.
                if slot < self.bytes.len() {
                    self.bytes[slot] = value as u8;
                } else if value as u8 != 0 {
                    let length = (slot as u32)
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("Native byte index overflows growth"))?
                        as usize;
                    self.bytes.try_reserve(length - self.bytes.len())?;
                    self.bytes.resize(length, 0);
                    self.bytes[slot] = value as u8;
                }
                done += 1;
            }
            Ok(())
        })();
        self.pending_bytes.drain(..done);
        result
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod binding_tests;
