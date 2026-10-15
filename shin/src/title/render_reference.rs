//! Independent CPU pixel oracle for native title UI quads and compositing.
use super::{
    TitleSetup,
    view::{self, TitleFrame},
};
use crate::{adv::character_view::CharacterTextures, render::PreRenderContext};
use std::sync::Arc;
#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_title_gpu_artwork_rows_and_background_cover() {
    use crate::asset::system::{AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache};
    use shin_render::{
        dynamic_buffer::DynamicBuffer,
        pipelines::PipelineStorage,
        resize::{SurfaceResizeSource, ViewportParams},
        shaders::types::{
            buffer::BytesAddress,
            texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
        },
    };
    crate::init_test_task_pools();
    let instance = wgpu::Instance::default();
    let adapter =
        shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
            adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
        } else {
            wgpu::Features::empty()
        },
            ..Default::default()
        }))
    .unwrap();
    let mut io = LayeredAssetIo::new();
    if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
        io.try_with_rom(patch).unwrap();
    }
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
    let server = std::sync::Arc::new(AssetServer::new(
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
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Title reference depth"),
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
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Title reference target"),
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
    });
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

    if std::env::var_os("SHIN_REFERENCE_GALLERY_RETURN_ONLY").is_some() {
        crate::app::SceneHost::reference_gallery_return(&server, &mut context, &target);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_BACKLOG_LATENCY_ONLY").is_some() {
        crate::adv::reference_saved_backlog_latency(&server, &mut context);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_MUSIC_REPEAT_ONLY").is_some() {
        let scenario: Arc<shin_core::format::scenario::Scenario> =
            server.load_sync("/main.snr").unwrap();
        super::music_gallery_audio::reference_gallery_audio_dispatch(server.clone(), &scenario);
        crate::app::SceneHost::reference_music_roundtrip(&server, &mut context, &target, &view);
        return;
    }

    use shin_core::{
        format::{
            rom::RomReader,
            save::{GameData, GameDataEntry, SaveData},
        },
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut read = |name: &str| {
        let file = rom.find_file(name).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    };
    let atlas_bytes = read("/title.txa");
    let original = shin_core::format::texture_archive::read_texture_archive(&atlas_bytes).unwrap();
    let atlas: Arc<CharacterTextures> = server.load_sync("/title.txa").unwrap();
    assert_eq!(
        original.textures[original.name_to_index["plate"]].dimensions(),
        (704, 820)
    );
    assert_eq!(
        original.textures[original.name_to_index["menu"]].dimensions(),
        (704, 902)
    );
    #[derive(Clone)]
    struct CpuImage {
        width: u32,
        height: u32,
        data: Vec<u8>,
    }
    let cpu_atlas = original
        .textures
        .iter()
        .map(|im| CpuImage {
            width: im.width(),
            height: im.height(),
            data: im.as_raw().clone(),
        })
        .collect::<Vec<_>>();
    fn sample(im: &CpuImage, u: f32, v: f32) -> [f32; 4] {
        let x = u * im.width as f32 - 0.5;
        let y = v * im.height as f32 - 0.5;
        let ix = x.floor();
        let iy = y.floor();
        let fx = x - ix;
        let fy = y - iy;
        std::array::from_fn(|c| {
            let p = |dx: f32, dy: f32| {
                let x = (ix + dx).clamp(0.0, im.width as f32 - 1.0) as u32;
                let y = (iy + dy).clamp(0.0, im.height as f32 - 1.0) as u32;
                im.data[((y * im.width + x) * 4) as usize + c] as f32 / 255.0
            };
            (p(0.0, 0.0) * (1.0 - fx) + p(1.0, 0.0) * fx) * (1.0 - fy)
                + (p(0.0, 1.0) * (1.0 - fx) + p(1.0, 1.0) * fx) * fy
        })
    }
    fn startup_blur(original: &CpuImage) -> CpuImage {
        let mut source = original.clone();
        let mut target = CpuImage {
            width: 1920,
            height: 1080,
            data: vec![0; 1920 * 1080 * 4],
        };
        let weights =
            [0x3eceed6b, 0x3ea127be, 0x3e437dbe, 0x3db8afdf].map(|bits| f32::from_bits(bits) * 0.5);
        for stage in 0..4 {
            let (width, height) = if stage == 3 { (1920, 1080) } else { (960, 540) };
            let scale = if stage == 0 { 1.0 } else { 0.5 };
            for y in 0..height {
                for x in 0..width {
                    let u = (x as f32 + 0.5) / width as f32 * scale;
                    let v = (y as f32 + 0.5) / height as f32 * scale;
                    let mut result = [0.0; 4];
                    if stage == 1 || stage == 2 {
                        for (index, weight) in weights.iter().enumerate() {
                            let offset = (index as f32 + 0.5) * 2.0;
                            let (du, dv) = if stage == 1 {
                                (offset / 1920.0, 0.0)
                            } else {
                                (0.0, offset / 1080.0)
                            };
                            let a = sample(&source, u - du, v - dv);
                            let b = sample(&source, u + du, v + dv);
                            for c in 0..4 {
                                result[c] += (a[c] + b[c]) * weight;
                            }
                        }
                    } else {
                        result = sample(&source, u, v);
                    }
                    for c in 0..4 {
                        target.data[((y * 1920 + x) * 4) as usize + c] =
                            (result[c] * 255.0).round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
            std::mem::swap(&mut source, &mut target);
        }
        source
    }
    fn color(v: [f32; 4]) -> [f32; 4] {
        v.map(|v| (v * 255.0).clamp(0.0, 255.0) as u8 as f32 / 255.0)
    }
    fn over(dst: &mut [f32; 4], src: [f32; 4]) {
        for c in 0..3 {
            dst[c] = src[c] * src[3] + dst[c] * (1.0 - src[3]);
        }
        dst[3] = src[3] + dst[3] * (1.0 - src[3]);
    }
    fn quad(
        dst: &mut [f32; 4],
        im: &CpuImage,
        rect: [f32; 4],
        uv: [f32; 4],
        tint: [f32; 4],
        x: f32,
        y: f32,
    ) {
        if x < rect[0] || x >= rect[2] || y < rect[1] || y >= rect[3] {
            return;
        }
        let u = uv[0] + (uv[2] - uv[0]) * (x - rect[0]) / (rect[2] - rect[0]);
        let v = uv[1] + (uv[3] - uv[1]) * (y - rect[1]) / (rect[3] - rect[1]);
        let sampled = sample(im, u, v);
        over(dst, std::array::from_fn(|c| sampled[c] * tint[c]));
    }
    for case in 0..8 {
        let mut save = SaveData::default();
        if case != 0 {
            save.persist_data.set(0, 31);
            save.persist_data.set(17, 3);
            save.auto_save_slot = Some(GameData::from_entry(GameDataEntry {
                scenario_id: 0,
                random_seed: 42,
                save_position: 1,
                route_data: Default::default(),
            }));
        }
        let setup = TitleSetup::from_save(&save, -1);
        let background = shin_core::format::picture::read_picture::<
            shin_core::format::picture::SimpleMergedPicture,
        >(&read(&setup.background_path()), ())
        .unwrap()
        .image;
        let bg = shin_render::gpu_texture::GpuTexture::new_static_from_rgba_image(
            &device,
            &queue,
            Some("Title ready rendered background"),
            &background,
        );
        let mut background = CpuImage {
            width: background.width(),
            height: background.height(),
            data: background.as_raw().clone(),
        };
        if case >= 5 {
            let pic = shin_core::format::picture::read_picture::<
                shin_core::format::picture::SimplePicture,
            >(&read(&setup.background_path()), ())
            .unwrap();
            background.data = vec![0; 1920 * 1080 * 4];
            for pixel in background.data.chunks_exact_mut(4) {
                pixel[3] = 255;
            }
            for transparent in [false, true] {
                for (positions, block) in &pic.blocks {
                    let im = CpuImage {
                        width: block.data.width(),
                        height: block.data.height(),
                        data: block.data.as_raw().clone(),
                    };
                    let rects = if transparent {
                        &block.transparent_rects
                    } else {
                        &block.opaque_rects
                    };
                    for &(bx, by) in positions {
                        for rect in rects {
                            let left = 960.0 - pic.origin_x as f32
                                + bx as f32
                                + block.offset_x as f32
                                + rect.from_x as f32;
                            let top = 540.0 - pic.origin_y as f32
                                + by as f32
                                + block.offset_y as f32
                                + rect.from_y as f32;
                            let right = left + (rect.to_x - rect.from_x) as f32 + 0.5;
                            let bottom = top + (rect.to_y - rect.from_y) as f32 + 0.5;
                            let u0 = (rect.from_x as f32 + 1.0) / im.width as f32;
                            let v0 = (rect.from_y as f32 + 1.0) / im.height as f32;
                            let u1 = (rect.to_x as f32 + 1.5) / im.width as f32;
                            let v1 = (rect.to_y as f32 + 1.5) / im.height as f32;
                            for y in top.floor().max(0.0) as u32..bottom.ceil().min(1080.0) as u32 {
                                for x in
                                    left.floor().max(0.0) as u32..right.ceil().min(1920.0) as u32
                                {
                                    let px = x as f32 + 0.5;
                                    let py = y as f32 + 0.5;
                                    if px < left || px >= right || py < top || py >= bottom {
                                        continue;
                                    }
                                    let uv = [
                                        u0 + (u1 - u0) * (px - left) / (right - left),
                                        v0 + (v1 - v0) * (py - top) / (bottom - top),
                                    ];
                                    let sample = sample(&im, uv[0], uv[1]);
                                    let offset = ((y * 1920 + x) * 4) as usize;
                                    let mut dst = std::array::from_fn(|c| {
                                        background.data[offset + c] as f32 / 255.0
                                    });
                                    if transparent {
                                        over(&mut dst, sample);
                                    } else {
                                        dst = sample;
                                    }
                                    for c in 0..4 {
                                        background.data[offset + c] =
                                            (dst[c] * 255.0).round().clamp(0.0, 255.0) as u8;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut frame = TitleFrame::ready(&setup);
        if case == 2 {
            frame.logo_offset = -47.0;
            frame.logo_alpha = 0.61;
            frame.caption_alpha = 0.37;
            frame.black_alpha = 0.23;
            for (index, row) in frame.rows.iter_mut().enumerate() {
                row.x += if index % 2 == 0 { -137.0 } else { 23.0 };
                row.focus = (index % 3) as f32 * 0.5;
                row.opacity = (index % 3) as f32 * 0.5;
                row.flash = if index % 2 == 0 { 0.42 } else { 0.0 };
            }
        }
        if case == 3 {
            let mut motion = super::motion::TitleMotion::new(setup.clone());
            motion.assets_ready();
            motion.advance_clocks(shin_core::time::Ticks::from_f32(120.0));
            motion.prepare_frame();
            motion.prepare_frame();
            motion.advance_clocks(shin_core::time::Ticks::from_f32(17.0));
            frame = motion.frame();
            assert_eq!(motion.phase(), super::motion::Phase::RowsOpening);
        }
        if case == 4 {
            let child_setup = TitleSetup::from_save(&save, super::TitleItem::Characters as i32);
            let mut motion = super::motion::TitleMotion::new(child_setup);
            motion.assets_ready();
            motion.advance_clocks(shin_core::time::Ticks::from_f32(60.0));
            for _ in 0..3 {
                motion.prepare_frame();
            }
            assert!(motion.confirm());
            motion.advance_clocks(shin_core::time::Ticks::from_f32(85.0));
            frame = motion.frame();
            assert_eq!(motion.phase(), super::motion::Phase::Retreating);
        }
        let mut owned_scene = None;
        if case >= 5 {
            let mut scene = super::scene::TitleScene::new(server.clone(), setup.clone());
            let state = enum_map::EnumMap::default();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let mut number = 0;
            while !scene.is_loaded() {
                scene
                    .update_frame(
                        shin_core::time::Ticks::ZERO,
                        &crate::adv::input_area::PointerFrame {
                            number,
                            time_seconds: number as f64 / 60.0,
                            events: &[],
                        },
                        &state,
                    )
                    .unwrap();
                number += 1;
                assert!(
                    std::time::Instant::now() < deadline,
                    "Title owner resource loading timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
            let mut bgm = crate::audio::BgmPlayer::new(manager.clone());
            let mut system = crate::audio::SystemSoundPlayer::new(manager);
            let scenario: Arc<shin_core::format::scenario::Scenario> =
                server.load_sync("/main.snr").unwrap();
            let mut settings = shin_core::format::save::Settings::default();
            settings.v0_bgmvol = 70;
            settings.v3_sysvol = 65;
            scene.update_audio(
                &server,
                &scenario,
                &settings,
                &mut bgm,
                &mut system,
                shin_core::time::Ticks::ZERO,
            );
            assert!(scene.take_stage_music().is_none());
            assert_eq!(
                bgm.reference_source().0.unwrap().into_i32(),
                setup.stage().bgm as i32
            );
            assert_eq!(system.volume_percent, 65);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while bgm
                .get_wait_status()
                .contains(shin_core::vm::command::types::AudioWaitStatus::LOADING)
            {
                bgm.update();
                assert!(
                    std::time::Instant::now() < deadline,
                    "Title stage music loading timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(
                bgm.get_wait_status()
                    .contains(shin_core::vm::command::types::AudioWaitStatus::PLAYING)
            );
            for delta in if case != 6 {
                &[120.0, 0.0, 0.0, 74.0, 0.0][..]
            } else {
                &[][..]
            } {
                scene
                    .update_frame(
                        shin_core::time::Ticks::from_f32(*delta),
                        &crate::adv::input_area::PointerFrame {
                            number,
                            time_seconds: number as f64 / 60.0,
                            events: &[],
                        },
                        &state,
                    )
                    .unwrap();
                number += 1;
            }
            assert_eq!(
                scene.motion.phase(),
                if case != 6 {
                    super::motion::Phase::Active
                } else {
                    super::motion::Phase::FirstWait
                }
            );
            if case == 6 {
                background = startup_blur(&background);
            }
            if case == 5 {
                assert!(scene.motion.navigate(true, false));
                let track = system.track.id();
                scene.update_audio(
                    &server,
                    &scenario,
                    &settings,
                    &mut bgm,
                    &mut system,
                    shin_core::time::Ticks::ZERO,
                );
                assert_eq!(system.track.id(), track);
                assert_eq!(system.sounds.len(), 1);
                assert!(scene.take_sounds().is_empty());
                scene
                    .motion
                    .advance_clocks(shin_core::time::Ticks::from_f32(30.0));
                bgm.stop(shin_core::time::Tween::IMMEDIATE);
            }
            if case == 7 {
                let mut confirm = enum_map::EnumMap::default();
                confirm[crate::app::AppAction::Enter] = shin_input::ActionState {
                    is_clicked: true,
                    ..Default::default()
                };
                for (delta, input) in [(7.0, &confirm), (0.0, &state), (45.0, &state)] {
                    scene
                        .update_frame(
                            shin_core::time::Ticks::from_f32(delta),
                            &crate::adv::input_area::PointerFrame {
                                number,
                                time_seconds: number as f64 / 60.0,
                                events: &[],
                            },
                            input,
                        )
                        .unwrap();
                    number += 1;
                    scene.update_audio(
                        &server,
                        &scenario,
                        &settings,
                        &mut bgm,
                        &mut system,
                        shin_core::time::Ticks::from_f32(delta),
                    );
                }
                assert_eq!(scene.motion.phase(), super::motion::Phase::Continuing);
                assert_eq!(scene.motion.frame().black_alpha, 0.5);
                assert!(bgm.reference_source().0.is_none());
                assert!(scene.take_destination().is_none());
                assert!(!scene.motion.accepts_input());
            }
            frame = scene.motion.frame();
            scene.prepare(&mut context);
            assert!(scene.take_sounds().is_empty());
            assert!(scene.take_destination().is_none());
            scene.resize((960, 540));
            owned_scene = Some(scene);
        }
        let target_view = target.create_view(&Default::default());
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &target_view,
                },
                Some(context.depth_stencil),
                "Native title UI reference",
            );
            if let Some(scene) = &owned_scene {
                scene.render(&mut pass);
            } else {
                view::render(
                    &setup,
                    &frame,
                    &atlas,
                    bg.as_source(),
                    &mut pass,
                    crate::render::top_left_projection_matrix(),
                );
            }
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        let mut worst = 0.0f32;
        let mut checks = 0usize;
        // Independent native-coordinate formulas, not production draw descriptions.
        for y in (0..1080).step_by(3) {
            for x in (0..1920).step_by(3) {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let mut expected = sample(&background, px / 1920.0, py / 1080.0);
                let im = |name: &str| &cpu_atlas[original.name_to_index[name]];
                let full = [0.0, 0.0, 1.0, 1.0];
                let white = [1.0; 4];
                let lx = if case == 0 { 840.0 } else { 96.0 } + frame.logo_offset;
                quad(
                    &mut expected,
                    im("logo"),
                    [lx, 64.0, lx + 1014.0, 424.0],
                    full,
                    color([1.0, 1.0, 1.0, frame.logo_alpha]),
                    px,
                    py,
                );
                quad(
                    &mut expected,
                    im("copyright"),
                    [0.0, 952.0, 672.0, 1080.0],
                    full,
                    white,
                    px,
                    py,
                );
                if let Some(caption) = frame.caption {
                    quad(
                        &mut expected,
                        im(caption.caption()),
                        [96.0, 448.0, 976.0, 948.0],
                        full,
                        color([1.0, 1.0, 1.0, frame.caption_alpha]),
                        px,
                        py,
                    );
                }
                for (index, (&item, row)) in setup.items.iter().zip(&frame.rows).enumerate() {
                    let x0 = row.x;
                    let y0 = row.y;
                    let tint = color([
                        0.2157 + (1.0 - 0.2157) * row.focus,
                        0.1059 + (0.1608 - 0.1059) * row.focus,
                        0.1059 + (0.1608 - 0.1059) * row.focus,
                        1.0,
                    ]);
                    quad(
                        &mut expected,
                        im("plate"),
                        [x0, y0 + 1.0, x0 + 704.0, y0 + 81.0],
                        [
                            0.0,
                            (index as f32 * 82.0 + 1.0) / 902.0,
                            1.0,
                            (index as f32 * 82.0 + 81.0) / 902.0,
                        ],
                        tint,
                        px,
                        py,
                    );
                    quad(
                        &mut expected,
                        im("menu"),
                        [x0, y0, x0 + 704.0, y0 + 82.0],
                        [
                            0.0,
                            item as u8 as f32 / 11.0,
                            1.0,
                            (item as u8 as f32 + 1.0) / 11.0,
                        ],
                        white,
                        px,
                        py,
                    );
                    if px >= x0 && px < x0 + 704.0 && py >= y0 + 1.0 && py < y0 + 81.0 {
                        over(&mut expected, color([1.0, 1.0, 1.0, row.flash]));
                    }
                    quad(
                        &mut expected,
                        &background,
                        [x0, y0, x0 + 704.0, y0 + 82.0],
                        [
                            x0 / 1920.0,
                            y0 / 1080.0,
                            (x0 + 704.0) / 1920.0,
                            (y0 + 82.0) / 1080.0,
                        ],
                        color([1.0, 1.0, 1.0, 1.0 - row.opacity]),
                        px,
                        py,
                    );
                }
                over(&mut expected, color([0.0, 0.0, 0.0, frame.black_alpha]));
                for c in 0..3 {
                    let actual = pixels[((y * 1920 + x) * 4 + c) as usize] as f32;
                    let error = (actual - expected[c] * 255.0).abs();
                    worst = worst.max(error);
                    assert!(
                        error <= 3.0,
                        "Title case{case} pixel{x},{y} c{c}: GPU{actual} expected{} error{error}",
                        expected[c] * 255.0
                    );
                }
                checks += 1;
            }
        }
        std::fs::write(
            format!("/private/tmp/shin-title-native-ui-{case}.rgba"),
            pixels,
        )
        .unwrap();
        println!("Title UI case{case}: {checks} sampled pixels, maxRGBerror{worst}");
    }
    // Independent chooser pixel oracle: packed episodes use original IDs for
    // atlas bands; opacity covers the composed body with matching background.
    for case in 0..4 {
        let mut chooser =
            super::episode_chooser::EpisodeChooser::new(super::TitleItem::Characters, 0x8500)
                .unwrap();
        if case == 0 {
            chooser.advance_clocks(shin_core::time::Ticks::from_f32(5.0));
        } else {
            chooser.advance_clocks(shin_core::time::Ticks::from_f32(18.0));
            chooser.prepare_frame();
            chooser.prepare_frame();
            chooser.navigate(true, false);
            chooser.advance_clocks(shin_core::time::Ticks::from_f32(5.0));
            if case == 2 {
                chooser.confirm();
                chooser.advance_clocks(shin_core::time::Ticks::from_f32(2.0));
            }
            if case == 3 {
                chooser.close();
                chooser.advance_clocks(shin_core::time::Ticks::from_f32(5.0));
            }
        }
        // Deterministic background distinguishes background covers from UI alpha.
        let mut data = Vec::with_capacity(1920 * 1080 * 4);
        for y in 0..1080u32 {
            for x in 0..1920u32 {
                data.extend_from_slice(&[(x % 251) as u8, (y % 241) as u8, 73, 255]);
            }
        }
        let background = CpuImage {
            width: 1920,
            height: 1080,
            data,
        };
        let gpu = shin_render::gpu_texture::GpuTexture::new_static_with_data(
            &device,
            &queue,
            Some("Chooser oracle background"),
            winit::dpi::PhysicalSize::new(1920, 1080),
            wgpu::TextureFormat::Rgba8Unorm,
            &background.data,
        );
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Chooser reference",
            );
            super::view::sprite(
                &mut pass,
                gpu.as_source(),
                glam::vec4(0.0, 0.0, 1920.0, 1080.0),
                glam::vec4(0.0, 0.0, 1.0, 1.0),
                shin_core::primitives::color::UnormColor::WHITE,
                crate::render::top_left_projection_matrix(),
            );
            super::episode_chooser_view::render(&chooser, &atlas, gpu.as_source(), &mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        let mut worst = 0.0f32;
        for y in (300..580).step_by(2) {
            for x in (1450..1920).step_by(2) {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let mut expected = sample(&background, px / 1920.0, py / 1080.0);
                for (episode, row) in chooser.rows() {
                    if row.opacity.abs() < f32::EPSILON {
                        continue;
                    }
                    if px >= row.x && px < row.x + 400.0 && py >= row.y + 1.0 && py < row.y + 71.0 {
                        over(
                            &mut expected,
                            color([
                                0.2157 + (1.0 - 0.2157) * row.focus,
                                0.1059 + (0.1608 - 0.1059) * row.focus,
                                0.1059 + (0.1608 - 0.1059) * row.focus,
                                1.0,
                            ]),
                        );
                    }
                    quad(
                        &mut expected,
                        &cpu_atlas[original.name_to_index["menu2"]],
                        [row.x + 80.0, row.y + 4.0, row.x + 400.0, row.y + 68.0],
                        [0.0, episode as f32 / 8.0, 1.0, (episode as f32 + 1.0) / 8.0],
                        [1.0; 4],
                        px,
                        py,
                    );
                    if px >= row.x && px < row.x + 400.0 && py >= row.y && py < row.y + 72.0 {
                        over(&mut expected, color([1.0, 1.0, 1.0, row.flash]));
                    }
                    quad(
                        &mut expected,
                        &background,
                        [row.x, row.y, row.x + 400.0, row.y + 72.0],
                        [
                            row.x / 1920.0,
                            row.y / 1080.0,
                            (row.x + 400.0) / 1920.0,
                            (row.y + 72.0) / 1080.0,
                        ],
                        color([1.0, 1.0, 1.0, 1.0 - row.opacity]),
                        px,
                        py,
                    );
                }
                for channel in 0..3 {
                    let error = (pixels[((y * 1920 + x) * 4 + channel) as usize] as f32
                        - expected[channel as usize] * 255.0)
                        .abs();
                    worst = worst.max(error);
                    assert!(
                        error <= 3.0,
                        "Chooser case{case} pixel{x},{y} c{channel} error{error}"
                    );
                }
            }
        }
        println!("Chooser GPU case{case}: max RGB error{worst}");
    }
    // MusicMode309c0: independent original-atlas oracle for list geometry,
    // texture-preserving two-pass highlight, clipped rows and bg edge covers.
    let music_original =
        shin_core::format::texture_archive::read_texture_archive(&read("/bgmmode.txa")).unwrap();
    let music_cpu = music_original
        .textures
        .iter()
        .map(|im| CpuImage {
            width: im.width(),
            height: im.height(),
            data: im.as_raw().clone(),
        })
        .collect::<Vec<_>>();
    let system_original =
        shin_core::format::texture_archive::read_texture_archive(&read("/systex.txa")).unwrap();
    let scroll_image = &system_original.textures[system_original.name_to_index["scroll"]];
    let scroll_cpu = CpuImage {
        width: scroll_image.width(),
        height: scroll_image.height(),
        data: scroll_image.as_raw().clone(),
    };
    let music_atlas: Arc<CharacterTextures> = server.load_sync("/bgmmode.txa").unwrap();
    let system_atlas: Arc<CharacterTextures> = server.load_sync("/systex.txa").unwrap();
    let scenario = shin_core::format::scenario::Scenario::new(read("/main.snr").into()).unwrap();
    let music_items = &scenario.info_tables().music_box_info;
    assert_eq!(music_items.len(), 215);
    for (name, dimensions) in [
        ("bg", (1920, 1080)),
        ("menu_off", (800, 88)),
        ("menu_on", (800, 88)),
        ("title0", (720, 2048)),
    ] {
        assert_eq!(
            music_original.textures[music_original.name_to_index[name]].dimensions(),
            dimensions
        );
    }
    for case in 0..5 {
        use shin_core::time::Ticks;
        let mut save = SaveData::default();
        if case != 0 {
            for item in music_items.iter().take(20) {
                save.save_vectors
                    .unlocked_bgms
                    .add(item.bgm_id.into_i32() as usize);
            }
        }
        let mut gallery = super::music_gallery::MusicGallery::new(music_items, &save).unwrap();
        gallery.publish_resources();
        gallery.update(Ticks::from_f32(60.0), false);
        gallery.update(Ticks::ZERO, false);
        if case == 1 {
            gallery.play();
            gallery.press_close();
            gallery.update(Ticks::from_f32(1.5), true);
        }
        if case == 2 {
            gallery.press_row(17);
            gallery.update(Ticks::from_f32(2.0), false);
        }
        if case == 3 {
            gallery.press_row(214);
            gallery.update(Ticks::from_f32(4.0), false);
        }
        if case == 4 {
            use crate::adv::input_area::{PointerEvent, PointerFrame, PointerKind};
            let mut input = super::music_gallery_input::MusicGalleryInput::new();
            input.resize(&mut gallery, (960, 540));
            let event = |kind, x, y| PointerEvent {
                kind,
                id: 0,
                position: glam::ivec2(x, y),
            };
            for (n, time, events) in [
                (0, 0.0, vec![event(PointerKind::Down, 480, 150)]),
                (1, 0.2, vec![event(PointerKind::Move, 480, 110)]),
                (2, 0.21, vec![event(PointerKind::Up, 0, 0)]),
            ] {
                input.input_frame(
                    &mut gallery,
                    &PointerFrame {
                        number: n,
                        time_seconds: time,
                        events: &events,
                    },
                    &Default::default(),
                    0.0,
                );
                if n < 2 {
                    gallery.update(Ticks::ZERO, false);
                }
            }
            assert!((gallery.list().scroll() - 80.0).abs() < 0.001);
            assert!(gallery.velocity() < -130.0 && gallery.velocity() > -140.0);
            gallery.update(Ticks::from_f32(2.0), false);
            assert!((gallery.list().scroll() - 346.93332).abs() < 0.01);
            assert_eq!(gallery.list().selected(), 8);
            assert!(gallery.take_music_requests().is_empty());
        }
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Music gallery reference",
            );
            super::music_gallery_view::render_artwork(
                &gallery,
                &music_atlas,
                &system_atlas,
                &mut pass,
            );
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        let mut worst = 0.0f32;
        // Include both five-pixel edge covers, the gaps and the viewport exterior.
        for y in (0..1080u32).step_by(3) {
            for x in (0..1920u32).step_by(11) {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let bg = &music_cpu[music_original.name_to_index["bg"]];
                let mut expected = sample(bg, px / 1920.0, py / 1080.0);
                if (136.0..1784.0).contains(&px) && (183.0..923.0).contains(&py) {
                    let list = gallery.list();
                    for (i, row) in list.rows().iter().enumerate() {
                        let left = 150.0 + (i % 2) as f32 * 820.0;
                        let top = 188.0 + (i / 2) as f32 * 92.0 - list.scroll();
                        let (focus, flash) = list.feedback(i);
                        let plate = if list.playing() == Some(i) {
                            "menu_on"
                        } else {
                            "menu_off"
                        };
                        let title = format!("title{}", row.name_index / 32);
                        let v = (row.name_index % 32) as f32 / 32.0;
                        for (name, rect, uv) in [
                            (
                                plate,
                                [left, top, left + 800.0, top + 88.0],
                                [0.0, 0.0, 1.0, 1.0],
                            ),
                            (
                                title.as_str(),
                                [left + 40.0, top + 14.0, left + 760.0, top + 78.0],
                                [0.0, v, 1.0, v + 1.0 / 32.0],
                            ),
                        ] {
                            if px >= rect[0] && px < rect[2] && py >= rect[1] && py < rect[3] {
                                let mut texel = sample(
                                    &music_cpu[music_original.name_to_index[name]],
                                    uv[0] + (uv[2] - uv[0]) * (px - rect[0]) / (rect[2] - rect[0]),
                                    uv[1] + (uv[3] - uv[1]) * (py - rect[1]) / (rect[3] - rect[1]),
                                );
                                for c in 0..3 {
                                    texel[c] += (1.0 - texel[c]) * focus * 0.5;
                                    texel[c] += (1.0 - texel[c]) * flash;
                                }
                                over(&mut expected, texel);
                            }
                        }
                    }
                    let edge = if py < 188.0 {
                        (188.0 - py) / 5.0
                    } else if py >= 918.0 {
                        (py - 918.0) / 5.0
                    } else {
                        0.0
                    };
                    if edge > 0.0 {
                        let mut texel = sample(bg, px / 1920.0, py / 1080.0);
                        texel[3] *= edge;
                        over(&mut expected, texel);
                    }
                }
                let list = gallery.list();
                let height = list.content_height().ceil();
                if height > 730.0 {
                    let scroll = list.scroll().trunc();
                    let start = 714.0 * scroll / height;
                    let end = 714.0 * (scroll + 730.0) / height;
                    for (u, ys) in [
                        (0.0, [188.0, 196.0, 910.0, 918.0]),
                        (
                            0.5,
                            [188.0 + start, 196.0 + start, 196.0 + end, 204.0 + end],
                        ),
                    ] {
                        for band in 0..3 {
                            quad(
                                &mut expected,
                                &scroll_cpu,
                                [1788.0, ys[band], 1804.0, ys[band + 1]],
                                [u, band as f32 / 4.0, u + 0.5, (band + 1) as f32 / 4.0],
                                [1.0; 4],
                                px,
                                py,
                            );
                        }
                    }
                }
                if (1764.0..1848.0).contains(&px) && (32.0..116.0).contains(&py) {
                    let mut texel = sample(
                        &music_cpu[music_original.name_to_index["close"]],
                        (px - 1764.0) / 84.0,
                        (py - 32.0) / 84.0,
                    );
                    for c in 0..3 {
                        texel[c] += (1.0 - texel[c]) * gallery.close_flash();
                    }
                    over(&mut expected, texel);
                }
                let mut right = 1872.0;
                for row in [1usize, 3, 2, 0] {
                    let (keys, width) = [
                        ([7, 0], 206.0),
                        ([4, -1], 72.0),
                        ([3, -1], 76.0),
                        ([6, -1], 74.0),
                    ][row];
                    let enabled = match row {
                        3 => list.playing().is_some(),
                        2 => list.rows()[list.selected()].unlocked,
                        _ => true,
                    };
                    let rgb = if enabled { 1.0 } else { 128.0 / 255.0 };
                    let tint = [rgb, rgb, rgb, 1.0];
                    let left = right - width;
                    quad(
                        &mut expected,
                        &music_cpu[music_original.name_to_index["help_bgmmode"]],
                        [left, 1012.0, right, 1060.0],
                        [0.0, row as f32 / 4.0, width / 320.0, (row + 1) as f32 / 4.0],
                        tint,
                        px,
                        py,
                    );
                    let mut icon_left = left + 2.0;
                    for key in keys.into_iter().filter(|key| *key >= 0) {
                        icon_left -= 48.0;
                        let u = (key & 7) as f32 / 8.0;
                        let v = (key >> 3) as f32 / 2.0;
                        quad(
                            &mut expected,
                            &music_cpu[music_original.name_to_index["key"]],
                            [icon_left, 1012.0, icon_left + 48.0, 1060.0],
                            [u, v, u + 0.125, v + 0.5],
                            tint,
                            px,
                            py,
                        );
                    }
                    right = icon_left - 16.0;
                }
                for c in 0..3usize {
                    let error = (pixels[((y * 1920 + x) * 4) as usize + c] as f32
                        - expected[c] * 255.0)
                        .abs();
                    worst = worst.max(error);
                    assert!(
                        error <= 3.0,
                        "Music gallery case{case} pixel{x},{y} channel{c} error{error}"
                    );
                }
            }
        }
        println!("Music gallery GPU case{case}: max RGB error{worst}");
    }
    // MovieMode883b0 independent original-bitmap oracle: centered focus scale,
    // opaque thumbnails, locked-logo overlay, flat flashes, close and footer.
    let movie_original =
        shin_core::format::texture_archive::read_texture_archive(&read("/moviemode.txa")).unwrap();
    let movie_cpu = movie_original
        .textures
        .iter()
        .map(|im| CpuImage {
            width: im.width(),
            height: im.height(),
            data: im.as_raw().clone(),
        })
        .collect::<Vec<_>>();
    let movie_atlas: Arc<CharacterTextures> = server.load_sync("/moviemode.txa").unwrap();
    for case in 0..3 {
        use shin_core::time::Ticks;
        let mut save = SaveData::default();
        if case > 0 {
            for id in 0..10 {
                if case == 1 || id % 2 == 0 {
                    save.save_vectors.unlocked_movies.add(id);
                }
            }
        }
        let mut model =
            super::movie_gallery::MovieGalleryList::new(&scenario.info_tables().movie_info, &save)
                .unwrap();
        if case == 1 {
            model.select(9);
            model.update(Ticks::from_f32(2.0));
            model.play();
            model.update(Ticks::from_f32(2.0));
        }
        if case == 2 {
            model.select(1);
            model.update(Ticks::from_f32(1.0));
            model.pointer_probe(glam::vec2(1800.0, 40.0));
            model.pointer_press();
            model.update(Ticks::from_f32(2.0));
            model.play();
            model.update(Ticks::from_f32(1.0));
        }
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "MovieMode original artwork reference",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::from_rgba(
                    255, 0, 255, 255,
                )),
                None,
                None,
            );
            super::movie_gallery_view::render_artwork(&model, &movie_atlas, &mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        let im = |name: &str| &movie_cpu[movie_original.name_to_index[name]];
        let mut worst = 0.0f32;
        for y in (1..1080).step_by(4) {
            for x in (1..1920).step_by(4) {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let mut expected = sample(im("bg"), px / 1920.0, py / 1080.0);
                for index in 0..10 {
                    let (focus, flash) = model.feedback(index);
                    let scale = 1.0 + focus * 0.05;
                    let cx = 342.0 + (index % 4) as f32 * 412.0;
                    let cy = 311.0 + (index / 4) as f32 * 242.0;
                    let rect = [
                        cx - 192.0 * scale,
                        cy - 108.0 * scale,
                        cx + 192.0 * scale,
                        cy + 108.0 * scale,
                    ];
                    if px >= rect[0] && px < rect[2] && py >= rect[1] && py < rect[3] {
                        expected = sample(
                            im(&format!("mv{:02}", index + 1)),
                            (px - rect[0]) / (rect[2] - rect[0]),
                            (py - rect[1]) / (rect[3] - rect[1]),
                        );
                        if !model.rows()[index].unlocked {
                            quad(
                                &mut expected,
                                im("logo"),
                                rect,
                                [0.0, 0.0, 1.0, 1.0],
                                [1.0; 4],
                                px,
                                py,
                            );
                        }
                        for alpha in [focus * 0.5, flash] {
                            over(&mut expected, color([1.0, 1.0, 1.0, alpha]));
                        }
                    }
                }
                let close = [1764.0, 32.0, 1848.0, 116.0];
                if px >= close[0] && px < close[2] && py >= close[1] && py < close[3] {
                    let mut tex =
                        sample(im("close"), (px - close[0]) / 84.0, (py - close[1]) / 84.0);
                    let alpha = color([1.0, 1.0, 1.0, model.close_flash()])[3];
                    for c in 0..3 {
                        tex[c] = tex[c] * (1.0 - alpha) + alpha;
                    }
                    over(&mut expected, tex);
                }
                let mut right = 1872.0;
                for (row, key, width) in [(1, 4, 72.0), (2, 3, 76.0), (0, 0, 206.0)] {
                    let rgb = if row == 2 && !model.rows()[model.selected()].unlocked {
                        128.0 / 255.0
                    } else {
                        1.0
                    };
                    let tint = [rgb, rgb, rgb, 1.0];
                    let left = right - width;
                    quad(
                        &mut expected,
                        im("help_moviemode"),
                        [left, 1012.0, right, 1060.0],
                        [0.0, row as f32 / 3.0, width / 320.0, (row + 1) as f32 / 3.0],
                        tint,
                        px,
                        py,
                    );
                    let icon_left = left - 46.0;
                    let u = (key % 8) as f32 / 8.0;
                    let v = (key / 8) as f32 / 2.0;
                    quad(
                        &mut expected,
                        im("key"),
                        [icon_left, 1012.0, icon_left + 48.0, 1060.0],
                        [u, v, u + 0.125, v + 0.5],
                        tint,
                        px,
                        py,
                    );
                    right = icon_left - 10.0;
                }
                for c in 0..3 {
                    let error = (pixels[((y * 1920 + x) * 4) as usize + c] as f32
                        - expected[c] * 255.0)
                        .abs();
                    worst = worst.max(error);
                    assert!(
                        error <= 3.0,
                        "MovieMode case{case} pixel{x},{y} channel{c} error{error}"
                    );
                }
            }
        }
        println!("MovieMode GPU case{case}:129600 sampled pixels, maxRGBerror{worst}");
    }
    // The completion label is native SimpleText(style1), width640/size40,
    // positioned at48,1012 with the supplied system font, white fill and dark brown outline.
    let font: Arc<crate::asset::font::GpuFontLazy> = server.load_sync("/system.fnt").unwrap();
    for (unlocked, expected_label) in [
        (0usize, "達成率:0.0％"),
        (1, "達成率:0.4％"),
        (215, "達成率:100％"),
    ] {
        let mut save = SaveData::default();
        for item in music_items.iter().take(unlocked) {
            save.save_vectors
                .unlocked_bgms
                .add(item.bgm_id.into_i32() as usize);
        }
        let list = super::music_gallery::MusicGalleryList::new(music_items, &save).unwrap();
        assert_eq!(list.completion_label(), expected_label);
        let text = super::music_gallery_text::MusicCompletionText::new(&server, &font, &list);
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Music completion font reference",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::WHITE),
                None,
                None,
            );
            text.render(&mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        let mut changed = 0;
        let mut brown = 0;
        for (i, pixel) in pixels.chunks_exact(4).enumerate() {
            if pixel[..3] != [255, 255, 255] {
                changed += 1;
                let (x, y) = (i % 1920, i / 1920);
                assert!(
                    (40..700).contains(&x) && (1001..1080).contains(&y),
                    "Completion text escaped native region{x},{y}"
                );
                if pixel[0].abs_diff(47) <= 2
                    && pixel[1].abs_diff(25) <= 2
                    && pixel[2].abs_diff(20) <= 2
                {
                    brown += 1;
                }
            }
        }
        assert!(
            changed > 1000 && brown > 100,
            "Supplied completion text did not render original glyphs/color"
        );
        println!(
            "Music completion {expected_label}: {changed} pixels, {brown} native brown outline pixels"
        );
    }
    super::music_gallery_audio::reference_gallery_audio_dispatch(server.clone(), &scenario);
    crate::app::SceneHost::reference_music_roundtrip(&server, &mut context, &target, &view);
    super::episode_chooser_scene::reference_title_chooser_roundtrips(
        server.clone(),
        &mut context,
        &target,
        &view,
    );
}

#[test]
#[ignore = "requires supplied ROM, Metal, Loopback Audio and SHIN_REFERENCE_MUSIC_REPEAT_ONLY"]
fn reference_music_repeat_gallery_audio_and_scene_roundtrip() {
    assert!(std::env::var_os("SHIN_REFERENCE_MUSIC_REPEAT_ONLY").is_some());
    reference_title_gpu_artwork_rows_and_background_cover();
}

#[test]
#[ignore = "requires supplied ROM/Metal and a read-only SHIN_REFERENCE_BACKLOG_SAVE snapshot"]
fn reference_saved_backlog_latency() {
    assert!(std::env::var_os("SHIN_REFERENCE_BACKLOG_LATENCY_ONLY").is_some());
    reference_title_gpu_artwork_rows_and_background_cover();
}

#[test]
#[ignore = "requires supplied ROM, Metal and exact Loopback Audio; no save writes"]
fn reference_gallery_return_fade_and_pointer_handoff() {
    assert!(std::env::var_os("SHIN_REFERENCE_GALLERY_RETURN_ONLY").is_some());
    reference_title_gpu_artwork_rows_and_background_cover();
}
