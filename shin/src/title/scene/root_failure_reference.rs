//! Mandatory title publication, rendering and reopening after repaired data.
use super::*;
use crate::{
    adv::{GlobalData, VmState},
    asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache, supplied_payload},
};
use shin_core::format::save::SaveData;
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
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires supplied ROM and Metal;temporary assets/progress only;no audio playback"]
fn reference_title_root_artwork_publication_error_and_reopen() {
    std::thread::Builder::new()
        .name("title-root-recovery".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}

fn update(scene: &mut TitleScene, globals: &mut GlobalData, delta: f32) -> Result<()> {
    scene.update_with_globals(
        Ticks::from_f32(delta),
        &PointerFrame {
            number: 0,
            time_seconds: 0.0,
            events: &[],
        },
        &EnumMap::default(),
        globals,
        glam::Vec2::ZERO,
        false,
        0.0,
        0.0,
    )
}

fn run() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_TITLE_ROOT_OUTPUT").unwrap());
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
        "Title root depth",
        shin_render::DEPTH_STENCIL_FORMAT,
    ));
    let depth_view = depth.create_view(&Default::default());
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = device.create_texture(&descriptor("Title root target", format));
    let view = target.create_view(&Default::default());
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), format);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut context = PreRenderContext {
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
    let draw = |scene: &mut TitleScene, context: &mut PreRenderContext| {
        scene.prepare(context);
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::RenderTexture,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Title root fixture",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            scene.render(&mut pass);
        }
        crate::layer::user::readback_test_texture(context, &target)
    };
    let mut cases = Vec::new();
    for (profile, asset, rename, unlocked) in [
        ("logo-name", "/title.txa", Some("logo"), false),
        ("copyright-name", "/title.txa", Some("copyright"), false),
        ("plate-name", "/title.txa", Some("plate"), false),
        ("menu-name", "/title.txa", Some("menu"), false),
        (
            "start-caption-name",
            "/title.txa",
            Some("text_start"),
            false,
        ),
        ("bustup-caption-name", "/title.txa", Some("text_bup"), true),
        (
            "unused-bustup-caption",
            "/title.txa",
            Some("text_bup"),
            false,
        ),
        ("title-short", "/title.txa", None, false),
        ("mask-short", "/default.msk", None, false),
        ("background-short", "/title/bg01.pic", None, false),
    ] {
        let bytes = supplied_payload(asset).2;
        let file = directory.join(&asset[1..]);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mut altered = bytes.clone();
        if let Some(name) = rename {
            let needle = format!("{name}\0");
            let at = altered
                .windows(needle.len())
                .position(|w| w == needle.as_bytes())
                .unwrap();
            altered[at + needle.len() - 2] = b'x';
        } else {
            altered.truncate(15);
        }
        std::fs::write(&file, &altered).unwrap();
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
        let mut saved = SaveData::default();
        saved.save_menu_position = 91;
        saved.manual_save_slots[91] = Some(VmState::for_scenario(0, 42).snapshot_game_data());
        if unlocked {
            saved.persist_data.set(0, 31);
            for index in (1..=8).chain(17..=24) {
                saved.persist_data.set(index, 3);
            }
            saved.auto_save_slot = saved.manual_save_slots[91].clone();
        }
        let disk = output.join(format!("progress-{profile}.dat"));
        let disk_bytes = saved.encode();
        std::fs::write(&disk, &disk_bytes).unwrap();
        let mut globals = GlobalData::open(Some(disk.clone()), None).unwrap();
        let before = globals.save().encode();
        let setup = TitleSetup::from_save(globals.save(), -1);
        let mut scene = TitleScene::new(server.clone(), setup.clone());
        let unused = profile == "unused-bustup-caption";
        let deadline = Instant::now() + Duration::from_secs(10);
        let error = loop {
            match update(&mut scene, &mut globals, 0.0) {
                Err(error) => break Some(format!("{error:#}")),
                Ok(()) => {}
            }
            // Exercise actual prepare/render while publication races worker
            // completion. The unfixed missing-logo case panics here.
            if scene.atlas.is_some() {
                draw(&mut scene, &mut context);
            }
            if scene.is_loaded() {
                assert!(
                    unused,
                    "{profile}: malformed mandatory artwork reached title rendering"
                );
                break None;
            }
            assert!(Instant::now() < deadline, "{profile}: no publication/error");
            std::thread::sleep(Duration::from_millis(1));
        };
        if !unused {
            assert!(error.as_ref().unwrap().contains(asset));
            if asset == "/title.txa" {
                assert!(scene.atlas.is_none());
            }
            assert!(!scene.is_loaded());
            draw(&mut scene, &mut context);
        }
        assert_eq!(globals.save().encode(), before);
        assert_eq!(std::fs::read(&disk).unwrap(), disk_bytes);
        drop(scene);
        std::fs::write(&file, &bytes).unwrap();
        // Reopening after mandatory data repair uses the same server and the
        // same globals. This is not an in-session SceneHost retry adapter.
        let mut reopened = TitleScene::new(server.clone(), setup.clone());
        let deadline = Instant::now() + Duration::from_secs(10);
        while !reopened.is_loaded() {
            update(&mut reopened, &mut globals, 0.0).unwrap();
            assert!(
                Instant::now() < deadline,
                "{profile}: repaired title did not load"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        for _ in 0..120 {
            update(&mut reopened, &mut globals, 12.0).unwrap();
            if reopened.motion.phase() == super::super::motion::Phase::Active {
                break;
            }
        }
        assert_eq!(reopened.motion.phase(), super::super::motion::Phase::Active);
        assert_eq!(reopened.motion.selected(), setup.selected);
        let pixels = draw(&mut reopened, &mut context);
        assert!(
            pixels
                .chunks_exact(4)
                .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
        );
        assert_eq!(globals.save().encode(), before);
        assert_eq!(std::fs::read(&disk).unwrap(), disk_bytes);
        std::fs::remove_file(file).unwrap();
        cases.push(
            serde_json::json!({"profile":profile,"asset":asset,"error":error,
            "unused_caption_accepted":unused,"same_server_reopen_after_repair":true,
            "title_rendered_after_repair":true,"global_and_disk_progress_preserved":true}),
        );
    }
    std::fs::write(output.join("title-root-recovery.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "cases":cases,"cases_passed":cases.len(),"patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),
        "window_created":false,"audio_playback":false,"working_progress_written":false,
        "retry_scope":"Fresh TitleScene after mandatory data repair;retains same globals/server;not SceneHost recovery"
    })).unwrap()).unwrap();
}
