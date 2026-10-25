use super::transitions::{Frames, assets};
use super::*;
use crate::adv::gerokasu2::{
    message_owner::{Gerokasu2MessageOwner, SharedMessageOwner},
    play_scene::{Gerokasu2PlayScene, SceneEvent},
    runner::FramePolicy,
};
use shin_core::primitives::update::FrameId;
use shin_render::shaders::types::RenderClone;

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
fn scenario() -> Arc<Gerokasu2Scenario> {
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut data = bytes();
    data.truncate(original.header.code_offset as usize);
    fn n(data: &mut Vec<u8>, v: i32) {
        let mut out = Cursor::new(Vec::new());
        NumberSpec::<i32>::constant(v).write_le(&mut out).unwrap();
        data.extend(out.into_inner());
    }
    fn op(data: &mut Vec<u8>, code: u8, args: &[i32]) {
        data.push(code);
        for &v in args {
            n(data, v);
        }
    }
    fn tile(data: &mut Vec<u8>, color: i32) {
        op(data, 0xc1, &[3, 1, 0]);
        data.push(31);
        for v in [color, -960, -540, 1920, 1080] {
            n(data, v);
        }
    }
    fn control(data: &mut Vec<u8>, p: i32, v: i32, time: i32) {
        op(data, 0xc3, &[-1, p]);
        data.push(3);
        n(data, v);
        n(data, time);
    }
    fn wait(data: &mut Vec<u8>, p: i32) {
        op(data, 0xc4, &[-1]);
        data.push(1);
        n(data, p);
    }
    tile(&mut data, 0xf00f);
    op(&mut data, 0x85, &[0]);
    data.extend([0x86, 1, 0, 0, 0]);
    n(&mut data, 0);
    let text = "@r共有ルート 😀";
    data.extend(((text.len() + 1) as u16).to_le_bytes());
    data.extend(text.as_bytes());
    data.push(0);
    data.extend([0x83, 0]);
    n(&mut data, 60);
    control(&mut data, 9, 0, 30);
    wait(&mut data, 9);
    control(&mut data, 9, 1000, 0);
    control(&mut data, 0, 120, 30);
    wait(&mut data, 0);
    data.push(0xcb);
    tile(&mut data, 0xf0f0);
    op(&mut data, 0x8e, &[0, 2, 30]);
    data.push(0);
    data.push(0x8f);
    op(&mut data, 0xc0, &[-1]);
    op(&mut data, 0x87, &[-1]);
    data.extend([0x8a, 1]);
    data.extend([0, 0]);
    n(&mut data, 0);
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    Arc::new(Gerokasu2Scenario::new(data.into()).unwrap())
}

#[test]
#[ignore = "requires supplied ROM,Metal and the default audio output"]
fn composed_root_scene_runs_utf8_message_root_controls_wipes_waits_and_exit() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let mut scene = shin_tasks::block_on(Gerokasu2PlayScene::load(
        assets,
        scenario(),
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
    let mut frames = Frames::new(&gpu);
    let mut frame = FrameId::default();
    let mut hidden = false;
    let mut moved = false;
    let mut saw_text = false;
    let mut saw_wipe = false;
    let mut installs = 0;
    let mut closed = false;
    let mut iterations = 0;
    let mut fast_skip = false;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        frame.advance();
        let skip_now = !fast_skip
            && scene
                .backend()
                .root
                .properties()
                .get_value(LayerProperty::TranslateX)
                == 60.0;
        fast_skip |= skip_now;
        let status = scene
            .step(
                frame,
                Ticks::from_f32(15.0),
                FramePolicy {
                    fast_skip: skip_now,
                    ..policy()
                },
                context(),
            )
            .unwrap();
        iterations += 1;
        assert_ne!(
            status,
            RunnerStatus::External,
            "unowned command {:?}",
            scene.runner().pending_command()
        );
        let pixels = frames.draw_scene(&gpu, scene.backend_mut());
        let root_alpha = scene
            .backend()
            .root
            .properties()
            .get_value(LayerProperty::MulColorAlpha);
        let root_x = scene
            .backend()
            .root
            .properties()
            .get_value(LayerProperty::TranslateX);
        if root_alpha == 0.0 {
            assert!(scene.runner().message().message().unwrap().clock().open);
            assert!(
                pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]),
                "root opacity must include message and screen"
            );
            hidden = true;
        }
        if root_x == 120.0 && root_alpha == 1000.0 && !moved {
            for x in 0..320 {
                let at = (40 * 320 + x) * 4;
                assert_eq!(
                    &pixels[at..at + 4],
                    if x < 20 {
                        &[0, 0, 0, 255]
                    } else {
                        &[0, 0, 255, 255]
                    }
                );
            }
            assert!(
                pixels[(135 * 320) * 4..]
                    .chunks_exact(4)
                    .any(|p| p != [0, 0, 0, 255] && p != [0, 0, 255, 255]),
                "message must be rendered in root"
            );
            moved = true;
        }
        if scene
            .runner()
            .message()
            .message()
            .is_some_and(|m| m.clock().open)
            && root_alpha == 1000.0
        {
            saw_text |= pixels[(135 * 320) * 4..]
                .chunks_exact(4)
                .any(|p| p != [0, 0, 255, 255] && p != [0, 0, 0, 255]);
        }
        if matches!(
            scene.runner().pending_command(),
            Some(Gerokasu2RuntimeCommand::Shared(
                shin_core::vm::command::RuntimeCommand::WIPEWAIT(_)
            ))
        ) {
            saw_wipe = true;
        }
        if matches!(
            scene.runner().pending_command(),
            Some(Gerokasu2RuntimeCommand::Shared(
                shin_core::vm::command::RuntimeCommand::MSGWAIT(_)
            ))
        ) {
            scene.click_message(false).unwrap();
        }
        for event in scene.take_events() {
            if matches!(event, SceneEvent::MessageInstalled(0, _)) {
                installs += 1;
            }
        }
        if status == RunnerStatus::Exited {
            assert!(pixels.chunks_exact(4).all(|p| p == [0, 255, 0, 255]));
            closed = true;
            break;
        }
        assert!(Instant::now() < deadline);
        assert!(iterations < 100);
    }
    assert!(hidden && moved && saw_text && saw_wipe && closed && fast_skip);
    assert_eq!(installs, 1);
    assert_eq!(scene.runner().completed_commands(), 17);
    let message = scene.runner().message_owner();
    // Native10AA0 releases the retained root at an enabled blocking poll;
    // EXIT's pending close has crossed that boundary before terminal status.
    assert!(!scene.backend().has_previous_root());
    assert!(
        scene
            .backend()
            .root()
            .message_owner()
            .unwrap()
            .same_owner(&message)
    );
    println!(
        "GEROKASU2_ROOT_SCENE commands=17 installs=1 root_opacity_translation_message_screen_history_shared_fast_skip_close_wipe_wait_EXIT=verified frames={} channels={} audio=default output",
        frames.frames, frames.channels
    );
}

