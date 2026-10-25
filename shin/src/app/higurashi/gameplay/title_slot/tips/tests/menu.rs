use super::*;

pub(super) fn open_question(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frames: &mut Frames,
    slot: &mut TitleSlot,
) {
    slot.game
        .as_mut()
        .unwrap()
        .controls
        .requests
        .push_back(crate::adv::higurashi::input::Request::SystemMenu);
    frames.step(device, queue, slot, 0.0, 0);
    assert_eq!(
        slot.game
            .as_ref()
            .unwrap()
            .host()
            .unwrap()
            .modal_pause_depth(),
        1
    );
    for _ in 0..30 {
        frames.step(device, queue, slot, 3.0, 0);
    }
    let rows = slot
        .game
        .as_ref()
        .unwrap()
        .host()
        .unwrap()
        .reference_system_menu_rows()
        .unwrap()
        .1;
    let index = rows.iter().position(|(id, _)| *id == 6).unwrap();
    for _ in 0..index {
        frames.buttons(
            device,
            queue,
            slot,
            0.0,
            Buttons {
                repeat: 1 << 6,
                ..Default::default()
            },
        );
    }
    frames.step(device, queue, slot, 0.0, 1 << 24);
    let deadline = Instant::now() + Duration::from_secs(10);
    while slot
        .game
        .as_ref()
        .unwrap()
        .host()
        .unwrap()
        .reference_menu_tips_question()
        != Some((2, 1))
    {
        frames.step(device, queue, slot, 3.0, 0);
        assert!(Instant::now() < deadline, "TIPS question did not load/open");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        slot.game
            .as_mut()
            .unwrap()
            .host_mut()
            .unwrap()
            .take_menu_child_requests()
            .is_empty()
    );
}
pub(super) fn accept(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frames: &mut Frames,
    slot: &mut TitleSlot,
) {
    // Native22070 selected1 defaults to No. Up selects the first answer,Yes.
    frames.buttons(
        device,
        queue,
        slot,
        0.0,
        Buttons {
            repeat: 1 << 4,
            ..Default::default()
        },
    );
    assert_eq!(
        slot.game
            .as_ref()
            .unwrap()
            .host()
            .unwrap()
            .reference_menu_tips_question(),
        Some((2, 0))
    );
    frames.step(device, queue, slot, 0.0, 1 << 24);
    let deadline = Instant::now() + Duration::from_secs(10);
    while slot
        .game
        .as_ref()
        .unwrap()
        .host()
        .unwrap()
        .reference_menu_tips_question()
        .is_some_and(|(phase, _)| phase >= 0)
    {
        assert!(!slot.pending_tips());
        frames.step(device, queue, slot, 1.0, 0);
        assert!(Instant::now() < deadline, "TIPS Yes did not finish closing");
    }
}

