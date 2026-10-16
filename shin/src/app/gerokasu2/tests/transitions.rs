//! Actual source/destination publication, using the production composition,
//! audio, storage and transition methods rather than a separate test renderer.
use super::system_menu::{Driver, driver};
use super::*;
use crate::adv::gerokasu2::{message_clock::VoiceRequest, save_menu::Phase};
use shin_core::{
    format::save::gerokasu2::{GameData, SaveDate},
    vm::command::types::AudioWaitStatus,
};

fn await_condition(d: &mut Driver<'_>, label: &str, ready: impl Fn(&Gerokasu2App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready(&d.app) {
        d.settle(1);
        assert!(Instant::now() < deadline, "{label}");
    }
}
fn accept_question(d: &mut Driver<'_>) {
    d.press(AppAction::Enter);
    d.settle(25);
    d.press(AppAction::Left);
    d.press(AppAction::Enter);
}
fn load_active(app: &Gerokasu2App) -> bool {
    matches!(app.title_child.as_ref(), Some(title_child::TitleChild::Load(host)) if host.reference_phase() == Phase::Active)
}
fn start_voice(d: &mut Driver<'_>) {
    let rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let filename = rom
        .traverse()
        .find_map(|(path, _)| {
            (path.starts_with("/voice/") && path.ends_with(".nxa"))
                .then(|| path[7..path.len() - 4].to_owned())
        })
        .unwrap();
    let voice = d.app.host.as_mut().unwrap().scene.runner_mut().voice_mut();
    voice.set_paused(true);
    assert!(voice.play(VoiceRequest {
        filename,
        volume: 1.,
        lipsync: false,
        segment_start: 0,
        segment_duration: 0
    }));
    await_condition(d, "voice must load on the real output", |app| {
        !app.host
            .as_ref()
            .unwrap()
            .scene
            .runner()
            .voice()
            .wait_status()
            .contains(AudioWaitStatus::LOADING)
    });
    assert!(
        d.app
            .host
            .as_ref()
            .unwrap()
            .scene
            .runner()
            .voice()
            .wait_status()
            .contains(AudioWaitStatus::PLAYING)
    );
    d.app
        .host
        .as_mut()
        .unwrap()
        .scene
        .runner_mut()
        .voice_mut()
        .set_paused(false);
}
fn broken_title_assets(
    gpu: &Offscreen,
    folder: &std::path::Path,
    include_rom: bool,
) -> Arc<AssetServer> {
    std::fs::write(folder.join("title.txa"), b"injected invalid title archive").unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_dir(folder).unwrap();
    if include_rom {
        io.try_with_rom(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap())
            .unwrap();
    }
    Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: Game::Gerokasu2,
            wgpu_device: gpu.device.clone(),
            wgpu_queue: gpu.queue.clone(),
            bustup_cache: AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ))
}

