//! Exercise real temporary storage errors through FirstBoot and its scene owner.
use super::*;
use crate::{
    asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache},
    render::PreRenderContext,
};
use shin_core::{format::save::SaveData, primitives::update::FrameId, time::Ticks};
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::BytesAddress,
        texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
    },
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires supplied ROM, Metal and the default audio output; isolated temporary saves only"]
fn reference_startup_storage_failure_visible_and_retry_preserves_save() {
    run_startup_storage_failure(false);
}

pub(super) fn run_startup_storage_failure(bad_notice: bool) {
    crate::init_test_task_pools();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
            adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
        } else {
            wgpu::Features::empty()
        },
        ..Default::default()
    }))
    .unwrap();
    let mut io = LayeredAssetIo::new();
    let _notice_fixture =
        bad_notice.then(|| super::notice_failure_reference::override_notice(&mut io));
    if let Ok(patch) = std::env::var("SHIN_REFERENCE_PATCH_ROM") {
        io.try_with_rom(patch).unwrap();
    }
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
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
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Startup storage failure reference"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Startup storage failure depth"),
        size: target.size(),
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
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
            let _ = fs::set_permissions(
                self.0.join("progress.dat"),
                fs::Permissions::from_mode(0o600),
            );
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let directory =
        std::env::temp_dir().join(format!("shin-startup-read-failure-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let _cleanup = Cleanup(directory.clone());
    let path = directory.join("progress.dat");
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut frame = FrameId::default();
    fn step<'a>(
        host: &mut SceneHost,
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut FrameId,
        action: Option<AppAction>,
        pointer: glam::Vec2,
        held: bool,
        extent: (u16, u16),
    ) {
        frame.advance();
        let mut input = EnumMap::default();
        if let Some(action) = action {
            input[action] = ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
        }
        input[AppAction::PointerClick].is_held = held;
        host.update(
            &mut UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: *frame,
                delta_ticks: Ticks::from_f32(1.0),
                asset_server: server,
                pre_render: context,
            },
            input,
            pointer * glam::Vec2::new(extent.0 as f32 / 1920.0, extent.1 as f32 / 1080.0),
            pointer,
            extent,
            0.0,
            1.0 / 60.0,
        )
        .expect("Filesystem read failure must remain recoverable in its boot owner");
    }
    fn retry_button(host: &SceneHost) -> Option<glam::Vec2> {
        let Some(Scene::Boot(boot)) = &host.scene else {
            return None;
        };
        boot.scene.reference_retry_button()
    }
    for (mouse, extent) in [(false, (1280, 720)), (true, (3840, 2160))] {
        let mut save = SaveData::default();
        save.play_seconds = 123;
        save.settings.v0_bgmvol = 17;
        save.settings.v3_sysvol = 0;
        save.save_menu_position = 99;
        save.manual_save_slots[99] =
            Some(crate::adv::VmState::for_scenario(19, 99).snapshot_game_data());
        let encoded = save.encode();
        // Native save timestamps retain seconds, not the constructor's
        // subsecond clock. Compare with what was actually written to disk.
        let save = SaveData::decode(&encoded).unwrap();
        fs::write(&path, &encoded).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        let error = fs::read(&path).unwrap_err();
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::PermissionDenied,
            "Fixture must exercise a real unreadable save"
        );
        let mut host = SceneHost::new_first_boot(
            GlobalAudio::new(manager.clone()),
            server.clone(),
            Some(path.clone()),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while retry_button(&host).is_none() {
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                None,
                glam::Vec2::ZERO,
                true,
                extent,
            );
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        for _ in 0..30 {
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                None,
                glam::Vec2::ZERO,
                true,
                extent,
            );
        }
        assert!(host.error.is_none() && host.pending.is_none());
        assert!(host.common.is_some());
        if bad_notice {
            assert!(
                matches!(&host.scene, Some(Scene::Boot(boot)) if boot.scene.reference_uses_plain_notice())
            );
        }
        let view = target.create_view(&Default::default());
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Visible startup read error",
            );
            host.render(&mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        if bad_notice {
            super::notice_failure_reference::assert_notice_pixels(
                &pixels,
                &format!("startup-read-{mouse}"),
            );
        }
        assert!(
            pixels
                .chunks_exact(4)
                .filter(|p| p[0] > 32 || p[1] > 32 || p[2] > 32)
                .count()
                > 1000,
            "Read failure must render a visible dialog, not permanent black"
        );
        let center = retry_button(&host).unwrap();
        // The held press which preceded the modal cannot dismiss it on release.
        step(
            &mut host,
            &mut context,
            &server,
            &mut frame,
            None,
            center,
            false,
            extent,
        );
        step(
            &mut host,
            &mut context,
            &server,
            &mut frame,
            Some(AppAction::Cancel),
            center,
            false,
            extent,
        );
        for _ in 0..40 {
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                None,
                center,
                false,
                extent,
            );
        }
        assert!(retry_button(&host).is_some() && host.pending.is_none());
        // Retry once while access is still denied; the owner shows the same
        // modal again instead of spinning, completing, or attaching defaults.
        step(
            &mut host,
            &mut context,
            &server,
            &mut frame,
            Some(AppAction::Enter),
            center,
            false,
            extent,
        );
        while retry_button(&host).is_some() {
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                None,
                center,
                false,
                extent,
            );
            assert!(Instant::now() < deadline);
        }
        while retry_button(&host).is_none() {
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                None,
                center,
                false,
                extent,
            );
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), encoded);
        assert!(host.reference_save().manual_save_slots[99].is_none());
        for _ in 0..30 {
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                None,
                center,
                false,
                extent,
            );
        }
        if mouse {
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                None,
                center,
                true,
                extent,
            );
            assert!(retry_button(&host).is_some() && host.pending.is_none());
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                None,
                center,
                false,
                extent,
            );
        } else {
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                Some(AppAction::Enter),
                center,
                false,
                extent,
            );
        }
        while host.pending.is_none() {
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                None,
                center,
                false,
                extent,
            );
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(matches!(host.pending, Some(Destination::Demo)));
        assert!(
            matches!(host.scene, Some(Scene::Boot(_))),
            "Demo is only installed on the following frame"
        );
        let loaded = host.reference_save();
        assert_eq!(loaded.settings.v0_bgmvol, 17);
        assert_eq!(loaded.save_menu_position, 99);
        assert_eq!(
            format!("{:?}", loaded.manual_save_slots[99]),
            format!("{:?}", save.manual_save_slots[99])
        );
        assert_eq!(fs::read(&path).unwrap(), encoded);
        // No movie/window startup is needed to prove recovery. Drop may write
        // desktop elapsed time, which is deliberately outside the read check.
        drop(host);
    }
    // Corrupt/incompatible notices retain native one-button/Cancel behavior,
    // but publish defaults only after final closure and preserving the source.
    for mode in 0..6 {
        let original = if mode == 2 {
            let mut raw = SaveData::default().encode_raw();
            raw[0] = 2;
            SaveData::obfuscate(&raw)
        } else {
            vec![1, 2, 3]
        };
        fs::write(&path, &original).unwrap();
        let before: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        let extent = if mode == 2 { (3840, 2160) } else { (1280, 720) };
        let mut host = SceneHost::new_first_boot(
            GlobalAudio::new(manager.clone()),
            server.clone(),
            Some(path.clone()),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while retry_button(&host).is_none() {
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                None,
                glam::Vec2::ZERO,
                true,
                extent,
            );
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let Some(Scene::Boot(boot)) = &host.scene else {
            unreachable!()
        };
        assert_eq!(
            boot.scene.reference_recovery_kind(),
            Some(if mode == 2 {
                crate::adv::SaveRecoveryKind::Incompatible
            } else {
                crate::adv::SaveRecoveryKind::Corrupt
            })
        );
        for _ in 0..30 {
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                None,
                glam::Vec2::ZERO,
                true,
                extent,
            );
        }
        let center = retry_button(&host).unwrap();
        step(
            &mut host,
            &mut context,
            &server,
            &mut frame,
            None,
            center,
            false,
            extent,
        );
        assert!(host.pending.is_none() && host.error.is_none());
        if bad_notice {
            assert!(
                matches!(&host.scene, Some(Scene::Boot(boot)) if boot.scene.reference_uses_plain_notice())
            );
        }
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), before.len());
        let view = target.create_view(&Default::default());
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Visible native save recovery notice",
            );
            host.render(&mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        if bad_notice {
            super::notice_failure_reference::assert_notice_pixels(
                &pixels,
                &format!("startup-recovery-{mode}"),
            );
        }
        assert!(
            pixels
                .chunks_exact(4)
                .filter(|p| p[0] > 32 || p[1] > 32 || p[2] > 32)
                .count()
                > 1000
        );
        if mode == 5 {
            drop(host);
            assert_eq!(fs::read(&path).unwrap(), original);
            assert_eq!(fs::read_dir(&directory).unwrap().count(), before.len());
            continue;
        }
        let mut replacement = SaveData::default();
        replacement.settings.v0_bgmvol = 29;
        replacement.manual_save_slots[99] =
            Some(crate::adv::VmState::for_scenario(19, 99).snapshot_game_data());
        let replacement_bytes = replacement.encode();
        if mode == 3 {
            fs::write(&path, &replacement_bytes).unwrap();
        }
        if mode == 4 {
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o555)).unwrap();
        }
        if mode == 2 {
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                None,
                center,
                true,
                extent,
            );
            assert!(host.pending.is_none());
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                None,
                center,
                false,
                extent,
            );
        } else {
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                Some(if mode == 1 {
                    AppAction::Cancel
                } else {
                    AppAction::Enter
                }),
                center,
                false,
                extent,
            );
        }
        while retry_button(&host).is_some()
            && (mode != 4
                || matches!(&host.scene, Some(Scene::Boot(boot)) if boot.scene.reference_recovery_kind().is_some()))
        {
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                None,
                center,
                false,
                extent,
            );
            assert!(Instant::now() < deadline);
        }
        if mode == 4 {
            while retry_button(&host).is_none() {
                step(
                    &mut host,
                    &mut context,
                    &server,
                    &mut frame,
                    None,
                    center,
                    false,
                    extent,
                );
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(host.pending.is_none() && host.error.is_none());
            let Some(Scene::Boot(boot)) = &host.scene else {
                unreachable!()
            };
            assert_eq!(
                boot.scene.reference_recovery_kind(),
                None,
                "Backup failure must retain a storage retry instead of resetting"
            );
            if bad_notice {
                assert!(boot.scene.reference_uses_plain_notice());
                println!("Malformed notice: backup failure retains retry and original bytes");
            }
            assert_eq!(fs::read(&path).unwrap(), original);
            assert_eq!(fs::read_dir(&directory).unwrap().count(), before.len());
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
            for _ in 0..30 {
                step(
                    &mut host,
                    &mut context,
                    &server,
                    &mut frame,
                    None,
                    center,
                    false,
                    extent,
                );
            }
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                Some(AppAction::Enter),
                center,
                false,
                extent,
            );
            while retry_button(&host).is_some() {
                step(
                    &mut host,
                    &mut context,
                    &server,
                    &mut frame,
                    None,
                    center,
                    false,
                    extent,
                );
                assert!(Instant::now() < deadline);
            }
            while retry_button(&host).is_none() {
                step(
                    &mut host,
                    &mut context,
                    &server,
                    &mut frame,
                    None,
                    center,
                    false,
                    extent,
                );
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            for _ in 0..30 {
                step(
                    &mut host,
                    &mut context,
                    &server,
                    &mut frame,
                    None,
                    center,
                    false,
                    extent,
                );
            }
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                Some(AppAction::Enter),
                center,
                false,
                extent,
            );
        }
        while host.pending.is_none() {
            step(
                &mut host,
                &mut context,
                &server,
                &mut frame,
                None,
                center,
                false,
                extent,
            );
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(matches!(host.pending, Some(Destination::Demo)));
        assert!(matches!(host.scene, Some(Scene::Boot(_))));
        let loaded = host.reference_save();
        let added: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| !before.contains(path))
            .collect();
        if mode == 3 {
            assert_eq!(loaded.settings.v0_bgmvol, 29);
            assert!(loaded.manual_save_slots[99].is_some());
            assert!(added.is_empty());
            assert_eq!(fs::read(&path).unwrap(), replacement_bytes);
        } else {
            assert_eq!(loaded.encode_raw(), SaveData::default().encode_raw());
            assert_eq!(added.len(), 1);
            assert_eq!(fs::read(&added[0]).unwrap(), original);
            assert_eq!(
                fs::read(&path).unwrap(),
                original,
                "Memory reset must not prematurely write progress"
            );
        }
        drop(host);
    }
    // Decode failures remain distinguishable from storage access failures;
    // never turn a corrupt destination into a default write/retry silently.
    let mut unsupported = SaveData::default().encode_raw();
    unsupported[0] = 2;
    for bytes in [
        vec![],
        vec![1],
        vec![1, 2],
        vec![1, 2, 3],
        SaveData::obfuscate(&unsupported),
        vec![0; 8],
    ] {
        fs::write(&path, &bytes).unwrap();
        let error = GlobalData::open(Some(path.clone()), None).err().unwrap();
        assert!(!GlobalData::is_save_read_error(&error));
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}