#[test]
#[ignore = "real SystemMenu item6/No-default and A-No/B-cancel early reopening,stage1 Yes complete active1-to0 return without completion/EXIT,original owners and composed captures at720p/1080p;offscreen Metal/the default audio output,no window/saves"]
fn higurashi_system_menu_tips_returns_unfinished_secondary_without_marking_completion() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let (_, common) = crate::app::higurashi::gameplay::tests::character::common(
        &server,
        &[0x83, 1, 0x83, 0xe8, 0, 0, 0, 0x83, 0, 1],
    );
    let mut captures = 0;
    for size in [(1280, 720), (1920, 1080)] {
        for id in [1000, 11000] {
            let mut globals = load_globals(&server, common.clone());
            globals.data.save_vectors.extra_words_0 = vec![0x55555555; 20];
            let unlocks = globals.data.save_vectors.extra_words_0.clone();
            let full_manual = globals.runtime_saves.as_ref().unwrap().manual[2]
                .data
                .clone();
            let mut data = crate::app::higurashi::tips_menu::tests::source(id, 1);
            data.progress.entries[0].scenario_id = if id > 9999 { 10100 } else { 100 };
            for entry in &mut data.progress.entries {
                entry.save_position = 0;
                entry.replay_target = 0;
            }
            let mut game = None;
            reference_render_to_texture(&device, &queue, size, |context, _| {
                game = Some(
                    Gameplay::new(
                        globals,
                        data,
                        0,
                        context.new_render_texture("Menu TIPS source".into()),
                        || [1, 2, 3, 4],
                    )
                    .unwrap(),
                );
            });
            let mut slot = TitleSlot::from_game(game.unwrap());
            let mut frames = Frames {
                number: 0,
                size,
                composed: None,
            };
            frames.step(&device, &queue, &mut slot, 0.0, 0);
            frames.wait_game(&device, &queue, &mut slot);
            slot.game
                .as_mut()
                .unwrap()
                .bindings
                .history
                .record_completion(true, 73);
            let full = slot.game.as_ref().unwrap().runtime_game_data().unwrap();
            let history = slot
                .game
                .as_ref()
                .unwrap()
                .bindings
                .history
                .bits()
                .words()
                .as_ptr();
            let commands = slot
                .game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .runner
                .completed_commands();
            for cancel in [false, true] {
                open_question(&device, &queue, &mut frames, &mut slot);
                frames.step(
                    &device,
                    &queue,
                    &mut slot,
                    0.0,
                    1 << if cancel { 25 } else { 24 },
                );
                let deadline = Instant::now() + Duration::from_secs(10);
                while slot
                    .game
                    .as_ref()
                    .unwrap()
                    .host()
                    .unwrap()
                    .reference_menu_tips_question()
                    .unwrap()
                    .0
                    < 4
                {
                    frames.step(&device, &queue, &mut slot, 1.0, 0);
                    assert!(Instant::now() < deadline);
                }
                let host = slot.game.as_ref().unwrap().host().unwrap();
                assert!(
                    host.reference_system_menu_rows().unwrap().1[0].1 > 0.0,
                    "No/cancel reopens parent in stage0,while question still closes"
                );
                assert_eq!(host.modal_pause_depth(), 1);
                assert!(!host.runner.exit_closing());
                assert_eq!(host.runner.completed_commands(), commands);
                assert!(!slot.pending_tips());
                while slot
                    .game
                    .as_ref()
                    .unwrap()
                    .host()
                    .unwrap()
                    .reference_menu_tips_question()
                    .is_some()
                {
                    frames.step(&device, &queue, &mut slot, 3.0, 0);
                }
                assert_eq!(
                    slot.game.as_ref().unwrap().runtime_game_data().unwrap(),
                    full
                );
                frames.step(&device, &queue, &mut slot, 0.0, 1 << 25);
                while slot
                    .game
                    .as_ref()
                    .unwrap()
                    .host()
                    .unwrap()
                    .modal_pause_depth()
                    != 0
                {
                    frames.step(&device, &queue, &mut slot, 3.0, 0);
                }
                for _ in 0..4 {
                    frames.step(&device, &queue, &mut slot, 0.0, 0);
                }
            }
            open_question(&device, &queue, &mut frames, &mut slot);
            accept(&device, &queue, &mut frames, &mut slot);
            assert!(slot.pending_tips());
            let Some(Pending::Tips(request)) = &slot.pending else {
                unreachable!()
            };
            let mut expected = full.clone();
            expected.progress.set_active_index(0).unwrap();
            assert_eq!(request.data, Some(expected.clone()));
            let game = slot.game.as_ref().unwrap();
            assert_eq!(game.globals.data.save_vectors.extra_words_0, unlocks);
            assert!(!game.host().unwrap().runner.exit_closing());
            assert_eq!(game.host().unwrap().runner.completed_commands(), commands);
            let before = frames.composed.as_ref().unwrap();
            let before = captured(&device, &queue, size, before);
            slot.reference_scene_events.clear();
            frames.step(&device, &queue, &mut slot, 0.0, 0);
            assert_eq!(slot.reference_scene_events, [1, 2, 3]);
            let menu = slot.tips.as_ref().unwrap();
            assert_eq!(menu.motion.model.data, Some(expected));
            assert_eq!(
                captured(&device, &queue, size, menu.reference_capture().unwrap()),
                before
            );
            assert_eq!(menu.globals.data.save_vectors.extra_words_0, unlocks);
            assert_eq!(
                menu.globals.runtime_saves.as_ref().unwrap().manual[2].data,
                full_manual
            );
            assert_eq!(
                slot.continuation
                    .as_ref()
                    .unwrap()
                    .history
                    .bits()
                    .words()
                    .as_ptr(),
                history
            );
            captures += 1;
        }
    }
    println!(
        "Menu TIPS:4 real normal/alternate active1 owners at720p/1080p/{captures} exact composed captures;actual SystemMenu item6/question/default No/A-No/B-cancel stage0 reopening/stage1 Yes/full primary continuation,unmarked completion/original runtime owners and no EXIT/extra commands pass"
    );
}
