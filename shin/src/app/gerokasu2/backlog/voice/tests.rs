use super::*;
use crate::app::gerokasu2::tests::{assets, bytes};
use crate::asset::font::gerokasu2::draw::gpu_tests::Offscreen;
use shin_core::{
    format::{
        rom::{IndexEntry, RomReader},
        save::Settings,
    },
    primitives::stateless_reader::StatelessFile,
};
use std::time::{Duration, Instant};

#[test]
#[ignore = "actual ROM browser-private serial voice on exact Loopback Audio; no game/save writes"]
fn ger_backlog_private_voice_uses_idle_gaps_ignores_character_mute_and_preserves_adv() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let scenario = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let name = rom
        .traverse()
        .find_map(|(path, entry)| {
            (matches!(entry, IndexEntry::File(_))
                && path.starts_with("/voice/")
                && path.ends_with(".nxa"))
            .then(|| path[7..path.len() - 4].to_owned())
        })
        .unwrap();
    let audio = Arc::new(AudioManager::new_for_tests());
    let mut story = Gerokasu2VoicePlayer::new(audio.clone(), assets.clone(), &scenario);
    story.set_preferences(100, u32::MAX, true);
    story.set_paused(true);
    assert!(story.play_backlog(&name, 0.8));
    let deadline = Instant::now() + Duration::from_secs(10);
    while story.wait_status().contains(AudioWaitStatus::LOADING) {
        story.update(Ticks::ZERO);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(story.wait_status().contains(AudioWaitStatus::PLAYING));
    let frozen = story.timeline().position;
    let settings = Settings {
        v2_voicevol: 100,
        v19: 0,
        ..Default::default()
    };
    let mut replay = Replay::new(audio, assets, &scenario, &settings);
    replay.start(vec![Voice {
        path: name.clone(),
        volume: 0.37,
    }]);
    for _ in 0..9 {
        replay.update();
        assert!(!replay.playing());
    }
    replay.update();
    assert!(
        replay.playing(),
        "native tenth idle tick must request the source"
    );
    while replay.player.timeline().position == 0 {
        replay.update();
        assert_eq!(story.timeline().position, frozen);
        assert!(story.wait_status().contains(AudioWaitStatus::PLAYING));
        assert!(
            Instant::now() < deadline,
            "browser flags0 must bypass the all-muted character mask"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let replay_position = replay.player.timeline().position;
    replay.stop();
    assert!(!replay.playing());
    for _ in 0..6 {
        replay.update();
    }
    drop(replay);
    assert_eq!(story.timeline().position, frozen);
    story.set_paused(false);
    while story.timeline().position == frozen {
        story.update(Ticks::from_f32(1.));
        assert!(
            Instant::now() < deadline,
            "retained ADV voice did not resume"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    story.stop();
    for _ in 0..6 {
        story.update(Ticks::from_f32(1.));
    }
    println!(
        "GER_BACKLOG_PRIVATE_VOICE name={name} replay_micros={replay_position} idle10_flags0_private_owner_frozen_ADV_resume_stop=verified audio=Loopback Audio"
    );
}
