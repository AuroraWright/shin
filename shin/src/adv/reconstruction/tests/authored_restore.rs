use crate::adv::reconstruction::native_replay_tests::{assert_audio, expected_rows};
use super::*;
use crate::{
    adv::{Adv, AdvAssets},
    asset::system::AssetServer,
    layer::{DrawableLayer, Layer, LayerProperties, LayerPropertiesState, user::UserLayer},
    render::PreRenderContext,
    update::UpdateContext,
};
use shin_core::{
    format::save::GameData, primitives::update::FrameId, time::Ticks,
    vm::command::types::LayerProperty,
};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

// Bridges native authored VM replay to actual desktop resource publication.
// Native asset loading/scheduling and independent NVN pixels remain separate.
#[test]
#[ignore = "requires supplied executable/ROMs, Metal, exact Loopback Audio and SHIN_REFERENCE_AUTHORED_RESTORE_ONLY"]
fn reference_authored_native_saved_scene_publication() {
    assert!(std::env::var_os("SHIN_REFERENCE_AUTHORED_RESTORE_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-authored-restore".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

fn properties(actual: &LayerProperties, expected: &LayerPropertiesState) {
    for i in 0..90 {
        let property = <LayerProperty as enum_map::Enum>::from_usize(i);
        assert_eq!(
            actual.get_value(property),
            expected.get_property(property) as f32,
            "{property:?}"
        );
    }
}

pub(super) fn verify<'a>(
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<AssetServer>,
    assets: &AdvAssets,
    scenario: &Arc<Scenario>,
    target: &wgpu::TextureView,
) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let data = std::path::PathBuf::from(std::env::var_os("SHIN_REFERENCE_ROM").unwrap());
    let updated = std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some();
    let label = if updated { "1.03" } else { "original" };
    let output = std::path::PathBuf::from(
        std::env::var_os("SHIN_REFERENCE_AUTHORED_RESTORE_OUTPUT").unwrap(),
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
    let cases = [(0, 0, 100), (7, 42, 50)];
    let mut route = RouteRecorder::default();
    for _ in 0..512 {
        route.record_persistent(0);
    }
    let payload: String = cases
        .iter()
        .map(|(entry, seed, boundary)| {
            format!("{entry} {seed:08x} {boundary} {}\n", "aa".repeat(128))
        })
        .collect();
    std::fs::write(output.join("input.txt"), &payload).unwrap();
    let mut child = Command::new(&helper)
        .arg(data.parent().unwrap().join(if updated {
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
    let native = String::from_utf8(native.stdout).unwrap();
    let mut rows = native.lines();
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut assets = assets.clone();
    assets.scenario = scenario.clone();
    let mut summaries = String::new();
    for (entry, seed, boundary) in cases {
        let mut live = Adv::new(
            audio.clone(),
            server.clone(),
            assets.clone(),
            Scripter::new(scenario, 0, 0),
        );
        let previous = live.vm_state.save_position;
        let requested = server.reference_requested_paths().len();
        live.load_game_data(
            &SaveData::default(),
            &GameData::from_entry(GameDataEntry {
                scenario_id: entry,
                random_seed: seed,
                save_position: boundary,
                route_data: route.snapshot(),
            }),
            server.clone(),
        )
        .unwrap();
        let mut frame = FrameId::default();
        let deadline = Instant::now() + Duration::from_secs(15);
        while live.pending_restore.is_some() {
            assert!(
                Instant::now() < deadline,
                "{label} entry{entry}: restore timed out"
            );
            assert_eq!(
                live.vm_state.save_position, previous,
                "Early VM publication"
            );
            live.update_saved_scene_load(&mut UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            });
            frame.advance();
            if live.pending_restore.is_some() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert!(
            live.saved_scene_load_error().is_none(),
            "{:?}",
            live.saved_scene_load_error()
        );
        assert_eq!(live.vm_state.save_position, boundary);
        let header: Vec<_> = rows.next().unwrap().split_whitespace().collect();
        assert_eq!(header[0], "V");
        assert_eq!(header[1].parse::<i32>().unwrap(), entry);
        assert_eq!(header[2].parse::<u32>().unwrap(), boundary);
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
        let compared_rows = expected.len();
        let mut audio_rows = 0;
        loop {
            let row = rows.next().unwrap();
            if row.starts_with("D ") {
                let fields: Vec<_> = row.split_whitespace().collect();
                assert_eq!(fields[1].parse::<i32>().unwrap(), entry);
                assert_eq!(fields[2].parse::<u32>().unwrap(), boundary);
                break;
            }
            if row.starts_with("A ") {
                assert_audio(row, &live.vm_state, label);
                audio_rows += 1;
            } else {
                let key = if row.starts_with("S ") {
                    "S".into()
                } else {
                    row.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
                };
                assert_eq!(
                    row,
                    expected.remove(&key).unwrap(),
                    "{label} entry{entry}: {key}"
                );
            }
        }
        assert!(expected.is_empty());
        assert_eq!(audio_rows, 33);
        // Verify actual installed objects, not just the reconstructed VM.
        properties(
            live.adv_state.root_layer_group().properties(),
            &live.vm_state.layers.root_layer_group,
        );
        properties(
            live.adv_state.screen_layer().properties(),
            &live.vm_state.layers.screen_layer,
        );
        properties(
            live.adv_state.page_layer().properties(),
            &live.vm_state.layers.page_layer,
        );
        for (plane, group) in &live.vm_state.layers.plane_layergroups {
            properties(
                live.adv_state.plane_layer_group(plane).properties(),
                &group.properties,
            );
        }
        let mut owners = Vec::new();
        let mut kinds = Vec::new();
        for (bank, saved) in &live.vm_state.layers.layerbanks {
            if saved.layer_type.is_none() {
                continue;
            }
            let layer = live
                .adv_state
                .plane_layer_group(saved.plane)
                .get_layer(bank)
                .unwrap();
            properties(layer.properties(), &saved.properties);
            assert_eq!(layer.properties().get_layer_id(), saved.layer_id);
            assert_eq!(
                layer.properties().layer_load_counter(),
                saved.layer_load_counter
            );
            let path = match &*layer {
                UserLayer::Picture(p) => {
                    let info = &scenario.info_tables().picture_info[saved.params.0 as usize];
                    assert!(p.reference_resource_owner().upgrade().unwrap().blocks.len() > 0);
                    owners.push((Some(p.reference_resource_owner()), None));
                    kinds.push("Picture");
                    info.path()
                }
                UserLayer::Bustup(b) => {
                    let info = &scenario.info_tables().bustup_info[saved.params.0 as usize];
                    let resource = b.reference_resource_owner().upgrade().unwrap();
                    assert!(!resource.base_blocks.is_empty());
                    assert_eq!(
                        resource.character_id,
                        crate::asset::bustup::CharacterId::from_metadata(
                            info.lipsync_character_id
                        )
                    );
                    owners.push((None, Some(b.reference_resource_owner())));
                    kinds.push("Bustup");
                    info.path()
                }
                _ => {
                    panic!("Chosen authored checkpoint must contain real Picture/Bustup layers")
                }
            };
            assert!(server.reference_requested_paths()[requested..].contains(&path));
        }
        assert!(kinds.contains(&"Picture") && kinds.contains(&"Bustup"));
        assert_eq!(kinds.len(), if entry == 0 { 2 } else { 3 });
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
                "Authored restored resources",
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
        drop(live);
        for (picture, bustup) in owners {
            if let Some(owner) = picture {
                assert!(
                    owner.upgrade().is_none(),
                    "Picture retained after scene shutdown"
                );
            }
            if let Some(owner) = bustup {
                assert!(
                    owner.upgrade().is_none(),
                    "Bustup retained after scene shutdown"
                );
            }
        }
        let line = format!(
            "{label} entry{entry} seed{seed} boundary{boundary}: {compared_rows} native state rows,33 audio descriptors,{} actual GPU layer owners; publication/render submission/retirement pass\n",
            kinds.len()
        );
        eprint!("{line}");
        summaries.push_str(&line);
    }
    assert!(rows.next().is_none());
    std::fs::write(output.join("desktop.txt"), summaries).unwrap();
}
