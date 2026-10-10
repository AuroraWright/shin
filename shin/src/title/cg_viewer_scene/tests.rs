use super::*;
#[test]
fn centered_picture_transform_matches_native_bounds_and_fit_translation() {
    let size = Vec2::new(2400.0, 1200.0);
    let transform = picture_transform(size, Vec4::new(20.0, -10.0, 500.0, 500.0));
    let center = transform.transform_point3((size * 0.5).extend(0.0));
    assert!((center.x - 20.0 / 960.0).abs() < 0.00001);
    assert!((center.y - 10.0 / 540.0).abs() < 0.00001);
    let left = transform.transform_point3(glam::vec3(0.0, 600.0, 0.0));
    assert!((left.x - (20.0 - 600.0) / 960.0).abs() < 0.00001);
}
#[test]
fn fully_locked_initial_viewer_has_no_load_task() {
    let mut motion = CgViewerMotion::without_load();
    motion.prepare_frame(false, true);
    assert!(motion.accepts_input());
    assert!(motion.close());
}
#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_cg_picture_owner_async_publication_capture_close_and_rom_pixels() {
    use crate::asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache};
    use shin_core::{
        format::{
            picture::{SimplePicture, read_picture},
            rom::RomReader,
            save::SaveData,
        },
        primitives::{stateless_reader::StatelessFile, update::FrameId},
        time::Ticks,
    };
    use shin_render::{
        dynamic_buffer::DynamicBuffer,
        pipelines::PipelineStorage,
        resize::{SurfaceResizeSource, ViewportParams},
        shaders::types::{
            buffer::BytesAddress,
            texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
        },
    };
    use std::io::Read;
    crate::init_test_task_pools();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: adapter.features() & wgpu::Features::SPIRV_SHADER_PASSTHROUGH,
            ..Default::default()
        },
        None,
    ))
    .unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
    let server = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let scenario: Arc<Scenario> =
        shin_tasks::block_on(server.load(crate::asset::asset_paths::SCENARIO)).unwrap();
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Startup reference depth"),
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
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Startup reference pixels"),
        size: depth.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), shin_render::TEXTURE_FORMAT);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());
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

    let mut save = SaveData::default();
    for row in &scenario.info_tables().picture_box_info {
        for id in &row.picture_ids.0 {
            save.save_vectors.unlocked_cgs.add(id.into_i32() as usize);
        }
    }
    save.settings.v3_sysvol = 0;
    let gallery = super::super::cg_gallery::CgGalleryList::new(
        &scenario.info_tables().picture_box_info,
        &save,
    )
    .unwrap();
    let row = gallery
        .rows()
        .iter()
        .find(|row| row.pictures.len() >= 2 && row.pictures[0] != row.pictures[1])
        .unwrap();
    let globals = GlobalData::open(None, Some(save)).unwrap();
    let before = globals.save().encode();
    let atlas: Arc<CharacterTextures> = shin_tasks::block_on(server.load("/cgmode.txa")).unwrap();
    let mut audio = GlobalAudio::new(Arc::new(shin_audio::AudioManager::new_for_tests()));
    let mut scene =
        CgViewerScene::new(server.clone(), scenario.clone(), row, (1920, 1080)).unwrap();
    fn parent(pass: &mut RenderPass) {
        QuadVertices::new()
            .with_box(0.0, 0.0, 1920.0, 1080.0)
            .with_color(UnormColor::WHITE)
            .fill(
                pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
                top_left_projection_matrix(),
            );
    }
    fn pixels(
        scene: &CgViewerScene,
        row: &CgRow,
        atlas: &CharacterTextures,
        ctx: &mut PreRenderContext,
        target: &wgpu::Texture,
    ) -> Vec<u8> {
        let view = target.create_view(&Default::default());
        {
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                Some(ctx.depth_stencil),
                "CG owner reference",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            parent(&mut pass);
            scene.render(row, atlas, &mut pass);
        }
        crate::layer::user::readback_test_texture(ctx, target)
    }
    fn step<'a>(
        scene: &mut CgViewerScene,
        row: &CgRow,
        ctx: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        globals: &GlobalData,
        audio: &mut GlobalAudio,
        n: &mut u32,
        delta: f32,
        action: Option<AppAction>,
    ) {
        *n += 1;
        let mut state = EnumMap::default();
        if let Some(action) = action {
            state[action] = ActionState {
                is_clicked: true,
                ..Default::default()
            };
        }
        scene
            .update(
                &mut UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: FrameId::default(),
                    delta_ticks: Ticks::from_f32(delta),
                    asset_server: server,
                    pre_render: ctx,
                },
                row,
                &PointerFrame {
                    number: *n,
                    time_seconds: *n as f64,
                    events: &[],
                },
                &state,
                Vec2::ZERO,
                (1920, 1080),
                globals,
                audio,
                true,
                parent,
                |_| {},
            )
            .unwrap();
    }
    let mut n = 0;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while scene.picture.is_none() {
        step(
            &mut scene,
            row,
            &mut ctx,
            &server,
            &globals,
            &mut audio,
            &mut n,
            0.0,
            Some(AppAction::Cancel),
        );
        assert!(!scene.navigation.closing());
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(!scene.motion.accepts_input());
    let initial = pixels(&scene, row, &atlas, &mut ctx, &target);
    // Initial capture excludes native indicators; the final composition draws them.
    for y in (100..950).step_by(25) {
        for x in (100..1800).step_by(25) {
            assert_eq!(
                &initial[(y * 1920 + x) * 4..(y * 1920 + x) * 4 + 4],
                &[255, 255, 255, 255]
            );
        }
    }
    step(
        &mut scene,
        row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        30.0,
        Some(AppAction::Cancel),
    );
    step(
        &mut scene,
        row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        0.0,
        Some(AppAction::Cancel),
    );
    assert!(!scene.motion.accepts_input());
    step(
        &mut scene, row, &mut ctx, &server, &globals, &mut audio, &mut n, 0.0, None,
    );
    assert!(scene.motion.accepts_input());
    let first = pixels(&scene, row, &atlas, &mut ctx, &target);
    // Capture remains owned after fade waiters retire: negative native delta
    // can reveal the initial gallery again without disabling active input.
    step(
        &mut scene, row, &mut ctx, &server, &globals, &mut audio, &mut n, -3.0, None,
    );
    assert!(scene.motion.accepts_input());
    let reverse = pixels(&scene, row, &atlas, &mut ctx, &target);
    for y in (100..950).step_by(23) {
        for x in (100..1800).step_by(23) {
            for c in 0..3 {
                let i = (y * 1920 + x) * 4 + c;
                assert!((reverse[i] as f32 - (first[i] as f32 * 0.9 + 25.5)).abs() <= 2.0);
            }
        }
    }
    step(
        &mut scene, row, &mut ctx, &server, &globals, &mut audio, &mut n, 3.0, None,
    );
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let file = rom
        .find_file(&scenario.info_tables().picture_info(row.pictures[0]).path())
        .unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let cpu = read_picture::<SimplePicture>(&bytes, ()).unwrap();
    let mut checked = 0;
    for y in (100..950usize).step_by(13) {
        for x in (100..1800usize).step_by(13) {
            let px = x as f32 + (cpu.effective_width as f32 - 1920.0) * 0.5;
            let py = y as f32 + (cpu.effective_height as f32 - 1080.0) * 0.5;
            let mut expected = [0.0f32; 3];
            let mut valid = true;
            for (positions, block) in &cpu.blocks {
                for &(bx, by) in positions {
                    let lx = px - bx as f32 - block.offset_x as f32;
                    let ly = py - by as f32 - block.offset_y as f32;
                    for rect in block.opaque_rects.iter().chain(&block.transparent_rects) {
                        if lx >= rect.from_x as f32
                            && lx <= rect.to_x as f32
                            && ly >= rect.from_y as f32
                            && ly <= rect.to_y as f32
                        {
                            // Skip primitive edges and non-integral sample positions; independent
                            // original blocks determine expected interior RGB, including alpha.
                            if lx < rect.from_x as f32 + 2.0
                                || lx > rect.to_x as f32 - 2.0
                                || ly < rect.from_y as f32 + 2.0
                                || ly > rect.to_y as f32 - 2.0
                                || lx.fract() != 0.0
                                || ly.fract() != 0.0
                            {
                                valid = false;
                                continue;
                            }
                            let src = block.data.get_pixel(lx as u32 + 1, ly as u32 + 1).0;
                            for c in 0..3 {
                                expected[c] = src[c] as f32 * src[3] as f32 / 255.0
                                    + expected[c] * (1.0 - src[3] as f32 / 255.0);
                            }
                        }
                    }
                }
            }
            if valid {
                for c in 0..3 {
                    assert!(
                        (first[(y * 1920 + x) * 4 + c] as f32 - expected[c]).abs() <= 2.0,
                        "PIC oracle {x},{y} channel{c}:{} vs{}",
                        first[(y * 1920 + x) * 4 + c],
                        expected[c]
                    );
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 1000);
    eprintln!("Original CG picture interior sample points: {checked}");
    step(
        &mut scene,
        row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        0.0,
        Some(AppAction::Enter),
    );
    assert!(scene.loading.is_some());
    assert!(!scene.motion.accepts_input());
    let old = scene.picture.as_ref().unwrap().clone();
    while scene.picture.as_ref().is_some_and(|p| Arc::ptr_eq(p, &old)) {
        step(
            &mut scene,
            row,
            &mut ctx,
            &server,
            &globals,
            &mut audio,
            &mut n,
            0.0,
            Some(AppAction::Cancel),
        );
        assert!(!scene.navigation.closing());
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let publication = pixels(&scene, row, &atlas, &mut ctx, &target);
    for y in (100..950).step_by(23) {
        for x in (100..1800).step_by(23) {
            assert_eq!(
                &publication[(y * 1920 + x) * 4..(y * 1920 + x) * 4 + 4],
                &first[(y * 1920 + x) * 4..(y * 1920 + x) * 4 + 4]
            );
        }
    }
    step(
        &mut scene,
        row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        15.0,
        Some(AppAction::Cancel),
    );
    let half = pixels(&scene, row, &atlas, &mut ctx, &target);
    step(
        &mut scene, row, &mut ctx, &server, &globals, &mut audio, &mut n, 15.0, None,
    );
    let second = pixels(&scene, row, &atlas, &mut ctx, &target);
    for y in (100..950).step_by(23) {
        for x in (100..1800).step_by(23) {
            for c in 0..3 {
                let i = (y * 1920 + x) * 4 + c;
                assert!((half[i] as f32 - (first[i] as f32 + second[i] as f32) * 0.5).abs() <= 2.0);
            }
        }
    }
    step(
        &mut scene, row, &mut ctx, &server, &globals, &mut audio, &mut n, 0.0, None,
    );
    step(
        &mut scene,
        row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        0.0,
        Some(AppAction::Cancel),
    );
    assert!(scene.navigation.closing());
    assert!(scene.picture.is_none());
    let closing = pixels(&scene, row, &atlas, &mut ctx, &target);
    assert_eq!(closing[..950 * 1920 * 4], second[..950 * 1920 * 4]);
    step(
        &mut scene, row, &mut ctx, &server, &globals, &mut audio, &mut n, 15.0, None,
    );
    let half = pixels(&scene, row, &atlas, &mut ctx, &target);
    for y in (100..950).step_by(23) {
        for x in (100..1800).step_by(23) {
            for c in 0..3 {
                let i = (y * 1920 + x) * 4 + c;
                assert!((half[i] as f32 - (second[i] as f32 + 255.0) * 0.5).abs() <= 2.0);
            }
        }
    }
    step(
        &mut scene, row, &mut ctx, &server, &globals, &mut audio, &mut n, 15.0, None,
    );
    assert!(!scene.is_finished());
    step(
        &mut scene, row, &mut ctx, &server, &globals, &mut audio, &mut n, 0.0, None,
    );
    assert!(!scene.is_finished());
    step(
        &mut scene, row, &mut ctx, &server, &globals, &mut audio, &mut n, 0.0, None,
    );
    assert!(scene.is_finished());
    assert!(scene.capture.is_some()); // Member survives waiters.
    let closed = pixels(&scene, row, &atlas, &mut ctx, &target);
    assert!(closed.iter().all(|v| *v == 255));
    let locked = CgRow {
        entry: row.entry,
        thumbnail: row.thumbnail.clone(),
        pictures: row.pictures.clone(),
        unlocked: vec![false; row.pictures.len()],
        plate: row.plate,
    };
    let mut blank =
        CgViewerScene::new(server.clone(), scenario.clone(), &locked, (1920, 1080)).unwrap();
    assert!(blank.loading.is_none());
    assert!(blank.picture.is_none());
    step(
        &mut blank, &locked, &mut ctx, &server, &globals, &mut audio, &mut n, 0.0, None,
    );
    assert!(blank.motion.accepts_input());
    let p = pixels(&blank, &locked, &atlas, &mut ctx, &target);
    assert!(p[..950 * 1920 * 4].iter().all(|v| *v == 255));
    step(
        &mut blank,
        &locked,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        0.0,
        Some(AppAction::Cancel),
    );
    assert!(blank.navigation.closing());
    assert!(blank.capture.is_some());
    let p = pixels(&blank, &locked, &atlas, &mut ctx, &target);
    assert!(p.iter().all(|v| *v == 255)); // Capture hid all locked variant markers.
    assert_eq!(before, globals.save().encode());
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_picture_bounds_capture_markers_and_closing_render_order() {
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, expected) in [
        (0x4a80c, 0x790182a8),
        (0x4a8d4, 0x940000ab),
        (0xc5e40, 0x7d40ba61),
        (0xc5e48, 0x7d40c262),
        (0xc5e50, 0x7d40c663),
        (0xc5e58, 0x1e218904),
        (0xc5e6c, 0x1e220902),
        (0xc5e74, 0x1e230803),
        (0xc5e7c, 0x1e218801),
        (0x4dd00, 0x1e2c0960),
        (0x4dd04, 0x1e202908),
        (0x4dd1c, 0x1e2c0940),
        (0x4dd20, 0x1e202929),
        (0x4dd3c, 0x9401b569),
        (0x4dd50, 0x9401b564),
        (0x4dbe8, 0x3903067f),
        (0x4dc78, 0xd63f0100),
        (0x4dca4, 0x39030668),
        (0x4dd84, 0x940053ff),
        (0x4dda8, 0x321f0fe1),
        (0x4ddac, 0x9400543d),
        (0x4b5c0, 0xb40007c8),
        (0x4b61c, 0x94021379),
        (0x4b698, 0x94009692),
        (0x4b6bc, 0x94005e2d),
        (0x4b88c, 0x79018268),
        (0x4b91c, 0xd63f0100),
        (0x4b94c, 0xc80afd3f),
        (0x4b9b0, 0x94005cf4),
        (0x4b9d8, 0x94005d32),
    ] {
        assert_eq!(word(a), expected, "Native CG owner {a:x}");
    }
}

#[test]
#[ignore = "requires supplied ROM, audio and GPU (SHIN_REFERENCE_ROM)"]
fn reference_live_cg_gallery_title6_roundtrip() {
    use crate::asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache};
    use shin_core::{
        format::{
            picture::{SimplePicture, read_picture},
            rom::RomReader,
            save::SaveData,
        },
        primitives::{stateless_reader::StatelessFile, update::FrameId},
        time::Ticks,
    };
    use shin_render::{
        dynamic_buffer::DynamicBuffer,
        pipelines::PipelineStorage,
        resize::{SurfaceResizeSource, ViewportParams},
        shaders::types::{
            buffer::BytesAddress,
            texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
        },
    };
    use std::io::Read;
    crate::init_test_task_pools();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: adapter.features() & wgpu::Features::SPIRV_SHADER_PASSTHROUGH,
            ..Default::default()
        },
        None,
    ))
    .unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
    let server = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let scenario: Arc<Scenario> =
        shin_tasks::block_on(server.load(crate::asset::asset_paths::SCENARIO)).unwrap();
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Startup reference depth"),
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
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Startup reference pixels"),
        size: depth.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), shin_render::TEXTURE_FORMAT);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());
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

    let view = target.create_view(&Default::default());
    crate::app::SceneHost::reference_cg_roundtrip(&server, &mut ctx, &target, &view);
}

