use super::*;
use crate::adv::higurashi::{autosave::Execution, input::Request};
fn tick(slot: &mut TitleSlot, number: &mut u32, pressed: u32, repeat: u32) {
    *number += 1;
    shin_tasks::block_on(slot.update_game(
        Ticks::from_f32(3.0),
        true,
        Buttons {
            pressed,
            repeat,
            ..Default::default()
        },
        &PointerFrame {
            number: *number,
            time_seconds: f64::from(*number) / 60.0,
            events: &[],
        },
        policy(),
        audio(),
    ))
    .unwrap();
}
fn until(slot: &mut TitleSlot, number: &mut u32, ready: impl Fn(&Gameplay) -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !ready(slot.game.as_ref().unwrap()) {
        tick(slot, number, 0, 0);
        assert!(Instant::now() < end, "Live ADV manual menu timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
#[ignore = "real Gameplay/SystemMenu/SaveMenu factories,Back0/1 source resumption,paused VM/pending commit isolation and Load final full flags0,next-frame dispatch at720p/1080p on offscreen Metal/the default audio output;no window/user writes"]
fn higurashi_gameplay_manual_children_preserve_source_until_back_or_load() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let (authored, common) = crate::app::higurashi::gameplay::tests::character::common(
        &server,
        &[
            0xa0, 0, 2, b'A', 0, 0xa0, 1, 2, b'B', 0, 0xa1, 0x83, 0, 0x83, 0xe8, 0, 0, 0,
        ],
    );
    let mut profiles = 0;
    for size in [(1280, 720), (1920, 1080)] {
        for save in [true, false] {
            let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
            let mut saved = SaveData::default();
            saved.persist_data.0 = vec![31; 32];
            saved.save_menu_position = 2;
            saved.manual_save_slots[2] = Some(fresh());
            let mut slots = Slots::loaded(&saved);
            slots.manual[2].data.progress.extra_word = 0xfedcba98;
            slots.manual[2].data.trailing_values = vec![0xffff, 0x1234];
            slots.manual[2].data.byte_variables = vec![255, 7];
            slots.manual[2].data.progress.entries[1] =
                crate::adv::higurashi::autosave::ProgressEntry {
                    scenario_id: 2084,
                    random_seed: 0xabcdef12,
                    save_position: 23,
                    replay_target: 29,
                    response_data: vec![0x23, 0xee],
                };
            let loaded = slots.manual[2].data.clone();
            let globals = Globals {
                common: common.clone(),
                data: saved,
                storage: None,
                runtime_saves: Some(slots),
                clock: PlayClock::from_saved_seconds(17),
                platform_flag: true,
                last_title_mode: 7,
                audio: HigurashiAudio::new(manager.clone(), server.clone(), authored.clone()),
                server: server.clone(),
                manager: manager.clone(),
                sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager))),
            };
            let mut game = None;
            reference_render_to_texture(&device, &queue, size, |context, _| {
                let mut initial = context.new_render_texture("manual child source".into());
                {
                    let mut pass = context.begin_pass(
                        initial.as_texture_target(),
                        Some(context.depth_stencil),
                        "manual child source",
                    );
                    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                }
                game = Some(
                    Gameplay::new(globals, RuntimeGameData::from(fresh()), 0, initial, || {
                        [1, 2, 3, 4]
                    })
                    .unwrap(),
                );
            });
            let mut slot = TitleSlot::from_game(game.unwrap());
            let mut number = 0;
            until(&mut slot, &mut number, |g| {
                g.input_permission
                    && !g.host().unwrap().has_modes()
                    && matches!(
                        g.host().unwrap().runner.pending_command(),
                        Some(
                            shin_core::vm::higurashi::command::HigurashiRuntimeCommand::WAIT {
                                amount: 1000,
                                ..
                            }
                        )
                    )
            });
            let game = slot.game.as_mut().unwrap();
            game.resize_manual_input((size.0 as u16, size.1 as u16))
                .unwrap();
            let variable =
                shin_core::vm::higurashi::command::HigurashiRuntimeCommand::SSET { args: [2, 999] };
            game.bindings
                .progress
                .dispatch_variables(&variable, Execution::Live, 0)
                .unwrap();
            game.bindings
                .progress
                .dispatch_variables(
                    &shin_core::vm::higurashi::command::HigurashiRuntimeCommand::OpcodeDB {
                        args: [0, 255],
                    },
                    Execution::Live,
                    0,
                )
                .unwrap();
            let source = game.bindings.runtime_game_data().unwrap();
            game.controls.requests.push_back(Request::SystemMenu);
            capture(&device, &queue, size, &mut slot);
            for _ in 0..10 {
                tick(&mut slot, &mut number, 0, 0);
            }
            if !save {
                tick(&mut slot, &mut number, 0, 1 << 6);
            }
            tick(&mut slot, &mut number, 1 << 24, 0);
            until(&mut slot, &mut number, |g| g.manual_menu.is_some());
            let game = slot.game.as_ref().unwrap();
            let menu = &game.manual_menu.as_ref().unwrap().owner;
            assert_eq!(
                menu.model.progress, 0.0,
                "Child ctor-frame excludes firstpoll"
            );
            assert_eq!(menu.reference_current(), save.then_some(&source));
            assert_eq!(game.bindings.runtime_game_data().unwrap(), source);
            assert_eq!(game.bindings.progress.pending_persistent().len(), 1);
            assert_eq!(game.bindings.progress.pending_bytes().len(), 1);
            until(&mut slot, &mut number, |g| {
                g.manual_menu
                    .as_ref()
                    .is_some_and(|m| m.owner.model.progress == 1.0)
            });
            tick(&mut slot, &mut number, 0, 0);
            capture(&device, &queue, size, &mut slot);
            assert_eq!(
                slot.game
                    .as_ref()
                    .unwrap()
                    .host()
                    .unwrap()
                    .modal_pause_depth(),
                1
            );
            if save {
                tick(&mut slot, &mut number, 1 << 25, 0);
                until(&mut slot, &mut number, |g| {
                    g.manual_menu.as_ref().unwrap().owner.model.closing
                });
                assert!(
                    !slot
                        .game
                        .as_ref()
                        .unwrap()
                        .manual_menu
                        .as_ref()
                        .unwrap()
                        .owner
                        .complete()
                );
                until(&mut slot, &mut number, |g| {
                    g.manual_menu.as_ref().unwrap().owner.complete()
                });
                assert!(
                    slot.game.as_ref().unwrap().manual_menu.is_some(),
                    "Completion-frame owner stays retained"
                );
                assert_eq!(
                    slot.game
                        .as_ref()
                        .unwrap()
                        .bindings
                        .runtime_game_data()
                        .unwrap(),
                    source
                );
                tick(&mut slot, &mut number, 0, 0);
                assert!(slot.game.as_ref().unwrap().manual_menu.is_none());
                assert_eq!(
                    slot.game
                        .as_ref()
                        .unwrap()
                        .host()
                        .unwrap()
                        .modal_pause_depth(),
                    1,
                    "Source SystemMenu resumes"
                );
                assert_eq!(
                    slot.game
                        .as_ref()
                        .unwrap()
                        .bindings
                        .progress
                        .pending_persistent()
                        .len(),
                    1
                );
            } else {
                tick(&mut slot, &mut number, 1 << 24, 0);
                until(&mut slot, &mut number, |g| {
                    g.manual_menu.as_ref().unwrap().owner.reference_question() == Some((2, 1))
                });
                tick(&mut slot, &mut number, 0, 1 << 7);
                tick(&mut slot, &mut number, 1 << 24, 0);
                let end = Instant::now() + Duration::from_secs(10);
                while !slot.pending_manual_load() {
                    tick(&mut slot, &mut number, 0, 0);
                    assert!(Instant::now() < end);
                }
                assert!(slot.game.as_ref().unwrap().manual_menu.is_some());
                assert_eq!(
                    slot.game
                        .as_ref()
                        .unwrap()
                        .bindings
                        .runtime_game_data()
                        .unwrap(),
                    source
                );
                let (composed, _) = capture(&device, &queue, size, &mut slot);
                reference_render_to_texture(&device, &queue, size, |context, _| {
                    assert!(slot.dispatch_manual_load(context, &composed).unwrap());
                });
                assert_eq!(
                    slot.game
                        .as_ref()
                        .unwrap()
                        .bindings
                        .runtime_game_data()
                        .unwrap(),
                    loaded
                );
                assert!(slot.game.as_ref().unwrap().manual_menu.is_none());
                assert!(
                    slot.game
                        .as_ref()
                        .unwrap()
                        .bindings
                        .progress
                        .pending_persistent()
                        .is_empty()
                );
                assert!(
                    slot.game
                        .as_ref()
                        .unwrap()
                        .bindings
                        .progress
                        .pending_bytes()
                        .is_empty()
                );
            }
            profiles += 1;
        }
    }
    eprintln!(
        "Gameplay manual SaveMenu:{profiles} real source-menu/Back/Load profiles at720p/1080p,original mixed-snapshot constructors/final callbacks,VM2530 copy with pending queues untouched,source resumption and full flags0 replacement pass"
    );
}
