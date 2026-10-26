use super::*;
use kira::{AudioManager as KiraManager, AudioManagerSettings};
use shin_audio::test_support::{MockBackendSettings, SampleBackend as MockBackend};
use shin_core::format::{
    audio::{AudioBuffer, AudioFrameSource},
    scenario::instruction_elements::FromNumber,
};

struct Tone {
    position: u32,
}
impl AudioFrameSource for Tone {
    fn max_frame_size(&self) -> usize {
        8
    }
    fn sample_rate(&self) -> u32 {
        1000
    }
    fn pre_skip(&self) -> u32 {
        0
    }
    fn pre_roll(&self) -> u32 {
        0
    }
    fn read_frame(&mut self, buffer: &mut AudioBuffer) -> bool {
        if self.position >= 32 {
            return false;
        }
        for _ in 0..8 {
            buffer.push((0.1, 0.1));
        }
        self.position += 8;
        true
    }
    fn samples_seek(&mut self, position: u32) -> anyhow::Result<u32> {
        self.position = position;
        Ok(0)
    }
    fn current_sample_position(&self) -> u32 {
        self.position
    }
}

fn audio(repeat: bool) -> AudioData<Tone> {
    AudioData {
        source: Tone { position: 0 },
        settings: AudioSettings {
            paused: false,
            playback_rate: 1.0,

            fade_in: Tween::IMMEDIATE,
            loop_start: repeat.then_some(0),
            loop_count: 0,
            volume: Volume::default(),
            pan: Pan::default(),
        },
    }
}

#[test]
fn pending_parameters_start_at_playback_and_preserve_queued_pan() {
    let mut tweens = pending_tweens(
        Volume(0.8),
        Pan(0.0),
        1.0,
        Tween::linear(Ticks::from_f32(12.0)),
    );
    assert_eq!(
        pending_status(&tweens),
        AudioWaitStatus::LOADING | AudioWaitStatus::PLAYING | AudioWaitStatus::VOLUME_TWEENING
    );
    tweens
        .volume
        .enqueue_now(0.6, Tween::linear(Ticks::from_f32(6.0)));
    tweens
        .panning
        .enqueue(1.0, Tween::linear(Ticks::from_f32(6.0)));
    tweens
        .panning
        .enqueue(-1.0, Tween::linear(Ticks::from_f32(6.0)));
    let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
        backend_settings: MockBackendSettings { sample_rate: 1000 },
        internal_buffer_size: 1,
        ..Default::default()
    })
    .unwrap();
    let handle = manager.play(audio(true).with_tweens(tweens)).unwrap();
    assert_eq!(
        handle.get_wait_status(),
        AudioWaitStatus::PLAYING
            | AudioWaitStatus::VOLUME_TWEENING
            | AudioWaitStatus::PANNING_TWEENING
    );
    manager.backend_mut().on_start_processing();
    for _ in 0..105 {
        manager.backend_mut().on_start_processing();
        let _ = manager.backend_mut().process();
    }
    assert!(
        !handle
            .get_wait_status()
            .contains(AudioWaitStatus::VOLUME_TWEENING)
    );
    assert!(
        handle
            .get_wait_status()
            .contains(AudioWaitStatus::PANNING_TWEENING)
    );
    manager.backend_mut().on_start_processing();
    for _ in 0..105 {
        manager.backend_mut().on_start_processing();
        let _ = manager.backend_mut().process();
    }
    assert_eq!(handle.get_wait_status(), AudioWaitStatus::PLAYING);
    let frame = manager.backend_mut().process();
    assert!(frame.left > 0.08 && frame.right.abs() < 0.00001);
}

#[test]
fn restore_shortens_owned_inactive_tail_to_four_ticks_without_cutting_it() {
    let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
        backend_settings: MockBackendSettings { sample_rate: 1000 },
        internal_buffer_size: 1,
        ..Default::default()
    })
    .unwrap();
    let mut active = Some(manager.play(audio(true)).unwrap());
    let mut tails = Vec::new();
    stop_effect(
        &mut active,
        &mut tails,
        Tween::linear(Ticks::from_f32(30.0)),
    );
    for _ in 0..10 {
        manager.backend_mut().on_start_processing();
        let _ = manager.backend_mut().process();
    }
    normalize_stopping_effects(&mut tails);
    let mut frame = kira::Frame::ZERO;
    for _ in 0..30 {
        manager.backend_mut().on_start_processing();
        frame = manager.backend_mut().process();
    }
    assert!(frame.left > 0.04 && frame.left < 0.06);
    assert!(
        tails[0]
            .get_wait_status()
            .contains(AudioWaitStatus::PLAYING)
    );
    for _ in 0..40 {
        manager.backend_mut().on_start_processing();
        frame = manager.backend_mut().process();
    }
    assert_eq!(frame, kira::Frame::ZERO);
    normalize_stopping_effects(&mut tails);
    assert!(tails.is_empty());
}

