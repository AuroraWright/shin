use super::*;
use crate::app::higurashi::quiz_scene::{catalog, tests::append_string};
use std::time::{Duration, Instant};

#[test]
#[ignore = "actual updated quiz S99 requests,scene voice constructor/disposal,BUP parent mouth callbacks and global gain reads;no audio/GPU/window/saves"]
fn higurashi_quiz_voice_adapter_matches_original_namespace_flags_and_feedback() {
    use std::fmt::Write as _;
    let mut names: Vec<&str> = catalog::POOLS
        .iter()
        .flatten()
        .chain(catalog::SPECIAL.iter())
        .map(|q| q.voice)
        .collect();
    names.extend(catalog::REACTIONS.iter().flatten().copied());
    names.extend(["VT_QUIZ_SYS01", "VT_QUIZ_SYS02"]);
    assert_eq!(names.len(), 582);
    let mut input = String::new();
    let mut expected = Vec::new();
    for name in names {
        for reaction in [false, true] {
            writeln!(input, "P {} {name}", u8::from(reaction)).unwrap();
            let (path, flags) = request(name, reaction);
            let mut row = format!("P {flags} {} 1", 1.0f32.to_bits());
            append_string(&mut row, &path);
            expected.push(row);
        }
    }
    for character in [0, 1, 7, 21, 255] {
        for level in [0, 1, 2, 3, 255, u32::MAX] {
            for panning_enabled in [false, true] {
                writeln!(input, "F {character} {level} {}", u8::from(panning_enabled)).unwrap();
                let mouth = if level <= 2 { level } else { 0 };
                expected.push(format!("F {character} {level} {mouth} 0 1"));
            }
        }
    }
    for percent in [0, 1, 55, 70, 99, 100, 101, 255] {
        for paused in [false, true] {
            writeln!(input, "G {percent} {}", u8::from(paused)).unwrap();
            let gain = if paused {
                0.0
            } else {
                percent as f32 * 0.01f32
            };
            expected.push(format!(
                "G {percent} {} {} {}",
                u8::from(paused),
                u8::from(!paused),
                gain.to_bits()
            ));
        }
    }
    assert_eq!(expected.len(), 1240);
    crate::app::higurashi::quiz_scene::tests::run_native(
        input,
        expected,
        "voice",
        "SHIN_HIGURASHI_NATIVE_QUIZ_SCENE_VOICE",
    );
    eprintln!(
        "Quiz native voice:1164 actual S99 request paths/flags,60 original parent BUP mouth/centered-pan callbacks,16 global voice-gain/pause cases;1240 rows with fresh own player and balanced disposal;no audio/GPU/window/saves"
    );
}

fn loaded(voice: &mut Voice) {
    let end = Instant::now() + Duration::from_secs(10);
    while voice.status() & 1 != 0 {
        assert!(Instant::now() < end, "Quiz voice did not finish loading");
        voice.advance(Ticks::ZERO);
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
#[ignore = "actual authored S99 quiz voices through scene-owned player on exact Loopback Audio and offscreen Metal;no window/saves"]
fn higurashi_quiz_voice_plays_authored_s99_requests_and_retains_own_transport() {
    let (_device, _queue, assets) = crate::adv::higurashi::reference_gpu_assets();
    let assets = Arc::new(assets);
    let scenario = assets.load_sync::<HigurashiScenario>("/main.snr").unwrap();
    let manager = Arc::new(AudioManager::new_for_tests());
    let mut first = Voice::new(manager.clone(), assets.clone(), &scenario);
    let mut second = Voice::new(manager.clone(), assets.clone(), &scenario);
    let reaction = catalog::REACTIONS[0][0];
    let path = request(reaction, true).0;
    let speakers = scenario
        .voice_mapping_info
        .iter()
        .find(|m| crate::audio::voice_pattern_matches(m.name_pattern.as_str(), &path))
        .unwrap()
        .lipsync_character_ids
        .0
        .clone();
    assert!(
        !speakers.is_empty(),
        "Quiz reaction must exercise authored speakers"
    );
    let character = i32::from(speakers[0]);
    first.set_volume(70);
    second.set_volume(100);
    first.player.set_paused(true);
    second.player.set_paused(true);
    first.play(catalog::POOLS[0][0].voice, false);
    assert_eq!(first.status(), 3);
    loaded(&mut first);
    assert_eq!(first.status(), 2);
    assert!(first.character_lipsync(character, 0.0).is_none());
    // A reaction replaces both the old voice and an unfinished request. A
    // disabled ADV character mask must not mute the quiz's bit0-only flags.
    first.play(catalog::POOLS[0][1].voice, false);
    first.player.set_preferences(70, 0, true);
    first.play(reaction, true);
    loaded(&mut first);
    assert_eq!(first.status(), 2);
    assert!(first.character_lipsync(character, -1.0).is_some());
    assert_eq!(
        first.character_lipsync(character, -1.0),
        first.character_lipsync(character, 1.0)
    );
    second.play("VT_QUIZ_SYS01", false);
    loaded(&mut second);
    assert_eq!(second.status(), 2);
    first.player.stop();
    assert_eq!(first.status(), 0);
    first.advance(Ticks::from_f32(10.0));
    assert_eq!(second.status(), 2, "Quiz player transport ownership leaked");
    first.play("VT_QUIZ_SYS02", false);
    loaded(&mut first);
    assert_eq!(first.status(), 2);
    first.set_volume(0);
    first.player.set_paused(false);
    let end = Instant::now() + Duration::from_secs(2);
    while first.player.position_micros() == 0 {
        assert!(Instant::now() < end, "Muted quiz voice clock did not run");
        first.advance(Ticks::from_f32(1.0));
        std::thread::sleep(Duration::from_millis(2));
    }
    drop(first);
    assert_eq!(
        second.status(),
        2,
        "Disposing one quiz player stopped another"
    );
    second.player.stop();
    second.advance(Ticks::from_f32(10.0));
    assert_eq!(second.status(), 0);
    eprintln!(
        "Quiz S99 transport:exact Loopback Audio;authored question/reaction/start/end requests,bit0 lipsync without character muting,pending replacement,independent player ownership,stop/drop and muted running clock pass;no window/saves"
    );
}
