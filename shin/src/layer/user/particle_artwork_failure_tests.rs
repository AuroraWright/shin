//! Consumer-specific effects requirements and replacement recovery using actual assets.
use super::*;
use crate::asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache, supplied_payload};
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::BytesAddress,
        texture::{DepthStencilTarget, TextureSamplerStore},
    },
};
use std::path::PathBuf;

#[test]
#[ignore = "requires supplied ROM, Metal and the default audio output; temporary assets only"]
fn reference_particle_artwork_consumer_and_replacement_recovery() {
    std::thread::Builder::new()
        .name("particle-artwork-recovery".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}
fn identity(layer: &UserLayer) -> usize {
    match layer {
        UserLayer::FocusLine(l) => l.reference_constructor_state().0,
        UserLayer::Rain(l) => l.reference_constructor_state().0,
        UserLayer::Animation(l) => l.reference_constructor_state().0,
        _ => panic!("Expected particle"),
    }
}
fn run() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_PARTICLE_ARTWORK_OUTPUT").unwrap());
    let directory = output.join("overrides");
    std::fs::create_dir_all(&directory).unwrap();
    let supplied = supplied_payload("/effects.txa").2;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
            adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
        } else {
            wgpu::Features::empty()
        },
        ..Default::default()
    }))
    .unwrap();
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let descriptor = |label, format| wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    };
    let depth = device.create_texture(&descriptor(
        "Particle artwork recovery depth",
        shin_render::DEPTH_STENCIL_FORMAT,
    ));
    let depth_view = depth.create_view(&Default::default());
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), wgpu::TextureFormat::Rgba8Unorm);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());

    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut cases = Vec::new();
    for (kind, mode, name) in [
        (LayerType::FocusLine, 0, "focusline"),
        (LayerType::Rain, 0, "rain"),
        (LayerType::Rain, 1, "hanabira"),
        (LayerType::Rain, 2, "snow"),
        (LayerType::Rain, 3, "goldsnow"),
        (LayerType::Animation, 0, "Sakura"),
        (LayerType::Animation, 4, "Sakura"),
    ] {
        for profile in ["prepared", "saved", "replacement", "unused"] {
            println!("particle artwork: {kind:?}/{mode}/{profile}/{name}");
            let mut io = LayeredAssetIo::new();
            io.try_with_dir(&directory).unwrap();
            if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
                io.try_with_rom(patch).unwrap();
            }
            io.try_with_rom(std::env::var_os("SHIN_REFERENCE_ROM").unwrap())
                .unwrap();
            let server = Arc::new(AssetServer::new(
                io.into(),
                AssetLoadContext {
                    game: crate::cli::Game::Umineko,
                    higurashi_bustup_cache: Default::default(),
                    wgpu_device: device.clone(),
                    wgpu_queue: queue.clone(),
                    bustup_cache: AssetCache::new(),
                    picture_cache: Default::default(),
                    font_cache: Default::default(),
                },
            ));
            let scenario: Arc<Scenario> = server.load_sync("/main.snr").unwrap();
            let params = if kind == LayerType::FocusLine {
                (-200, 100, 0, 0, 0, 0, 0, 0)
            } else {
                (mode, 100, 1500, 0, 0, 0, 0, 0)
            };
            let mut ctx = crate::render::PreRenderContext {
                breakup_texture: None,
                device: &device,
                queue: &queue,
                resize_source: &resize,
                sampler_store: &samplers,
                depth_stencil: DepthStencilTarget { view: &depth_view },
                pipeline_storage: &mut pipelines,
                dynamic_buffer: &mut dynamic,
                encoder: &mut encoder,
            };
            let mut good = supplied.clone();
            // The supplied images have no Sakura. A renamed authored descriptor is
            // a bounded repair/name control, never authentic Animation artwork.
            if kind == LayerType::Animation {
                replace_name(&mut good, "snow", "Sakura");
            }
            let mut bad = good.clone();
            let missing = if profile == "unused" {
                if name == "focusline" {
                    "rain"
                } else {
                    "focusline"
                }
            } else {
                name
            };
            replace_name(&mut bad, missing, "!absent");
            let file = directory.join("effects.txa");
            let load = || {
                shin_tasks::block_on(UserLayer::prepare_checked(
                    &device,
                    &server,
                    &manager,
                    &scenario,
                    Default::default(),
                    kind,
                    params,
                ))
            };
            let mut errors = Vec::new();
            if profile == "replacement" {
                std::fs::write(&file, &good).unwrap();
                let prepared = load().unwrap();
                let mut layer = prepared.publish(&mut ctx.render_clone_ctx());
                let copy = layer.render_clone(&mut ctx.render_clone_ctx());
                let before = identity(&layer);
                let old = shin_tasks::block_on(server.effects_atlas())
                    .unwrap()
                    .reference_weak();
                assert_eq!(
                    old.strong_count(),
                    1,
                    "Consumers must not retain an atlas Arc"
                );
                std::fs::write(&file, &bad).unwrap();
                let error = shin_tasks::block_on(server.initialize_effects_atlas())
                    .err()
                    .expect("Incomplete replacement published to active particle");
                errors.push(format!("{error:#}"));
                assert_eq!(identity(&layer), before);
                assert_eq!(identity(&copy), before);
                assert_eq!(old.strong_count(), 1);
                render(&mut layer, &mut ctx, &server);
                std::fs::write(&file, &good).unwrap();
                let repaired = shin_tasks::block_on(server.initialize_effects_atlas()).unwrap();
                assert_ne!(before, repaired.reference_identity());
                assert_eq!(identity(&layer), repaired.reference_identity());
                assert_eq!(identity(&copy), repaired.reference_identity());
                assert_eq!(old.strong_count(), 0);
                drop(prepared);
                drop(layer);
                std::fs::write(&file, &bad).unwrap();
                errors.push(format!(
                    "{:#}",
                    shin_tasks::block_on(server.initialize_effects_atlas())
                        .err()
                        .expect("Live render clone requirement was lost")
                ));
                assert_eq!(identity(&copy), repaired.reference_identity());
                drop(copy);
                // The last consumer is gone, so its now-unused name is optional.
                let released = shin_tasks::block_on(server.initialize_effects_atlas()).unwrap();
                assert!(released.read().get(name).is_none());
            } else {
                std::fs::write(&file, &bad).unwrap();
                if profile == "unused" {
                    let prepared = load().unwrap();
                    let mut layer = prepared.publish(&mut ctx.render_clone_ctx());
                    render(&mut layer, &mut ctx, &server);
                } else {
                    let error = if profile == "saved" {
                        shin_tasks::block_on(UserLayer::load_checked(
                            &device,
                            &server,
                            &manager,
                            &scenario,
                            Default::default(),
                            kind,
                            params,
                        ))
                        .err()
                    } else {
                        load().err()
                    }
                    .expect("Missing reached particle artwork accepted");
                    errors.push(format!("{error:#}"));
                    std::fs::write(&file, &good).unwrap();
                    let mut layer = if profile == "saved" {
                        shin_tasks::block_on(UserLayer::load_checked(
                            &device,
                            &server,
                            &manager,
                            &scenario,
                            Default::default(),
                            kind,
                            params,
                        ))
                        .unwrap()
                    } else {
                        load().unwrap().publish(&mut ctx.render_clone_ctx())
                    };
                    render(&mut layer, &mut ctx, &server);
                }
            }
            for error in &errors {
                assert!(
                    error.contains(name) && error.contains("/effects.txa"),
                    "{error}"
                );
            }
            let mut upload = ctx.device.create_command_encoder(&Default::default());
            ctx.dynamic_buffer.finish(&mut upload);
            let completed = std::mem::replace(
                ctx.encoder,
                ctx.device.create_command_encoder(&Default::default()),
            );
            ctx.queue.submit([upload.finish(), completed.finish()]);
            ctx.device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("GPU work must complete before readback or retirement");
            ctx.dynamic_buffer.recall();
            std::fs::remove_file(file).unwrap();
            cases.push(serde_json::json!({"layer":format!("{kind:?}"),"mode":mode,"name":name,"profile":profile,"errors":errors,"same_server_retry":profile!="unused","artwork_control":if kind==LayerType::Animation {"renamed authored descriptor;not native Sakura art"} else {"actual supplied artwork"}}));
        }
    }
    // Invalid Animation modes retain their deferred null factory, without
    // requesting an atlas or rejecting a missing texture ahead of publication.
    for mode in [-1, 6] {
        let mut io = LayeredAssetIo::new();
        io.try_with_rom(std::env::var_os("SHIN_REFERENCE_ROM").unwrap())
            .unwrap();
        let server = Arc::new(AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: crate::cli::Game::Umineko,
                higurashi_bustup_cache: Default::default(),
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ));
        let scenario: Arc<Scenario> = server.load_sync("/main.snr").unwrap();
        let result = shin_tasks::block_on(UserLayer::prepare_checked(
            &device,
            &server,
            &manager,
            &scenario,
            Default::default(),
            LayerType::Animation,
            (mode, 0, 0, 0, 0, 0, 0, 0),
        ))
        .unwrap();
        assert!(matches!(result,PreparedUserLayer::InvalidAnimation(value) if value==mode));
        assert!(
            !server
                .reference_requested_paths()
                .iter()
                .any(|p| p == "/effects.txa")
        );
        cases.push(serde_json::json!({"layer":"Animation","mode":mode,"profile":"invalid-mode-no-resource-control"}));
    }
    std::fs::write(output.join("particle-artwork-recovery.json"),serde_json::to_vec_pretty(&serde_json::json!({"cases":cases,"cases_passed":cases.len(),"patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),"window_created":false,"working_progress_written":false,"audio_device":"default output","scope":"Actual prepare_checked/load_checked producers and singleton replacement,not full ADV error-notice dispatch"})).unwrap()).unwrap();
}
fn render(
    layer: &mut UserLayer,
    ctx: &mut crate::render::PreRenderContext,
    server: &Arc<AssetServer>,
) {
    match layer {
        UserLayer::FocusLine(l) => l.assert_reference_render(ctx, server),
        UserLayer::Rain(l) => l.assert_reference_render(ctx, server),
        UserLayer::Animation(_) => {
            let borrow = shin_tasks::block_on(server.effects_atlas()).unwrap();
            borrow.read().required("Sakura");
        }
        _ => unreachable!(),
    }
}
fn replace_name(bytes: &mut [u8], name: &str, replacement: &str) {
    let count = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let mut offset = 32;
    for _ in 0..count {
        offset = (offset + 3) & !3;
        let length = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize;
        let start = offset + 20;
        let end = start
            + bytes[start..offset + length]
                .iter()
                .position(|b| *b == 0)
                .unwrap();
        if &bytes[start..end] == name.as_bytes() {
            // Parsing advances through the zero-terminated name, then aligns
            // to four bytes. Keep that boundary as well as entry offsets.
            if replacement == "!absent" {
                bytes[start] = b'!';
            } else {
                let next = (end + 1 + 3) & !3;
                assert!((start + replacement.len() + 1 + 3) & !3 == next);
                bytes[start..next].fill(0);
                bytes[start..start + replacement.len()].copy_from_slice(replacement.as_bytes());
            }
            return;
        }
        offset += length;
    }
    panic!("Missing supplied descriptor {name}");
}
