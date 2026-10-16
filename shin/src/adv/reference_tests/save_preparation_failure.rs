//! Reach the actual end-of-scenario SaveGame callback with damaged UI assets.
use crate::adv::*;
use crate::{
    asset::system::{
        AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache, supplied_payload,
    },
    render::PreRenderContext,
};
use shin_core::{primitives::update::FrameId, time::Ticks};
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::BytesAddress,
        texture::{DepthStencilTarget, TextureSamplerStore},
    },
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires supplied ROM, Metal and exact Loopback Audio; isolated temporary saves/assets"]
fn reference_exit_save_dialog_failure_preserves_progress_and_retries() {
    std::thread::Builder::new()
        .name("save-preparation-reference".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}

fn run() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_SAVE_UI_OUTPUT").unwrap());
    let directory = output.join("overrides");
    std::fs::create_dir_all(&directory).unwrap();
    let system = supplied_payload("/systex.txa").2;
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
    let make_server = || {
        let mut io = LayeredAssetIo::new();
        io.try_with_dir(&directory).unwrap();
        if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
            io.try_with_rom(patch).unwrap();
        }
        io.try_with_rom(std::env::var_os("SHIN_REFERENCE_ROM").unwrap())
            .unwrap();
        Arc::new(AssetServer::new(
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
        ))
    };
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Save preparation reference depth"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::DEPTH_STENCIL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&Default::default());
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), shin_render::TEXTURE_FORMAT);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());
    let file = directory.join("systex.txa");
    let mut cases = Vec::new();
    for profile in ["missing-dialog-frame", "short-system-atlas"] {
        std::fs::write(&file, &system).unwrap();
        let server = make_server();
        let assets = shin_tasks::block_on(AdvAssets::load(&server)).unwrap();
        let mut adv = Adv::new(
            manager.clone(),
            server.clone(),
            assets.clone(),
            Scripter::new(&assets.scenario, 0, 42),
        );
        let path = output.join(format!("exit-{profile}.dat"));
        let mut disk = shin_core::format::save::SaveData::default();
        disk.save_menu_position = 99;
        disk.manual_save_slots[99] = Some(VmState::for_scenario(19, 99).snapshot_game_data());
        let disk = shin_core::format::save::SaveData::decode(&disk.encode()).unwrap();
        std::fs::write(&path, disk.encode()).unwrap();
        adv.configure_save_file(path.clone(), None).unwrap();
        adv.vm_state.persist.set(28, 124);
        adv.vm_state.save_revision += 1;
        adv.finished = true;
        let before_disk = std::fs::read(&path).unwrap();
        let before_save = adv.save_menu_data().encode();
        let before_entry = serde_json::to_vec(&adv.reference_entry()).unwrap();
        let mut bad = system.clone();
        if profile == "missing-dialog-frame" {
            let at = bad
                .windows(b"dialog_frame\0".len())
                .position(|w| w == b"dialog_frame\0")
                .unwrap();
            bad[at + 11] = b'x';
        } else {
            bad.truncate(15);
        }
        std::fs::write(&file, bad).unwrap();
        let mut context = PreRenderContext {
            device: &device,
            queue: &queue,
            breakup_texture: None,
            resize_source: &resize,
            sampler_store: &samplers,
            depth_stencil: DepthStencilTarget { view: &depth_view },
            pipeline_storage: &mut pipelines,
            dynamic_buffer: &mut dynamic,
            encoder: &mut encoder,
        };
        let mut frame = FrameId::default();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            step(&mut adv, &server, &mut context, &mut frame, None)
        }));
        assert!(
            result.is_ok(),
            "{profile}: actual ADV exit save callback panicked"
        );
        assert!(adv.pending_save.is_none() && !adv.exit_save_finished);
        assert!(adv.pending_title_setup().is_none());
        assert_eq!(adv.save_menu_data().encode(), before_save);
        assert_eq!(std::fs::read(&path).unwrap(), before_disk);
        assert_eq!(
            serde_json::to_vec(&adv.reference_entry()).unwrap(),
            before_entry
        );
        let error = adv
            .restore_error
            .clone()
            .expect("Preparation failure not recorded");
        for _ in 0..3 {
            step(&mut adv, &server, &mut context, &mut frame, None);
        }
        assert_eq!(adv.restore_error.as_ref(), Some(&error));
        assert!(adv.pending_save.is_none());
        std::fs::write(&file, &system).unwrap();
        assert!(
            adv.restore_error_dialog
                .as_ref()
                .unwrap()
                .reference_uses_plain_frame()
        );
        for _ in 0..30 {
            step_delta(&mut adv, &server, &mut context, &mut frame, None, 1.0);
        }
        step(
            &mut adv,
            &server,
            &mut context,
            &mut frame,
            Some(AppAction::Enter),
        );
        while adv.restore_error_dialog.is_some() {
            step_delta(&mut adv, &server, &mut context, &mut frame, None, 1.0);
        }
        assert!(
            adv.pending_save.is_none(),
            "Acknowledgement frame stays consumed"
        );
        step(&mut adv, &server, &mut context, &mut frame, None);
        assert!(
            adv.pending_save.is_some(),
            "{profile}: repaired data retry did not start"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !adv.exit_save_finished {
            assert!(Instant::now() < deadline, "{profile}: save retry stalled");
            frame.advance();
            adv.update(
                &mut UpdateContext {
                    frame_started_at: Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(1.0),
                    asset_server: &server,
                    pre_render: &mut context,
                },
                EnumMap::default(),
            );
            let encoder = std::mem::replace(
                context.encoder,
                device.create_command_encoder(&Default::default()),
            );
            let mut upload = device.create_command_encoder(&Default::default());
            context.dynamic_buffer.finish(&mut upload);
            queue.submit([upload.finish(), encoder.finish()]);
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("GPU work must complete before readback or retirement");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(adv.pending_title_setup().is_some());
        let saved =
            shin_core::format::save::SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.persist_data.get(28), 124);
        assert_eq!(saved.save_menu_position, 99);
        assert_eq!(
            format!("{:?}", saved.manual_save_slots[99]),
            format!("{:?}", disk.manual_save_slots[99])
        );
        cases.push(serde_json::json!({"profile":profile,"error":error,"no_partial_save":true,
            "same_scene_preserved":true,"no_unacknowledged_title_return":true,"same_owner_repaired_data_retry":true}));
    }
    std::fs::write(output.join("save-preparation-recovery.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "cases":cases,"audio_device":"exact Loopback Audio","patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),
        "window_created":false,"working_progress_written":false,"authored_route_claim":false
    })).unwrap()).unwrap();
}

fn step<'a>(
    adv: &mut Adv,
    server: &'a Arc<AssetServer>,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    frame: &mut FrameId,
    action: Option<AppAction>,
) {
    step_delta(adv, server, context, frame, action, 0.0);
}

fn step_delta<'a>(
    adv: &mut Adv,
    server: &'a Arc<AssetServer>,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    frame: &mut FrameId,
    action: Option<AppAction>,
    ticks: f32,
) {
    let mut input = EnumMap::default();
    if let Some(action) = action {
        input[action] = ActionState {
            is_clicked: true,
            is_clicked_or_repeated: true,
            ..Default::default()
        };
    }
    frame.advance();
    adv.update(
        &mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: *frame,
            delta_ticks: Ticks::from_f32(ticks),
            asset_server: server,
            pre_render: context,
        },
        input,
    );
}
