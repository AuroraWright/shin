//! Continue regression from a read-only copy of the user's actual autosave.
use super::*;
use crate::layer::message_layer::reference_saved_sentence_state;
use shin_core::{format::save::SaveData, primitives::update::FrameId, time::Ticks};
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires copied progress, supplied ROM, Metal and exact Loopback Audio"]
fn reference_continue_restarts_saved_sentence_and_voice() {
    assert!(std::env::var_os("SHIN_REFERENCE_CONTINUE_MESSAGE_ONLY").is_some());
    std::thread::Builder::new()
        .name("continue-message-reference".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(super::save_menu_render_reference::reference_save_menu_gpu_window_and_artwork)
        .unwrap()
        .join()
        .unwrap();
}

pub(super) fn verify<'a>(
    ctx: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<crate::asset::system::AssetServer>,
) {
    let source = std::env::var("SHIN_REFERENCE_CONTINUE_MESSAGE_SAVE").unwrap();
    assert!(Path::new(&source).starts_with(std::env::temp_dir()));
    let bytes = std::fs::read(&source).unwrap();
    let globals = SaveData::decode(&bytes).unwrap();
    let game = globals.auto_save_slot.as_ref().unwrap();
    assert_eq!(
        (
            game.entry.scenario_id,
            game.entry.random_seed,
            game.entry.save_position
        ),
        (0, 2050585271, 119)
    );
    let assets = shin_tasks::block_on(AdvAssets::load(server)).unwrap();
    let rebuilt = reconstruction::ReconstructedScene::from_game_data(
        &assets.scenario,
        game,
        &globals,
        1_000_000,
    )
    .unwrap();
    let reconstruction::ReconstructionStop::Command(
        shin_core::vm::command::RuntimeCommand::MSGSET(message),
    ) = &rebuilt.stop
    else {
        panic!("User autosave must stop at MSGSET")
    };
    eprintln!(
        "User Continue boundary119 pc={:?} message_id={:?} text={}",
        rebuilt.scripter.position(),
        message.msg_id,
        message.text
    );
    assert!(
        message.text.contains("@v"),
        "User's Battler line has authored voice"
    );
    let original_text = message.text.clone();
    let original_pc = rebuilt.scripter.position();
    assert_eq!(
        original_pc.0, 0x9694c,
        "Matches both supplied native replay traces"
    );
    assert!(
        !message.auto_wait,
        "Authored async MSGSET is followed by MSGWAIT0"
    );
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut frame = FrameId::default();
    for (name, kind) in [
        ("Continue/Load", SceneLoadKind::Saved),
        ("History", SceneLoadKind::History),
        ("Resume", SceneLoadKind::Resume),
    ] {
        let mut live = Adv::new(
            manager.clone(),
            server.clone(),
            assets.clone(),
            Scripter::new(&assets.scenario, 0, 42),
        );
        if kind == SceneLoadKind::Saved {
            // Public API used by the actual title Continue factory.
            live.load_game_data(&globals, game, server.clone()).unwrap();
        } else {
            let scene = reconstruction::ReconstructedScene::from_game_data(
                &assets.scenario,
                game,
                &globals,
                1_000_000,
            )
            .unwrap();
            live.queue_reconstructed_scene_with_kind(Box::new(scene), server.clone(), None, kind);
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        while live.pending_restore.is_some() {
            frame.advance();
            live.update_saved_scene_load(&mut UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: ctx,
            });
            assert!(Instant::now() < deadline, "{name} restore timeout");
            submit(ctx);
            if live.pending_restore.is_some() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert!(live.saved_scene_load_error().is_none());
        assert_eq!(live.scripter.position(), original_pc);
        assert_eq!(
            live.vm_state.messagebox_state.text.as_deref(),
            Some(original_text.as_str())
        );
        assert!(
            live.current_command.is_none(),
            "Async MSGSET completes its token at installation"
        );
        reference_saved_sentence_state(&live.adv_state.message_layer(), false);
        assert!(
            !live
                .adv_state
                .message_layer()
                .voice_player()
                .get_wait_status()
                .is_empty(),
            "{name} must schedule the voice again"
        );
        assert_eq!(
            live.adv_state
                .message_layer()
                .voice_player()
                .position_millis(),
            0,
            "{name} voice must start at zero"
        );
        while live
            .adv_state
            .message_layer()
            .voice_player()
            .get_wait_status()
            .contains(shin_core::vm::command::types::AudioWaitStatus::LOADING)
        {
            live.adv_state
                .message_layer_mut()
                .voice_player_mut()
                .update(Ticks::ZERO);
            assert!(Instant::now() < deadline, "{name} voice load timeout");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            live.adv_state
                .message_layer()
                .voice_player()
                .get_wait_status()
                .contains(shin_core::vm::command::types::AudioWaitStatus::PLAYING)
        );
        if kind == SceneLoadKind::Saved {
            // Execute the removed duplicate restoration operation as a
            // counterfactual: this is exactly how old Continue lost voice.
            restoration::restore_message(
                &mut live.adv_state,
                &live.vm_state,
                &assets.scenario,
                ctx,
            );
            reference_saved_sentence_state(&live.adv_state.message_layer(), true);
            assert!(
                live.adv_state
                    .message_layer()
                    .voice_player()
                    .get_wait_status()
                    .is_empty()
            );
        } else {
            live.adv_state.message_layer_mut().voice_player_mut().stop();
        }
        eprintln!(
            "{name}: saved MSGSET119 restarts text/voice at zero; genuine voice asset reaches PLAYING; PC and pending command preserved"
        );
    }
    // A response boundary restores a prior displayed sentence statically. This
    // controlled scene ensures the fix does not restart an older voice there.
    let mut live = Adv::new(
        manager,
        server.clone(),
        assets.clone(),
        Scripter::new(&assets.scenario, 0, 42),
    );
    let mut scene = reconstruction::ReconstructedScene::from_game_data(
        &assets.scenario,
        game,
        &globals,
        1_000_000,
    )
    .unwrap();
    scene.stop =
        reconstruction::ReconstructionStop::Response(shin_core::vm::command::CommandResult::None);
    live.queue_reconstructed_scene(Box::new(scene), server.clone(), None);
    let deadline = Instant::now() + Duration::from_secs(20);
    while live.pending_restore.is_some() {
        frame.advance();
        live.update_saved_scene_load(&mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::ZERO,
            asset_server: server,
            pre_render: ctx,
        });
        submit(ctx);
        assert!(Instant::now() < deadline);
        if live.pending_restore.is_some() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    reference_saved_sentence_state(&live.adv_state.message_layer(), true);
    assert!(
        live.adv_state
            .message_layer()
            .voice_player()
            .get_wait_status()
            .is_empty()
    );
    assert_eq!(
        std::fs::read(&source).unwrap(),
        bytes,
        "Copied save must remain untouched"
    );
    assert!(live.persistence.is_none());
    eprintln!(
        "Non-MSGSET response retains static text/final wait without replaying voice; copied progress unchanged; no persistence configured"
    );
}

fn submit(ctx: &mut PreRenderContext) {
    let encoder = std::mem::replace(
        ctx.encoder,
        ctx.device.create_command_encoder(&Default::default()),
    );
    let mut upload = ctx.device.create_command_encoder(&Default::default());
    ctx.dynamic_buffer.finish(&mut upload);
    ctx.queue.submit([upload.finish(), encoder.finish()]);
    ctx.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
}