#[test]
fn native_se_speed_clamps_without_reversing_or_freezing_audio() {
    for (input, expected) in [
        (-2.0, 0.01),
        (0.0, 0.01),
        (0.001, 0.01),
        (0.01, 0.01),
        (0.5, 0.5),
        (1.0, 1.0),
        (1.5, 1.5),
        (10.0, 10.0),
        (100.0, 10.0),
    ] {
        assert_eq!(native_se_play_speed(input), expected);
    }
    assert_eq!(native_se_play_speed(f32::NAN), 0.01);
}

#[test]
fn native_stop_always_has_at_least_four_ticks() {
    for (requested, expected) in [
        (-1.0, 4.0),
        (0.0, 4.0),
        (0.5, 4.0),
        (1.0, 4.0),
        (4.0, 4.0),
        (10.0, 10.0),
    ] {
        assert_eq!(
            native_se_stop_tween(Tween::linear(Ticks::from_f32(requested))).duration,
            Ticks::from_f32(expected)
        );
    }
}

#[test]
fn matching_repeat_retargets_live_pcm_without_restarting_the_source() {
    let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
        backend_settings: MockBackendSettings { sample_rate: 1000 },
        internal_buffer_size: 1,
        ..Default::default()
    })
    .unwrap();
    let mut handle = Some(manager.play(audio(true)).unwrap());
    let advance = |manager: &mut KiraManager<MockBackend>, count| {
        let backend = manager.backend_mut();
        let mut frame = kira::Frame::ZERO;
        for _ in 0..count {
            backend.on_start_processing();
            frame = backend.process();
        }
        frame
    };
    advance(&mut manager, 12);
    let before = handle.as_ref().unwrap().position_millis();
    let duration = Tween::linear(shin_core::time::Ticks::from_millis(10.0));
    for (id, old_repeat, new_id, new_repeat) in [
        (Some(SeId::from_number(1)), true, SeId::from_number(2), true),
        (
            Some(SeId::from_number(1)),
            false,
            SeId::from_number(1),
            true,
        ),
        (
            Some(SeId::from_number(1)),
            true,
            SeId::from_number(1),
            false,
        ),
        (None, true, SeId::from_number(1), true),
    ] {
        assert!(!retarget_matching_repeat(
            &mut handle,
            id,
            old_repeat,
            new_id,
            new_repeat,
            Volume(0.5),
            Pan(-1.0),
            2.0,
            duration
        ));
        assert_eq!(handle.as_ref().unwrap().position_millis(), before);
    }
    // Three updates before the next audio callback must use the latest
    // target without replacing the source or exhausting the command queue.
    for volume in [0.8, 0.6, 0.5] {
        assert!(retarget_matching_repeat(
            &mut handle,
            Some(SeId::from_number(1)),
            true,
            SeId::from_number(1),
            true,
            Volume(volume),
            Pan(-1.0),
            2.0,
            duration,
        ));
    }
    advance(&mut manager, 5);
    let live = handle.as_ref().unwrap();
    assert!(
        live.position_millis() > before + 4,
        "Matching repeat must retain and advance its existing decoder"
    );
    assert!(live.get_wait_status().contains(
        AudioWaitStatus::VOLUME_TWEENING
            | AudioWaitStatus::PANNING_TWEENING
            | AudioWaitStatus::PLAY_SPEED_TWEENING
    ));
    let frame = advance(&mut manager, 12);
    assert!((frame.left - 0.05 * std::f32::consts::SQRT_2).abs() < 0.00001);
    assert!(frame.right.abs() < 0.00001);
    assert_eq!(
        handle.as_ref().unwrap().get_wait_status(),
        AudioWaitStatus::PLAYING
    );
    handle.as_mut().unwrap().stop(Tween::IMMEDIATE).unwrap();
    advance(&mut manager, 4);
    assert!(!retarget_matching_repeat(
        &mut handle,
        Some(SeId::from_number(1)),
        true,
        SeId::from_number(1),
        true,
        Volume(1.0),
        Pan(0.0),
        1.0,
        duration
    ));
}

