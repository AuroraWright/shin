//! Native6ba8..7278 audio dispatch,with a retained status/sync/one-shot wait.
use anyhow::{Result, ensure};
use shin_core::vm::gerokasu2::audio::Gerokasu2AudioCommand as Cmd;

pub trait AudioCommands {
    fn bgm_play(&mut self, id: i32, loops: i32, volume: f32, fade: i32) -> Result<bool>;
    fn bgm_changed(&mut self, id: i32) -> Result<()>;
    fn bgm_stop(&mut self, fade: i32) -> Result<()>;
    fn bgm_volume(&mut self, volume: f32, fade: i32) -> Result<()>;
    fn bgm_status(&self) -> i32;
    fn bgm_position(&self) -> u64;
    fn se_play(
        &mut self,
        slot: i32,
        id: i32,
        repeat: i32,
        volume: f32,
        pan: f32,
        speed: f32,
        fade: i32,
    ) -> Result<()>;
    fn se_stop(&mut self, slot: i32, fade: i32) -> Result<()>;
    fn se_stop_all(&mut self, fade: i32) -> Result<()>;
    fn se_volume(&mut self, slot: i32, volume: f32, fade: i32) -> Result<()>;
    fn se_pan(&mut self, slot: i32, pan: f32, fade: i32) -> Result<()>;
    fn se_status(&self, slot: i32) -> i32;
    fn se_once_status(&self, slot: i32) -> i32;
}
#[derive(Debug, Clone, Copy)]
enum Pending {
    BgmMask(i32),
    BgmTime(u64),
    SeMask(i32, i32),
    Once(i32, i32),
}
#[derive(Default)]
pub struct AudioCommandState {
    pending: Option<Pending>,
}
fn scalar(word: i32) -> f32 {
    word as f32 * f32::from_bits(0x3a83126f)
}
pub fn sync_target(ticks: i32) -> u64 {
    (i64::from(ticks) * 1_000_000) as u64 / 60
}
impl AudioCommandState {
    pub fn dispatch(
        &mut self,
        command: &Cmd,
        skip: bool,
        audio: &mut dyn AudioCommands,
    ) -> Result<bool> {
        ensure!(self.pending.is_none(), "Ger audio command is still pending");
        let fade = |value| if skip { 0 } else { value };
        match *command {
            Cmd::BgmPlay {
                id,
                loops,
                volume,
                fade: time,
            } => {
                if audio.bgm_play(id, loops, scalar(volume), fade(time))? {
                    audio.bgm_changed(id)?;
                }
            }
            Cmd::BgmStop { fade: time } => audio.bgm_stop(fade(time))?,
            Cmd::BgmVolume { volume, fade: time } => {
                audio.bgm_volume(scalar(volume), fade(time))?
            }
            Cmd::BgmWait { mask } => self.pending = Some(Pending::BgmMask(mask)),
            Cmd::BgmSync { ticks } => self.pending = Some(Pending::BgmTime(sync_target(ticks))),
            Cmd::SePlay {
                slot,
                id,
                fade: time,
                no_repeat,
                volume,
                pan,
                speed,
            } => {
                if skip && no_repeat != 0 {
                    audio.se_stop(slot, 0)?;
                } else {
                    audio.se_play(
                        slot,
                        id,
                        no_repeat,
                        scalar(volume),
                        scalar(pan),
                        scalar(speed),
                        fade(time),
                    )?;
                }
            }
            Cmd::SeStop { slot, fade: time } => audio.se_stop(slot, fade(time))?,
            Cmd::SeStopAll { fade: time } => audio.se_stop_all(fade(time))?,
            Cmd::SeVolume {
                slot,
                volume,
                fade: time,
            } => audio.se_volume(slot, scalar(volume), fade(time))?,
            Cmd::SePan {
                slot,
                pan,
                fade: time,
            } => audio.se_pan(slot, scalar(pan), fade(time))?,
            Cmd::SeWait { slot, mask } => self.pending = Some(Pending::SeMask(slot, mask)),
            Cmd::SeOnce {
                id,
                mask,
                volume,
                pan,
                ..
            } => {
                if skip {
                    return Ok(true);
                }
                let mut slot = -1i32;
                while audio.se_once_status(slot) & 2 != 0 {
                    slot = slot
                        .checked_sub(1)
                        .ok_or_else(|| anyhow::anyhow!("Ger SE slots exhausted"))?;
                }
                audio.se_play(slot, id, 1, scalar(volume), scalar(pan), 1.0, 0)?;
                self.pending = Some(Pending::Once(slot, mask));
            }
        }
        Ok(self.poll(audio, skip))
    }
    pub fn poll(&mut self, audio: &dyn AudioCommands, skip: bool) -> bool {
        let finished = match self.pending {
            None => true,
            Some(Pending::BgmMask(mask)) => audio.bgm_status() & mask == 0,
            Some(Pending::BgmTime(target)) => {
                audio.bgm_status() & 2 == 0 || audio.bgm_position() >= target
            }
            Some(Pending::SeMask(slot, mask)) => audio.se_status(slot) & mask == 0,
            Some(Pending::Once(slot, mask)) => skip || audio.se_once_status(slot) & mask == 0,
        };
        if finished {
            self.pending = None;
        }
        finished
    }
}

#[cfg(test)]
mod tests;
