//! Updated BGM command callbacks; the host supplies resolved skip/read policy.
use anyhow::Result;
use shin_core::vm::higurashi::command::HigurashiRuntimeCommand as Cmd;

pub trait MusicCommands {
    fn play_bgm(&mut self, id: i32, fade: i32, loop_count: u32, volume: f32) -> Result<()>;
    fn stop_bgm(&mut self, fade: i32);
    fn volume_bgm(&mut self, volume: f32, fade: i32) -> Result<()>;
    fn bgm_status(&self) -> i32;
    fn bgm_clock(&self) -> u64;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicWait {
    Status(i32),
    Sync(u64),
}
impl MusicWait {
    pub fn complete(self, audio: &dyn MusicCommands) -> bool {
        match self {
            Self::Status(mask) => audio.bgm_status() & mask == 0,
            Self::Sync(target) => audio.bgm_status() & 2 == 0 || audio.bgm_clock() >= target,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicCommand {
    Complete,
    Wait(MusicWait),
}

pub fn sync_target(time: i32) -> u64 {
    // Actual76c4: signed multiply, unsigned divide; negative ticks retain the
    // native large deadline and normally finish only when playback stops.
    (i64::from(time) * 1_000_000) as u64 / 60
}

pub fn dispatch(
    command: &Cmd,
    suppress: bool,
    audio: &mut dyn MusicCommands,
) -> Result<Option<MusicCommand>> {
    let result = match command {
        Cmd::BGMPLAY {
            args: [id, fade, count, volume],
        } => {
            audio.play_bgm(
                *id,
                if suppress { 0 } else { *fade },
                *count as u32,
                *volume as f32 * 0.001,
            )?;
            MusicCommand::Complete
        }
        Cmd::BGMSTOP { fade } => {
            audio.stop_bgm(if suppress { 0 } else { *fade });
            MusicCommand::Complete
        }
        Cmd::BGMVOL {
            args: [volume, fade],
        } => {
            audio.volume_bgm(*volume as f32 * 0.001, if suppress { 0 } else { *fade })?;
            MusicCommand::Complete
        }
        Cmd::BGMWAIT { status } => MusicCommand::Wait(MusicWait::Status(*status)),
        Cmd::BGMSYNC { time } => MusicCommand::Wait(MusicWait::Sync(sync_target(*time))),
        _ => return Ok(None),
    };
    Ok(Some(result))
}

#[cfg(test)]
mod tests;