#[test]
#[ignore = "actual saved Load and SystemMenu Title publication/failure recovery on Metal/exact Loopback Audio; isolated files"]
fn ger_app_load_and_title_destinations_preserve_source_until_success() {
    let gpu = Offscreen::new();
    let (temporary, path, mut d) = driver(&gpu, true);
    await_condition(&mut d, "story must expose SystemMenu with audio", |app| {
        app.host.as_ref().unwrap().scene.system_menu_allowed()
            && app
                .menu_bgm
                .lock()
                .shared_audio()
                .lock()
                .se_player
                .reference_source(0)
                .0
                .is_some()
    });
    d.settle(15);
    start_voice(&mut d);
    let source = d
        .app
        .host
        .as_ref()
        .unwrap()
        .scene
        .runner()
        .message_owner()
        .weak_owner();
    let saved = GameData {
        date: SaveDate::now(),
        progress: d
            .app
            .host
            .as_ref()
            .unwrap()
            .scene
            .save_session()
            .progress
            .clone(),
    };
    d.app.save_store.as_mut().unwrap().data.manual_save_slots[0] = Some(saved.clone());
    let assets = d.app.assets.clone();
    let shared = d.app.menu_bgm.lock().shared_audio();
    let original_volume = shared.lock().bgm_player.reference_settings_volume();
    let mut canceled_data = d.app.save_store.as_ref().unwrap().data.clone();
    canceled_data.settings.v0_bgmvol = 7;
    let canceled = shin_tasks::block_on(title::TitleHost::load(
        assets.clone(),
        &d.app.scenario,
        d.app.audio.clone(),
        &canceled_data,
        -1,
        d.app.menu_bgm.clone(),
    ))
    .unwrap();
    drop(canceled);
    assert_eq!(
        shared.lock().bgm_player.reference_settings_volume(),
        original_volume,
        "an unpublished Title factory must not change the retained source's gain"
    );
    d.open();
    d.settle(5);
    let voice_position = d
        .app
        .host
        .as_ref()
        .unwrap()
        .scene
        .runner()
        .voice()
        .timeline()
        .position;
    let bgm_id = shared.lock().bgm_player.reference_source().0;
    let effect = shared.lock().se_player.reference_source(0);
    d.press(AppAction::AnyDown); // Load
    d.press(AppAction::Enter);
    d.ready();
    assert!(load_active(&d.app));
    // The source and retained Load UI keep their original server. Destination
    // construction deliberately has no ROM layer and must fail asset loading.
    d.app.assets = broken_title_assets(&gpu, &temporary.0, false);
    accept_question(&mut d);
    await_condition(
        &mut d,
        "failed Load must reopen its retained child",
        load_active,
    );
    assert!(source() > 0);
    assert_eq!((d.app.modal.pause_depth, d.app.modal.conceal_depth), (1, 1));
    assert_eq!(
        d.app
            .host
            .as_ref()
            .unwrap()
            .scene
            .runner()
            .voice()
            .timeline()
            .position,
        voice_position
    );
    assert_eq!(shared.lock().se_player.reference_source(0), effect);
    assert_eq!(shared.lock().bgm_player.reference_source().0, bgm_id);
    assert_eq!(
        shared.lock().bgm_player.reference_settings_volume(),
        original_volume
    );
    assert!(shared.lock().se_player.reference_is_paused());
    d.draw();
    d.app.assets = assets.clone();
    d.app.save_store.as_mut().unwrap().data.manual_save_slots[0] = Some(saved.clone());
    let data = d.app.save_store.as_ref().unwrap().data.clone();
    d.app
        .title_child
        .as_mut()
        .unwrap()
        .manual_write_staged(&data);
    accept_question(&mut d);
    await_condition(&mut d, "valid Load must replace the source ADV", |app| {
        app.system.is_none() && app.title_child.is_none()
    });
    assert!(
        source() == 0,
        "accepted Load must release the outgoing Message/voice owner"
    );
    assert_eq!((d.app.modal.pause_depth, d.app.modal.conceal_depth), (0, 0));
    assert!(!shared.lock().se_player.reference_is_paused());
    assert_eq!(
        d.app
            .host
            .as_ref()
            .unwrap()
            .scene
            .save_session()
            .timeline
            .boundary,
        saved.progress.active_entry().save_position
    );
    await_condition(&mut d, "restored story effect must publish", |app| {
        app.menu_bgm
            .lock()
            .shared_audio()
            .lock()
            .se_player
            .reference_source(0)
            .0
            .is_some()
    });
    assert_eq!(shared.lock().se_player.reference_scene_effect_count(), 1);
    d.draw();
    d.settle(15);
    d.open();
    let loaded_source = d
        .app
        .host
        .as_ref()
        .unwrap()
        .scene
        .runner()
        .message_owner()
        .weak_owner();
    let loaded_effect = shared.lock().se_player.reference_source(0);
    d.app.assets = broken_title_assets(&gpu, &temporary.0, true);
    for _ in 0..4 {
        d.press(AppAction::AnyDown);
    }
    d.press(AppAction::Enter);
    d.settle(25);
    d.press(AppAction::Left);
    d.press(AppAction::Enter);
    await_condition(&mut d, "Title must create its write wait", |app| {
        app.write_wait.is_some()
    });
    assert!(d.app.system_title_exit && d.app.host.is_some());
    d.written();
    await_condition(&mut d, "failed Title must reopen SystemMenu", |app| {
        app.loading_title.is_none() && app.system.as_ref().is_some_and(|s| s.model.accepts_input())
    });
    assert!(!d.app.system_title_exit && d.app.error.is_none());
    assert!(loaded_source() > 0);
    assert_eq!((d.app.modal.pause_depth, d.app.modal.conceal_depth), (1, 1));
    assert_eq!(shared.lock().se_player.reference_source(0), loaded_effect);
    d.draw();
    d.app.assets = assets;
    // The same selected Title row can be retried after desktop preparation failure.
    d.press(AppAction::Enter);
    d.settle(25);
    d.press(AppAction::Left);
    d.press(AppAction::Enter);
    await_condition(&mut d, "Title retry must create its wait", |app| {
        app.write_wait.is_some()
    });
    d.written();
    await_condition(&mut d, "Title must become playable", |app| {
        app.title
            .as_ref()
            .is_some_and(|t| t.model.phase() == crate::adv::gerokasu2::title::TitlePhase::Active)
    });
    assert!(d.app.host.is_none() && d.app.system.is_none() && d.app.backdrop.is_none());
    assert!(loaded_source() == 0);
    assert_eq!(shared.lock().se_player.reference_scene_effect_count(), 0);
    assert_eq!((d.app.modal.pause_depth, d.app.modal.conceal_depth), (0, 0));
    assert!(saves::read(&path).unwrap().manual_save_slots[0].is_some());
    d.draw();
    println!(
        "GER_APP_DESTINATIONS failed_Load_source_voice_SE_BGM_retained_valid_Load_retires_and_restores_Title_failure_retry_snapshot_write_Title_ready=verified frames={} audio=Loopback Audio",
        d.frames.frames
    );
}

