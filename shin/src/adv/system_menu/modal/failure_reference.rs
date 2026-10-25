//! Failure/retry through ADV's actual parent-menu callback, using temporary art.
use super::*;
use crate::{
    adv::{Adv, assets::AdvAssets},
    asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache, supplied_payload},
    render::PreRenderContext,
    update::UpdateContext,
};
use shin_core::{primitives::update::FrameId, vm::Scripter};
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

mod save_preparation_reference;

#[test]
#[ignore = "requires supplied ROM, Metal and the default audio output; temporary artwork only"]
fn reference_system_menu_child_failure_preserves_parent_and_retries() {
    std::thread::Builder::new()
        .name("menu-recovery".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}

fn step<'a>(
    adv: &mut Adv,
    server: &'a Arc<AssetServer>,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    frame: &mut FrameId,
    ticks: f32,
    action: Option<AppAction>,
) {
    let mut input = EnumMap::default();
    if let Some(action) = action {
        input[action] = ActionState {
            is_clicked: true,
            is_clicked_or_repeated: true,
            ..Default::default()
        };
    }
    let point = glam::vec2(1900.0, 1050.0);
    adv.sample_modal_pointer(point, (1920, 1080), false, ticks as f64 / 60.0);
    adv.handle_pointer_state(point, false, false, ticks as f64 / 60.0);
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
    let encoder = std::mem::replace(
        context.encoder,
        context.device.create_command_encoder(&Default::default()),
    );
    let mut upload = context.device.create_command_encoder(&Default::default());
    context.dynamic_buffer.finish(&mut upload);
    context.queue.submit([upload.finish(), encoder.finish()]);
    context
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU work must complete before readback or retirement");
}
fn ready(menu: &SystemMenuModal) -> bool {
    match &menu.child {
        Some(Child::Config(c)) => c.model().is_some(),
        Some(Child::Manual(c)) => c.model().is_some(),
        Some(Child::Tips(c)) => c.model().is_some(),
        Some(Child::SaveMenu(c)) => {
            c.model().phase() != crate::adv::save_menu::SaveMenuPhase::Loading
        }
        Some(Child::Characters(_)) => true,
        _ => false,
    }
}
fn activate<'a>(
    adv: &mut Adv,
    server: &'a Arc<AssetServer>,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    frame: &mut FrameId,
) {
    step(adv, server, context, frame, 0.0, Some(AppAction::Enter));
    step(adv, server, context, frame, 7.0, None);
    step(adv, server, context, frame, 0.0, None);
}
fn settle_parent<'a>(
    adv: &mut Adv,
    server: &'a Arc<AssetServer>,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    frame: &mut FrameId,
) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while adv.system_menu.as_ref().unwrap().screen.model.phase() != SystemMenuPhase::Active {
        assert!(
            Instant::now() < deadline,
            "parent rows did not finish their queued retreat/reopen"
        );
        step(adv, server, context, frame, 22.0, None);
        step(adv, server, context, frame, 0.0, None);
    }
}
fn run() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_MENU_OUTPUT").unwrap());
    let directory = output.join("overrides");
    std::fs::create_dir_all(&directory).unwrap();
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
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Menu recovery depth"),
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
    let mut cases = Vec::new();
    let mut command_cases = Vec::new();
    for (name, item, path) in [
        ("config", 2, "/config.txa"),
        ("manual", 5, "/manual.txa"),
        ("tips", 4, "/tips.txa"),
        ("save", 0, "/saveload.txa"),
        ("load", 1, "/saveload.txa"),
        ("characters", 3, "/chars.txa"),
        ("character-scrollbar", 3, "/systex.txa"),
    ] {
        let bytes = supplied_payload(path).2;
        let file = directory.join(&path[1..]);
        std::fs::write(&file, &bytes).unwrap();
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
        let assets = shin_tasks::block_on(AdvAssets::load(&server)).unwrap();
        let mut adv = Adv::new(
            manager.clone(),
            server.clone(),
            assets.clone(),
            Scripter::new(&assets.scenario, 0, 42),
        );
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
        // Establish a live message checkpoint before measuring preservation.
        // A fresh, unstarted VM would legitimately execute on normal menu close.
        let deadline = Instant::now() + Duration::from_secs(10);
        while adv.vm_state.save_position == 0 {
            assert!(
                Instant::now() < deadline,
                "initial story checkpoint did not start"
            );
            step(&mut adv, &server, &mut context, &mut frame, 0.0, None);
        }
        adv.vm_state.information.character_grid = 0;
        adv.vm_state.information.character_grid_argument = 0;
        let tip = assets
            .scenario
            .info_tables()
            .tips_info
            .iter()
            .position(|t| t.episode == 0)
            .unwrap();
        adv.vm_state.information.acquired_tips = vec![tip as i32];
        // Nested scopes belong to a different owner and must survive cancellation.
        adv.adv_state.acquire_audio_pause();
        adv.adv_state.acquire_ui_hide();
        let before_entry = serde_json::to_vec(&adv.reference_entry()).unwrap();
        let before_save = adv.reference_global_save().encode();
        let before_info = adv.vm_state.information.clone();
        let before_settings = serde_json::to_vec(&adv.vm_state.settings).unwrap();
        let before_control = adv.vm_state.system_controls_allowed;
        std::fs::write(&file, &bytes[..15]).unwrap();
        adv.pending_system_menu = true;
        step(&mut adv, &server, &mut context, &mut frame, 0.0, None);
        settle_parent(&mut adv, &server, &mut context, &mut frame);
        let row = adv
            .system_menu
            .as_ref()
            .unwrap()
            .screen
            .model
            .items()
            .iter()
            .position(|&v| v == item)
            .unwrap();
        for _ in 0..row {
            step(
                &mut adv,
                &server,
                &mut context,
                &mut frame,
                0.0,
                Some(AppAction::AnyDown),
            );
        }
        activate(&mut adv, &server, &mut context, &mut frame);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let menu = adv
                .system_menu
                .as_ref()
                .expect("failed child must retain parent");
            if menu.child.is_none() && menu.screen.model.phase() == SystemMenuPhase::Opening {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "{name}: failed child did not reopen parent"
            );
            step(&mut adv, &server, &mut context, &mut frame, 0.0, None);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            (adv.adv_state.ui_hide_count, adv.adv_state.audio_pause_count),
            (2, 2)
        );
        assert!(adv.pending_save.is_none() && adv.pending_restore.is_none());
        assert_eq!(
            serde_json::to_vec(&adv.reference_entry()).unwrap(),
            before_entry
        );
        assert_eq!(adv.reference_global_save().encode(), before_save);
        assert_eq!(adv.vm_state.information, before_info);
        assert_eq!(
            serde_json::to_vec(&adv.vm_state.settings).unwrap(),
            before_settings
        );
        assert_eq!(adv.vm_state.system_controls_allowed, before_control);
        let error = match server.load_sync::<CharacterTextures>(path) {
            Err(e) => format!("{e:#}"),
            Ok(_) => panic!("Malformed {path} loaded"),
        };
        std::fs::write(&file, &bytes).unwrap();
        settle_parent(&mut adv, &server, &mut context, &mut frame);
        assert_eq!(
            adv.system_menu.as_ref().unwrap().screen.model.selected(),
            row
        );
        activate(&mut adv, &server, &mut context, &mut frame);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready(adv.system_menu.as_ref().unwrap()) {
            assert!(
                Instant::now() < deadline,
                "{name}: valid retry did not load"
            );
            step(&mut adv, &server, &mut context, &mut frame, 0.0, None);
            std::thread::sleep(Duration::from_millis(1));
        }
        step(&mut adv, &server, &mut context, &mut frame, 22.0, None);
        step(&mut adv, &server, &mut context, &mut frame, 0.0, None);
        step(
            &mut adv,
            &server,
            &mut context,
            &mut frame,
            0.0,
            Some(AppAction::Cancel),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while adv.system_menu.as_ref().unwrap().child.is_some() {
            assert!(Instant::now() < deadline, "{name}: child did not close");
            step(&mut adv, &server, &mut context, &mut frame, 12.0, None);
        }
        settle_parent(&mut adv, &server, &mut context, &mut frame);
        assert_eq!(
            (adv.adv_state.ui_hide_count, adv.adv_state.audio_pause_count),
            (2, 2)
        );
        step(
            &mut adv,
            &server,
            &mut context,
            &mut frame,
            0.0,
            Some(AppAction::Cancel),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while adv.system_menu.is_some() {
            assert!(Instant::now() < deadline, "parent did not close");
            step(&mut adv, &server, &mut context, &mut frame, 22.0, None);
        }
        assert_eq!(
            (adv.adv_state.ui_hide_count, adv.adv_state.audio_pause_count),
            (1, 1)
        );
        assert_eq!(
            serde_json::to_vec(&adv.reference_entry()).unwrap(),
            before_entry
        );
        assert_eq!(adv.reference_global_save().encode(), before_save);
        assert_eq!(adv.vm_state.information, before_info);
        assert_eq!(
            serde_json::to_vec(&adv.vm_state.settings).unwrap(),
            before_settings
        );
        assert!(adv.pending_save.is_none() && adv.pending_restore.is_none());
        if name == "characters" {
            use crate::adv::command::{CommandStartResult, StartableCommand, UpdatableCommand};
            use shin_core::vm::IntoRuntimeForm;
            for permission in [true, false] {
                std::fs::write(&file, &bytes[..15]).unwrap();
                adv.vm_state.system_controls_allowed = permission;
                let runtime = shin_core::vm::command::compiletime::SHOWCHARS {}
                    .into_runtime_form(&shin_core::vm::VmCtx::new(0, 42));
                let previous = runtime.apply_state(&mut adv.vm_state);
                let mut update = UpdateContext {
                    frame_started_at: Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::ZERO,
                    asset_server: &server,
                    pre_render: &mut context,
                };
                let CommandStartResult::Yield(mut command) = runtime.start(
                    &mut update,
                    &assets.scenario,
                    &adv.vm_state,
                    previous,
                    &mut adv.adv_state,
                ) else {
                    panic!("SHOWCHARS must retain command");
                };
                assert!(adv.adv_state.character_screen.is_none());
                assert!(!adv.vm_state.system_controls_allowed);
                assert!(
                    command
                        .update(
                            &mut update,
                            &assets.scenario,
                            &mut adv.vm_state,
                            &mut adv.adv_state,
                            false
                        )
                        .is_none()
                );
                let error = command
                    .take_error()
                    .expect("failed SHOWCHARS must report its error for the existing notice");
                assert!(format!("{error:#}").contains("/chars.txa"));
                assert!(command.take_error().is_none());
                std::fs::write(&file, &bytes).unwrap();
                assert!(
                    command
                        .update(
                            &mut update,
                            &assets.scenario,
                            &mut adv.vm_state,
                            &mut adv.adv_state,
                            false
                        )
                        .is_none()
                );
                assert!(command.take_error().is_none());
                let screen = adv.adv_state.character_screen.as_mut().unwrap();
                screen.update(Ticks::from_f32(12.0), &server);
                screen.update(Ticks::ZERO, &server);
                screen.model.request_close();
                for _ in 0..4 {
                    screen.update(Ticks::from_f32(12.0), &server);
                }
                assert!(screen.model.is_closed());
                assert!(
                    command
                        .update(
                            &mut update,
                            &assets.scenario,
                            &mut adv.vm_state,
                            &mut adv.adv_state,
                            false
                        )
                        .is_some()
                );
                assert!(adv.adv_state.character_screen.is_none());
                assert_eq!(adv.vm_state.system_controls_allowed, permission);
                assert_eq!(
                    serde_json::to_vec(&adv.reference_entry()).unwrap(),
                    before_entry
                );
                assert_eq!(adv.reference_global_save().encode(), before_save);
                command_cases.push(serde_json::json!({"previous_system_controls":permission,
                    "error":format!("{error:#}"),"pending_token_preserved":true,"same_command_retry":true,
                    "previous_permission_restored_on_normal_close":true,"story_and_progress_preserved":true}));
            }
        }
        std::fs::remove_file(file).unwrap();
        cases.push(serde_json::json!({"child":name,"asset":path,"profile":"short-header","error":error,
            "story_entry_preserved":true,"global_progress_preserved":true,"settings_preserved":true,
            "parent_and_nested_modal_scopes_preserved":true,"no_save_or_load_started":true,
            "same_parent_same_server_retry":true,"selected_row_retained":true,"normal_child_and_parent_close":true}));
    }
    std::fs::write(output.join("menu-recovery.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "cases_passed":cases.len(),"cases":cases,"showchars_cases_passed":command_cases.len(),"showchars_cases":command_cases,"patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),
        "window_created":false,"working_progress_written":false,"audio_device":"default output"
    })).unwrap()).unwrap();
    println!(
        "{} real ADV child failures preserve parent/story/progress and retry",
        cases.len()
    );
}
