use super::*;
use crate::adv::higurashi::{
    descriptors::{MessageDescriptor, MusicDescriptor},
    message_playback::{HigurashiMessagePlayback, MessageVoice},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[derive(Default)]
pub(in crate::adv::higurashi) struct Audio {
    pub events: Vec<String>,
    pub stops: u32,
    pub fail_slot: Option<i32>,
    pub installed: Option<[EffectDescriptor; 32]>,
}
impl MessageVoice for Audio {
    fn is_playing(&self) -> bool {
        false
    }
    fn position_micros(&self) -> u64 {
        0
    }
    fn play(&mut self, _: &str, _: f32, _: bool, _: u32, _: u32) -> bool {
        false
    }
    fn stop(&mut self) {
        self.stops += 1;
    }
}
impl VoiceCommands for Audio {
    fn play_command(&mut self, _: &str, _: f32, _: i32) {
        unreachable!()
    }
    fn status_bits(&self) -> i32 {
        0
    }
    fn music(&mut self) -> Option<&mut dyn MusicCommands> {
        Some(self)
    }
    fn effects(&mut self) -> Option<&mut dyn EffectCommands> {
        Some(self)
    }
}
impl MusicCommands for Audio {
    fn play_bgm(&mut self, id: i32, fade: i32, count: u32, volume: f32) -> Result<()> {
        assert_eq!((fade, count), (0, 0));
        self.events.push(format!("B {id} {}", volume.to_bits()));
        Ok(())
    }
    fn stop_bgm(&mut self, fade: i32) {
        assert_eq!(fade, 0);
        self.events.push("B -1 0".into());
    }
    fn volume_bgm(&mut self, _: f32, _: i32) -> Result<()> {
        unreachable!()
    }
    fn bgm_status(&self) -> i32 {
        0
    }
    fn bgm_clock(&self) -> u64 {
        0
    }
}
impl EffectCommands for Audio {
    fn prepare_effect_command(
        &mut self,
        _: &shin_core::vm::higurashi::command::HigurashiRuntimeCommand,
    ) {
    }
    fn effect_descriptor_id(&self, slot: i32) -> i32 {
        self.installed.as_ref().map_or(-1, |e| e[slot as usize].id)
    }
    fn play_effect(&mut self, r: EffectPlay) -> Result<()> {
        anyhow::ensure!(self.fail_slot != Some(r.slot), "effect unavailable");
        assert_eq!((r.fade, r.count), (0, 0));
        self.events.push(format!(
            "E {} {} {} {} {}",
            r.slot,
            r.id,
            r.volume.to_bits(),
            r.pan.to_bits(),
            r.speed.to_bits()
        ));
        Ok(())
    }
    fn stop_effect(&mut self, slot: i32, fade: i32) -> Result<()> {
        anyhow::ensure!(self.fail_slot != Some(slot), "effect unavailable");
        assert_eq!(fade, 0);
        self.events.push(format!("T {slot}"));
        Ok(())
    }
    fn stop_all_effects(&mut self, _: i32) -> Result<()> {
        unreachable!()
    }
    fn volume_effect(&mut self, _: i32, _: f32, _: i32) -> Result<()> {
        unreachable!()
    }
    fn pan_effect(&mut self, _: i32, _: f32, _: i32) -> Result<()> {
        unreachable!()
    }
    fn effect_status(&self, _: i32) -> i32 {
        0
    }
    fn active_effect_slots(&self) -> Vec<i32> {
        vec![]
    }
}
impl ReconstructionAudio for Audio {
    fn prepare_effect_reconstruction(&mut self) -> Result<()> {
        self.events.push("F 2".into());
        Ok(())
    }
    fn install_effect_descriptors(&mut self, effects: &[EffectDescriptor; 32]) {
        self.installed = Some(*effects);
    }
}

#[test]
#[ignore = "executes original message preparation and native audio publication block with checked services"]
fn higurashi_live_reconstruction_matches_native_message_and_audio_publication() {
    let mut input = String::new();
    let mut expected = Vec::new();
    for flags in [0, 1, 2, 3, u32::MAX] {
        for disabled in [false, true] {
            for blocked in [false, true] {
                let mut voice = Audio::default();
                let mut message = HigurashiMessagePlayback::default();
                message.flags = flags;
                message.voice_disabled = disabled;
                message.input_blocked = blocked;
                message.current_time = 42.5;
                message.voice_delay = 17.0;
                message.voice_playing = true;
                message.natural_slide = 0.75;
                message.modal_slide = 0.5;
                message.shown = true;
                message.prepare_reconstruction(&mut voice);
                input.push_str(&format!(
                    "M {flags:x} {} {} 1\n",
                    u8::from(disabled),
                    u8::from(blocked)
                ));
                expected.push(format!(
                    "M {} {} {} {}",
                    message.flags,
                    u8::from(message.voice_disabled),
                    u8::from(message.input_blocked),
                    voice.stops
                ));
                assert_eq!(
                    (
                        message.current_time,
                        message.voice_delay,
                        message.voice_playing
                    ),
                    (42.5, 17.0, true)
                );
                assert_eq!(
                    (message.natural_slide, message.modal_slide, message.shown),
                    (0.75, 0.5, true)
                );
                assert!(!message.click(Default::default(), &mut voice));
            }
        }
    }
    // Native nullable voice handle also leaves unrelated message memory intact.
    input.push_str("M ffffffff 1 0 0\n");
    expected.push("M 0 0 1 0".into());
    let values = [
        0.0,
        -0.0,
        0.5,
        -1.0,
        2.0,
        f32::INFINITY,
        f32::NAN,
        f32::MIN_POSITIVE,
    ];
    for profile in 0..128 {
        let descriptors = PlaybackDescriptors {
            music: MusicDescriptor {
                id: match profile % 4 {
                    0 => -1,
                    1 => i32::MIN,
                    _ => profile as i32,
                },
                volume: values[profile % 8],
                count: profile as u32 * 1234567,
            },
            effects: std::array::from_fn(|slot| EffectDescriptor {
                id: if (slot + profile) % 3 == 0 {
                    -1
                } else {
                    (slot + profile) as i32
                },
                volume: values[(slot + profile) % 8],
                pan: values[(slot + profile + 1) % 8],
                speed: values[(slot + profile + 2) % 8],
            }),
            message: MessageDescriptor {
                wait_flag: 0,
                style: 0,
                options: [0; 2],
                text: None,
            },
        };
        input.push_str(&format!(
            "A {} {:x} {}",
            descriptors.music.id,
            descriptors.music.volume.to_bits(),
            descriptors.music.count
        ));
        for e in &descriptors.effects {
            input.push_str(&format!(
                " {} {:x} {:x} {:x}",
                e.id,
                e.volume.to_bits(),
                e.pan.to_bits(),
                e.speed.to_bits()
            ));
        }
        input.push('\n');
        let mut audio = Audio::default();
        restore_audio(&descriptors, &mut audio).unwrap();
        assert_eq!(
            audio.installed.unwrap().map(|e| e.id),
            descriptors.effects.map(|e| e.id)
        );
        expected.push(format!("A {}", audio.events.join(" ")));
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_LIVE_RECONSTRUCTION").unwrap())
            .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.lines().count(), expected.len());
    for (i, (actual, expected)) in output.lines().zip(expected).enumerate() {
        assert_eq!(actual, expected, "native publication {i}");
    }
    println!(
        "Higurashi live reconstruction:41 message preparations and128 audio publications/4096 slot decisions match original840b8/19f40/c312c; full unrelated native message memory retained"
    );
}

#[test]
fn higurashi_audio_publication_failure_retains_prefix_and_defers_descriptor_install() {
    let d = PlaybackDescriptors::new();
    let mut a = Audio {
        fail_slot: Some(5),
        ..Default::default()
    };
    assert!(restore_audio(&d, &mut a).is_err());
    assert!(a.installed.is_none());
    assert_eq!(
        a.events,
        vec!["B -1 0", "F 2", "T 0", "T 1", "T 2", "T 3", "T 4"]
    );
    a.fail_slot = None;
    restore_audio(&d, &mut a).unwrap();
    assert!(a.installed.is_some());
}
