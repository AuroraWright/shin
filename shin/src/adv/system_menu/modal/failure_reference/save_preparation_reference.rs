//! Actual menu requests and SaveGame failure callbacks, not replaced UI models.
use super::*;
use crate::adv::{config::ConfigPhase, dialog::view::DialogTextures, save_menu::SaveMenuPhase};
use shin_core::format::save::{GameData, SaveData};

#[test]
#[ignore = "requires supplied ROM, Metal and exact Loopback Audio; temporary saves/assets only"]
fn reference_menu_save_preparation_failures_keep_owners_and_retry() {
    std::thread::Builder::new()
        .name("menu-save-preparation".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run_callers)
        .unwrap()
        .join()
        .unwrap();
}

fn open_child<'a>(
    adv: &mut Adv,
    server: &'a Arc<AssetServer>,
    ctx: &mut PreRenderContext<'a, '_, '_, '_>,
    frame: &mut FrameId,
    item: u8,
) {
    settle_parent(adv, server, ctx, frame);
    let row = adv
        .system_menu
        .as_ref()
        .unwrap()
        .screen
        .model
        .items()
        .iter()
        .position(|&id| id == item)
        .unwrap();
    while adv.system_menu.as_ref().unwrap().screen.model.selected() != row {
        step(adv, server, ctx, frame, 0.0, Some(AppAction::AnyDown));
    }
    activate(adv, server, ctx, frame);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let menu = adv.system_menu.as_ref().unwrap();
        let loaded = if item == 6 {
            menu.reference_title_return()
                .is_some_and(|dialog| dialog.reference_is_open())
        } else {
            ready(menu)
        };
        if loaded {
            break;
        }
        assert!(Instant::now() < deadline, "Child {item} not ready");
        step(adv, server, ctx, frame, 1.0, None);
        std::thread::sleep(Duration::from_millis(1));
    }
    step(adv, server, ctx, frame, 22.0, None);
    step(adv, server, ctx, frame, 0.0, None);
}

