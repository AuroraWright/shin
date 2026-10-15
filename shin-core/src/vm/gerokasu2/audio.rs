//! Later-engine audio words retained until the native command consumer.
#[derive(Debug, Clone)]
pub enum Gerokasu2AudioCommand {
    BgmPlay {
        id: i32,
        fade: i32,
        loops: i32,
        volume: i32,
    },
    BgmStop {
        fade: i32,
    },
    BgmVolume {
        volume: i32,
        fade: i32,
    },
    BgmWait {
        mask: i32,
    },
    BgmSync {
        ticks: i32,
    },
    SePlay {
        slot: i32,
        id: i32,
        fade: i32,
        no_repeat: i32,
        volume: i32,
        pan: i32,
        speed: i32,
    },
    SeStop {
        slot: i32,
        fade: i32,
    },
    SeStopAll {
        fade: i32,
    },
    SeVolume {
        slot: i32,
        volume: i32,
        fade: i32,
    },
    SePan {
        slot: i32,
        pan: i32,
        fade: i32,
    },
    SeWait {
        slot: i32,
        mask: i32,
    },
    SeOnce {
        id: i32,
        mask: i32,
        volume: i32,
        pan: i32,
        speed: i32,
    },
}
impl Gerokasu2AudioCommand {
    pub fn opcode(&self) -> u8 {
        match self {
            Self::BgmPlay { .. } => 0x90,
            Self::BgmStop { .. } => 0x91,
            Self::BgmVolume { .. } => 0x92,
            Self::BgmWait { .. } => 0x93,
            Self::BgmSync { .. } => 0x94,
            Self::SePlay { .. } => 0x95,
            Self::SeStop { .. } => 0x96,
            Self::SeStopAll { .. } => 0x97,
            Self::SeVolume { .. } => 0x98,
            Self::SePan { .. } => 0x99,
            Self::SeWait { .. } => 0x9a,
            Self::SeOnce { .. } => 0x9b,
        }
    }
    pub fn words(&self) -> Vec<i32> {
        match *self {
            Self::BgmPlay {
                id,
                fade,
                loops,
                volume,
            } => vec![id, fade, loops, volume],
            Self::BgmStop { fade } | Self::SeStopAll { fade } => vec![fade],
            Self::BgmVolume { volume, fade } => vec![volume, fade],
            Self::BgmWait { mask } => vec![mask],
            Self::BgmSync { ticks } => vec![ticks],
            Self::SePlay {
                slot,
                id,
                fade,
                no_repeat,
                volume,
                pan,
                speed,
            } => vec![slot, id, fade, no_repeat, volume, pan, speed],
            Self::SeStop { slot, fade } => vec![slot, fade],
            Self::SeVolume { slot, volume, fade } => vec![slot, volume, fade],
            Self::SePan { slot, pan, fade } => vec![slot, pan, fade],
            Self::SeWait { slot, mask } => vec![slot, mask],
            Self::SeOnce {
                id,
                mask,
                volume,
                pan,
                speed,
            } => vec![id, mask, volume, pan, speed],
        }
    }
}