#[test]
fn zero_duration_replacement_retains_the_native_four_tick_tail() {
    let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
        backend_settings: MockBackendSettings { sample_rate: 1000 },
        internal_buffer_size: 1,
        ..Default::default()
    })
    .unwrap();
    let mut old = Some(manager.play(audio(true)).unwrap());
    // past the resampler window's start, so the old source is audible
    for _ in 0..24 {
        let backend = manager.backend_mut();
        backend.on_start_processing();
        backend.process();
    }
    let mut tails = Vec::new();
    assert!(stop_effect(&mut old, &mut tails, Tween::IMMEDIATE));
    assert!(old.is_none());
    assert_eq!(tails.len(), 1);
    let replacement = manager.play(audio(true)).unwrap();
    let backend = manager.backend_mut();
    backend.on_start_processing();
    let first = backend.process();
    assert!(
        first.left > 0.09,
        "Old source retains the native four-tick fade while the new source starts"
    );
    let mut frame = kira::Frame::ZERO;
    for _ in 0..75 {
        backend.on_start_processing();
        frame = backend.process();
    }
    assert!(tails[0].get_wait_status().is_empty());
    assert!((frame.left - 0.1).abs() < 0.00001);
    assert!(
        replacement
            .get_wait_status()
            .contains(AudioWaitStatus::PLAYING)
    );
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_se_play_speed() {
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, expected) in [
        (0xf1c50, 0xa9415013),
        (0xf1c60, 0x1e221009),
        (0xf1c68, 0x39400109),
        (0xf1c6c, 0x340000a9),
        (0xf1c80, 0x91006100),
        (0xf1c84, 0x97ff25e7),
        (0xf1c8c, 0x4ea91d21),
        (0xf1c98, 0x97ff275a),
        (0x1aa5c, 0x2d7f0500),
        (0x1aa60, 0xbd400102),
        (0x1aa64, 0x2a1503e1),
        (0x1aa68, 0x2a1f03e3),
        (0x1aa6c, 0x2a1f03e4),
        (0x1aa70, 0x94035cf8),
        (0x1aa78, 0x2a1503e1),
        (0x1aa80, 0x94035e60),
        (0x1aa90, 0xf10082bf),
        (0x1aa94, 0x54fffda1),
        (0xf2054, 0x390002a8),
        (0xf205c, 0x2901debf),
        (0xf2070, 0x97ff2664),
        (0xf207c, 0x97ff2499),
        (0xf20c4, 0x9400066f),
        (0xf20cc, 0xc85ffed7),
        (0xf20d4, 0x35ffffc9),
        (0xf2900, 0x7100011f),
        (0xf2908, 0x91006260),
        (0xf290c, 0x1a881514),
        (0xf2b04, 0x39400008),
        (0xf2bb4, 0x91048315),
        (0xf2bbc, 0x940007f5),
        (0xf2bc4, 0xb9000f1a),
        (0xf2bcc, 0x940007b5),
        (0xf2be8, 0x97ff239a),
        (0xf2bf8, 0x97ff2396),
        (0xf2c08, 0x97ff2392),
        (0xf2814, 0x91006140),
        (0xf281c, 0x2a1f03e2),
        (0xf2884, 0x9101c140),
        (0xf288c, 0x2a1f03e2),
        (0xbb6e0, 0x12180688),
        (0xbb6f0, 0x7104011f),
        (0xbb88c, 0x97fffe95),
        (0xbb890, 0x2d428660),
        (0xbb898, 0x540003e5),
        (0xbb910, 0x14000029),
        (0x6f24, 0xbd403263),
        (0x6f34, 0x5e21d863),
        (0x6f38, 0xbd4a5902),
        (0x6f4c, 0x1e220862),
        (0x6f54, 0x9403abbf),
        (0xf1e9c, 0xbd494901),
        (0xf1ea4, 0x1e249000),
        (0xf1eb4, 0x1e212040),
        (0xf1eb8, 0x1e205840),
        (0xf1ebc, 0x1e218c08),
        (0xf2080, 0x4ea81d00),
        (0xf2088, 0x97ff2496),
        (0xf2c00, 0x91032315),
        (0xf2c58, 0x97ff2212),
        (0xf2c60, 0x94000838),
        (0xf4d68, 0xbd02aa88),
        (0xf1f20, 0x2a1f03e2),
        (0xf1f24, 0x94000137),
        (0xf2430, 0x710012df),
        (0xf2434, 0x321e03e8),
        (0xf2438, 0x1a88c2c8),
        (0xf243c, 0x7100011f),
        (0xf2440, 0x540002ad),
        (0xf2468, 0x97ff23ee),
        (0xf247c, 0x97ff2561),
        (0xf2480, 0x390002df),
        (0xf24f0, 0x97fffdac),
        (0xf1d2c, 0x39400328),
        (0xf1d30, 0x340005e8),
        (0xf28bc, 0x3940026a),
        (0xf28c0, 0x3400008a),
        (0xf28c8, 0x6b01015f),
        (0xf28cc, 0x54000180),
        (0xf1ee0, 0x394002aa),
        (0xf1ee4, 0x3400014a),
        (0xf1eec, 0x6b19015f),
        (0xf1ef8, 0x6b18015f),
        (0xf1f00, 0xb94012aa),
        (0xf1f04, 0x2a17014a),
        (0xf1f08, 0x3400158a),
        (0xf21bc, 0x97ff2499),
        (0xf21c0, 0x1e2202cb),
        (0xf21d4, 0x97ff260b),
        (0xf21dc, 0x97ff2491),
        (0xf21f0, 0x97ff2604),
        (0xf21f4, 0x910322a0),
        (0xf21f8, 0x97ff248a),
        (0xf220c, 0x97ff25fd),
        (0xf2930, 0x91032260),
        (0xf2938, 0x97ff242e),
        (0xf293c, 0x321c0288),
    ] {
        assert_eq!(word(a), expected, "native SE speed{a:x}");
    }
    assert_eq!(word(0x159a58), 0.001f32.to_bits());
    assert_eq!(word(0x159948), 0.01f32.to_bits());
}