#[test]
#[ignore = "requires supplied ROM,Metal and the default audio output"]
fn root_copies_keep_message_properties_graphics_frame_guard_and_lifetime() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let scenario = scenario();
    let owner = SharedMessageOwner::new(
        shin_tasks::block_on(Gerokasu2MessageOwner::load(&assets, 0, false, -1)).unwrap(),
    );
    let weak = owner.weak_owner();
    let mut root = crate::adv::gerokasu2::root::Gerokasu2Root::new();
    root.attach_message(owner.clone()).unwrap();
    root.properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX)
        .enqueue(100.0, shin_core::time::Tween::linear(Ticks::from_f32(10.0)));
    let mut frame = FrameId::default();
    frame.advance();
    let ctx = crate::update::AdvUpdateContext {
        frame_id: frame,
        delta_ticks: Ticks::from_f32(5.0),
        asset_server: &assets,
        device: &gpu.device,
        queue: &gpu.queue,
        are_animations_allowed: true,
        voice_callback: None,
        movie_volume_settings: Default::default(),
    };
    root.prepare_message_frame(&ctx);
    root.update(&ctx);
    assert_eq!(root.properties().get_value(LayerProperty::TranslateX), 50.0);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let mut clone = shin_render::shaders::types::RenderCloneCtx {
        device: &gpu.device,
        encoder: &mut encoder,
    };
    let mut copy = root.render_clone(&mut clone);
    let mut nested = copy.render_clone(&mut clone);
    gpu.queue.submit([encoder.finish()]);
    assert_eq!(
        root.reference_message_ownership(),
        copy.reference_message_ownership()
    );
    assert_eq!(
        copy.reference_message_ownership(),
        nested.reference_message_ownership()
    );
    assert!(
        root.message_owner()
            .unwrap()
            .same_owner(&copy.message_owner().unwrap())
    );
    assert_ne!(root.properties().identity(), copy.properties().identity());
    copy.properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX)
        .fast_forward_to(-120.0);
    copy.prepare_message_frame(&ctx);
    copy.update(&ctx);
    nested.prepare_message_frame(&ctx);
    nested.update(&ctx);
    assert_eq!(root.properties().get_value(LayerProperty::TranslateX), 50.0);
    assert_eq!(
        nested.properties().get_value(LayerProperty::TranslateX),
        50.0
    );
    owner.lock().ui_settings.window_opacity = 33;
    assert_eq!(
        copy.message_owner()
            .unwrap()
            .lock()
            .ui_settings
            .window_opacity,
        33
    );
    let other = SharedMessageOwner::new(
        shin_tasks::block_on(Gerokasu2MessageOwner::load(&assets, 0, false, -1)).unwrap(),
    );
    assert!(copy.attach_message(other).is_err());
    drop(owner);
    assert_eq!(weak(), 3);
    drop(root);
    assert_eq!(weak(), 2);
    drop(copy);
    assert_eq!(weak(), 1);
    drop(nested);
    assert_eq!(weak(), 0);
    drop(scenario);
    println!(
        "GEROKASU2_ROOT_COPY message_properties_graphics_shared_parent_properties_independent_frame_once_wrong_owner_rejected_lifetime=verified audio=default output"
    );
}
