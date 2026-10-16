// Desktop publication of the exact four-plane script used by the native worker
// fixture. Native IDs/flags/counts/data are independent captured expectations.
use super::*;
use crate::{
    adv::{Adv, AdvAssets},
    asset::{mask::MaskTexture, system::AssetServer},
    render::PreRenderContext,
    update::UpdateContext,
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
    vm::command::types::PlaneId,
};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

fn n<T>(v: i32) -> NumberSpec<T> {
    NumberSpec::new(NumberSpec::constant(v).into_untyped())
}
fn scenario_with_masks(original: &Scenario) -> Arc<Scenario> {
    let mut bytes = original.raw().to_vec();
    let offset = bytes.len();
    bytes[32..36].copy_from_slice(&(offset as u32).to_le_bytes());
    let mut cursor = std::io::Cursor::new(&mut bytes);
    cursor.set_position(offset as u64);
    let count = original.info_tables().mask_info.len();
    for (plane, id) in [0, count / 3, count * 2 / 3, count - 1]
        .into_iter()
        .enumerate()
    {
        for cmd in [
            C::PLANESELECT(c::PLANESELECT {
                plane_id: n(plane as i32),
            }),
            C::MASKLOAD(c::MASKLOAD {
                mask_id: n(id as i32),
                mask_flags: n(plane as i32 + 1),
                smth_smth_transition: n(1),
            }),
            C::MSGSET(c::MSGSET {
                msg_id: MessageId(plane as u32 + 1),
                auto_wait: U8Bool(true),
                text: U16FixupString::new("Saved mask boundary"),
            }),
        ] {
            Instruction::Command(cmd).write_le(&mut cursor).unwrap();
        }
    }
    Instruction::Command(C::EXIT(c::EXIT {
        arg1: 0,
        arg2: n(0),
    }))
    .write_le(&mut cursor)
    .unwrap();
    let size = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    Arc::new(Scenario::new(bytes.into()).unwrap())
}

fn step<'a>(
    live: &mut Adv,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<AssetServer>,
    frame: &mut FrameId,
    ticks: f32,
) {
    let input = enum_map::EnumMap::from_fn(|action| shin_input::ActionState {
        is_clicked: matches!(action, crate::app::AppAction::Enter),
        is_clicked_or_repeated: matches!(action, crate::app::AppAction::Enter),
        is_held: matches!(action, crate::app::AppAction::Enter),
        ..Default::default()
    });
    live.update(
        &mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: *frame,
            delta_ticks: Ticks::from_f32(ticks),
            asset_server: server,
            pre_render: context,
        },
        input,
    );
    frame.advance();
    let encoder = std::mem::replace(
        context.encoder,
        context.device.create_command_encoder(&Default::default()),
    );
    let mut upload = context.device.create_command_encoder(&Default::default());
    context.dynamic_buffer.finish(&mut upload);
    context.queue.submit([upload.finish(), encoder.finish()]);
    context.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
}

fn gpu_texels(device: &wgpu::Device, queue: &wgpu::Queue, mask: &MaskTexture) -> Vec<u8> {
    // Independent textureLoad readback: no production mask/sprite shader or
    // sampler is used to inspect the actual published R8 texture contents.
    let source = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Published mask texel oracle"),
        source: wgpu::ShaderSource::Wgsl(
            r#"
            @group(0) @binding(0) var image: texture_2d<f32>;
            @group(0) @binding(1) var<storage, read_write> output: array<u32>;
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) id: vec3<u32>) {
                let size = textureDimensions(image);
                if id.x >= size.x * size.y { return; }
                output[id.x] = u32(round(textureLoad(image,
                    vec2<i32>(i32(id.x % size.x), i32(id.x / size.x)), 0).r * 255.0));
            }
        "#
            .into(),
        ),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Published mask texel oracle"),
        layout: None,
        module: &source,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let size = mask.texture.size();
    let count = size.width * size.height;
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Mask texels"),
        size: count as u64 * 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Mask texel readback"),
        size: count as u64 * 4,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let view = mask.texture.wgpu_texture().create_view(&Default::default());
    let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.dispatch_workgroups(count.div_ceil(64), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, count as u64 * 4);
    queue.submit([encoder.finish()]);
    let (done, received) = std::sync::mpsc::channel();
    readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        done.send(r).unwrap();
    });
    device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
    received
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    let mapped = readback.slice(..).get_mapped_range()
        .expect("Readback mapping must be ready");
    let bytes = mapped
        .chunks_exact(4)
        .map(|v| u32::from_le_bytes(v.try_into().unwrap()) as u8)
        .collect();
    drop(mapped);
    readback.unmap();
    bytes
}

