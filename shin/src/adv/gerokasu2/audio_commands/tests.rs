use super::*;
#[derive(Default)]
struct Services {
    events: Vec<u32>,
    status: i32,
    clock: u64,
    changed: bool,
    occupied: i32,
    played: Option<i32>,
}
impl Services {
    fn event(&mut self, words: &[u32]) {
        self.events.extend(words);
    }
}
impl AudioCommands for Services {
    fn bgm_play(&mut self, id: i32, loops: i32, volume: f32, fade: i32) -> Result<bool> {
        self.event(&[1, id as u32, loops as u32, volume.to_bits(), fade as u32]);
        Ok(self.changed)
    }
    fn bgm_changed(&mut self, id: i32) -> Result<()> {
        self.event(&[2, id as u32]);
        Ok(())
    }
    fn bgm_stop(&mut self, fade: i32) -> Result<()> {
        self.event(&[3, fade as u32]);
        Ok(())
    }
    fn bgm_volume(&mut self, volume: f32, fade: i32) -> Result<()> {
        self.event(&[4, volume.to_bits(), fade as u32]);
        Ok(())
    }
    fn bgm_status(&self) -> i32 {
        self.status
    }
    fn bgm_position(&self) -> u64 {
        self.clock
    }
    fn se_play(
        &mut self,
        slot: i32,
        id: i32,
        repeat: i32,
        volume: f32,
        pan: f32,
        speed: f32,
        fade: i32,
    ) -> Result<()> {
        self.played = Some(slot);
        self.event(&[
            5,
            slot as u32,
            id as u32,
            repeat as u32,
            volume.to_bits(),
            pan.to_bits(),
            speed.to_bits(),
            fade as u32,
        ]);
        Ok(())
    }
    fn se_stop(&mut self, slot: i32, fade: i32) -> Result<()> {
        self.event(&[6, slot as u32, fade as u32]);
        Ok(())
    }
    fn se_stop_all(&mut self, fade: i32) -> Result<()> {
        self.event(&[7, fade as u32]);
        Ok(())
    }
    fn se_volume(&mut self, slot: i32, volume: f32, fade: i32) -> Result<()> {
        self.event(&[8, slot as u32, volume.to_bits(), fade as u32]);
        Ok(())
    }
    fn se_pan(&mut self, slot: i32, pan: f32, fade: i32) -> Result<()> {
        self.event(&[9, slot as u32, pan.to_bits(), fade as u32]);
        Ok(())
    }
    fn se_status(&self, _: i32) -> i32 {
        self.status
    }
    fn se_once_status(&self, slot: i32) -> i32 {
        if self.played == Some(slot) {
            self.status
        } else if -1 - slot < self.occupied {
            2
        } else {
            0
        }
    }
}
fn command(index: u32, args: [i32; 7]) -> Cmd {
    let [a, b, c, d, e, f, g] = args;
    match index {
        0 => Cmd::BgmPlay {
            id: a,
            fade: b,
            loops: c,
            volume: d,
        },
        1 => Cmd::BgmStop { fade: a },
        2 => Cmd::BgmVolume { volume: a, fade: b },
        3 => Cmd::BgmWait { mask: a },
        4 => Cmd::BgmSync { ticks: a },
        5 => Cmd::SePlay {
            slot: a,
            id: b,
            fade: c,
            no_repeat: d,
            volume: e,
            pan: f,
            speed: g,
        },
        6 => Cmd::SeStop { slot: a, fade: b },
        7 => Cmd::SeStopAll { fade: a },
        8 => Cmd::SeVolume {
            slot: a,
            volume: b,
            fade: c,
        },
        9 => Cmd::SePan {
            slot: a,
            pan: b,
            fade: c,
        },
        10 => Cmd::SeWait { slot: a, mask: b },
        11 => Cmd::SeOnce {
            id: a,
            mask: b,
            volume: c,
            pan: d,
            speed: e,
        },
        _ => unreachable!(),
    }
}
#[test]
fn ger_audio_retained_once_skip_and_signed_sync_preserve_owner() {
    let mut s = Services {
        status: 3,
        occupied: 2,
        ..Default::default()
    };
    let mut state = AudioCommandState::default();
    assert!(
        !state
            .dispatch(
                &Cmd::SeOnce {
                    id: 7,
                    mask: 3,
                    volume: 2000,
                    pan: -999,
                    speed: 555
                },
                false,
                &mut s
            )
            .unwrap()
    );
    assert_eq!(s.played, Some(-3));
    let events = s.events.clone();
    assert!(!state.poll(&s, false));
    assert_eq!(s.events, events);
    assert!(state.poll(&s, true));
    assert_eq!(s.played, Some(-3));
    assert!(
        !state
            .dispatch(&Cmd::BgmSync { ticks: -1 }, true, &mut s)
            .unwrap()
    );
    s.status = 0;
    assert!(state.poll(&s, false));
    assert_eq!(sync_target(-1), (-1_000_000i64 as u64) / 60);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original complete Ger BGM/SE callbacks on bounded host players;CPU only"]
fn ger_audio_commands_match_native_callbacks_and_retained_polls() {
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("gerokasu2/.local/audio");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-audio-commands");
    let c = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-audio-commands.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
    let mut input = Vec::new();
    let mut expected = Vec::new();
    let mut emit = |row: [u32; 16], done: bool, s: &Services| {
        input.extend(row.into_iter().flat_map(u32::to_le_bytes));
        let mut out = vec![done as u32, s.events.len() as u32];
        out.extend(&s.events);
        expected.push(out);
    };
    for kind in 0..12 {
        for base in [i32::MIN, -3000, -1, 0, 1, 1000, 9000, i32::MAX] {
            for skip in [false, true] {
                for status in [0, 1, 2, 3, 4, 31, -1] {
                    let args = std::array::from_fn(|i| base.wrapping_add(i as i32));
                    let cmd = command(kind, args);
                    let clock = if kind == 4 {
                        sync_target(args[0]).saturating_sub(1)
                    } else {
                        1234567
                    };
                    let mut s = Services {
                        status,
                        clock,
                        changed: base & 1 != 0,
                        occupied: 3,
                        ..Default::default()
                    };
                    let mut state = AudioCommandState::default();
                    let done = state.dispatch(&cmd, skip, &mut s).unwrap();
                    let mut row = [0u32; 16];
                    row[0] = kind;
                    row[1] = skip as u32;
                    row[2] = 1;
                    row[3] = status as u32;
                    row[4] = clock as u32;
                    row[5] = (clock >> 32) as u32;
                    row[6] = s.changed as u32;
                    row[7] = 3;
                    row[8..15].copy_from_slice(&args.map(|v| v as u32));
                    emit(row, done, &s);
                    if !done {
                        for (status, skip) in [(status, false), (0, true)] {
                            s.status = status;
                            s.events.clear();
                            row[2] = 0;
                            row[1] = skip as u32;
                            row[3] = status as u32;
                            emit(row, state.poll(&s, skip), &s);
                        }
                    }
                }
            }
        }
    }
    let request = folder.join("request.bin");
    fs::write(&request, input).unwrap();
    let output = Command::new(binary)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("gerokasu2/.local/newrodin-medium.fnt"))
        .arg(root.join("gerokasu2/.local/system.fnt"))
        .stdin(Stdio::from(fs::File::open(request).unwrap()))
        .output()
        .unwrap();
    fs::write(folder.join("native.trace"), &output.stdout).unwrap();
    fs::write(folder.join("native.stderr"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut at = 0;
    let mut words = 0;
    for (row, expected) in expected.iter().enumerate() {
        assert_eq!(&output.stdout[at..at + 4], b"GAC1");
        at += 4;
        let actual: Vec<_> = output.stdout[at..at + expected.len() * 4]
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        at += expected.len() * 4;
        assert_eq!(&actual, expected, "audio callback {row}");
        words += expected.len();
    }
    assert_eq!(at, output.stdout.len());
    println!(
        "GEROKASU2_AUDIO_COMMANDS_NATIVE snapshots={} words={} callbacks=12 raw_flags_signed_parameters_retained_wait_sync_once_skip=verified",
        expected.len(),
        words
    );
}