fn start_request<'a>(
    name: &str,
    adv: &mut Adv,
    server: &'a Arc<AssetServer>,
    ctx: &mut PreRenderContext<'a, '_, '_, '_>,
    frame: &mut FrameId,
) {
    if name == "config" {
        step(adv, server, ctx, frame, 0.0, Some(AppAction::Cancel));
        return;
    }
    if name != "title-return" {
        step(
            adv,
            server,
            ctx,
            frame,
            0.0,
            Some(if name == "delete" {
                AppAction::DeleteSave
            } else {
                AppAction::Enter
            }),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while !adv
            .system_menu
            .as_mut()
            .unwrap()
            .reference_save_menu()
            .unwrap()
            .reference_dialog_is_open()
        {
            assert!(Instant::now() < deadline, "Save confirmation not open");
            step(adv, server, ctx, frame, 1.0, None);
        }
    }
    step(adv, server, ctx, frame, 0.0, Some(AppAction::Left));
    step(adv, server, ctx, frame, 0.0, Some(AppAction::Enter));
}

fn recovered(adv: &mut Adv, name: &str) -> bool {
    let menu = adv.system_menu.as_mut().unwrap();
    match name {
        "config" => menu
            .reference_config_if_any()
            .is_some_and(|c| c.model().unwrap().phase() == ConfigPhase::Active),
        "title-return" => menu.child.is_none(),
        _ => menu.reference_save_menu().unwrap().model().phase() == SaveMenuPhase::Active,
    }
}

fn slot_bytes(data: &SaveData) -> Vec<u8> {
    let mut only_slots = SaveData::default();
    only_slots.manual_save_slots = data.manual_save_slots.clone();
    only_slots.save_menu_position = data.save_menu_position;
    only_slots.encode()
}

fn run_callers() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_SAVE_CALLERS_OUTPUT").unwrap());
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
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Menu save preparation depth"),
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
    for (name, item) in [
        ("config", 2),
        ("save", 0),
        ("delete", 0),
        ("title-return", 6),
    ] {
        let file = directory.join("systex.txa");
        std::fs::write(&file, &system).unwrap();
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
        let mut ctx = PreRenderContext {
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
        let deadline = Instant::now() + Duration::from_secs(10);
        while adv.vm_state.save_position == 0 {
            assert!(
                Instant::now() < deadline,
                "Initial authored message did not start"
            );
            step(&mut adv, &server, &mut ctx, &mut frame, 0.0, None);
        }
        let mut old: GameData = adv.vm_state.snapshot_game_data();
        old.entry.random_seed = 99;
        let mut disk = SaveData::default();
        disk.save_menu_position = 37;
        disk.settings = adv.vm_state.settings.clone();
        disk.manual_save_slots[37] = Some(old.clone());
        disk.manual_save_slots[99] = Some(old);
        let disk = SaveData::decode(&disk.encode()).unwrap();
        let path = output.join(format!("{name}.dat"));
        std::fs::write(&path, disk.encode()).unwrap();
        adv.configure_save_file(path.clone(), None).unwrap();
        adv.vm_state.persist.set(28, 124);
        adv.vm_state.save_revision += 1;
        adv.adv_state.acquire_audio_pause();
        adv.adv_state.acquire_ui_hide();
        adv.pending_system_menu = true;
        step(&mut adv, &server, &mut ctx, &mut frame, 0.0, None);
        open_child(&mut adv, &server, &mut ctx, &mut frame, item);
        if name == "config" {
            step(
                &mut adv,
                &server,
                &mut ctx,
                &mut frame,
                0.0,
                Some(AppAction::Right),
            );
        } else if name == "save" {
            step(
                &mut adv,
                &server,
                &mut ctx,
                &mut frame,
                0.0,
                Some(AppAction::AnyDown),
            );
            step(&mut adv, &server, &mut ctx, &mut frame, 4.0, None);
            assert_eq!(
                adv.system_menu
                    .as_mut()
                    .unwrap()
                    .reference_save_menu()
                    .unwrap()
                    .model()
                    .selected(),
                38
            );
        }
        let before_entry = serde_json::to_vec(&adv.reference_entry()).unwrap();
        let before_save = adv.save_menu_data().encode();
        let before_disk = std::fs::read(&path).unwrap();
        // For manual Save/Delete, prepare the confirmation but do not accept
        // it yet. Its live resources are intentionally preserved throughout.
        if name == "save" || name == "delete" {
            step(
                &mut adv,
                &server,
                &mut ctx,
                &mut frame,
                0.0,
                Some(if name == "delete" {
                    AppAction::DeleteSave
                } else {
                    AppAction::Enter
                }),
            );
            let deadline = Instant::now() + Duration::from_secs(5);
            while !adv
                .system_menu
                .as_mut()
                .unwrap()
                .reference_save_menu()
                .unwrap()
                .reference_dialog_is_open()
            {
                assert!(Instant::now() < deadline);
                step(&mut adv, &server, &mut ctx, &mut frame, 1.0, None);
            }
        }
        let mut bad = system.clone();
        let at = bad
            .windows(b"dialog_frame\0".len())
            .position(|b| b == b"dialog_frame\0")
            .unwrap();
        bad[at + 11] = b'x';
        std::fs::write(&file, bad).unwrap();
        let warm = name != "config";
        if warm {
            let one = server.load_sync::<DialogTextures>("/systex.txa").unwrap();
            let two = server.load_sync::<DialogTextures>("/systex.txa").unwrap();
            assert!(
                Arc::ptr_eq(&one, &two),
                "Live confirmation lost its cached atlas"
            );
            // Remove only a weak lookup entry. Owners and their real loaded
            // resources remain intact; this forces the fresh-load error branch.
            server.reference_forget_cached_asset::<DialogTextures>("/systex.txa");
        }
        if name == "save" || name == "delete" {
            step(
                &mut adv,
                &server,
                &mut ctx,
                &mut frame,
                0.0,
                Some(AppAction::Left),
            );
            step(
                &mut adv,
                &server,
                &mut ctx,
                &mut frame,
                0.0,
                Some(AppAction::Enter),
            );
        } else {
            start_request(name, &mut adv, &server, &mut ctx, &mut frame);
        }
        // Cross the close flash before checking recovery, so Config's initial
        // Active state cannot accidentally count as a failed-save callback.
        step(&mut adv, &server, &mut ctx, &mut frame, 7.0, None);
        step(&mut adv, &server, &mut ctx, &mut frame, 0.0, None);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !recovered(&mut adv, name) {
            assert!(
                Instant::now() < deadline,
                "{name}: preparation error left caller waiting"
            );
            assert!(
                adv.pending_save.is_none(),
                "{name}: bad atlas unexpectedly prepared save"
            );
            step(&mut adv, &server, &mut ctx, &mut frame, 1.0, None);
        }
        assert!(adv.pending_save.is_none() && adv.pending_title_setup().is_none());
        assert_eq!(
            (adv.adv_state.ui_hide_count, adv.adv_state.audio_pause_count),
            (2, 2)
        );
        assert_eq!(
            serde_json::to_vec(&adv.reference_entry()).unwrap(),
            before_entry
        );
        assert_eq!(adv.save_menu_data().encode(), before_save);
        assert_eq!(std::fs::read(&path).unwrap(), before_disk);
        if name == "save" || name == "delete" {
            let menu = adv
                .system_menu
                .as_mut()
                .unwrap()
                .reference_save_menu()
                .unwrap()
                .model();
            assert_eq!(menu.remembered(), 37);
            assert_eq!(slot_bytes(&adv.save_menu_data()), slot_bytes(&disk));
        }
        let error = match server.load_sync::<DialogTextures>("/systex.txa") {
            Err(e) => format!("{e:#}"),
            Ok(_) => panic!("Failed atlas was published"),
        };
        assert!(error.contains("dialog_framx"));
        std::fs::write(&file, &system).unwrap();
        if name == "title-return" {
            open_child(&mut adv, &server, &mut ctx, &mut frame, 6);
        }
        start_request(name, &mut adv, &server, &mut ctx, &mut frame);
        let deadline = Instant::now() + Duration::from_secs(8);
        while adv.pending_save.is_none() {
            assert!(
                Instant::now() < deadline,
                "{name}: repaired save did not start"
            );
            step(&mut adv, &server, &mut ctx, &mut frame, 1.0, None);
        }
        while adv.pending_save.is_some() {
            assert!(
                Instant::now() < deadline,
                "{name}: repaired save did not retire"
            );
            step(&mut adv, &server, &mut ctx, &mut frame, 1.0, None);
            std::thread::sleep(Duration::from_millis(2));
        }
        let saved = SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.persist_data.get(28), 124);
        let only99 = |save: &SaveData| {
            let mut one = SaveData::default();
            one.manual_save_slots[99] = save.manual_save_slots[99].clone();
            one.encode()
        };
        assert_eq!(only99(&saved), only99(&disk));
        match name {
            "config" => assert!(saved.settings.serialized_eq(&adv.vm_state.settings)),
            "save" => {
                assert_eq!(saved.save_menu_position, 38);
                assert!(saved.manual_save_slots[38].is_some());
            }
            "delete" => {
                assert_eq!(saved.save_menu_position, 37);
                assert!(saved.manual_save_slots[37].is_none());
            }
            "title-return" => assert!(adv.pending_title_setup().is_some()),
            _ => unreachable!(),
        }
        cases.push(serde_json::json!({"caller":name,"error":error,"warm_cache_control":warm,
            "fixture_only_cache_invalidation":warm,"memory_disk_and_foreign_modal_scopes_preserved":true,
            "caller_waiting_state_released":true,"same_owner_repaired_retry":true,"slot99_preserved":true}));
    }
    std::fs::write(output.join("save-caller-recovery.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "cases":cases,"profiles_passed":cases.len(),"audio_device":"exact Loopback Audio",
        "patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),"window_created":false,
        "working_progress_written":false,"authored_asset_corruption_claim":false
    })).unwrap()).unwrap();
}