#[test]
#[ignore = "requires supplied ROM, audio and GPU (SHIN_REFERENCE_ROM)"]
fn reference_cg_owner_pan_fit_half_size_pointer_and_variant_exhaustion_pixels() {
    use crate::asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache};
    use shin_core::{
        format::{
            picture::{SimplePicture, read_picture},
            rom::RomReader,
            save::SaveData,
        },
        primitives::{stateless_reader::StatelessFile, update::FrameId},
        time::Ticks,
    };
    use shin_render::{
        dynamic_buffer::DynamicBuffer,
        pipelines::PipelineStorage,
        resize::{SurfaceResizeSource, ViewportParams},
        shaders::types::{
            buffer::BytesAddress,
            texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
        },
    };
    use std::io::Read;
    crate::init_test_task_pools();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: adapter.features() & wgpu::Features::SPIRV_SHADER_PASSTHROUGH,
            ..Default::default()
        },
        None,
    ))
    .unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
    let server = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let scenario: Arc<Scenario> =
        shin_tasks::block_on(server.load(crate::asset::asset_paths::SCENARIO)).unwrap();
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Startup reference depth"),
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
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Startup reference pixels"),
        size: depth.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), shin_render::TEXTURE_FORMAT);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());
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

    use crate::adv::input_area::{PointerEvent, PointerKind};
    use shin_core::format::picture::read_picture_header;
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut chosen = None;
    'rows: for (entry, row) in scenario.info_tables().picture_box_info.iter().enumerate() {
        for (ordinal, id) in row.picture_ids.0.iter().enumerate() {
            let path = scenario.info_tables().picture_info(*id).path();
            let mut bytes = Vec::new();
            rom.open_file(rom.find_file(&path).unwrap())
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            let header = read_picture_header(&bytes).unwrap();
            if header.effective_width > 1920 || header.effective_height > 1080 {
                chosen = Some((
                    entry,
                    ordinal,
                    *id,
                    read_picture::<SimplePicture>(&bytes, ()).unwrap(),
                ));
                break 'rows;
            }
        }
    }
    let (entry, ordinal, id, cpu) = chosen.expect("Original oversized CG for real pan/fit oracle");
    eprintln!(
        "Original oversized CG: row{entry} slot{ordinal} ID{} {}x{} origin{},{}",
        id.into_i32(),
        cpu.effective_width,
        cpu.effective_height,
        cpu.origin_x,
        cpu.origin_y
    );
    let pictures = scenario.info_tables().picture_box_info[entry]
        .picture_ids
        .0
        .iter()
        .copied()
        .collect::<Vec<_>>();
    let mut unlocked = vec![false; pictures.len()];
    unlocked[ordinal] = true;
    let row = CgRow {
        entry,
        thumbnail: "unused".into(),
        pictures,
        unlocked,
        plate: [0.0; 4],
    };
    let mut save = SaveData::default();
    save.settings.v3_sysvol = 0;
    let globals = GlobalData::open(None, Some(save)).unwrap();
    let before = globals.save().encode();
    let atlas: Arc<CharacterTextures> = shin_tasks::block_on(server.load("/cgmode.txa")).unwrap();
    let mut audio = GlobalAudio::new(Arc::new(shin_audio::AudioManager::new_for_tests()));
    let mut scene = CgViewerScene::new(server.clone(), scenario.clone(), &row, (960, 540)).unwrap();
    let mut n = 0u32;
    fn step<'a>(
        scene: &mut CgViewerScene,
        row: &CgRow,
        ctx: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        globals: &GlobalData,
        audio: &mut GlobalAudio,
        n: &mut u32,
        delta: f32,
        action: Option<AppAction>,
        axes: Vec2,
        events: &[PointerEvent],
        eligible: bool,
    ) {
        *n += 1;
        let mut state = EnumMap::default();
        if let Some(a) = action {
            state[a] = ActionState {
                is_clicked: true,
                is_held: true,
                ..Default::default()
            };
        }
        scene
            .update(
                &mut UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: FrameId::default(),
                    delta_ticks: Ticks::from_f32(delta),
                    asset_server: server,
                    pre_render: ctx,
                },
                row,
                &PointerFrame {
                    number: *n,
                    time_seconds: *n as f64 / 60.0,
                    events,
                },
                &state,
                axes,
                (960, 540),
                globals,
                audio,
                eligible,
                |pass| {
                    QuadVertices::new()
                        .with_box(0.0, 0.0, 1920.0, 1080.0)
                        .with_color(UnormColor::WHITE)
                        .fill(
                            pass,
                            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
                            top_left_projection_matrix(),
                        );
                },
                |_| {},
            )
            .unwrap();
    }
    fn event(kind: PointerKind, x: i32, y: i32) -> PointerEvent {
        PointerEvent {
            kind,
            id: 0,
            position: glam::ivec2(x, y),
        }
    }
    fn pixels(
        scene: &CgViewerScene,
        row: &CgRow,
        atlas: &CharacterTextures,
        ctx: &mut PreRenderContext,
        target: &wgpu::Texture,
    ) -> Vec<u8> {
        let view = target.create_view(&Default::default());
        {
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                Some(ctx.depth_stencil),
                "CG pan fit reference",
            );
            pass.clear(Some(UnormColor::WHITE), Some(0), Some(1.0));
            scene.render(row, atlas, &mut pass);
        }
        crate::layer::user::readback_test_texture(ctx, target)
    }
    fn oracle(cpu: &SimplePicture, actual: &[u8], translation: Vec2, scale: f32, label: &str) {
        let mut count = 0;
        let mut worst = 0.0f32;
        let size = Vec2::new(cpu.effective_width as f32, cpu.effective_height as f32);
        for y in (40..990usize).step_by(11) {
            for x in (40..1880usize).step_by(11) {
                let source = (Vec2::new(x as f32 + 0.5, y as f32 + 0.5)
                    - Vec2::new(960.0, 540.0)
                    - translation)
                    / scale
                    + size * 0.5;
                let mut expected = [0.0f32; 3];
                let mut valid = true;
                for opaque in [true, false] {
                    for (positions, block) in &cpu.blocks {
                        for &(bx, by) in positions {
                            let lx = source.x - bx as f32 - block.offset_x as f32;
                            let ly = source.y - by as f32 - block.offset_y as f32;
                            let rects = if opaque {
                                &block.opaque_rects
                            } else {
                                &block.transparent_rects
                            };
                            for rect in rects {
                                if lx >= rect.from_x as f32
                                    && lx <= rect.to_x as f32 + 0.5
                                    && ly >= rect.from_y as f32
                                    && ly <= rect.to_y as f32 + 0.5
                                {
                                    if lx < rect.from_x as f32 + 2.0
                                        || lx > rect.to_x as f32 - 2.0
                                        || ly < rect.from_y as f32 + 2.0
                                        || ly > rect.to_y as f32 - 2.0
                                    {
                                        valid = false;
                                        continue;
                                    }
                                    let tx = lx + 0.5;
                                    let ty = ly + 0.5;
                                    let ix = tx.floor() as u32;
                                    let iy = ty.floor() as u32;
                                    let fx = tx.fract();
                                    let fy = ty.fract();
                                    let color: [f32; 4] = std::array::from_fn(|c| {
                                        let p = |dx, dy| {
                                            block.data.get_pixel(ix + dx, iy + dy).0[c] as f32
                                        };
                                        (p(0, 0) * (1.0 - fx) + p(1, 0) * fx) * (1.0 - fy)
                                            + (p(0, 1) * (1.0 - fx) + p(1, 1) * fx) * fy
                                    });
                                    for c in 0..3 {
                                        expected[c] = if opaque {
                                            color[c]
                                        } else {
                                            color[c] * color[3] / 255.0
                                                + expected[c] * (1.0 - color[3] / 255.0)
                                        };
                                    }
                                }
                            }
                        }
                    }
                }
                if valid {
                    for c in 0..3 {
                        let error = (actual[(y * 1920 + x) * 4 + c] as f32 - expected[c]).abs();
                        worst = worst.max(error);
                        assert!(
                            error <= 2.5,
                            "{label} at{x},{y} channel{c}:{} vs{}",
                            actual[(y * 1920 + x) * 4 + c],
                            expected[c]
                        );
                    }
                    count += 1;
                }
            }
        }
        assert!(count > 1000);
        eprintln!("CG {label}: {count} original-block sample points maxRGB{worst}");
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !scene.reference_ready() {
        step(
            &mut scene,
            &row,
            &mut ctx,
            &server,
            &globals,
            &mut audio,
            &mut n,
            0.0,
            None,
            Vec2::ZERO,
            &[],
            true,
        );
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    for delta in [30.0, 0.0, 0.0] {
        step(
            &mut scene,
            &row,
            &mut ctx,
            &server,
            &globals,
            &mut audio,
            &mut n,
            delta,
            None,
            Vec2::ZERO,
            &[],
            true,
        );
    }
    assert_eq!(scene.navigation.ordinal(), ordinal as i32);
    oracle(
        &cpu,
        &pixels(&scene, &row, &atlas, &mut ctx, &target),
        Vec2::ZERO,
        1.0,
        "centered",
    );
    let limit = (Vec2::new(cpu.effective_width as f32, cpu.effective_height as f32) * 0.5
        - Vec2::new(960.0, 540.0))
    .max(Vec2::ZERO);
    step(
        &mut scene,
        &row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        0.0,
        None,
        Vec2::new(-1.0, 1.0),
        &[],
        true,
    );
    let mut logical = Vec2::splat(32.0).min(limit);
    assert_eq!(scene.navigation.position(), logical);
    oracle(
        &cpu,
        &pixels(&scene, &row, &atlas, &mut ctx, &target),
        Vec2::new(logical.x, -logical.y),
        1.0,
        "left stick",
    );
    step(
        &mut scene,
        &row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        0.0,
        Some(AppAction::ViewerPanLeft),
        Vec2::ZERO,
        &[],
        true,
    );
    logical.x = (logical.x + 16.0).min(limit.x);
    oracle(
        &cpu,
        &pixels(&scene, &row, &atlas, &mut ctx, &target),
        Vec2::new(logical.x, -logical.y),
        1.0,
        "held Dpad",
    );
    step(
        &mut scene,
        &row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        0.0,
        None,
        Vec2::new(-1.0, 1.0),
        &[],
        false,
    );
    assert_eq!(scene.navigation.position(), logical); // Another owner suppresses pan.
    step(
        &mut scene,
        &row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        0.0,
        Some(AppAction::CharacterVariant),
        Vec2::ZERO,
        &[],
        true,
    );
    let fit = 1.0 / (cpu.effective_width as f32 / 1920.0).max(cpu.effective_height as f32 / 1080.0);
    step(
        &mut scene,
        &row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        5.0,
        None,
        Vec2::new(-1.0, 1.0),
        &[],
        true,
    );
    oracle(
        &cpu,
        &pixels(&scene, &row, &atlas, &mut ctx, &target),
        Vec2::new(logical.x, -logical.y) * 0.5,
        (1.0 + fit) * 0.5,
        "fit midpoint",
    );
    step(
        &mut scene,
        &row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        5.0,
        None,
        Vec2::ZERO,
        &[],
        true,
    );
    oracle(
        &cpu,
        &pixels(&scene, &row, &atlas, &mut ctx, &target),
        Vec2::ZERO,
        fit,
        "fit complete",
    );
    step(
        &mut scene,
        &row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        0.0,
        Some(AppAction::CharacterVariant),
        Vec2::ZERO,
        &[],
        true,
    );
    step(
        &mut scene,
        &row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        10.0,
        None,
        Vec2::ZERO,
        &[],
        true,
    );
    step(
        &mut scene,
        &row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        0.0,
        None,
        Vec2::ZERO,
        &[event(PointerKind::Down, 300, 250)],
        true,
    );
    // Half-size canvas doubles pointer displacement; native Y pan adds .5625.
    step(
        &mut scene,
        &row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        0.0,
        None,
        Vec2::ZERO,
        &[event(PointerKind::Move, 320, 270)],
        true,
    );
    logical = (logical + Vec2::new(40.0, -22.5)).clamp(-limit, limit);
    assert_eq!(scene.navigation.position(), logical);
    oracle(
        &cpu,
        &pixels(&scene, &row, &atlas, &mut ctx, &target),
        Vec2::new(logical.x, -logical.y),
        1.0,
        "half size pointer drag",
    );
    step(
        &mut scene,
        &row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        0.0,
        None,
        Vec2::ZERO,
        &[event(PointerKind::Cancel, 320, 270)],
        true,
    );
    let marked = pixels(&scene, &row, &atlas, &mut ctx, &target);
    step(
        &mut scene,
        &row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        0.0,
        Some(AppAction::CharacterState),
        Vec2::ZERO,
        &[],
        true,
    );
    let hidden = pixels(&scene, &row, &atlas, &mut ctx, &target);
    if row.pictures.len() > 1 {
        assert_ne!(marked, hidden);
    }
    // Fresh half-size tap advances original ordinal cursor past locked tail,
    // producing native exhaustion close without creating an async load.
    step(
        &mut scene,
        &row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        0.0,
        None,
        Vec2::ZERO,
        &[event(PointerKind::Down, 320, 270)],
        true,
    );
    step(
        &mut scene,
        &row,
        &mut ctx,
        &server,
        &globals,
        &mut audio,
        &mut n,
        0.0,
        None,
        Vec2::ZERO,
        &[event(PointerKind::Up, 320, 270)],
        true,
    );
    assert!(scene.navigation.closing());
    assert!(scene.loading.is_none());
    assert_eq!(scene.navigation.ordinal(), -1);
    for delta in [30.0, 0.0, 0.0] {
        step(
            &mut scene,
            &row,
            &mut ctx,
            &server,
            &globals,
            &mut audio,
            &mut n,
            delta,
            None,
            Vec2::ZERO,
            &[],
            true,
        );
    }
    assert!(scene.is_finished());

    let (entry, authored) = scenario
        .info_tables()
        .picture_box_info
        .iter()
        .enumerate()
        .max_by_key(|(_, r)| r.picture_ids.0.len())
        .unwrap();
    assert_eq!(authored.picture_ids.0.len(), 19);
    let mut publications = 0;
    for alternating in [false, true] {
        let row = CgRow {
            entry,
            thumbnail: "unused".into(),
            pictures: authored.picture_ids.0.iter().copied().collect(),
            unlocked: (0..19).map(|i| !alternating || i % 2 == 0).collect(),
            plate: [0.0; 4],
        };
        let expected = row
            .pictures
            .iter()
            .enumerate()
            .filter(|(i, _)| row.unlocked[*i])
            .map(|(i, id)| (i, *id))
            .collect::<Vec<_>>();
        let mut viewer =
            CgViewerScene::new(server.clone(), scenario.clone(), &row, (960, 540)).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        for (ordinal, id) in expected {
            assert_eq!(viewer.navigation.ordinal(), ordinal as i32);
            while viewer.loading.is_some() {
                step(
                    &mut viewer,
                    &row,
                    &mut ctx,
                    &server,
                    &globals,
                    &mut audio,
                    &mut n,
                    0.0,
                    Some(AppAction::Cancel),
                    Vec2::ZERO,
                    &[],
                    true,
                );
                assert!(!viewer.navigation.closing());
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            let expected_asset: Arc<Picture> = server
                .load_sync(scenario.info_tables().picture_info(id).path())
                .unwrap();
            assert!(
                Arc::ptr_eq(viewer.picture.as_ref().unwrap(), &expected_asset),
                "Wrong CG source at authored ordinal{ordinal}"
            );
            assert!(!viewer.motion.accepts_input());
            for delta in [30.0, 0.0, 0.0] {
                step(
                    &mut viewer,
                    &row,
                    &mut ctx,
                    &server,
                    &globals,
                    &mut audio,
                    &mut n,
                    delta,
                    None,
                    Vec2::ZERO,
                    &[],
                    true,
                );
            }
            assert!(viewer.motion.accepts_input());
            let p = pixels(&viewer, &row, &atlas, &mut ctx, &target);
            assert!(p.chunks_exact(4).all(|p| p[3] == 255)); // Actual picture and black surround rendered.
            publications += 1;
            step(
                &mut viewer,
                &row,
                &mut ctx,
                &server,
                &globals,
                &mut audio,
                &mut n,
                0.0,
                Some(AppAction::Enter),
                Vec2::ZERO,
                &[],
                true,
            );
        }
        assert!(viewer.navigation.closing());
        assert!(viewer.loading.is_none());
        assert_eq!(viewer.navigation.ordinal(), -1);
        for delta in [30.0, 0.0, 0.0] {
            step(
                &mut viewer,
                &row,
                &mut ctx,
                &server,
                &globals,
                &mut audio,
                &mut n,
                delta,
                None,
                Vec2::ZERO,
                &[],
                true,
            );
        }
        assert!(viewer.is_finished());
    }
    assert_eq!(publications, 29);
    eprintln!(
        "Original largest CG row{entry}:29 actual async publications, all19 and10 alternating unlocked slots, correct file/ordinal order and exhaustion"
    );
    assert_eq!(globals.save().encode(), before);
}