pub(super) fn verify<'a>(
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<AssetServer>,
    assets: &AdvAssets,
    original: &Arc<Scenario>,
) {
    let version = if std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some() {
        "1.03"
    } else {
        "original"
    };
    let native = std::env::var("SHIN_REFERENCE_NATIVE_MASK_OUTPUT").unwrap();
    let native = Path::new(&native);
    let scenario = scenario_with_masks(original);
    assert!(
        scenario.raw() == std::fs::read(native.join(format!("{version}.snr"))).unwrap(),
        "Use the exact previously verified native script/metadata"
    );
    let golden =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../reverse-engineering/native-save-mask-desktop-golden.txt"))
            .lines()
            .map(|l| l.split_whitespace().collect::<Vec<_>>())
            .collect::<Vec<_>>();
    let mut assets = assets.clone();
    assets.scenario = scenario.clone();
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut live = Adv::new(
        audio.clone(),
        server.clone(),
        assets.clone(),
        Scripter::new(&scenario, 0, 0),
    );
    let globals = SaveData::default();
    let game = |boundary| {
        GameData::from_entry(GameDataEntry {
            scenario_id: 0,
            random_seed: 0,
            save_position: boundary,
            route_data: RouteRecorder::default().snapshot(),
        })
    };
    let mut frame = FrameId::default();
    let mut checked_texels = 0;
    for (iteration, boundary) in [4, 2, 0, 1, 3, 4, 0].into_iter().enumerate() {
        let old = (0..4)
            .map(|plane| {
                live.adv_state
                    .plane_layer_group(PlaneId::new(plane))
                    .reference_mask_state()
                    .0
                    .map(Arc::downgrade)
            })
            .collect::<Vec<_>>();
        let old_position = live.scripter.position();
        live.load_game_data(&globals, &game(boundary), server.clone())
            .unwrap();
        let task = live.pending_restore.as_mut().unwrap().task.take().unwrap();
        let (release, gate) = futures::channel::oneshot::channel();
        let (ready, received) = std::sync::mpsc::channel();
        live.pending_restore.as_mut().unwrap().task =
            Some(shin_tasks::async_io::spawn(async move {
                let result = task.await?;
                ready.send(result.0.reference_mask_owners()).unwrap();
                gate.await
                    .map_err(|_| anyhow::anyhow!("Test publication gate retired"))?;
                Ok(result)
            }));
        let prepared = received.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(prepared.len(), boundary as usize);
        for _ in 0..3 {
            step(&mut live, context, server, &mut frame, 5.0);
            assert_eq!(
                live.scripter.position(),
                old_position,
                "Pending input cannot advance VM"
            );
            for (plane, previous) in old.iter().enumerate() {
                let current = live
                    .adv_state
                    .plane_layer_group(PlaneId::new(plane as u8))
                    .reference_mask_state()
                    .0;
                assert_eq!(
                    current.map(Arc::as_ptr),
                    previous.as_ref().map(|w| w.as_ptr())
                );
            }
            assert!(live.pending_restore.is_some());
        }
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while live.pending_restore.is_some() {
            assert!(Instant::now() < deadline, "Mask publication timed out");
            step(&mut live, context, server, &mut frame, 1.0);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(live.saved_scene_load_error().is_none());
        assert_eq!(live.vm_state.save_position, boundary);
        assert!(live.adv_state.scene_load_fade.is_some());
        for plane in 0..4 {
            let row = golden
                .iter()
                .find(|r| {
                    r[0] == "P"
                        && r[1].parse::<u32>().unwrap() == boundary * 3
                        && r[2].parse::<usize>().unwrap() == plane
                })
                .unwrap();
            let group = &live.vm_state.layers.plane_layergroups[PlaneId::new(plane as u8)];
            assert_eq!(group.mask_id.into_i32(), row[3].parse::<i32>().unwrap());
            let (mask, flags) = live
                .adv_state
                .plane_layer_group(PlaneId::new(plane as u8))
                .reference_mask_state();
            assert_eq!(flags.bits(), row[4].parse::<i32>().unwrap());
            assert_eq!(mask.is_some(), row[5] == "1");
            if let Some(mask) = mask {
                let owner = prepared
                    .iter()
                    .find(|(p, _, _)| p.raw() as usize == plane)
                    .unwrap();
                assert_eq!(
                    Arc::as_ptr(mask),
                    owner.1.as_ptr(),
                    "Publish the exact prepared resource"
                );
                let m = golden
                    .iter()
                    .find(|r| r[0] == "M" && r[1] == row[1] && r[2] == row[2])
                    .unwrap();
                assert_eq!(mask.label, format!("Mask[{}]", m[3]));
                assert_eq!(mask.texture.size().width, m[4].parse::<u32>().unwrap());
                assert_eq!(mask.texture.size().height, m[5].parse::<u32>().unwrap());
                assert_eq!(mask.vertex_buffer.count(), m[6].parse::<usize>().unwrap());
                assert_eq!(mask.index_buffer.count(), m[7].parse::<usize>().unwrap());
                if iteration == 0 {
                    let expected = std::fs::read(
                        native
                            .join(version)
                            .join(format!("{}-pixels.bin", boundary as usize * 12 + plane)),
                    )
                    .unwrap();
                    let hash = expected.iter().fold(0xcbf29ce484222325u64, |h, v| {
                        (h ^ *v as u64).wrapping_mul(0x100000001b3)
                    });
                    assert_eq!(
                        format!("{hash:016x}"),
                        m[10],
                        "Native reference payload identity"
                    );
                    let actual = gpu_texels(context.device, context.queue, mask);
                    assert!(
                        actual == expected,
                        "Published GPU texels differ from native output: first mismatch {:?}",
                        actual.iter().zip(&expected).position(|(a, b)| a != b)
                    );
                    checked_texels += actual.len();
                }
            } else if let Some(previous) = &old[plane] {
                assert!(
                    previous.upgrade().is_none(),
                    "Old cleared plane resource must retire"
                );
            }
        }
        let position = live.scripter.position();
        step(&mut live, context, server, &mut frame, 15.0);
        assert_eq!(live.scripter.position(), position);
        step(&mut live, context, server, &mut frame, 30.0);
        assert!(!live.is_loading_saved_scene());
        assert_eq!(live.scripter.position(), position);
        eprintln!(
            "{version}: desktop mask boundary{boundary} iteration{iteration} native plane state, exact prepared owners, pending/fade VM gate and old-mask retirement pass"
        );
    }
    // Destroy actual prepared mask results during replacement and shutdown.
    for shutdown in [false, true] {
        live.load_game_data(&globals, &game(4), server.clone())
            .unwrap();
        let task = live.pending_restore.as_mut().unwrap().task.take().unwrap();
        let (release, gate) = futures::channel::oneshot::channel::<()>();
        let (ready, received) = std::sync::mpsc::channel();
        live.pending_restore.as_mut().unwrap().task =
            Some(shin_tasks::async_io::spawn(async move {
                let result = task.await?;
                ready.send(result.0.reference_mask_owners()).unwrap();
                gate.await
                    .map_err(|_| anyhow::anyhow!("Test cancelled prepared worker"))?;
                Ok(result)
            }));
        let owners = received.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(owners.len(), 4);
        assert!(owners.iter().all(|(_, w, _)| w.upgrade().is_some()));
        if shutdown {
            let mut dying = Adv::new(
                audio.clone(),
                server.clone(),
                assets.clone(),
                Scripter::new(&scenario, 0, 0),
            );
            dying.pending_restore = live.pending_restore.take();
            drop(dying);
        } else {
            live.load_game_data(&globals, &game(0), server.clone())
                .unwrap();
        }
        assert!(
            owners.iter().all(|(_, w, _)| w.upgrade().is_none()),
            "Cancellation drains prepared mask owners"
        );
        drop(release);
        eprintln!("{version}: prepared four-mask worker retirement shutdown={shutdown} pass");
    }
    eprintln!(
        "{version}: saved mask desktop publication passes7 scene loads,28 plane comparisons,{checked_texels} native GPU texels and2 prepared-worker retirement cases"
    );
}