#[test]
fn wait_all_includes_one_shots_and_excludes_repeating_tracks() {
    let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
        backend_settings: MockBackendSettings { sample_rate: 1000 },
        internal_buffer_size: 1,
        ..Default::default()
    })
    .unwrap();
    let mut slots = [(); SE_SLOT_COUNT].map(|_| None);
    slots[0] = Some(manager.play(audio(true)).unwrap());
    let mut repeating = [false; SE_SLOT_COUNT];
    repeating[0] = true;
    let mut one_shots = BTreeMap::new();
    assert!(se_wait_status(&slots, &repeating, &one_shots, -1).is_empty());
    assert!(
        se_wait_status(&slots, &repeating, &one_shots, 0).contains(AudioWaitStatus::PLAYING)
    );
    one_shots.insert(-1, manager.play(audio(false)).unwrap());
    one_shots.insert(-2, manager.play(audio(false)).unwrap());
    assert!(
        se_wait_status(&slots, &repeating, &one_shots, -2).contains(AudioWaitStatus::PLAYING)
    );
    let backend = manager.backend_mut();
    for _ in 0..64 {
        backend.on_start_processing();
        backend.process();
    }
    assert!(se_wait_status(&slots, &repeating, &one_shots, -1).is_empty());
    assert!(
        se_wait_status(&slots, &repeating, &one_shots, 0).contains(AudioWaitStatus::PLAYING)
    );
    // Native SESTOP removes the entry from both slot and all-effects
    // waits immediately;its audible fade remains owned independently.
    let mut tails = Vec::new();
    assert!(stop_effect(
        &mut slots[0],
        &mut tails,
        Tween::linear(Ticks::from_f32(1.0))
    ));
    repeating[0] = false;
    assert!(se_wait_status(&slots, &repeating, &one_shots, 0).is_empty());
    assert!(se_wait_status(&slots, &repeating, &one_shots, -1).is_empty());
    backend.on_start_processing();
    let first = backend.process();
    assert!(first.left > 0.09);
    assert_eq!(tails.len(), 1);
    assert!(
        tails[0]
            .get_wait_status()
            .contains(AudioWaitStatus::PLAYING)
    );
    for _ in 0..60 {
        backend.on_start_processing();
        backend.process();
    }
    assert!(
        tails[0]
            .get_wait_status()
            .contains(AudioWaitStatus::PLAYING)
    );
    for _ in 0..10 {
        backend.on_start_processing();
        backend.process();
    }
    assert!(tails[0].get_wait_status().is_empty());
    tails.retain(|handle| !handle.get_wait_status().is_empty());
    assert!(tails.is_empty());
}
