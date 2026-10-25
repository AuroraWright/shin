use super::*;
use crate::adv::higurashi::{
    audio::HigurashiAudio, autosave::Execution, exit::Destination, reference_gpu_assets,
    reference_render_to_texture,
};
use crate::app::higurashi::runtime_game_data::RuntimeGameData;
use shin_core::{
    primitives::color::UnormColor, vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};

fn update(
    slot: &mut title_slot::TitleSlot,
    number: &mut u32,
    elapsed: f32,
    buttons: Buttons,
) -> Status {
    *number += 1;
    // Isolate the direct-return music target from generic voice-focus policy.
    // Otherwise StopVoice also clears a browser target and can mask a missing
    // native15e54/32700 restore.
    let mut audio_policy = audio();
    audio_policy.focus_mode_active = false;
    shin_tasks::block_on(slot.update_game(
        Ticks::from_f32(elapsed),
        true,
        buttons,
        &PointerFrame {
            number: *number,
            time_seconds: *number as f64 / 60.0,
            events: &[],
        },
        policy(),
        audio_policy,
    ))
    .unwrap()
}

#[test]
#[ignore = "actual Gameplay zero EXIT retains command/ordered closing/30-tick clock/commits and full native destinations on offscreen Metal/the default audio output;fragment returns dispatch real Kakera,no window/saves"]
fn higurashi_gameplay_exit_closes_waits_commits_and_dispatches_fragments() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    // A controllable WAIT precedes the pending writes and EXIT. The trailing
    // WAIT must never execute while the old EXIT command remains retained.
    let code = [
        0x83, 1, 0x83, 0xe8, 0x82, 18, 2, 0xdb, 52, 2, 0, 0, 1, 0x83, 0, 1,
    ];
    let (authored, common) = super::character::common(&server, &code);
    let mut owners = 0;
    let mut fragment_transfers = 0;
    let mut bonus_requests = 0;
    for extent in [(1280, 720), (1920, 1080)] {
        for (scenario, enabled, destination) in [
            (0, true, Destination::SaveThenTitle),
            (1000, true, Destination::Tips),
            (2001, true, Destination::Kakera { selected: 1 }),
            (12050, true, Destination::Kakera { selected: 50 }),
            (12051, true, Destination::SaveThenTitle),
            (12049, true, Destination::Bonus),
            (12049, false, Destination::Kakera { selected: 49 }),
        ] {
            let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
            let mut saved = SaveData::default();
            saved.persist_data.0 = vec![1; 192];
            let legacy = fresh();
            saved.auto_save_slot = Some(legacy.clone());
            saved.manual_save_slots[2] = Some(legacy.clone());
            let globals = Globals {
                storage: None,
                runtime_saves: None,
                common: common.clone(),
                data: saved,
                clock: PlayClock::from_saved_seconds(7),
                platform_flag: true,
                last_title_mode: 3,
                audio: HigurashiAudio::new(manager.clone(), server.clone(), authored.clone()),
                server: server.clone(),
                manager: manager.clone(),
                sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager))),
            };
            let mut data = RuntimeGameData::from(fresh());
            data.progress.extra_word = 0xdeadbeef;
            data.progress.entries[0].scenario_id = scenario;
            data.progress.entries[0].response_data = vec![0x23, 0xee];
            data.progress.entries[1] = crate::adv::higurashi::autosave::ProgressEntry {
                scenario_id: 2084,
                random_seed: 0x87654321,
                save_position: 17,
                replay_target: 19,
                response_data: vec![0xf1, 0x55],
            };
            data.trailing_values = vec![7, 0xffff];
            data.byte_variables = vec![0; 64];
            if scenario == 12050 {
                // Alternate fragment50 is omitted entirely while locked.
                // Unlock it so the actual menu can expose the routed selection.
                data.byte_variables[50] = 1;
            }
            let dormant = data.progress.entries[1].clone();
            let mut game = None;
            reference_render_to_texture(&device, &queue, extent, |context, _| {
                let mut frame = context.new_render_texture("EXIT initial frame".into());
                let mut pass = context.begin_pass(
                    frame.as_texture_target(),
                    Some(context.depth_stencil),
                    "EXIT initial",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                drop(pass);
                game = Some(Gameplay::new(globals, data, 0, frame, || [1, 2, 3, 4]).unwrap());
            });
            let mut slot = title_slot::TitleSlot::from_game(game.unwrap());
            let mut number = 0;
            let deadline = Instant::now() + Duration::from_secs(10);
            while !matches!(
                slot.game
                    .as_ref()
                    .unwrap()
                    .host()
                    .unwrap()
                    .runner
                    .pending_command(),
                Some(Cmd::WAIT { amount: 1000, .. })
            ) {
                update(&mut slot, &mut number, 3.0, Buttons::default());
                assert!(
                    Instant::now() < deadline,
                    "EXIT fixture First did not reach WAIT"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            let held_mode = scenario == 0;
            if held_mode {
                // First's captured fade/input gate must retire before opening
                // a real HiddenWindow. B only hides an interested message.
                for _ in 0..20 {
                    update(&mut slot, &mut number, 3.0, Buttons::default());
                }
                let host = slot.game.as_mut().unwrap().host_mut().unwrap();
                host.runner
                    .reference_frame_mut()
                    .message
                    .dispatch(
                        &Cmd::MSGSET {
                            msg_id: shin_core::format::scenario::instruction_elements::MessageId(1),
                            auto_wait: 0,
                            argument: -1,
                            text: "@rA".into(),
                        },
                        &device,
                        &queue,
                        false,
                        0,
                        0,
                        0,
                        &host.audio,
                    )
                    .unwrap();
                host.runner
                    .reference_frame_mut()
                    .message
                    .playback
                    .natural_slide = 1.0;
                update(
                    &mut slot,
                    &mut number,
                    0.0,
                    Buttons {
                        pressed: 1 << 25,
                        ..Default::default()
                    },
                );
                assert!(slot.game.as_ref().unwrap().host().unwrap().has_modes());
            }
            let game = slot.game.as_mut().unwrap();
            game.bindings.memory.commits_enabled = enabled;
            game.bindings
                .progress
                .dispatch_variables(&Cmd::SSET { args: [161, 2] }, Execution::Live, 0)
                .unwrap();
            game.controls.state.modes = 3;
            let host = game.host.as_mut().unwrap();
            if matches!(destination, Destination::Kakera { .. }) {
                // Exercise15e54's32700(1) with a nondefault source gain.
                host.audio.browser_music_multiplier(0.5);
                host.audio.update_global_music(Ticks::from_f32(60.0));
                assert_eq!(host.audio.reference_music_focus(), 0.5);
            }
            let message = &mut host.runner.reference_frame_mut().message;
            message.playback.shown = true;
            message.playback.natural_slide = 1.0;
            host.ui
                .indicator
                .transition
                .control(1.0, Ticks::from_f32(120.0), 0)
                .unwrap();
            host.ui
                .menu
                .indicator
                .transition
                .control(1.0, Ticks::from_f32(120.0), 0)
                .unwrap();
            assert!(host.runner.interrupt_wait());
            let completed = host.runner.completed_commands();
            assert_eq!(
                update(&mut slot, &mut number, 120.0, Buttons::default()),
                Status::External
            );
            let game = slot.game.as_ref().unwrap();
            let runner = &game.host().unwrap().runner;
            assert!(runner.exit_closing());
            assert_eq!(runner.exit_command().unwrap().phase(), 1);
            assert_eq!(runner.completed_commands(), completed + 3); // WAIT,SSET,DB;EXIT remains owned.
            assert_eq!(game.controls.state.modes, 0);
            assert!(!runner.frame().message.playback.shown);
            if held_mode {
                // Concealment gates the natural slide until the mode retires.
                assert_eq!(runner.frame().message.playback.natural_slide, 1.0);
            } else {
                assert!(runner.frame().message.playback.natural_slide < 1.0);
            }
            assert_eq!(game.bindings.progress.persistent().get(18), 1);
            assert_eq!(game.bindings.progress.bytes()[52], 0);
            assert!(game.pending_exit_request().is_none());
            assert!(!slot.pending_kakera());
            // Clamp120 to3:the initial poll requested30 and only then ticked3.
            // Zero elapsed cannot finish the wait;input cannot re-enable auto/skip.
            for _ in 0..2 {
                assert_eq!(
                    update(
                        &mut slot,
                        &mut number,
                        0.0,
                        Buttons {
                            pressed: (1 << 10) | (1 << 11),
                            ..Default::default()
                        }
                    ),
                    Status::External
                );
                assert_eq!(slot.game.as_ref().unwrap().controls.state.modes, 0);
            }
            for _ in 0..9 {
                assert_eq!(
                    update(&mut slot, &mut number, 3.0, Buttons::default()),
                    Status::External
                );
                assert!(slot.game.as_ref().unwrap().pending_exit_request().is_none());
                assert!(!slot.pending_kakera());
            }
            // A closing message still gates commits after the full WAIT.
            slot.game
                .as_mut()
                .unwrap()
                .host_mut()
                .unwrap()
                .runner
                .reference_frame_mut()
                .message
                .playback
                .natural_slide = 1.0;
            assert_eq!(
                update(&mut slot, &mut number, 0.0, Buttons::default()),
                Status::External
            );
            assert_eq!(
                slot.game
                    .as_ref()
                    .unwrap()
                    .host()
                    .unwrap()
                    .runner
                    .exit_command()
                    .unwrap()
                    .phase(),
                1
            );
            assert_eq!(
                slot.game
                    .as_ref()
                    .unwrap()
                    .bindings
                    .progress
                    .persistent()
                    .get(18),
                1
            );
            // b94a8 observes wiper presence,including a finished endpoint that
            // has not yet had its enabled owning update.
            let frame = slot
                .game
                .as_mut()
                .unwrap()
                .host_mut()
                .unwrap()
                .runner
                .reference_frame_mut();
            frame.message.playback.natural_slide = 0.0;
            assert_eq!(
                frame
                    .scene
                    .dispatch(
                        &Cmd::WIPE {
                            args: [0, 1, 90],
                            params: Default::default()
                        },
                        false
                    )
                    .unwrap(),
                Some(crate::adv::higurashi::scene::CommandStatus::Complete)
            );
            assert_eq!(
                update(&mut slot, &mut number, 0.0, Buttons::default()),
                Status::External
            );
            assert_eq!(
                slot.game
                    .as_ref()
                    .unwrap()
                    .host()
                    .unwrap()
                    .runner
                    .exit_command()
                    .unwrap()
                    .phase(),
                1
            );
            slot.game
                .as_mut()
                .unwrap()
                .host_mut()
                .unwrap()
                .runner
                .reference_frame_mut()
                .scene
                .fast_forward_wiper();
            assert_eq!(
                update(&mut slot, &mut number, 0.0, Buttons::default()),
                Status::External
            );
            assert_eq!(
                slot.game
                    .as_ref()
                    .unwrap()
                    .host()
                    .unwrap()
                    .runner
                    .exit_command()
                    .unwrap()
                    .phase(),
                1
            );
            assert!(
                slot.game
                    .as_ref()
                    .unwrap()
                    .host()
                    .unwrap()
                    .runner
                    .frame()
                    .scene
                    .transition()
                    .is_none()
            );
            if held_mode {
                assert_eq!(
                    update(&mut slot, &mut number, 0.0, Buttons::default()),
                    Status::External
                );
                assert_eq!(
                    slot.game
                        .as_ref()
                        .unwrap()
                        .host()
                        .unwrap()
                        .runner
                        .exit_command()
                        .unwrap()
                        .phase(),
                    1
                );
                update(
                    &mut slot,
                    &mut number,
                    0.0,
                    Buttons {
                        pressed: 1 << 25,
                        ..Default::default()
                    },
                );
                assert!(slot.game.as_ref().unwrap().host().unwrap().has_modes());
                assert_eq!(
                    slot.game
                        .as_ref()
                        .unwrap()
                        .host()
                        .unwrap()
                        .runner
                        .exit_command()
                        .unwrap()
                        .phase(),
                    1
                );
            }
            assert_eq!(
                update(&mut slot, &mut number, 0.0, Buttons::default()),
                Status::External
            );
            let game = slot.game.as_ref().unwrap();
            assert_eq!(
                game.host().unwrap().runner.exit_command().unwrap().phase(),
                -1
            );
            assert_eq!(
                game.host().unwrap().runner.completed_commands(),
                completed + 3
            );
            assert_eq!(game.bindings.progress.progress().extra_word, 0xdeadbeef);
            assert_eq!(game.bindings.progress.progress().entries[1], dormant);
            assert_eq!(game.bindings.progress.responses(), (&[0x23, 0xee][..], 0));
            assert_eq!(
                game.bindings.progress.persistent().get(18),
                if enabled { 2 } else { 1 }
            );
            assert_eq!(
                game.bindings.progress.bytes()[52],
                if enabled { 2 } else { 0 }
            );
            assert_eq!(
                game.bindings.progress.pending_persistent().len(),
                if enabled { 0 } else { 2 }
            );
            assert_eq!(
                game.bindings.progress.pending_bytes().len(),
                if enabled { 0 } else { 1 }
            );
            assert_eq!(game.globals.data.auto_save_slot, Some(legacy.clone()));
            assert_eq!(game.globals.data.manual_save_slots[2], Some(legacy));
            if let Destination::Kakera { selected } = destination {
                assert!(slot.pending_kakera());
                let current = game.runtime_game_data().unwrap();
                let audio = &mut slot.game.as_mut().unwrap().host_mut().unwrap().audio;
                audio.update_global_music(Ticks::from_f32(60.0));
                assert_eq!(audio.reference_music_focus(), 1.0);
                reference_render_to_texture(&device, &queue, extent, |context, _| {
                    let mut frame = context.new_render_texture("Closed EXIT frame".into());
                    let mut pass = context.begin_pass(
                        frame.as_texture_target(),
                        Some(context.depth_stencil),
                        "Closed EXIT",
                    );
                    pass.clear(Some(UnormColor::WHITE), Some(0), Some(1.0));
                    drop(pass);
                    assert!(
                        slot.dispatch_kakera(context, &frame, (extent.0 as u16, extent.1 as u16))
                            .unwrap()
                    );
                });
                assert!(slot.game.is_none());
                let menu = slot.kakera.as_ref().unwrap();
                assert_eq!(menu.motion.model.data, current);
                assert_eq!(menu.motion.model.selected_fragment(), selected as usize);
                fragment_transfers += 1;
            } else if destination == Destination::Bonus {
                assert!(slot.pending_bonus_adv());
                assert!(slot.pending_scene_factory());
                assert!(game.pending_exit_request().is_none());
                assert!(slot.game.is_some());
                bonus_requests += 1;
            } else {
                let request = game.pending_exit_request().unwrap();
                assert_eq!(request.destination, destination);
                assert_eq!(request.data, game.runtime_game_data().unwrap());
                // Storage/TIPS services are not fabricated. Further frames
                // retain the same command and destination without decoding.
                assert_eq!(
                    update(&mut slot, &mut number, 3.0, Buttons::default()),
                    Status::External
                );
                assert_eq!(
                    slot.game
                        .as_ref()
                        .unwrap()
                        .host()
                        .unwrap()
                        .runner
                        .completed_commands(),
                    completed + 3
                );
                assert!(slot.title.is_none());
            }
            owners += 1;
        }
    }
    assert_eq!(bonus_requests, 2);
    println!(
        "Live EXIT:{owners} actual Gameplay owners at720p/1080p close,clear auto/skip,wait30 clamped post-poll ticks,commit only when enabled,retain complete progress/destinations,{fragment_transfers} concrete Kakera scene transfers and{bonus_requests} fresh Bonus requests;absent-storage title/TIPS remain retained"
    );
}
