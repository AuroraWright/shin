//! Real UTF-8 BUP uploads and retained message portrait owners on Metal.
use super::*;
use crate::{
    asset::{
        font::gerokasu2::draw::gpu_tests::Offscreen,
        system::{AssetLoadContext, locate_assets_for_game},
    },
    cli::Game,
};
use shin_audio::AudioManager;
use std::time::{Duration, Instant};

fn loaded(portrait: &mut Gerokasu2MessagePortrait) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !portrait.loads.is_empty() {
        assert!(Instant::now() < deadline, "portrait loader deadline");
        portrait.poll_loads();
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn assets(gpu: &Offscreen) -> Arc<AssetServer> {
    let path = std::path::PathBuf::from(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap());
    Arc::new(AssetServer::new(
        locate_assets_for_game(Game::Gerokasu2, path.parent().unwrap())
            .unwrap()
            .into(),
        AssetLoadContext {
            game: crate::cli::Game::Gerokasu2,
            wgpu_device: gpu.device.clone(),
            wgpu_queue: gpu.queue.clone(),
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ))
}

#[test]
#[ignore = "requires supplied ROM, Metal and the default audio output"]
fn utf8_gpu_portrait_publication_replacement_clear_error_and_frames() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let scenario = scenario();
    let mut voice = Gerokasu2VoicePlayer::new(
        Arc::new(AudioManager::new_for_tests()),
        assets.clone(),
        &scenario,
    );
    let mut portrait = Gerokasu2MessagePortrait::default();
    let mut frame = FrameId::default();
    let request = |index| {
        let PortraitChange::Load(request) =
            MessageFaceState::new(index).on_message_installed(&scenario.bustup_info)
        else {
            panic!()
        };
        request
    };
    let first = request(0);
    let second = request(1);
    assert!(!first.expression.is_ascii());
    let job = portrait
        .holder
        .apply(PortraitChange::Load(first.clone()))
        .unwrap();
    assert!(shin_tasks::block_on(acquire(job, assets.clone())).unwrap());
    assert_eq!(portrait.published_request(), Some(first.clone()));
    assert!(portrait.active_request().is_none());
    frame.advance();
    portrait.update(frame, Ticks::from_f32(0.0), &voice);
    assert_eq!(portrait.active_request(), Some(first.clone()));
    let owner = portrait.active.as_ref().unwrap().clone();
    let resource = Arc::downgrade(&owner.lock()._asset.0);

    let old = portrait
        .holder
        .apply(PortraitChange::Load(second.clone()))
        .unwrap();
    let current = portrait
        .holder
        .apply(PortraitChange::Load(first.clone()))
        .unwrap();
    assert!(!shin_tasks::block_on(acquire(old, assets.clone())).unwrap());
    assert!(shin_tasks::block_on(acquire(current, assets.clone())).unwrap());
    assert!(resource.upgrade().is_some());
    let pending = portrait
        .holder
        .apply(PortraitChange::Load(second.clone()))
        .unwrap();
    portrait.apply(assets.clone(), PortraitChange::Clear);
    assert!(!shin_tasks::block_on(acquire(pending, assets.clone())).unwrap());
    assert!(portrait.published_request().is_none());
    assert_eq!(portrait.active_request(), Some(first.clone()));
    // Publication and render snapshots differ until a new frame is updated.
    portrait.update(frame, Ticks::from_f32(10.0), &voice);
    assert!(portrait.active_request().is_some());
    frame.advance();
    portrait.update(frame, Ticks::from_f32(0.0), &voice);
    assert!(portrait.active_request().is_none());
    drop(owner);
    assert!(resource.upgrade().is_none());

    portrait.apply(assets.clone(), PortraitChange::Load(first.clone()));
    loaded(&mut portrait);
    assert!(portrait.last_error().is_none());
    frame.advance();
    portrait.update(frame, Ticks::from_f32(1.0), &voice);
    let mut missing = first.clone();
    missing.path = "/bustup/M/absent.bup".into();
    portrait.apply(assets.clone(), PortraitChange::Load(missing));
    loaded(&mut portrait);
    assert!(portrait.last_error().is_some());
    assert_eq!(portrait.published_request(), Some(first.clone()));

    // Two UTF-8 expressions retain shared base blocks but separate facial owners.
    let a: Arc<Gerokasu2Bustup> = shin_tasks::block_on(assets.load_with_args(
        &first.path,
        BustupArgs {
            expression: first.expression.clone(),
            character_id: CharacterId::new(first.lipsync_character_id),
            disable_animations: false,
        },
    ))
    .unwrap();
    let b: Arc<Gerokasu2Bustup> = shin_tasks::block_on(assets.load_with_args(
        &second.path,
        BustupArgs {
            expression: second.expression.clone(),
            character_id: CharacterId::new(second.lipsync_character_id),
            disable_animations: false,
        },
    ))
    .unwrap();
    assert!(!a.0.base_blocks.is_empty());
    assert!(a.0.face1.is_some() || a.0.face2.is_some() || !a.0.mouth_blocks.is_empty());
    if first.path == second.path {
        for (left, right) in a.0.base_blocks.iter().zip(&b.0.base_blocks) {
            assert!(Arc::ptr_eq(left, right));
        }
    }
    let maps = crate::adv::gerokasu2::voice::mappings(&scenario);
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let (name, speakers) = rom
        .traverse()
        .filter_map(|(path, entry)| {
            if !matches!(entry, shin_core::format::rom::IndexEntry::File(_))
                || !path.starts_with("/voice/")
                || !path.ends_with(".nxa")
            {
                return None;
            }
            let name = &path[7..path.len() - 4];
            let speakers = crate::audio::voice_speakers(
                &maps,
                name,
                crate::audio::VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC,
            );
            (!speakers.is_empty()).then(|| (name.to_owned(), speakers))
        })
        .next()
        .unwrap();
    let mut voiced = first.clone();
    voiced.lipsync_character_id = i32::from(speakers[0]);
    portrait.apply(assets.clone(), PortraitChange::Load(voiced.clone()));
    loaded(&mut portrait);
    voice.set_preferences(100, u32::MAX, true);
    voice.set_paused(true);
    assert!(
        voice.play(crate::adv::gerokasu2::message_clock::VoiceRequest {
            filename: name,
            volume: 1.0,
            lipsync: true,
            segment_start: 10_000,
            segment_duration: 120_000
        })
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while voice
        .wait_status()
        .contains(shin_core::vm::command::types::AudioWaitStatus::LOADING)
    {
        assert!(Instant::now() < deadline);
        voice.update(Ticks::ZERO);
        std::thread::sleep(Duration::from_millis(2));
    }
    let level = voice
        .character_lipsync(voiced.lipsync_character_id, -0.5)
        .expect("mapped portrait voice feedback");
    frame.advance();
    portrait.update(frame, Ticks::from_f32(1.0), &voice);
    let active = portrait.active.as_ref().unwrap().lock();
    let count = active._asset.0.mouth_blocks.len();
    let expected = if count == 0 {
        0
    } else {
        (usize::from(level) * count / 3).min(count - 1) as u32
    };
    assert_eq!(active.layer.portrait_animation_state().0, expected);
    drop(active);
    voice.stop();
    frame.advance();
    portrait.update(frame, Ticks::from_f32(1.0), &voice);
    assert_eq!(
        portrait
            .active
            .as_ref()
            .unwrap()
            .lock()
            .layer
            .portrait_animation_state()
            .0,
        0
    );
    let parent = Mat4::from_cols_array(&[
        1.0 / 960.0,
        0.0,
        0.0,
        0.0,
        0.0,
        -1.0 / 540.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]);
    let render = |kind, hidden, mode| {
        let mut clock = MessageClock::new(
            &[],
            &[],
            glam::Vec2::ZERO,
            crate::adv::gerokasu2::message_clock::MessageParams {
                flags: 0,
                mode,
                message_id: 0,
                message_arg: 0,
                animate_open: false,
            },
        )
        .unwrap();
        clock.slide = 1.0;
        clock.modal_slide = if hidden { 0.5 } else { 1.0 };
        crate::adv::gerokasu2::message_ui::reference_frame(
            &gpu.device,
            &gpu.queue,
            (640, 360),
            false,
            |pass| portrait.render(pass, &clock, parent, 0, kind),
        )
    };
    let empty = render(PassKind::Opaque, true, 0);
    let opaque = render(PassKind::Opaque, false, 0);
    let transparent = render(PassKind::Transparent, false, 0);
    assert_ne!(opaque, empty);
    assert_ne!(transparent, empty);
    assert_eq!(render(PassKind::Transparent, false, 2), empty);
    assert_eq!(render(PassKind::Opaque, true, 0), empty);
    let orphan = MessagePortrait::<SharedLayer>::new();
    let job = orphan.apply(PortraitChange::Load(first.clone())).unwrap();
    drop(orphan);
    assert!(!shin_tasks::block_on(acquire(job, assets.clone())).unwrap());
    println!(
        "GEROKASU2_PORTRAIT_GPU utf8=2 frames=5 channels=4608000 authored_voice=1 publication_replacement_clear_error_shared_blocks_voice_feedback_native_pass_gates=verified audio=default output"
    );
}
