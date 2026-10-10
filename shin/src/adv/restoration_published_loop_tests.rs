#[test]
#[ignore = "requires supplied ROMs, Metal, exact Loopback Audio and SHIN_REFERENCE_PUBLISHED_LOOP_ONLY"]
fn reference_published_loop_worker_retirement() {
    assert!(std::env::var_os("SHIN_REFERENCE_PUBLISHED_LOOP_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-published-loop".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires supplied ROMs, Metal, exact Loopback Audio and SHIN_REFERENCE_PUBLISHED_HANDOFF_ONLY"]
fn reference_published_loop_queued_and_transferred_retirement() {
    assert!(std::env::var_os("SHIN_REFERENCE_PUBLISHED_HANDOFF_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-published-handoff".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

mod published_loop {
    use super::*;
    use crate::{
        adv::{Adv, AdvAssets},
        asset::system::AssetServer,
        layer::user::UserLayer,
        render::PreRenderContext,
        update::{Updatable, UpdateContext},
    };
    use shin_core::{
        format::{
            save::GameData,
            scenario::{
                instruction_elements::{MessageId, U8Bool},
                types::U16FixupString,
            },
        },
        primitives::update::FrameId,
        time::Ticks,
        vm::command::types::{LayerbankId, PlaneId},
    };
    use std::{
        path::PathBuf,
        sync::{Arc, mpsc},
        time::{Duration, Instant},
    };

    fn source(original: &Scenario) -> Arc<Scenario> {
        let movie = original
            .info_tables()
            .movie_info
            .iter()
            .position(|m| m.name.as_str().eq_ignore_ascii_case("door"))
            .unwrap();
        let mut bytes = original.raw().to_vec();
        let offset = bytes.len();
        bytes[32..36].copy_from_slice(&(offset as u32).to_le_bytes());
        let mut cursor = std::io::Cursor::new(&mut bytes);
        cursor.set_position(offset as u64);
        let mut params = std::io::Cursor::new(vec![255u8]);
        params.set_position(1);
        for value in [movie as i32, 1000, 1, 0, 0, 0, 0, 0] {
            NumberSpec::constant(value).write_le(&mut params).unwrap();
        }
        params.set_position(0);
        let params =
            shin_core::format::scenario::instruction_elements::BitmaskNumberArray::read_le(
                &mut params,
            )
            .unwrap();
        for command in [
            C::LAYERLOAD(c::LAYERLOAD {
                layer_id: NumberSpec::new(NumberSpec::constant(1).into_untyped()),
                layer_type: NumberSpec::new(NumberSpec::constant(6).into_untyped()),
                flags: NumberSpec::new(NumberSpec::constant(0).into_untyped()),
                params,
            }),
            C::MSGSET(c::MSGSET {
                msg_id: MessageId(1),
                auto_wait: U8Bool(true),
                text: U16FixupString::new("@rPublished loop"),
            }),
        ] {
            Instruction::Command(command).write_le(&mut cursor).unwrap();
        }
        let size = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        Arc::new(Scenario::new(bytes.into()).unwrap())
    }

    fn poll<'a>(
        live: &mut Adv,
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut FrameId,
        delta: Ticks,
    ) {
        frame.advance();
        live.update_saved_scene_load(&mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: *frame,
            delta_ticks: delta,
            asset_server: server,
            pre_render: context,
        });
    }
    fn finish<'a>(
        live: &mut Adv,
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut FrameId,
        boundary: u32,
    ) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while live.pending_restore.is_some() {
            assert!(Instant::now() < deadline);
            poll(live, context, server, frame, Ticks::ZERO);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(live.saved_scene_load_error().is_none());
        assert_eq!(live.vm_state.save_position, boundary);
    }
    pub(super) fn verify<'a>(
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        assets: &AdvAssets,
        original: &Arc<Scenario>,
    ) {
        let output =
            PathBuf::from(std::env::var_os("SHIN_REFERENCE_PUBLISHED_LOOP_OUTPUT").unwrap());
        std::fs::create_dir_all(&output).unwrap();
        let scenario = source(original);
        std::fs::write(output.join("fixture.snr"), scenario.raw()).unwrap();
        let mut assets = assets.clone();
        assets.scenario = scenario.clone();
        let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
        let game = |boundary| {
            GameData::from_entry(GameDataEntry {
                scenario_id: 0,
                random_seed: 0,
                save_position: boundary,
                route_data: RouteRecorder::default().snapshot(),
            })
        };
        let globals = SaveData::default();
        let mut frame = FrameId::default();
        let mut summaries = String::new();
        if std::env::var_os("SHIN_REFERENCE_PUBLISHED_HANDOFF_ONLY").is_some() {
            for transferred in [false, true] {
                for action in ["replace", "shutdown"] {
                    let mut live = Adv::new(
                        audio.clone(),
                        server.clone(),
                        assets.clone(),
                        Scripter::new(&scenario, 0, 0),
                    );
                    live.load_game_data(&globals, &game(1), server.clone())
                        .unwrap();
                    finish(&mut live, context, server, &mut frame, 1);
                    let Some(UserLayer::Movie(movie)) = live
                        .adv_state
                        .plane_layer_group(PlaneId::new(0))
                        .get_layer(LayerbankId::new(0))
                    else {
                        panic!("Published Movie missing");
                    };
                    let owner = movie.reference_publication_owner();
                    let identity = owner.identity();
                    let (entered, entry) = mpsc::channel();
                    let (resume, gate) = mpsc::channel();
                    let (prepared, preparation) = mpsc::channel();
                    let (finish_job, finish_gate) = mpsc::channel();
                    owner.gate_next_loop(shin_video::ReferenceLoopPreparationGate {
                        entered,
                        resume: gate,
                        prepared,
                        finish: finish_gate,
                    });
                    frame.advance();
                    live.adv_state.update(&mut UpdateContext {
                        frame_started_at: Instant::now(),
                        frame_id: frame,
                        delta_ticks: Ticks::from_seconds(5.01),
                        asset_server: server,
                        pre_render: context,
                    });
                    entry.recv_timeout(Duration::from_secs(15)).unwrap();
                    let before = owner.loop_worker().unwrap();
                    assert!(!before.result_ready && !before.transferred);
                    let retirement = before.retirement;
                    assert!(!retirement.is_finished());
                    resume.send(()).unwrap();
                    assert_eq!(
                        preparation
                            .recv_timeout(Duration::from_secs(15))
                            .unwrap()
                            .frame_number,
                        0
                    );
                    finish_job.send(()).unwrap();
                    let deadline = Instant::now() + Duration::from_secs(15);
                    loop {
                        let worker = owner.loop_worker().unwrap();
                        assert!(!worker.transferred);
                        if worker.result_ready {
                            break;
                        }
                        assert!(
                            Instant::now() < deadline,
                            "Actual PreparedVideo output never queued"
                        );
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    assert!(
                        !retirement.is_finished(),
                        "Queued real decoder still owns child I/O"
                    );
                    let old_time = owner.time();
                    let old_frame = owner.frame_start_time().unwrap();
                    if transferred {
                        frame.advance();
                        live.adv_state.update(&mut UpdateContext {
                            frame_started_at: Instant::now(),
                            frame_id: frame,
                            delta_ticks: Ticks::ZERO,
                            asset_server: server,
                            pre_render: context,
                        });
                        let worker = owner.loop_worker().unwrap();
                        assert!(worker.transferred && !worker.result_ready);
                        assert!(
                            !worker.retirement.is_finished(),
                            "Transferred decoder scope stays live"
                        );
                        assert_eq!(owner.identity(), identity);
                        assert_eq!(
                            owner.time(),
                            old_time,
                            "Transfer cannot reset or advance the clock"
                        );
                        assert!(
                            owner.frame_start_time().unwrap() > old_frame,
                            "Next cycle frame must be displayed"
                        );
                    }
                    // Hold only executor scheduling after observing the exact
                    // real queued/transferred phase. The decoder's own child
                    // tasks must still retire on the final owner's caller.
                    let (io_entered, io_entry) = mpsc::channel();
                    let (release_io, io_gate) = mpsc::channel();
                    let mut io_blocker = shin_tasks::async_io::spawn(async move {
                        io_entered.send(()).unwrap();
                        io_gate.recv_timeout(Duration::from_secs(15)).unwrap();
                    });
                    io_entry.recv_timeout(Duration::from_secs(15)).unwrap();
                    assert!(!retirement.is_finished());
                    let (started, start) = mpsc::channel();
                    let (returned, caller_return) = mpsc::channel();
                    let observer = retirement.clone();
                    let helper = std::thread::spawn(move || {
                        start.recv_timeout(Duration::from_secs(15)).unwrap();
                        let premature = caller_return
                            .recv_timeout(Duration::from_millis(100))
                            .is_ok();
                        let pending = !observer.is_finished();
                        release_io.send(()).unwrap();
                        (premature, pending)
                    });
                    started.send(()).unwrap();
                    if action == "replace" {
                        live.load_game_data(&globals, &game(0), server.clone())
                            .unwrap();
                        finish(&mut live, context, server, &mut frame, 0);
                        drop(live);
                    } else {
                        drop(live);
                    }
                    let _ = returned.send(());
                    let (premature, pending) = helper.join().unwrap();
                    assert!(
                        !premature,
                        "Final Movie owner returned before child I/O retirement"
                    );
                    assert!(
                        pending,
                        "Actual decoder work must remain pending at the caller wait"
                    );
                    assert!(
                        retirement.is_finished(),
                        "Observed restart scope must drain before return"
                    );
                    assert_eq!(
                        io_blocker.poll_with_timeout(Duration::from_secs(15)),
                        Some(())
                    );
                    assert_eq!(
                        shin_tasks::async_io::spawn(async { 42 })
                            .poll_with_timeout(Duration::from_secs(15)),
                        Some(42)
                    );
                    owner.assert_released();
                    let row = format!(
                        "{action} transferred={transferred}: actual PreparedVideo queue observed without polling, same-player/clock next-cycle publication, final-owner caller waits for real decoder child I/O, observed restart scope fully drained and backend/player released.\n"
                    );
                    eprint!("{row}");
                    summaries.push_str(&row);
                }
            }
            std::fs::write(output.join("desktop.txt"), summaries).unwrap();
            return;
        }
        for after_preparation in [false, true] {
            for action in ["replace", "shutdown"] {
                let mut live = Adv::new(
                    audio.clone(),
                    server.clone(),
                    assets.clone(),
                    Scripter::new(&scenario, 0, 0),
                );
                live.load_game_data(&globals, &game(1), server.clone())
                    .unwrap();
                finish(&mut live, context, server, &mut frame, 1);
                let Some(UserLayer::Movie(movie)) = live
                    .adv_state
                    .plane_layer_group(PlaneId::new(0))
                    .get_layer(LayerbankId::new(0))
                else {
                    panic!("Published Movie missing")
                };
                let owner = movie.reference_publication_owner();
                let (entered, entry) = mpsc::channel();
                let (resume, gate) = mpsc::channel();
                let (prepared, preparation) = mpsc::channel();
                let (finish_job, finish_gate) = mpsc::channel();
                owner.gate_next_loop(shin_video::ReferenceLoopPreparationGate {
                    entered,
                    resume: gate,
                    prepared,
                    finish: finish_gate,
                });
                frame.advance();
                live.adv_state.update(&mut UpdateContext {
                    frame_started_at: Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_seconds(5.01),
                    asset_server: server,
                    pre_render: context,
                });
                entry.recv_timeout(Duration::from_secs(15)).unwrap();
                assert!(
                    owner.frame_start_time().is_some(),
                    "Retain uploaded frame during natural EOF restart"
                );
                if after_preparation {
                    resume.send(()).unwrap();
                    assert_eq!(
                        preparation
                            .recv_timeout(Duration::from_secs(15))
                            .unwrap()
                            .frame_number,
                        0
                    );
                }
                let (started, start) = mpsc::channel();
                let (returned, caller_return) = mpsc::channel();
                let helper = std::thread::spawn(move || {
                    start.recv_timeout(Duration::from_secs(15)).unwrap();
                    let premature = caller_return
                        .recv_timeout(Duration::from_millis(100))
                        .is_ok();
                    assert_eq!(
                        shin_tasks::async_io::spawn(async { 42 })
                            .poll_with_timeout(Duration::from_secs(15)),
                        Some(42)
                    );
                    if !after_preparation {
                        resume.send(()).unwrap();
                        assert_eq!(
                            preparation
                                .recv_timeout(Duration::from_secs(15))
                                .unwrap()
                                .frame_number,
                            0
                        );
                    }
                    finish_job.send(()).unwrap();
                    premature
                });
                started.send(()).unwrap();
                if action == "replace" {
                    live.load_game_data(&globals, &game(0), server.clone())
                        .unwrap();
                    finish(&mut live, context, server, &mut frame, 0);
                    drop(live);
                } else {
                    drop(live);
                }
                let _ = returned.send(());
                let premature = helper.join().unwrap();
                assert!(
                    !premature,
                    "Final published Movie owner must drain its active restart producer"
                );
                owner.assert_released();
                let row = format!(
                    "{action} after_preparation={after_preparation}: actual published door loop reaches EOF/restart; caller waits for real FFmpeg preparation/output retirement, I/O remains available, backend/player owners released.\n"
                );
                eprint!("{row}");
                summaries.push_str(&row);
            }
        }
        // Exercise successful publication and a second natural rewind as well
        // as rejection while the producer is gated above. Reuse the focused
        // real-door clock/clone regression without running the broad fixture.
        let movie_id = original
            .info_tables()
            .movie_info
            .iter()
            .position(|m| m.name.as_str().eq_ignore_ascii_case("door"))
            .unwrap();
        let load_device = context.device.clone();
        let load_server = server.clone();
        let load_audio = audio.clone();
        let load_scenario = original.clone();
        let layer = shin_tasks::block_on(shin_tasks::async_io::spawn(async move {
            UserLayer::load(
                &load_device,
                &load_server,
                &load_audio,
                &load_scenario,
                Default::default(),
                shin_core::vm::command::types::LayerType::Movie,
                (movie_id as i32, 1000, 1, 0, 0, 0, 0, 0),
            )
            .await
        }));
        let UserLayer::Movie(mut movie) = layer else {
            panic!("Movie missing")
        };
        let owner = movie.reference_publication_owner();
        movie.assert_reference_loop_restarts(context, server);
        drop(movie);
        owner.assert_released();
        let row = "Two successful natural door loop restarts: monotonic timestamps, stable player/shared clone clocks and final owner release passed.\n";
        eprint!("{row}");
        summaries.push_str(row);
        std::fs::write(output.join("desktop.txt"), summaries).unwrap();
    }
}
