use super::transitions::{Frames, assets};
use super::*;
use crate::adv::gerokasu2::{play_scene::Gerokasu2PlayScene, runner::FramePolicy};
use shin_core::primitives::update::FrameId;
fn context() -> MessageCommandContext {
    MessageCommandContext {
        was_read: false,
        event_mode: -1,
        skip: false,
        message_speed: 50,
    }
}
fn policy() -> FramePolicy {
    FramePolicy {
        paused: false,
        scene_changing: false,
        animations_allowed: true,
        autoplay: false,
        fast_skip: false,
    }
}
fn num(data: &mut Vec<u8>, v: i32) {
    let mut out = Cursor::new(Vec::new());
    NumberSpec::<i32>::constant(v).write_le(&mut out).unwrap();
    data.extend(out.into_inner());
}
#[test]
#[ignore = "requires supplied ROM,Metal and the default audio output"]
fn ger_gallery_scene_owns_trophy_unlock_all_sets_and_exit() {
    let gpu = Offscreen::new();
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut data = bytes();
    data.truncate(original.header.code_offset as usize);
    for id in [1, -1, 65535] {
        data.push(0xb0);
        num(&mut data, id);
    }
    for kind in [0, 1, 2, 255] {
        data.extend([0xb1, kind, 3]);
        for value in [31, 8192, 65535] {
            num(&mut data, value);
        }
    }
    data.extend([0, 0]);
    num(&mut data, 0);
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    let mut scene = shin_tasks::block_on(Gerokasu2PlayScene::load(
        assets(&gpu),
        Arc::new(Gerokasu2Scenario::new(data.into()).unwrap()),
        Arc::new(AudioManager::new_for_tests()),
        0,
        42,
        InitialMessage {
            style: 0,
            asynchronous: false,
            face: -1,
        },
        Default::default(),
        context(),
    ))
    .unwrap();
    let mut loaded = crate::adv::gerokasu2::gallery::GalleryUnlocks::default();
    loaded.cgs.add(7);
    scene.replace_gallery(loaded);
    let mut frame = FrameId::default();
    frame.advance();
    assert_eq!(
        scene
            .step(frame, Ticks::from_f32(1.0), policy(), context())
            .unwrap(),
        RunnerStatus::Exited
    );
    assert_eq!(scene.runner().completed_commands(), 8);
    for set in [
        &scene.gallery().cgs,
        &scene.gallery().bgms,
        &scene.gallery().movies,
    ] {
        for id in [31, 8192, 65535] {
            assert!(set.contains(id));
        }
        assert_eq!(set.size_in_words(), 2048);
    }
    assert!(scene.gallery().cgs.contains(7));
    assert!(!scene.gallery().bgms.contains(7));
    assert!(!scene.save_session().autosave.valid);
    assert_eq!(scene.save_session().timeline.boundary, 0);
    let mut frames = Frames::new(&gpu);
    assert!(
        frames
            .draw_scene(&gpu, scene.backend_mut())
            .chunks_exact(4)
            .all(|p| p == [0, 0, 0, 255])
    );
    println!(
        "GEROKASU2_GALLERY_SCENE commands=8 loaded_global_gallery_trophy_no_progress_effects_all3_unlock_sets_EXIT=verified frames={} channels={} audio=default output",
        frames.frames, frames.channels
    );
}
