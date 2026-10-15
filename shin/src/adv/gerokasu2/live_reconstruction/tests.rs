use super::*;
use crate::adv::gerokasu2::descriptors::EffectDescriptor;

#[derive(Default)]
struct Audio {
    words: Vec<u32>,
    stopping_mask: u32,
    fail_slot: Option<i32>,
}
impl AudioCommands for Audio {
    fn bgm_play(&mut self, id: i32, loops: i32, volume: f32, fade: i32) -> Result<bool> {
        self.words
            .extend([1, id as u32, loops as u32, volume.to_bits(), fade as u32]);
        Ok(true)
    }
    fn bgm_stop(&mut self, fade: i32) -> Result<()> {
        self.words.extend([2, fade as u32]);
        Ok(())
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
        anyhow::ensure!(self.fail_slot != Some(slot), "prepared effect unavailable");
        self.words.extend([
            3,
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
        anyhow::ensure!(self.fail_slot != Some(slot), "prepared effect unavailable");
        self.words.extend([4, slot as u32, fade as u32]);
        Ok(())
    }
    fn bgm_changed(&mut self, _: i32) -> Result<()> {
        panic!("reconstruction does not publish BGM title cues")
    }
    fn bgm_volume(&mut self, _: f32, _: i32) -> Result<()> {
        unreachable!()
    }
    fn bgm_status(&self) -> i32 {
        unreachable!()
    }
    fn bgm_position(&self) -> u64 {
        unreachable!()
    }
    fn se_stop_all(&mut self, _: i32) -> Result<()> {
        unreachable!()
    }
    fn se_volume(&mut self, _: i32, _: f32, _: i32) -> Result<()> {
        unreachable!()
    }
    fn se_pan(&mut self, _: i32, _: f32, _: i32) -> Result<()> {
        unreachable!()
    }
    fn se_status(&self, _: i32) -> i32 {
        unreachable!()
    }
    fn se_once_status(&self, _: i32) -> i32 {
        unreachable!()
    }
}
impl ReconstructionAudio for Audio {
    fn prepare_effect_reconstruction(&mut self) -> Result<()> {
        for slot in 0..5 {
            if self.stopping_mask & (1 << slot) == 0 {
                self.words
                    .extend([5, slot, 6, slot, 0.0f32.to_bits(), 4.0f32.to_bits()]);
            }
        }
        Ok(())
    }
}

#[test]
fn ger_audio_restore_failure_keeps_applied_prefix_and_can_retry() {
    let descriptors = Descriptors::default();
    let mut audio = Audio {
        fail_slot: Some(2),
        stopping_mask: 31,
        ..Default::default()
    };
    assert!(restore_audio(&descriptors, &mut audio).is_err());
    assert_eq!(audio.words, [2, 0, 4, 0, 0, 4, 1, 0]);
    audio.words.clear();
    audio.fail_slot = None;
    restore_audio(&descriptors, &mut audio).unwrap();
    assert_eq!(audio.words.len(), 2 + 32 * 3);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original Ger audio publication block and stopping normalization; CPU only"]
fn ger_audio_restore_matches_native_float_targets_slots_and_stopping_entries() {
    use std::{
        fs,
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("gerokasu2/.local/live-reconstruction");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-live-reconstruction");
    let output = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-live-reconstruction.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut child = Command::new(binary)
        .args([
            root.join("gerokasu2/uncompressed_executable"),
            root.join("gerokasu2/.local/newrodin-medium.fnt"),
            root.join("gerokasu2/.local/system.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let values = [
        0, 0x80000000, 0x3f000000, 0xbf800000, 0x40000000, 0x7f800000, 0x7fc01234, 0x00800000,
    ];
    let mut input = Vec::new();
    let mut expected = Vec::new();
    for profile in 0..256usize {
        let mut descriptors = Descriptors::default();
        descriptors.music.id = match profile % 4 {
            0 => -1,
            1 => i32::MIN,
            _ => profile as i32,
        };
        descriptors.music.volume = f32::from_bits(values[profile % values.len()]);
        descriptors.music.count = (profile as u32).wrapping_mul(1234567);
        descriptors.effects = std::array::from_fn(|slot| EffectDescriptor {
            id: if (slot + profile) % 3 == 0 {
                -1
            } else {
                (slot + profile) as i32
            },
            volume: f32::from_bits(values[(slot + profile) % values.len()]),
            pan: f32::from_bits(values[(slot + profile + 1) % values.len()]),
            speed: f32::from_bits(values[(slot + profile + 2) % values.len()]),
        });
        let mut words = vec![
            descriptors.music.id as u32,
            descriptors.music.volume.to_bits(),
            descriptors.music.count,
        ];
        for effect in descriptors.effects {
            words.extend([
                effect.id as u32,
                effect.volume.to_bits(),
                effect.pan.to_bits(),
                effect.speed.to_bits(),
            ]);
        }
        words.push(profile as u32 & 31);
        for word in words {
            input.extend(word.to_le_bytes());
        }
        let mut audio = Audio {
            stopping_mask: profile as u32 & 31,
            ..Default::default()
        };
        restore_audio(&descriptors, &mut audio).unwrap();
        expected.extend(b"GLR1");
        expected.extend((audio.words.len() as u32).to_le_bytes());
        for word in audio.words {
            expected.extend(word.to_le_bytes());
        }
    }
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&input).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, expected);
    println!(
        "GEROKASU2_LIVE_AUDIO_NATIVE cases=256 slots=8192 float_bits_and_request_order=verified stopping_masks=32 device_playback=excluded"
    );
}
