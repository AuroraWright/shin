// Focused normal saved-load -> authored opening Movie -> B skip -> scenario.
#[test]
#[ignore = "requires supplied original/1.03 ROMs, Metal and exact Loopback Audio"]
fn reference_authored_movie_after_saved_scene() {
    assert!(std::env::var_os("SHIN_REFERENCE_AUTHORED_MOVIE_AFTER_LOAD").is_some());
    std::thread::Builder::new()
        .name("reference-authored-movie-load".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires supplied executables/ROMs, Metal, exact Loopback Audio and SHIN_REFERENCE_AUTHORED_SAVED_MOVIE_ONLY"]
fn reference_authored_saved_movie_hydration() {
    assert!(std::env::var_os("SHIN_REFERENCE_AUTHORED_SAVED_MOVIE_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-authored-saved-movie".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap().join().unwrap();
}

#[test]
#[ignore = "requires supplied ROMs, Metal and exact Loopback Audio"]
fn reference_authored_concurrent_movies_after_saved_scene() {
    assert!(std::env::var_os("SHIN_REFERENCE_AUTHORED_CONCURRENT_MOVIES_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-authored-concurrent-movies".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap().join().unwrap();
}

mod authored_movie_load {
    use super::super::native_replay_tests::{assert_audio, expected_rows};
    use super::*;
    use crate::{
        adv::{Adv, AdvAssets},
        app::AppAction,
        asset::system::AssetServer,
        layer::{DrawableLayer, Layer, user::UserLayer},
        render::PreRenderContext,
        update::{Updatable, UpdateContext},
    };
    use shin_core::{
        format::save::GameData,
        primitives::update::FrameId,
        time::Ticks,
        vm::command::types::{LayerType, LayerbankId},
    };
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    use std::{
        io::Write,
        path::Path,
        process::{Command, Stdio},
        sync::Arc,
        time::{Duration, Instant},
    };

    fn native_state(rows: &str, live: &Adv, boundary: u32) -> usize {
        let mut rows = rows.lines();
        let header = rows.next().unwrap().split_whitespace().collect::<Vec<_>>();
        assert_eq!(header[0], "V");
        assert_eq!(header[2].parse::<u32>().unwrap(), boundary);
        assert_eq!(live.vm_state.save_position, boundary);
        assert_eq!(
            header[7].parse::<u32>().unwrap(),
            live.vm_state.save_position
        );
        assert_eq!(
            header[8].parse::<usize>().unwrap(),
            live.vm_state.route.bit_position()
        );
        assert_eq!(
            u32::from_str_radix(header[9], 16).unwrap(),
            live.scripter.position().0
        );
        let mut expected = expected_rows(&live.vm_state);
        let count = expected.len();
        let mut audio = 0;
        for row in rows {
            if row.starts_with("D ") {
                break;
            }
            if row.starts_with("A ") {
                assert_audio(row, &live.vm_state, "authored Movie load");
                audio += 1;
            } else {
                let key = if row.starts_with("S ") {
                    "S".into()
                } else {
                    row.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
                };
                assert_eq!(row, expected.remove(&key).unwrap(), "{key}");
            }
        }
        assert!(expected.is_empty());
        assert_eq!(audio, 33);
        count
    }

    pub(super) fn verify<'a>(
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        assets: &AdvAssets,
        scenario: &Arc<Scenario>,
        target: &wgpu::TextureView,
    ) {
        if std::env::var_os("SHIN_REFERENCE_AUTHORED_CONCURRENT_MOVIES_ONLY").is_some() {
            verify_concurrent_movies(context, server, assets, scenario, target);
            return;
        }
        if std::env::var_os("SHIN_REFERENCE_AUTHORED_SAVED_MOVIE_ONLY").is_some() {
            verify_saved_movies(context, server, assets, scenario, target);
            return;
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let updated = std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some();
        let label = if updated { "1.03" } else { "original" };
        let output = std::path::PathBuf::from(
            std::env::var_os("SHIN_REFERENCE_AUTHORED_MOVIE_OUTPUT").unwrap(),
        );
        std::fs::create_dir_all(&output).unwrap();
        let helper = output.join("native-replay");
        assert!(
            Command::new("clang")
                .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
                .arg(root.join("reverse-engineering/native-save-replay.c"))
                .arg("-o")
                .arg(&helper)
                .status()
                .unwrap()
                .success()
        );
        let source = output.join("main.snr");
        std::fs::write(&source, scenario.raw()).unwrap();
        let payload = format!("0 0 56 {}\n0 0 57 {}\n", "aa".repeat(128), "aa".repeat(128));
        std::fs::write(output.join("input.txt"), &payload).unwrap();
        let mut child = Command::new(&helper)
            .arg(root.join("umineko_switch_files").join(if updated {
                "uncompressed_executable_update"
            } else {
                "uncompressed_executable"
            }))
            .arg(&source)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        let native = child.wait_with_output().unwrap();
        std::fs::write(output.join("native.txt"), &native.stdout).unwrap();
        std::fs::write(output.join("native.stderr"), &native.stderr).unwrap();
        assert!(
            native.status.success(),
            "{}",
            String::from_utf8_lossy(&native.stderr)
        );
        let text = String::from_utf8(native.stdout).unwrap();
        let split = text.find("V 0 57 ").unwrap();
        let mut route = RouteRecorder::default();
        for _ in 0..512 {
            route.record_persistent(0);
        }
        let mut authored_assets = assets.clone();
        authored_assets.scenario = scenario.clone();
        let mut live = Adv::new(
            Arc::new(shin_audio::AudioManager::new_for_tests()),
            server.clone(),
            authored_assets,
            Scripter::new(scenario, 0, 0),
        );
        live.load_game_data(
            &SaveData::default(),
            &GameData::from_entry(GameDataEntry {
                scenario_id: 0,
                random_seed: 0,
                save_position: 56,
                route_data: route.snapshot(),
            }),
            server.clone(),
        )
        .unwrap();
        let mut frame = FrameId::default();
        let deadline = Instant::now() + Duration::from_secs(20);
        while live.pending_restore.is_some() {
            assert!(
                Instant::now() < deadline,
                "Load before opening Movie timed out"
            );
            assert_eq!(live.vm_state.save_position, 0, "Early scene publication");
            frame.advance();
            live.update_saved_scene_load(&mut UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            });
            if live.pending_restore.is_some() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert!(
            live.saved_scene_load_error().is_none(),
            "{:?}",
            live.saved_scene_load_error()
        );
        let before_rows = native_state(&text[..split], &live, 56);
        assert!(
            live.vm_state
                .layers
                .layerbanks
                .values()
                .all(|saved| saved.layer_type != Some(LayerType::Movie))
        );
        live.vm_state.settings.v11_disallowskipunread = false;
        let movie_bank = LayerbankId::new(1);
        let mut owner = None;
        let mut playback = String::new();
        let mut cancel_sent = false;
        let mut movie_frames = 0;
        while live.vm_state.save_position < 57 || live.pending_restore.is_some() {
            assert!(
                Instant::now() < deadline,
                "Authored opening after load timed out: saved{} pc{:?} {:?}",
                live.vm_state.save_position,
                live.scripter.position(),
                live.current_command
            );
            let layer = live
                .adv_state
                .plane_layer_group(live.vm_state.layers.current_plane)
                .get_layer(movie_bank);
            if let Some(UserLayer::Movie(movie)) = layer.as_deref() {
                if owner.is_none() {
                    let saved = &live.vm_state.layers.layerbanks[movie_bank];
                    assert_eq!(saved.layer_type, Some(LayerType::Movie));
                    assert_eq!(saved.params.0, 1);
                    assert_eq!(saved.layer_load_counter, 39);
                    assert_eq!(layer.as_ref().unwrap().properties().layer_load_counter(), 39);
                    assert!(
                        server
                            .reference_requested_paths()
                            .contains(&scenario.info_tables().movie_info[1].path())
                    );
                    owner = Some(movie.reference_publication_owner());
                }
                movie_frames += 1;
                playback = movie.reference_playback_status();
            }
            let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
            if let Some(owner) = &owner {
                // Require actual decoded-frame publication and CoreAudio clock
                // progress before exercising the normal B/Cancel event skip.
                if !cancel_sent
                    && movie_frames > 2
                    && owner.time() > 20_000
                    && owner.frame_start_time().is_some()
                {
                    input[AppAction::Cancel].is_clicked = true;
                    cancel_sent = true;
                }
            }
            frame.advance();
            live.update(
                &mut UpdateContext {
                    frame_started_at: Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(6.0),
                    asset_server: server,
                    pre_render: context,
                },
                input,
            );
            live.adv_state
                .root_layer_group
                .pre_render(context, &Default::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Authored restored opening",
                );
                live.adv_state.render_scene(&mut pass);
            }
            let encoder = std::mem::replace(
                context.encoder,
                context.device.create_command_encoder(&Default::default()),
            );
            let mut upload = context.device.create_command_encoder(&Default::default());
            context.dynamic_buffer.finish(&mut upload);
            context.queue.submit([upload.finish(), encoder.finish()]);
            context.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            cancel_sent,
            "Opening must run and receive B before progressing"
        );
        assert!(
            live.saved_scene_load_error().is_none(),
            "{:?}",
            live.saved_scene_load_error()
        );
        let after_rows = native_state(&text[split..], &live, 57);
        assert!(
            live.vm_state
                .layers
                .layerbanks
                .values()
                .all(|saved| saved.layer_type != Some(LayerType::Movie))
        );
        assert!(
            live.adv_state
                .plane_layer_group(live.vm_state.layers.current_plane)
                .get_layer(movie_bank)
                .is_none()
        );
        frame.advance();
        live.adv_state.update(&mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(100.0),
            asset_server: server,
            pre_render: context,
        });
        owner.as_ref().unwrap().assert_released();
        assert!(
            !live.is_finished(),
            "The scenario remains usable after B skip"
        );
        drop(live);
        let summary = format!(
            "{label}: load56 matches{before_rows} native state rows; actual Movie1/bank1/counter39 frame/audio progress and B skip; boundary57 matches{after_rows} native rows; decoder/voice ownership retired while resumed scenario remains open. Movie frames={movie_frames}; last status={playback}\n"
        );
        std::fs::write(output.join("desktop.txt"), &summary).unwrap();
        eprint!("{summary}");
    }

    fn verify_concurrent_movies<'a>(
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>, assets: &AdvAssets,
        scenario: &Arc<Scenario>, target: &wgpu::TextureView,
    ) {
        use shin_core::vm::command::types::PlaneId;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let updated = std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some();
        let label = if updated { "1.03" } else { "original" };
        let executable = root.join("umineko_switch_files").join(if updated {
            "uncompressed_executable_update"
        } else { "uncompressed_executable" });
        let output = std::path::PathBuf::from(std::env::var_os("SHIN_REFERENCE_AUTHORED_MOVIE_OUTPUT").unwrap(),
        );
        std::fs::create_dir_all(&output).unwrap();
        let source = output.join("main.snr");
        std::fs::write(&source, scenario.raw()).unwrap();
        let payload = format!("3 0 6796 {}\n3 0 6797 {}\n", "aa".repeat(4096), "aa".repeat(4096));
        let mut native_text = String::new();
        for (name, input) in [("native-save-replay", payload),
                              ("native-authored-movie-checkpoints", "3 6797\n".into()),
        ] {
            let helper = output.join(name);
            assert!(Command::new("clang")
                .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
                .arg(root.join("reverse-engineering").join(format!("{name}.c")))
                .arg("-o").arg(&helper).status().unwrap().success());
            let input_path = output.join(format!("{name}.input"));
            std::fs::write(&input_path, input).unwrap();
            let native = Command::new(helper).arg(&executable).arg(&source)
                .stdin(Stdio::from(std::fs::File::open(input_path).unwrap()))
                .output().unwrap();
            std::fs::write(output.join(format!("{name}.txt")), &native.stdout).unwrap();
            std::fs::write(output.join(format!("{name}.stderr")), &native.stderr).unwrap();
            assert!(native.status.success(), "{}", String::from_utf8_lossy(&native.stderr));
            let text = String::from_utf8(native.stdout).unwrap();
            if name == "native-save-replay" { native_text = text; }
            else {
                // Native execution must load both owners before either unload;
                // the updated SNR has shifted PCs, so compare ordering/IDs.
                let first = text.find("O 3 6796 6 46 6344 ").unwrap();
                let second = text.find("O 3 6796 8 18 6345 ").unwrap();
                let unload_first = text[second..].find("X 3 6796 6 -1 ").unwrap() + second;
                let unload_second = text[second..].find("X 3 6796 8 -1 ").unwrap() + second;
                assert!(first < second && second < unload_first && unload_first < unload_second);
            }
        }
        let split = native_text.find("V 3 6797 ").unwrap();
        let mut route = RouteRecorder::default();
        for _ in 0..16384 { route.record_persistent(0); }
        let mut authored_assets = assets.clone();
        authored_assets.scenario = scenario.clone();
        let mut live = Adv::new(Arc::new(shin_audio::AudioManager::new_for_tests()),
                               server.clone(), authored_assets, Scripter::new(scenario, 0, 0),
        );
        live.load_game_data(&SaveData::default(), &GameData::from_entry(GameDataEntry {
            scenario_id: 3, random_seed: 0, save_position: 6796, route_data: route.snapshot(),
        }), server.clone(),
        ).unwrap();
        let mut frame = FrameId::default();
        let deadline = Instant::now() + Duration::from_secs(45);
        while live.pending_restore.is_some() {
            assert!(Instant::now() < deadline, "Concurrent Movie checkpoint load timed out");
            assert_eq!(live.vm_state.save_position, 0, "Early scene publication");
            frame.advance();
            live.update_saved_scene_load(&mut UpdateContext {
                frame_started_at: Instant::now(), frame_id: frame, delta_ticks: Ticks::ZERO,
                asset_server: server, pre_render: context,
            });
            if live.pending_restore.is_some() { std::thread::sleep(Duration::from_millis(1)); }
        }
        assert!(live.saved_scene_load_error().is_none(), "{:?}", live.saved_scene_load_error());
        let before_rows = native_state(&native_text[..split], &live, 6796);
        let mut owners = [None, None];
        let mut first_frames = [None, None];
        let mut progressed = [false; 2];
        let mut statuses = [String::new(), String::new()];
        let mut overlap_frames = 0;
        while live.vm_state.save_position < 6797 || live.pending_restore.is_some() {
            assert!(Instant::now() < deadline, "Concurrent Movie playback timed out: {:?} {:?}",
                    live.scripter.position(), live.current_command);
            let mut present = [false; 2];
            for (index, (bank_id, source_id, counter)) in [(6, 46, 6344), (8, 18, 6345)].into_iter().enumerate() {
                let bank = LayerbankId::new(bank_id);
                let saved = &live.vm_state.layers.layerbanks[bank];
                if saved.layer_type != Some(LayerType::Movie) || saved.params.0 != source_id { continue; }
                for plane in (0..4).map(PlaneId::new) {
                    if let Some(layer) = live.adv_state.plane_layer_group(plane).get_layer(bank) {
                        if let UserLayer::Movie(movie) = &*layer {
                            assert_eq!(saved.layer_load_counter, counter);
                            assert_eq!(layer.properties().layer_load_counter(), counter);
                            // A naturally completed layer can remain installed
                            // until MOVIEWAIT observes completion next frame.
                            if movie.is_finished() { continue; }
                            let owner = owners[index].get_or_insert_with(|| movie.reference_publication_owner());
                            assert_eq!(owner.identity(), movie.reference_publication_owner().identity());
                            if let Some(time) = owner.frame_start_time() {
                                let first = first_frames[index].get_or_insert(time);
                                progressed[index] |= time > *first && owner.time() > time;
                            }
                            statuses[index] = movie.reference_playback_status();
                            present[index] = true;
                        }
                    }
                }
            }
            if present.iter().all(|p| *p) {
                overlap_frames += 1;
                assert_ne!(owners[0].as_ref().unwrap().identity(), owners[1].as_ref().unwrap().identity());
            }
            let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
            // Advance only message commands. Neither target Movie receives skip
            // input; its authored WAIT/LAYERUNLOAD sequence runs normally.
            if matches!(live.current_command, Some(command::ExecutingCommand::MSGSET(_)
                                                   | command::ExecutingCommand::MSGWAIT(_))) {
                input[AppAction::Enter].is_clicked = true;
            }
            frame.advance();
            live.update(&mut UpdateContext {
                frame_started_at: Instant::now(), frame_id: frame, delta_ticks: Ticks::from_f32(1.0),
                asset_server: server, pre_render: context,
            }, input,
            );
            live.adv_state.root_layer_group.pre_render(context, &Default::default());
            {
                let mut pass = context.begin_pass(TextureTarget { view: target, kind: TextureTargetKind::Screen,
                    },
                                                  Some(context.depth_stencil), "Authored concurrent Movies",
                );
                live.adv_state.render_scene(&mut pass);
            }
            let encoder = std::mem::replace(context.encoder, context.device.create_command_encoder(&Default::default()),
            );
            let mut upload = context.device.create_command_encoder(&Default::default());
            context.dynamic_buffer.finish(&mut upload);
            context.queue.submit([upload.finish(), encoder.finish()]);
            context.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
            std::thread::sleep(Duration::from_millis(16));
        }
        assert!(overlap_frames > 2, "Both authored Movies must coexist in actual rendered frames");
        assert!(progressed.iter().all(|p| *p), "Both decoders must publish later frames: {statuses:?}");
        let after_rows = native_state(&native_text[split..], &live, 6797);
        assert!(!live.is_finished());
        for source_id in [46, 18] {
            assert!(server.reference_requested_paths().contains(&scenario.info_tables().movie_info[source_id].path()));
        }
        let retained = owners.each_ref().map(|owner| owner.as_ref().unwrap().owners());
        drop(live);
        for owner in owners.into_iter().flatten() { owner.assert_released(); }
        let summary = format!("{label}: entry3 public load6796 matches {before_rows} native state rows/33 audio descriptors; actual Movie46/bank6/counter6344 and Movie18/bank8/counter6345 use distinct playback owners, publish advancing decoded frames and coexist for {overlap_frames} Metal submissions without skip. Authored boundary6797 matches {after_rows} native rows; retained playback owners before final ADV release={retained:?}, both fully retired afterward. Last status={statuses:?}\n");
        eprint!("{summary}");
        std::fs::write(output.join("desktop.txt"), summary).unwrap();
    }

    fn verify_saved_movies<'a>(
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>, assets: &AdvAssets,
        scenario: &Arc<Scenario>, target: &wgpu::TextureView,
    ) {
        use shin_core::vm::command::types::{LayerProperty, PlaneId};
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let updated = std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some();
        let label = if updated { "1.03" } else { "original" };
        let output = std::path::PathBuf::from(std::env::var_os("SHIN_REFERENCE_AUTHORED_MOVIE_OUTPUT").unwrap(),
        );
        std::fs::create_dir_all(&output).unwrap();
        let helper = output.join("native-replay");
        assert!(Command::new("clang")
            .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
            .arg(root.join("reverse-engineering/native-save-replay.c"))
            .arg("-o").arg(&helper).status().unwrap().success());
        let source = output.join("main.snr");
        std::fs::write(&source, scenario.raw()).unwrap();
        // Reuse the real hydration/retirement checks for newly discovered origins.
        let fixture = std::env::var_os("SHIN_REFERENCE_EPISODE8_SAVED_MOVIE_FIXTURE")
            .map(std::path::PathBuf::from);
        let cases = if fixture.is_some() {
            vec![(7, 6896, 1, 144), (7, 6934, 1, 144)]
        } else {
            vec![(2, 2364, 4, 78), (3, 7733, 5, 78), (5, 3852, 4, 144),
                 (25, 93, 1, 84), (45, 55, 3, 84),
            ]
        };
        let mut route = RouteRecorder::default();
        for _ in 0..16384 { route.record_persistent(0); }
        let packets = cases.iter().map(|&(entry, boundary, _, _)| {
            let packet = if let Some(fixture) = &fixture {
                SaveData::decode(&std::fs::read(fixture.join(format!("last0-boundary{boundary}.dat"))).unwrap(),
                    ).unwrap()
            } else {
                let mut packet = SaveData::default();
                packet.manual_save_slots[99] = Some(GameData::from_entry(GameDataEntry {
                    scenario_id: entry, random_seed: 0, save_position: boundary,
                    route_data: route.snapshot(),
                }));
                packet
            };
            let saved = packet.manual_save_slots[99].as_ref().unwrap();
            assert_eq!(saved.entry.scenario_id, entry);
            assert_eq!(saved.entry.save_position, boundary);
            // Exercise the native-compatible packet before public load.
            SaveData::decode(&packet.encode()).unwrap()
        }).collect::<Vec<_>>();
        let payload: String = packets.iter().map(|packet| {
            let entry = &packet.manual_save_slots[99].as_ref().unwrap().entry;
            let bytes: Vec<u8> = serde_json::from_value(serde_json::to_value(&entry.route_data).unwrap()).unwrap();
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            format!("{} {:x} {} {hex}\n", entry.scenario_id, entry.random_seed, entry.save_position)
        }).collect();
        let input_path = output.join("input.txt");
        std::fs::write(&input_path, &payload).unwrap();
        // File input avoids pipe deadlock when native output is larger than
        // a pipe while the five long route packets are still being written.
        let native = Command::new(&helper)
            .arg(root.join("umineko_switch_files").join(if updated {
                "uncompressed_executable_update"
            } else { "uncompressed_executable" }))
            .arg(&source).stdin(Stdio::from(std::fs::File::open(input_path).unwrap()))
            .output().unwrap();
        std::fs::write(output.join("native.txt"), &native.stdout).unwrap();
        std::fs::write(output.join("native.stderr"), &native.stderr).unwrap();
        assert!(native.status.success(), "{}", String::from_utf8_lossy(&native.stderr));
        let native_text = String::from_utf8(native.stdout).unwrap();
        let mut summaries = String::new();
        for ((entry, boundary, bank_id, source_id), packet) in cases.into_iter().zip(packets) {
            let mut authored_assets = assets.clone();
            authored_assets.scenario = scenario.clone();
            let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
            let mut live = Adv::new(audio, server.clone(), authored_assets, Scripter::new(scenario, 0, 0),
            );
            live.load_game_data(&packet, packet.manual_save_slots[99].as_ref().unwrap(), server.clone(),
            ).unwrap();
            let mut frame = FrameId::default();
            let deadline = Instant::now() + Duration::from_secs(20);
            while live.pending_restore.is_some() {
                assert!(Instant::now() < deadline, "{label} entry{entry} saved Movie restore timed out");
                assert_eq!(live.vm_state.save_position, 0, "Early VM publication");
                frame.advance();
                live.update_saved_scene_load(&mut UpdateContext {
                    frame_started_at: Instant::now(), frame_id: frame,
                    delta_ticks: Ticks::ZERO, asset_server: server, pre_render: context,
                });
                if live.pending_restore.is_some() { std::thread::sleep(Duration::from_millis(1)); }
            }
            assert!(live.saved_scene_load_error().is_none(), "{:?}", live.saved_scene_load_error());
            let start = native_text.find(&format!("V {entry} {boundary} ")).unwrap();
            let rows = native_state(&native_text[start..], &live, boundary);
            let bank = LayerbankId::new(bank_id);
            let descriptor = &live.vm_state.layers.layerbanks[bank];
            assert_eq!(descriptor.layer_type, Some(LayerType::Movie));
            assert_eq!(descriptor.params.0, source_id);
            let repeat = descriptor.params.2 != 0;
            let properties = descriptor.properties.clone();
            let counter = descriptor.layer_load_counter;
            let plane = (0..4).map(PlaneId::new).find(|&p| {
                    live.adv_state.plane_layer_group(p).get_layer(bank).is_some()
                }).unwrap();
            let layer = live.adv_state.plane_layer_group(plane).get_layer(bank).unwrap();
            for index in 0..90 {
                let property = <LayerProperty as enum_map::Enum>::from_usize(index);
                assert_eq!(layer.properties().get_value(property), properties.get_property(property) as f32,
                           "{label} entry{entry} Movie property{index}");
            }
            assert_eq!(layer.properties().layer_load_counter(), counter);
            let UserLayer::Movie(movie) = &*layer else { panic!("Saved Movie hydrated as wrong layer type"); };
            let owner = movie.reference_publication_owner();
            let identity = owner.identity();
            let movie_path = scenario.info_tables().movie_info[source_id as usize].path();
            assert!(server.reference_requested_paths().contains(&movie_path));
            let time = owner.time();
            drop(layer); // The update below writes this same owned layer.
            let mut ticks = 0;
            loop {
                assert!(Instant::now() < deadline, "{label} entry{entry} hydrated Movie frame did not advance");
                frame.advance();
                live.adv_state.update(&mut UpdateContext {
                    frame_started_at: Instant::now(), frame_id: frame,
                    delta_ticks: Ticks::from_f32(6.0), asset_server: server, pre_render: context,
                });
                ticks += 6;
                assert_eq!(live.vm_state.save_position, boundary, "Playback cannot advance the VM checkpoint");
                live.adv_state.root_layer_group.pre_render(context, &Default::default());
                {
                    let mut pass = context.begin_pass(TextureTarget { view: target, kind: TextureTargetKind::Screen,
                        },
                                                      Some(context.depth_stencil), "Authored hydrated Movie",
                    );
                    live.adv_state.render_scene(&mut pass);
                }
                let encoder = std::mem::replace(context.encoder, context.device.create_command_encoder(&Default::default()),
                );
                let mut upload = context.device.create_command_encoder(&Default::default());
                context.dynamic_buffer.finish(&mut upload);
                context.queue.submit([upload.finish(), encoder.finish()]);
                context.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
                if ticks >= 36 && owner.time() > time && owner.frame_start_time().is_some() { break; }
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(owner.identity(), identity);
            let layer = live.adv_state.plane_layer_group(plane).get_layer(bank).unwrap();
            let UserLayer::Movie(movie) = &*layer else { panic!("Hydrated Movie disappeared"); };
            let status = movie.reference_playback_status();
            assert!(status.contains(&format!("repeat={repeat}")));
            drop(layer); // Final release must not retain a test read owner.
            drop(live);
            owner.assert_released();
            let summary = format!("{label} entry{entry} boundary{boundary}: native {rows} state rows/33 audio descriptors match; real saved Movie{source_id} bank{bank_id} repeat={repeat} counter{counter}/90 properties, packet roundtrip, atomic public load, actual decoded frame/clock, mixed scene Metal render and final backend/player retirement pass. path={movie_path}; {status}\n");
            eprint!("{summary}");summaries.push_str(&summary);
        }
        std::fs::write(output.join("desktop.txt"), summaries).unwrap();
    }
}
