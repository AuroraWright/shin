use super::*;
use crate::adv::higurashi::{descriptors::PlaybackDescriptors, live_reconstruction::restore_audio};

fn loaded(audio: &mut HigurashiAudio, slot: i32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while audio.effect_status(slot) & 1 != 0 {
        assert!(Instant::now() < deadline, "effect load {slot}");
        audio.update(Ticks::ZERO, false);
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
#[ignore = "real Higurashi descriptor restoration on offscreen Metal and the default audio output"]
fn higurashi_audio_reconstruction_preserves_matching_sources_and_loader_ownership() {
    let (_, _, assets) = super::super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let scenario = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut audio = HigurashiAudio::new(Arc::new(AudioManager::new_for_tests()), assets, scenario);
    audio.set_paused(true);
    audio.play_bgm(114, 0, 0, 0.8).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while audio.bgm_status() & 1 != 0 {
        assert!(Instant::now() < deadline);
        audio.update(Ticks::ZERO, false);
        std::thread::sleep(Duration::from_millis(2));
    }
    while audio.pop_music_event().is_some() {}
    let request = |slot| EffectPlay {
        slot,
        id: 48,
        fade: 0,
        count: 0,
        volume: 0.8,
        pan: 0.0,
        speed: 1.0,
    };
    for slot in [0, -1, 3, 6] {
        audio.play_effect(request(slot)).unwrap();
        loaded(&mut audio, slot);
    }
    let positions = [0, -1, 6].map(|slot| audio.effects.position(slot));
    audio.play_effect(request(4)).unwrap();
    audio.play_effect(request(5)).unwrap();
    assert_eq!(audio.effect_status(4) & 3, 3);
    assert_eq!(audio.effect_status(5) & 3, 3);
    let mut descriptors = PlaybackDescriptors::new();
    descriptors.music.id = 114;
    descriptors.music.volume = 0.5;
    for slot in [0, 4, 6] {
        descriptors.effects[slot].id = 48;
        descriptors.effects[slot].volume = 0.6;
        descriptors.effects[slot].pan = -0.5;
        descriptors.effects[slot].speed = 1.5;
    }
    audio.stop_effect(3, 60).unwrap();
    audio.stop_effect(6, 60).unwrap();
    restore_audio(&descriptors, &mut audio).unwrap();
    assert_eq!(
        audio.bgm_status() & 1,
        0,
        "matching BGM source stays loaded"
    );
    assert_eq!(audio.pop_music_event(), None);
    assert_eq!(
        [0, -1, 6].map(|slot| audio.effects.position(slot)),
        positions
    );
    assert_eq!(audio.effect_status(0) & 1, 0);
    assert_eq!(audio.effect_status(-1) & 1, 0);
    assert_eq!(
        audio.effect_status(4) & 3,
        3,
        "matching loading entry stays owned"
    );
    assert_eq!(
        audio.effect_status(5),
        0,
        "inactive saved slot cancels its loader"
    );
    assert_eq!(audio.effect_descriptor_id(0), 48);
    assert_eq!(audio.effect_descriptor_id(4), 48);
    assert_eq!(audio.effect_descriptor_id(3), -1);
    loaded(&mut audio, 4);
    assert_eq!(audio.effects.position(4), Some(0));
    let deadline = Instant::now() + Duration::from_secs(5);
    while audio.effects.position(3).is_some() {
        assert!(
            Instant::now() < deadline,
            "four-tick reconstructed tail did not retire"
        );
        audio.update(Ticks::ZERO, false);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        audio.effects.position(6),
        positions[2],
        "saved repeat revives its matching stopping source"
    );
    assert!(
        audio.active_effect_slots().contains(&-1),
        "negative active source is outside saved32-slot replacement"
    );
    audio.stop_all_effects(0).unwrap();
    audio.stop_bgm(0);
    println!(
        "Higurashi audio reconstruction:the default audio output; matching BGM and active/negative SE clocks, matching loader ownership, inactive loader cancellation, stopping-tail four-tick retirement, matching stopping repeat revival and restored32 descriptors pass"
    );
}
