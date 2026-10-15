//! Independent native-coordinate pixel oracle for Start tab drawing.
use crate::{adv::character_view::CharacterTextures, render::PreRenderContext};
use std::sync::Arc;
#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_start_tab_gpu_order_scale_sparse_rows_and_new_badges() {
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
    if let Ok(patch) = std::env::var("SHIN_REFERENCE_PATCH_ROM") {
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

    use shin_core::{
        format::{rom::RomReader, save::SaveData},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(
                std::env::var("SHIN_REFERENCE_PATCH_ROM")
                    .or_else(|_| std::env::var("SHIN_REFERENCE_ROM"))
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let file = rom.find_file("/snrsel.txa").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let original = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
    let atlas: Arc<CharacterTextures> = server.load_sync("/snrsel.txa").unwrap();
    assert_eq!(
        original.textures[original.name_to_index["tab_off"]].dimensions(),
        (320, 432)
    );
    assert_eq!(
        original.textures[original.name_to_index["tab_on"]].dimensions(),
        (320, 432)
    );
    fn sample(data: &[u8], width: u32, height: u32, u: f32, v: f32) -> [f32; 4] {
        let x = u * width as f32 - 0.5;
        let y = v * height as f32 - 0.5;
        let ix = x.floor();
        let iy = y.floor();
        let fx = x - ix;
        let fy = y - iy;
        std::array::from_fn(|c| {
            let p = |dx: f32, dy: f32| {
                let x = (ix + dx).clamp(0.0, width as f32 - 1.0) as u32;
                let y = (iy + dy).clamp(0.0, height as f32 - 1.0) as u32;
                data[((y * width + x) * 4) as usize + c] as f32 / 255.0
            };
            (p(0.0, 0.0) * (1.0 - fx) + p(1.0, 0.0) * fx) * (1.0 - fy)
                + (p(0.0, 1.0) * (1.0 - fx) + p(1.0, 1.0) * fx) * fy
        })
    }
    for (case, ids, focus, flash) in [
        (
            "six",
            vec![0usize, 1, 2, 3, 4, 5],
            vec![0.0, 0.4, 0.8, 1.0, 0.6, 0.2],
            vec![0.0, 0.25, 0.0, 0.42, 0.0, 0.7],
        ),
        (
            "crossfade",
            vec![0, 1, 2, 3, 4, 5],
            vec![0.0, 0.0, 0.5, 0.5, 0.0, 0.0],
            vec![0.0; 6],
        ),
        (
            "sparse",
            vec![0, 3, 5],
            vec![0.5, 1.0, 0.0],
            vec![0.0, 0.42, 0.25],
        ),
        (
            "unlocked",
            vec![0, 1, 2, 3],
            vec![1.0, 0.0, 0.0, 0.0],
            vec![0.0; 4],
        ),
        ("single", vec![0], vec![1.0], vec![0.7]),
    ] {
        let mut save = SaveData::default();
        // Exact-one new marker predicate; selected entries can have other states.
        for &id in &ids {
            save.persist_data.set(
                [2, 9, 17, 25, 45, 47][id],
                if case == "unlocked" || id % 2 == 0 {
                    1
                } else {
                    2
                },
            );
        }
        let setup = super::scenario_select::ScenarioSelectSetup::from_save(&save);
        assert_eq!(
            setup
                .groups
                .iter()
                .map(|g| g.id as usize)
                .collect::<Vec<_>>(),
            ids
        );
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Start tabs native reference",
            );
            shin_render::quad_vertices::QuadVertices::new()
                .with_box(0.0, 0.0, 1920.0, 1080.0)
                .with_color(shin_core::primitives::color::UnormColor::BLACK)
                .fill(
                    &mut pass,
                    shin_render::RenderRequestBuilder::new(),
                    crate::render::top_left_projection_matrix(),
                );
            super::scenario_select_view::render_tabs(
                &setup,
                &focus,
                &flash,
                &atlas,
                &mut pass,
                crate::render::top_left_projection_matrix(),
            );
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        // Native short insertion sort: compare only focus and leave ties in order.
        let mut order: Vec<_> = (0..ids.len()).collect();
        for i in 1..order.len() {
            let mut j = i;
            while j > 0 && focus[order[j]] < focus[order[j - 1]] {
                order.swap(j, j - 1);
                j -= 1;
            }
        }
        let mut worst = 0.0f32;
        let mut checks = 0;
        for y in (0..1080).step_by(3) {
            for x in (0..1920).step_by(3) {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let mut expected = [0.0f32; 3];
                if ids.len() > 1 {
                    for &index in &order {
                        // Independent scalar inverse of native center-pivot transform.
                        let scale = 1.0 + focus[index] * 0.15;
                        let center_x =
                            960.0 - 150.0 * ids.len() as f32 + 300.0 * index as f32 + 150.0;
                        let lx = (px - center_x) / scale + 150.0;
                        let ly = (py - 177.75) / scale + 33.75;
                        let composite = |expected: &mut [f32; 3], texel: [f32; 4]| {
                            for c in 0..3 {
                                expected[c] = ((texel[c] * texel[3]
                                    + expected[c] * (1.0 - texel[3]))
                                    * 255.0)
                                    .round()
                                    / 255.0;
                            }
                        };
                        if (0.0..300.0).contains(&lx) && (0.0..67.5).contains(&ly) {
                            let u = lx / 300.0;
                            let v = ids[index] as f32 / 6.0 + ly / 67.5 / 6.0;
                            let get = |name: &str| {
                                let t = &original.textures[original.name_to_index[name]];
                                sample(t.as_raw(), t.width(), t.height(), u, v)
                            };
                            let off = get("tab_off");
                            let on = get("tab_on");
                            let mut mixed: [f32; 4] = std::array::from_fn(|c| {
                                off[c] * (1.0 - focus[index]) + on[c] * focus[index]
                            });
                            for c in 0..3 {
                                mixed[c] = mixed[c] * (1.0 - flash[index]) + flash[index];
                            }
                            composite(&mut expected, mixed);
                        }
                        if (case == "unlocked" || ids[index] % 2 == 0)
                            && (204.0..300.0).contains(&lx)
                            && (-8.0..32.0).contains(&ly)
                        {
                            let t = &original.textures[original.name_to_index["new"]];
                            let mut texel = sample(
                                t.as_raw(),
                                t.width(),
                                t.height(),
                                (lx - 204.0) / 96.0,
                                (ly + 8.0) / 40.0,
                            );
                            for c in 0..3 {
                                texel[c] = texel[c] * (1.0 - flash[index]) + flash[index];
                            }
                            composite(&mut expected, texel);
                        }
                    }
                }
                for c in 0..3 {
                    let actual = pixels[((y * 1920 + x) * 4 + c) as usize] as f32;
                    let error = (actual - expected[c] * 255.0).abs();
                    worst = worst.max(error);
                    assert!(
                        error <= 3.0,
                        "Start tabs {case} pixel{x},{y} channel{c}: GPU{actual} CPU{} error{error}",
                        expected[c] * 255.0
                    );
                }
                checks += 1;
            }
        }
        std::fs::write(format!("/private/tmp/shin-start-tabs-{case}.rgba"), pixels).unwrap();
        println!("Start tabs {case}: {checks} sampled pixels; maxRGBerror{worst}");
    }
}

#[test]
#[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
fn reference_native_start_tabs_pivot_sort_atlas_and_blend_program() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, expected) in [
        (0xdd32c, 0x1e2c3920),
        (0xdd334, 0x1e210800),
        (0xdd33c, 0x1e212801),
        (0xdeb30, 0x94000b94),
        (0xdeb9c, 0x91004100),
        (0xdebac, 0x9101a100),
        (0xdebdc, 0x1e2a0900),
        (0xdebe4, 0x1e222800),
        (0xdec24, 0xbd400280),
        (0xdec2c, 0x1e2b0800),
        (0xdec74, 0x2d5b0500),
        (0xded54, 0x9b1a7f28),
        (0xded60, 0x5e21d800),
        (0xded64, 0x1e211801),
        (0xded68, 0x1e2e2823),
        (0xded88, 0x4ea81d00),
        (0xdedc0, 0x97ffa0ec),
        (0xdedc8, 0x39401108),
        (0xdedcc, 0x34000488),
        (0xdedf8, 0x2d4707e3),
        (0xdedfc, 0x1e212800),
        (0xdee00, 0x1e2c2802),
        (0xdee04, 0x1e341001),
        (0xdee58, 0x97ffa02e),
        (0xe1a08, 0xf100151f),
        (0xe1a14, 0x5400210d),
        (0xe1e98, 0x1e202100),
        (0xe1ef4, 0x54fffdc4),
        (0xc7258, 0x321f07e0),
        (0xc726c, 0x9400214d),
        (0xcd868, 0xf900011f),
    ] {
        assert_eq!(word(a), expected, "native tab instruction {a:x}");
    }
    for (a, value) in [
        (0x15935c, 0.15f32),
        (0x159540, 1.0f32 / 6.0),
        (0x159818, 96.0),
        (0x159a7c, 144.0),
        (0x159730, 300.0),
        (0x15941c, 67.5),
        (0x159694, 32.0),
        (0x159b14, -96.0),
    ] {
        assert_eq!(word(a), value.to_bits(), "native tab float {a:x}");
    }
    let relocation = |target: u64| {
        (0x11e040..0x152eb8)
            .step_by(24)
            .find_map(|a| {
                let slot = u64::from_le_bytes(exe[a + 256..a + 264].try_into().unwrap());
                (slot == target)
                    .then(|| u64::from_le_bytes(exe[a + 272..a + 280].try_into().unwrap()))
            })
            .unwrap()
    };
    assert_eq!(relocation(0x1dc2e0), 0x1dd870);
    assert_eq!(relocation(0x1dc2e8), 0x1dcbb0);
    assert_eq!(relocation(0x2246b0), 0x22b194);
    for (offset, expected) in [
        (0x48, 0xd832008040570406),
        (0x50, 0xd83200a020370100),
        (0xb0, 0x49a0038c00170c02),
        (0xb8, 0x49a0030c00070000),
        (0xc8, 0x49a0020c00270d0d),
        (0xd0, 0x49a0028c00370305),
        (0xf8, 0x5c68100000b70503),
        (0x108, 0x49a0011000370402),
        (0x110, 0x49a0001000370100),
        (0x118, 0x49a0069000370606),
        (0x128, 0x5c68100000970201),
        (0x130, 0x5c68100000a70000),
        (0x138, 0x5c68100000870602),
    ] {
        let a = 0x1dd6f0 + offset + 256;
        assert_eq!(
            u64::from_le_bytes(exe[a..a + 8].try_into().unwrap()),
            expected,
            "native blend fragment {offset:x}"
        );
    }
    for (a, name) in [
        (0x15c0d2, "tab_off"),
        (0x15fffc, "tab_on"),
        (0x15c0c3, "new"),
    ] {
        assert_eq!(&exe[a + 256..a + 256 + name.len()], name.as_bytes());
    }
}

#[test]
#[ignore = "requires supplied updated executable next to SHIN_REFERENCE_ROM"]
fn reference_updated_native_tab_badge_right_edge() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable_update"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, expected) in [
        (0xdeb48, 0xbd4af500),
        (0xdeb7c, 0xbd003fe0),
        (0xdede0, 0xbd400280),
        (0xdede4, 0x2d4707e3),
        (0xdede8, 0x1e212800),
        (0xdedec, 0x1e2c2802),
    ] {
        assert_eq!(word(a), expected, "updated tab instruction {a:x}");
    }
    assert_eq!(word(0x15aaf4), (-96.0f32).to_bits());
    assert_eq!(word(0x15a7f8), 96.0f32.to_bits());
}