#[test]
#[ignore = "actual app script EXIT write/focus/capture/Title path on Metal/exact Loopback Audio; isolated progress"]
fn ger_app_script_exit_waits_for_write_and_publishes_title() {
    let gpu = Offscreen::new();
    let (_temporary, path, mut d) = driver(&gpu, false);
    d.settle(20);
    let source = d
        .app
        .host
        .as_ref()
        .unwrap()
        .scene
        .runner()
        .message_owner()
        .weak_owner();
    for _ in 0..12 {
        d.press(AppAction::Enter);
        d.settle(8);
        if d.app.waiting_for_exit_save {
            break;
        }
    }
    await_condition(&mut d, "script EXIT must enter native write wait", |app| {
        app.waiting_for_exit_save
    });
    assert!(d.app.host.is_some() && d.app.title.is_none() && d.app.loading_title.is_none());
    assert!(d.app.adv_frame.is_some() && source() > 0);
    let started = Instant::now();
    while d.app.write_wait.is_some() {
        d.press(AppAction::Enter);
        assert!(started.elapsed() < Duration::from_secs(15));
    }
    await_condition(
        &mut d,
        "script EXIT must publish its prepared Title",
        |app| {
            app.title.as_ref().is_some_and(|t| {
                t.model.phase() == crate::adv::gerokasu2::title::TitlePhase::Active
            })
        },
    );
    assert!(source() == 0);
    assert!(d.app.host.is_none() && d.app.adv_frame.is_none());
    assert!(saves::read(&path).unwrap().auto_save_slot.is_some());
    d.draw();
    println!(
        "GER_APP_SCRIPT_EXIT wait_write_final_callback_capture_publication_old_owner_drop_Title_ready=verified frames={} audio=Loopback Audio",
        d.frames.frames
    );
}
