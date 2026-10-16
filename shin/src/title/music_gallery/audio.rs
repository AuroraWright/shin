//!33800/33c30 gallery dispatch uses the retained global music player. Requests
//! are accepted immediately; async loading, matching-ID reuse and stop tails
//! remain owned by BgmPlayer across scene retirement.
use super::{MusicGallery, MusicRequest};
use crate::{
    adv::GlobalData,
    asset::system::AssetServer,
    audio::{BgmPlayer, SystemSoundPlayer},
};
use shin_core::{format::scenario::Scenario, time::Tween, vm::command::types::Volume};
use std::sync::Arc;

pub(crate) fn consume_requests(
    gallery: &mut MusicGallery,
    bgm: &mut BgmPlayer,
    system: &mut SystemSoundPlayer,
    server: &Arc<AssetServer>,
    scenario: &Scenario,
    globals: &mut GlobalData,
) -> anyhow::Result<()> {
    for request in gallery.take_music_requests() {
        match request {
            MusicRequest::Play { bgm: id, once_flag } => {
                let info = scenario
                    .info_tables()
                    .bgm_info
                    .get(id.into_i32() as usize)
                    .ok_or_else(|| {
                        anyhow::anyhow!("MusicMode BGM{} has no metadata", id.into_i32())
                    })?;
                if bgm.request_with_loop_count(
                    server.clone(),
                    info.path(),
                    id,
                    u32::from(once_flag),
                    Volume::default(),
                    Tween::IMMEDIATE,
                ) {
                    globals.unlock_bgm(scenario.info_tables(), id.into_i32());
                }
            }
            MusicRequest::Stop { ticks } => {
                bgm.stop(Tween::linear(shin_core::time::Ticks::from_u32(ticks)))
            }
        }
    }
    for cue in gallery.take_sounds() {
        system.play(server, cue, Volume::default());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn reference_gallery_audio_dispatch(server: Arc<AssetServer>, scenario: &Scenario) {
    use shin_core::{format::save::SaveData, time::Ticks, vm::command::types::AudioWaitStatus};
    let items = &scenario.info_tables().music_box_info;
    assert!(items.iter().all(|item| item.once_flag <= 1));
    let looping = items
        .iter()
        .position(|item| item.once_flag == 0)
        .expect("No looping original music entry");
    let once = items
        .iter()
        .position(|item| item.once_flag == 1 && item.bgm_id != items[looping].bgm_id)
        .expect("No one-shot original music entry");
    let locked = items
        .iter()
        .position(|item| item.bgm_id != items[looping].bgm_id && item.bgm_id != items[once].bgm_id)
        .unwrap();
    let mut save = SaveData::default();
    for i in [looping, once] {
        save.save_vectors
            .unlocked_bgms
            .add(items[i].bgm_id.into_i32() as usize);
    }
    let mut globals = GlobalData::open(None, Some(save)).unwrap();
    let mut gallery = MusicGallery::new(items, globals.save()).unwrap();
    gallery.publish_resources();
    gallery.update(Ticks::from_f32(60.0), false);
    gallery.update(Ticks::ZERO, false);
    gallery.take_requests();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut bgm = BgmPlayer::new(manager.clone());
    let mut system = SystemSoundPlayer::new(manager);
    bgm.set_settings_volume(37);
    system.set_settings_volume(25);
    gallery.release_row(looping);
    consume_requests(
        &mut gallery,
        &mut bgm,
        &mut system,
        &server,
        scenario,
        &mut globals,
    )
    .unwrap();
    assert_eq!(bgm.reference_source().0, Some(items[looping].bgm_id));
    assert_eq!(bgm.reference_loading_repeat(), Some(true));
    assert!(
        bgm.get_wait_status()
            .contains(AudioWaitStatus::LOADING | AudioWaitStatus::PLAYING)
    );
    assert!(
        globals
            .save()
            .save_vectors
            .unlocked_bgms
            .contains(items[looping].bgm_id.into_i32() as usize)
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while bgm.get_wait_status().contains(AudioWaitStatus::LOADING) {
        bgm.update();
        assert!(
            std::time::Instant::now() < deadline,
            "Music gallery BGM load did not complete"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    bgm.reference_pause_source(true);
    let source = bgm.reference_source();
    let revision = globals.revision_for_reference();
    gallery.update(Ticks::from_f32(4.0), true);
    gallery.play();
    consume_requests(
        &mut gallery,
        &mut bgm,
        &mut system,
        &server,
        scenario,
        &mut globals,
    )
    .unwrap();
    assert_eq!(
        bgm.reference_source(),
        source,
        "Matching gallery request replaced its global source"
    );
    assert_eq!(globals.revision_for_reference(), revision);
    gallery.release_row(once);
    consume_requests(
        &mut gallery,
        &mut bgm,
        &mut system,
        &server,
        scenario,
        &mut globals,
    )
    .unwrap();
    assert_eq!(bgm.reference_source().0, Some(items[once].bgm_id));
    assert_eq!(bgm.reference_loading_repeat(), Some(false));
    gallery.release_row(once);
    consume_requests(
        &mut gallery,
        &mut bgm,
        &mut system,
        &server,
        scenario,
        &mut globals,
    )
    .unwrap();
    assert!(bgm.get_wait_status().is_empty());
    assert_eq!(bgm.reference_source().0, None);
    gallery.release_row(locked);
    consume_requests(
        &mut gallery,
        &mut bgm,
        &mut system,
        &server,
        scenario,
        &mut globals,
    )
    .unwrap();
    assert_eq!(
        system.sounds.len(),
        1,
        "Locked gallery item must play its error cue"
    );
    assert!(
        !globals
            .save()
            .save_vectors
            .unlocked_bgms
            .contains(items[locked].bgm_id.into_i32() as usize)
    );
    assert!(bgm.get_wait_status().is_empty());
    // Supplied rows use 0/1, but native metadata preserves the full u16 count.
    // Exercise the actual gallery dispatch/load owner with larger counts too.
    for count in [2, 3, 17, u16::MAX] {
        let mut rows: Vec<_> = items
            .iter()
            .map(|item| shin_core::format::scenario::info::MusicBoxInfoItem {
                bgm_id: item.bgm_id,
                name_index: item.name_index,
                once_flag: item.once_flag,
            })
            .collect();
        rows[looping].once_flag = count;
        let mut counted = MusicGallery::new(&rows, globals.save()).unwrap();
        counted.publish_resources();
        counted.update(Ticks::from_f32(60.0), false);
        counted.update(Ticks::ZERO, false);
        counted.take_requests();
        counted.release_row(looping);
        consume_requests(
            &mut counted,
            &mut bgm,
            &mut system,
            &server,
            scenario,
            &mut globals,
        )
        .unwrap();
        assert_eq!(bgm.reference_loading_loop_count(), Some(u32::from(count)));
        assert!(!bgm.request_with_loop_count(
            server.clone(),
            "/missing-same-id-repeat.nxa".into(),
            items[looping].bgm_id,
            1,
            Volume(0.5),
            Tween::IMMEDIATE,
        ));
        assert_eq!(bgm.reference_loading_loop_count(), Some(u32::from(count)));
        bgm.stop(Tween::IMMEDIATE);
        assert!(bgm.get_wait_status().is_empty());
    }
    println!(
        "Music gallery audio: original async BGM, reuse, unlock, one-shot replacement, pointer stop and locked error pass"
    );
    println!(
        "Music gallery numeric counts2/3/17/65535: real dispatch/pending owner/same-ID retention/stop pass"
    );
}
