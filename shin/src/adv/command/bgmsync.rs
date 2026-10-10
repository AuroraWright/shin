use shin_core::vm::command::types::AudioWaitStatus;

use super::prelude::*;

#[derive(Debug)]
pub struct BGMSYNC {
    token: Option<command::token::BGMSYNC>,
    target_micros: u64,
}

fn sync_target(sync_time: i32) -> u64 {
    (i64::from(sync_time) * 1_000_000) as u64 / 60
}

fn sync_finished(status: AudioWaitStatus, position: u64, target: u64) -> bool {
    !status.contains(AudioWaitStatus::PLAYING) || position >= target
}

impl StartableCommand for command::runtime::BGMSYNC {
    type StateInfo = ();

    fn apply_state(&self, _state: &mut VmState) {}

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        // Switch poller 0x6e20 compares the playback position in microseconds
        // against unsigned(sync_time * 1_000_000) / 60. Preserve its behavior
        // for negative values too: they wait until playback stops.
        let target = sync_target(self.sync_time);
        Yield(
            BGMSYNC {
                token: Some(self.token),
                target_micros: target,
            }
            .into(),
        )
    }
}

impl UpdatableCommand for BGMSYNC {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        let bgm = &adv_state.audio.bgm_player;
        if sync_finished(
            bgm.get_wait_status(),
            bgm.position_micros(),
            self.target_micros,
        ) {
            Some(self.token.take().unwrap().finish())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires supplied executable and compiled SHIN_REFERENCE_BGM_ORACLE on AArch64"]
    fn reference_native_bgmsync_arithmetic_oracle() {
        let oracle = std::env::var("SHIN_REFERENCE_BGM_ORACLE").unwrap();
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable");
        for ticks in [i32::MIN, -61, -1, 0, 1, 60, 1647, i32::MAX] {
            let target = sync_target(ticks);
            for clock in [0, target.saturating_sub(1), target, target + 1, u64::MAX] {
                let output = std::process::Command::new(&oracle)
                    .arg(&executable)
                    .arg(ticks.to_string())
                    .arg(clock.to_string())
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "Native BGMSYNC oracle failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let native: u32 = std::str::from_utf8(&output.stdout)
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                assert_eq!(
                    native != 0,
                    sync_finished(AudioWaitStatus::PLAYING, clock, target),
                    "ticks={ticks}, clock={clock}"
                );
            }
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_microsecond_sync_clock_and_unsigned_deadline() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x6e3c, 0x9400b3f5),
            (0x6e40, 0x370800a0),
            (0x6e44, 0x320003e0),
            (0x6e58, 0x9400b40a),
            (0x6e5c, 0xb9801a68),
            (0x6e60, 0x52884809),
            (0x6e64, 0x72a001e9),
            (0x6e68, 0x9b097d08),
            (0x6e6c, 0xb201e3e9),
            (0x6e70, 0xf2911129),
            (0x6e74, 0x9bc97d08),
            (0x6e78, 0xeb48141f),
            (0x6e7c, 0x1a9f37e0),
            (0x33ea0, 0xaa1f03e0),
            (0x33eb0, 0x14030350),
            (0xf4c0c, 0xf9414a80),
            (0xf4c18, 0xf9401508),
            (0xf4c28, 0xaa1f03f4),
        ] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected,
                "Native BGMSYNC instruction {address:x}"
            );
        }
    }

    #[test]
    fn synchronizes_to_ticks_and_releases_on_stop() {
        let target = sync_target(60);
        assert_eq!(target, 1_000_000);
        assert!(!sync_finished(AudioWaitStatus::PLAYING, 999_000, target));
        assert!(sync_finished(AudioWaitStatus::PLAYING, 1_000_000, target));
        assert!(sync_finished(AudioWaitStatus::empty(), 0, target));
    }

    #[test]
    fn exact_microsecond_deadline_avoids_millisecond_and_float_rounding() {
        let target = sync_target(1);
        assert_eq!(target, 16_666);
        assert!(!sync_finished(AudioWaitStatus::PLAYING, 16_665, target));
        assert!(sync_finished(AudioWaitStatus::PLAYING, 16_666, target));
        assert!(!sync_finished(
            AudioWaitStatus::LOADING | AudioWaitStatus::PLAYING,
            0,
            target
        ));
        let large = sync_target(i32::MAX);
        assert_eq!(large, 35_791_394_116_666);
        assert!(!sync_finished(AudioWaitStatus::PLAYING, large - 1, large));
        assert!(sync_finished(AudioWaitStatus::PLAYING, large, large));
    }

    #[test]
    fn negative_target_waits_until_playback_stops() {
        let target = sync_target(-1);
        let position = 3_600_000_000;
        assert!(!sync_finished(AudioWaitStatus::PLAYING, position, target));
        assert!(sync_finished(AudioWaitStatus::empty(), position, target));
    }
}
