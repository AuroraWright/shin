// Inject only an unsupported reconstruction stop after genuine resources have
// reached the normal ready/capacity handoff. Keep a real decoder I/O worker from
// processing cancellation until the caller has exercised command-start failure.
pub(super) fn verify_post_install_failure<'a>(
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<AssetServer>,
    assets: &AdvAssets,
    original: &Arc<Scenario>,
) {
    use shin_core::{
        format::scenario::instruction_elements::U8Bool,
        vm::{IntoRuntimeForm, VmCtx},
    };
    use std::{path::PathBuf, sync::mpsc};
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_POST_INSTALL_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let scenario = source(original, false, None);
    std::fs::write(output.join("fixture.snr"), scenario.raw()).unwrap();
    let mut assets = assets.clone();
    assets.scenario = scenario.clone();
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let globals = SaveData::default();
    let game = |boundary| {
        GameData::from_entry(GameDataEntry {
            scenario_id: 0,
            random_seed: 0,
            save_position: boundary,
            route_data: RouteRecorder::default().snapshot(),
        })
    };
    let mut live = Adv::new(
        audio.clone(),
        server.clone(),
        assets,
        Scripter::new(&scenario, 0, 0),
    );
    let mut frame = FrameId::default();
    live.load_game_data(&globals, &game(1), server.clone())
        .unwrap();
    finish(&mut live, context, server, &mut frame);
    let old_rows = super::super::native_replay_tests::expected_rows(&live.vm_state);
    let old_pc = live.scripter.position();
    let old_route = live.vm_state.route.bit_position();
    let old_command = format!("{:?}", live.current_command);
    let old_layer = live
        .adv_state
        .plane_layer_group(PlaneId::new(0))
        .get_layer(LayerbankId::new(0))
        .unwrap();
    let old_owner = Owner::of(&old_layer);
    let blockers = {
        let mut manager = audio.kira_manager().lock();
        let available = manager.sub_track_capacity() - manager.num_sub_tracks();
        (0..available)
            .map(|_| {
                manager
                    .add_sub_track(kira::track::TrackBuilder::new())
                    .unwrap()
            })
            .collect::<Vec<_>>()
    };
    live.load_game_data(&globals, &game(3), server.clone())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while live.pending_restore.as_ref().unwrap().ready.is_none() {
        assert!(Instant::now() < deadline, "Ready candidate timed out");
        step(&mut live, context, server, &mut frame);
        std::thread::sleep(Duration::from_millis(1));
    }
    let pending = live.pending_restore.as_mut().unwrap();
    assert!(pending.task.is_none(), "Exercise the real ready handoff");
    let retirement = pending.retirement.as_ref().unwrap().clone();
    assert!(!retirement.is_finished(), "Real decoder I/O remains owned");
    let result = pending.ready.as_mut().unwrap().as_mut().unwrap();
    let candidate = owners(&result.0);
    assert_eq!(candidate.len(), 3);
    assert!(matches!(
        result.1.stop,
        ReconstructionStop::Command(RuntimeCommand::MSGSET(_))
    ));
    result.1.stop = ReconstructionStop::Command(
        C::WAIT(c::WAIT {
            wait_amount: n(1),
            allow_interrupt: U8Bool(false),
        })
        .into_runtime_form(&VmCtx::new(0, 0)),
    );
    drop(blockers);
    while {
        let manager = audio.kira_manager().lock();
        manager.sub_track_capacity() - manager.num_sub_tracks()
            < crate::adv::AdvState::AUDIO_TRACK_COUNT
    } {
        assert!(Instant::now() < deadline, "Track capacity never recovered");
        std::thread::sleep(Duration::from_millis(1));
    }
    let (io_entered, io_entry) = mpsc::channel();
    let (resume, gate) = mpsc::channel();
    let mut io_blocker = shin_tasks::async_io::spawn(async move {
        io_entered.send(()).unwrap();
        gate.recv_timeout(Duration::from_secs(15)).unwrap();
    });
    io_entry.recv_timeout(Duration::from_secs(15)).unwrap();
    let (entered, entry) = mpsc::channel();
    let (returned, caller_return) = mpsc::channel();
    let observer = retirement.clone();
    let helper = std::thread::spawn(move || {
        entry.recv_timeout(Duration::from_secs(15)).unwrap();
        let premature = caller_return
            .recv_timeout(Duration::from_millis(100))
            .is_ok();
        let work_pending = !observer.is_finished();
        resume.send(()).unwrap();
        (premature, work_pending)
    });
    entered.send(()).unwrap();
    step(&mut live, context, server, &mut frame);
    let _ = returned.send(());
    let (premature, work_pending) = helper.join().unwrap();
    assert!(
        !premature,
        "Command-start failure must drain candidate decoder I/O before return"
    );
    assert!(
        work_pending,
        "Controlled scheduling keeps actual decoder work pending"
    );
    assert!(
        retirement.is_finished(),
        "Failed installed candidate scope is drained"
    );
    assert_eq!(
        io_blocker.poll_with_timeout(Duration::from_secs(15)),
        Some(())
    );
    assert!(live.pending_restore.is_none());
    assert!(
        live.saved_scene_load_error()
            .unwrap()
            .contains("Unsupported reconstruction stop")
    );
    assert_eq!(
        super::super::native_replay_tests::expected_rows(&live.vm_state),
        old_rows
    );
    assert_eq!(live.scripter.position(), old_pc);
    assert_eq!(live.vm_state.route.bit_position(), old_route);
    assert_eq!(format!("{:?}", live.current_command), old_command);
    old_owner.assert_live(
        &live.adv_state
            .plane_layer_group(PlaneId::new(0))
            .get_layer(LayerbankId::new(0))
            .unwrap(),
    );
    for (_, owner) in &candidate {
        if let Owner::Movie(movie, _) = owner {
            movie.assert_released();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while audio.kira_manager().lock().main_track().num_sounds() != 0 {
        assert!(
            Instant::now() < deadline,
            "Failed candidate voice did not retire"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(live.restore_error_dialog.is_some());
    assert_eq!(live.adv_state.audio_pause_count, 1);
    assert!(live.failed_scene_title_setup().is_none());
    // The ordinary update owns acknowledgement and consumes its final frame.
    let frozen_pc = live.scripter.position();
    let frozen_position = live.vm_state.save_position;
    for _ in 0..100 {
        step(&mut live, context, server, &mut frame);
        assert_eq!(live.scripter.position(), frozen_pc);
        assert_eq!(live.vm_state.save_position, frozen_position);
        if live.restore_error_dialog.is_none() { break; }
    }
    assert!(live.restore_error_dialog.is_none(), "Load error must acknowledge");
    assert_eq!(live.adv_state.audio_pause_count, 0);
    assert!(live.failed_scene_title_setup().is_some());
    // A repeated public load releases just its error scope, retaining others.
    live.adv_state.acquire_audio_pause();
    live.adv_state.acquire_audio_pause();
    live.show_scene_load_error(&anyhow::anyhow!("repeat resource failure"), server).unwrap();
    assert_eq!(live.adv_state.audio_pause_count, 3);
    live.load_game_data(&globals, &game(3), server.clone())
        .unwrap();
    assert!(live.restore_error_dialog.is_none());
    assert_eq!(live.adv_state.audio_pause_count, 2);
    live.adv_state.release_audio_pause();
    live.adv_state.release_audio_pause();
    finish(&mut live, context, server, &mut frame);
    assert_eq!(live.vm_state.save_position, 3);
    assert!(live.saved_scene_load_error().is_none());
    let group = live.adv_state.plane_layer_group(PlaneId::new(0));
    let movie = group.get_layer(LayerbankId::new(2)).unwrap();
    let retry = Owner::of(&movie);
    assert!(matches!(&*movie, UserLayer::Movie(_)));
    drop(live);
    retry.assert_released();
    let summary = "Post-install command-start rejection: real mixed PIC/BUP/Movie candidate, capacity/ready handoff, actual decoder work held by controlled I/O scheduling, caller join, old VM/PC/route/current-command/PIC identity preservation, Movie/voice retirement and successful retry pass. Unsupported stop is injected; no authored rejection reachability claim.\n";
    eprint!("{summary}");
    std::fs::write(output.join("desktop.txt"), summary).unwrap();
}
