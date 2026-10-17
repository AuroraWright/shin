use super::*;
use crate::{
    asset::{
        picture::Picture,
        system::{AssetServer, LayeredAssetIo},
    },
    cli::Game,
    layer::{
        DrawableLayer, Layer,
        user::{BustupLayer, higurashi::HigurashiUserLayer},
    },
};
use shin_core::{
    format::scenario::higurashi::HigurashiScenario,
    time::Ticks,
    vm::{
        command::CommandResult,
        command::types::{LayerProperty, LayerType},
        higurashi::{HigurashiScripter, command::HigurashiRuntimeCommand},
    },
};
use std::{io::Read, path::PathBuf};

fn words(data: &mut Vec<u8>, values: &[u32]) {
    for v in values {
        data.extend(v.to_le_bytes());
    }
}
fn fragment(color: [u8; 4]) -> Vec<u8> {
    let mut data = Vec::new();
    for v in [1u16, 1, 0, 0, 0, 0, 1, 1, 0, 0] {
        data.extend(v.to_le_bytes());
    }
    for v in [0u16, 0, 1, 1] {
        data.extend(v.to_le_bytes());
    }
    data.extend(color);
    data.extend([0; 12]);
    assert_eq!(data.len(), 44);
    data
}
fn picture(id: u32) -> Vec<u8> {
    let mut data = b"PIC4".to_vec();
    words(&mut data, &[2, 88, 0, 0x0001_0001, 1, 1, id]);
    words(&mut data, &[0, 44, 44]);
    data.extend(fragment([100, 150, 200, 255]));
    data
}
fn portrait() -> Vec<u8> {
    let start = 48 + 2 * 8 + 12 + 2 * 76;
    let locations: Vec<_> = (0..6)
        .map(|i| FileLocation {
            offset: start + i * 44,
            size: 44,
        })
        .collect();
    let mut data = b"BUP4".to_vec();
    words(
        &mut data,
        &[3, start + 6 * 44, 0, 0x0001_0001, 1, 2, 2, 1, 101, 0, 0],
    );
    let location = |loc: Option<usize>, data: &mut Vec<u8>| {
        let loc = loc
            .map(|i| locations[i])
            .unwrap_or(FileLocation { offset: 0, size: 0 });
        words(data, &[loc.offset, loc.size]);
    };
    location(Some(0), &mut data);
    location(None, &mut data);
    words(&mut data, &[202]);
    location(Some(1), &mut data);
    for name in ["a", "b"] {
        let mut encoded = [0u8; 16];
        encoded[0] = name.as_bytes()[0];
        data.extend(encoded);
        words(&mut data, &[0]);
        location(Some(2), &mut data);
        location(Some(3), &mut data);
        location(Some(5), &mut data);
        location(None, &mut data);
        location(Some(4), &mut data);
        location(None, &mut data);
        location(Some(5), &mut data);
    }
    assert_eq!(data.len(), start as usize);
    for (index, color) in [
        [20, 30, 40, 255],
        [40, 50, 60, 255],
        [60, 70, 80, 255],
        [80, 90, 100, 255],
        [200, 10, 10, 255],
        [10, 10, 200, 255],
    ]
    .into_iter()
    .enumerate()
    {
        let mut block = fragment(color);
        // Native 36bf4 -> 697f0 selects strict stencil comparison, so opaque
        // body/face fragments own their pixels before the later mouth draw.
        // Keep independent regions to observe every selected fragment.
        let x = [20u16, 30, 40, 10, 0, 0][index];
        block[8..10].copy_from_slice(&x.to_le_bytes());
        data.extend(block);
    }
    data
}
fn context(game: Game, device: &wgpu::Device, queue: &wgpu::Queue) -> AssetLoadContext {
    AssetLoadContext {
        game,
        higurashi_bustup_cache: Default::default(),
        wgpu_device: device.clone(),
        wgpu_queue: queue.clone(),
        bustup_cache: crate::asset::system::cache::AssetCache::new(),
        picture_cache: Default::default(),
        font_cache: Default::default(),
    }
}
fn gpu() -> (wgpu::Device, wgpu::Queue) {
    crate::init_test_task_pools();
    let instance = wgpu::Instance::default();
    let adapter =
        shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
            adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
        } else {
            wgpu::Features::empty()
        },
        ..Default::default()
    }))
    .unwrap()
}
fn args(expression: &str) -> BustupArgs {
    BustupArgs {
        expression: expression.into(),
        character_id: CharacterId::new(123),
        disable_animations: false,
    }
}
fn io(dir: &std::path::Path) -> LayeredAssetIo {
    let mut io = LayeredAssetIo::new();
    io.try_with_dir(dir).unwrap();
    io
}

#[test]
#[ignore = "Requires Metal; automated GPU asset/owner test, no window/audio/saves"]
fn higurashi_gpu_assets_preserve_owner_groups_sparse_slots_and_game_policy() {
    let (device, queue) = gpu();
    let dir = std::env::temp_dir().join(format!("higu-gpu-assets-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("picture.pic"), picture(303)).unwrap();
    std::fs::write(dir.join("portrait.bup"), portrait()).unwrap();
    let mut alias = portrait();
    alias[12..14].copy_from_slice(&42i16.to_le_bytes());
    std::fs::write(dir.join("alias.bup"), alias).unwrap();
    let server = AssetServer::new(io(&dir).into(), context(Game::Higurashi, &device, &queue));
    shin_tasks::block_on(async {
        let picture = server.load::<Picture, _>("/picture.pic").await.unwrap();
        assert_eq!(picture.scale, 1.0);
        assert_eq!(picture.blocks.iter_placements().count(), 1);
        let (a, b) = futures::join!(
            server.load_with_args::<HigurashiBustup, _>("/portrait.bup", args("a")),
            server.load_with_args::<HigurashiBustup, _>("/alias.bup", args("b"))
        );
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_eq!(a.origin_x, 0);
        assert_eq!(b.origin_x, 42);
        assert!(Arc::ptr_eq(&a.body, &b.body));
        assert_eq!(
            a.body
                .blocks
                .iter()
                .map(Option::is_some)
                .collect::<Vec<_>>(),
            [true, false]
        );
        assert!(Arc::ptr_eq(
            a.auxiliary.as_ref().unwrap(),
            b.auxiliary.as_ref().unwrap()
        ));
        assert_eq!(
            a.mouths.iter().map(Option::is_some).collect::<Vec<_>>(),
            [true, false, true]
        );
        assert!(a.eye.is_some());
        assert_eq!(
            server
                .context()
                .higurashi_bustup_cache
                .body
                .reference_ownership_counts(),
            (1, 0)
        );
        assert_eq!(
            server
                .context()
                .higurashi_bustup_cache
                .auxiliary
                .reference_ownership_counts(),
            (1, 0)
        );
        assert_eq!(
            server.context().bustup_cache.reference_ownership_counts(),
            (0, 0)
        );
        let mut layer = BustupLayer::new_higurashi(a.clone(), None);
        layer
            .properties_mut()
            .property_tweener_mut(LayerProperty::Prop27)
            .fast_forward_to(1.0);
        for level in [0, 1, 2, 3, 255] {
            layer.set_voice_lipsync(Some(level));
            assert_eq!(
                layer.reference_mouth_state(),
                if level <= 2 { u32::from(level) } else { 0 }
            );
        }
        for (level, color) in [
            (0, [200, 10, 10, 255]),
            (1, [0, 0, 0, 255]),
            (2, [10, 10, 200, 255]),
        ] {
            layer.set_voice_lipsync(Some(level));
            let rendered = render_layer(
                &device,
                &queue,
                &mut layer,
                glam::Mat4::from_scale(glam::Vec3::splat(5.0)),
            );
            let pixel = |x: usize, y: usize| &rendered[(y * 384 + x) * 4..(y * 384 + x + 1) * 4];
            assert_eq!(pixel(192, 108), &color, "fixed sparse mouth slot {level}");
            assert_eq!(pixel(202, 108), &[80, 90, 100, 255], "static eye slot zero");
            assert_eq!(pixel(212, 108), &[20, 30, 40, 255], "body");
            assert_eq!(pixel(222, 108), &[40, 50, 60, 255], "auxiliary");
            assert_eq!(pixel(232, 108), &[60, 70, 80, 255], "face");
        }

        // Exercise the existing Umineko layer branch with the same uploaded
        // geometry: its two-frame mouth policy scales feedback rather than
        // indexing the older fixed array. This covers the shared draw change.
        let umineko = Arc::new(super::super::Bustup {
            character_id: a.character_id,
            origin_x: a.origin_x,
            origin_y: a.origin_y,
            effective_width: a.effective_width,
            effective_height: a.effective_height,
            bustup_id: a.bustup_id,
            base_blocks: a.body.blocks.iter().flatten().cloned().collect(),
            face1: a.auxiliary.as_ref().and_then(|aux| aux.block.clone()),
            face2: a.face.clone(),
            mouth_blocks: vec![a.mouths[0].clone().unwrap(), a.mouths[2].clone().unwrap()],
            eye_blocks: Vec::new(),
        });
        let mut umineko_layer = BustupLayer::new(umineko, None);
        umineko_layer
            .properties_mut()
            .property_tweener_mut(LayerProperty::Prop27)
            .fast_forward_to(1.0);
        for (level, expected_state, color) in [
            (0, 0, [200, 10, 10, 255]),
            (1, 0, [200, 10, 10, 255]),
            (2, 1, [10, 10, 200, 255]),
        ] {
            umineko_layer.set_voice_lipsync(Some(level));
            assert_eq!(umineko_layer.reference_mouth_state(), expected_state);
            let rendered = render_layer(
                &device,
                &queue,
                &mut umineko_layer,
                glam::Mat4::from_scale(glam::Vec3::splat(5.0)),
            );
            assert_eq!(
                &rendered[(108 * 384 + 192) * 4..(108 * 384 + 193) * 4],
                &color,
                "Umineko two-frame mouth feedback {level}"
            );
        }
        drop(umineko_layer);

        layer
            .properties_mut()
            .property_tweener_mut(LayerProperty::Prop27)
            .fast_forward_to(0.0);
        layer.set_voice_lipsync(Some(2));
        assert_eq!(
            layer.reference_mouth_state(),
            2,
            "Higurashi does not use Umineko property27 permission"
        );
        assert!(
            server
                .load_with_args::<HigurashiBustup, _>("/portrait.bup", args("missing"))
                .await
                .is_err()
        );
        let body = Arc::downgrade(&a.body);
        let aux = Arc::downgrade(a.auxiliary.as_ref().unwrap());
        drop(a);
        drop(b);
        assert!(body.upgrade().is_some());
        drop(layer);
        assert!(body.upgrade().is_none());
        assert!(aux.upgrade().is_none());
        let umi = AssetServer::new(io(&dir).into(), context(Game::Umineko, &device, &queue));
        assert!(umi.load::<Picture, _>("/picture.pic").await.is_err());
        assert!(
            umi.load_with_args::<HigurashiBustup, _>("/portrait.bup", args("a"))
                .await
                .is_err()
        );
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    });
    println!(
        "Higurashi GPU synthetic assets: picture v2, body/aux cache owners, per-file origins, sparse mouth slots, static eye, feedback levels, failure and final-owner retirement pass; no audio/window/saves"
    );
}

#[test]
#[ignore = "Requires supplied Higurashi archives and Metal; no window/audio/saves"]
fn higurashi_updated_prologue_factory_loads_authored_pictures_and_selected_characters() {
    use crate::asset::system::locate_assets_for_game;
    let root = PathBuf::from(std::env::var("SHIN_HIGURASHI_ASSETS").unwrap());
    let (device, queue) = gpu();
    let server = AssetServer::new(
        locate_assets_for_game(Game::Higurashi, &root)
            .unwrap()
            .into(),
        context(Game::Higurashi, &device, &queue),
    );
    shin_tasks::block_on(async {
        let scenario = server
            .load::<HigurashiScenario, _>("/main.snr")
            .await
            .unwrap();
        assert_eq!(
            scenario.variant,
            shin_core::format::scenario::higurashi::HigurashiVariant::Updated
        );
        let mut vm = HigurashiScripter::new(&scenario, 0, 42).unwrap();
        let mut result = CommandResult::None;
        let mut picture_ids = std::collections::BTreeSet::new();
        let mut tiles = 0;
        for _ in 0..100 {
            let command = vm.run(result, 100000).unwrap();
            result = CommandResult::None;
            match command {
                HigurashiRuntimeCommand::SGET { dest, slot: 2 } => {
                    result = CommandResult::WriteMemory(dest, 0)
                }
                HigurashiRuntimeCommand::LAYERLOAD {
                    args: [_, 1, _],
                    params,
                } => {
                    let layer = HigurashiUserLayer::load(
                        &server,
                        &scenario,
                        LayerType::Tile,
                        params,
                        [1, 2, 3, 4],
                    )
                    .await
                    .unwrap();
                    assert_eq!(layer.layer_type(), LayerType::Tile);
                    for size in [(1280, 720), (1920, 1080)] {
                        let pixels = render_higurashi(&device, &queue, &layer, size);
                        assert!(
                            pixels.chunks_exact(4).all(|p| p == [255, 255, 255, 255]),
                            "authored first-scene Tile covers logical viewport"
                        );
                    }
                    tiles += 1;
                }
                HigurashiRuntimeCommand::LAYERLOAD {
                    args: [_, 2, _],
                    params,
                } => {
                    picture_ids.insert(params.0);
                    let layer = HigurashiUserLayer::load(
                        &server,
                        &scenario,
                        LayerType::Picture,
                        params,
                        [1, 2, 3, 4],
                    )
                    .await
                    .unwrap();
                    assert_eq!(layer.layer_type(), LayerType::Picture);
                    for size in [(1280, 720), (1920, 1080)] {
                        let pixels = render_higurashi(&device, &queue, &layer, size);
                        assert_eq!(pixels.len(), (size.0 * size.1 * 4) as usize);
                        let visible = pixels
                            .chunks_exact(4)
                            .filter(|p| p[..3] != [0, 0, 0])
                            .count();
                        println!(
                            "Higurashi authored PIC {} at {}x{}: {} nonblack pixels",
                            params.0, size.0, size.1, visible
                        );
                    }
                }
                _ => {}
            }
        }
        assert!(!picture_ids.is_empty());
        assert_eq!(
            tiles, 1,
            "first prologue prefix includes its solid Tile layer"
        );
        for path in ["/bustup/l/aks2.bup", "/bustup/org/m/kei7.bup"] {
            let mut rom = shin_core::format::rom::RomReader::new(
                shin_core::primitives::stateless_reader::StatelessFile::new(
                    std::fs::File::open(root.join(if path.contains("org/") {
                        "append.rom"
                    } else {
                        "data.rom"
                    }))
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            let mut bytes = Vec::new();
            rom.open_file(rom.find_file(path).unwrap())
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            let meta = HigurashiBustupMetadata::read(&bytes).unwrap();
            let expression = &meta.expressions[0].name;
            let asset = server
                .load_with_args::<HigurashiBustup, _>(path, args(expression))
                .await
                .unwrap();
            assert!(asset.body.blocks.iter().any(Option::is_some));
            assert_eq!(
                asset.mouths.iter().map(Option::is_some).collect::<Vec<_>>(),
                meta.expressions[0].mouths.map(|loc| !loc.is_null())
            );
            let layer = HigurashiUserLayer::bustup(asset, None, [1, 2, 3, 4]);
            for size in [(1280, 720), (1920, 1080)] {
                let pixels = render_higurashi(&device, &queue, &layer, size);
                let visible = pixels
                    .chunks_exact(4)
                    .filter(|p| p[..3] != [0, 0, 0])
                    .count();
                assert!(visible > 0, "authored bustup {path} must render");
                println!(
                    "Higurashi authored BUP {path} at {}x{}: {visible} nonblack pixels",
                    size.0, size.1
                );
            }
            drop(layer);
            println!("Higurashi authored GPU bustup {path}: {expression}");
        }
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        println!(
            "Higurashi prologue factory: {} distinct pictures load into production GPU layers: {:?}; selected base/append characters load",
            picture_ids.len(),
            picture_ids
        );
    });
}

#[test]
#[ignore = "Requires Metal; older drawable/projection at720p/1080p; no window/audio/saves"]
fn higurashi_drawable_geometry_sparse_voice_and_viewport_sizes() {
    use shin_core::time::Ticks;
    let (device, queue) = gpu();
    let dir = std::env::temp_dir().join(format!("higu-drawable-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("picture.pic"), picture(707)).unwrap();
    std::fs::write(dir.join("portrait.bup"), portrait()).unwrap();
    let server = AssetServer::new(io(&dir).into(), context(Game::Higurashi, &device, &queue));
    shin_tasks::block_on(async {
        let pic = server.load::<Picture, _>("/picture.pic").await.unwrap();
        let mut picture = HigurashiUserLayer::picture(pic, None, [11, 12, 13, 14]);
        let bup = server
            .load_with_args::<HigurashiBustup, _>("/portrait.bup", args("a"))
            .await
            .unwrap();
        let mut bustup = HigurashiUserLayer::bustup(bup, None, [21, 22, 23, 24]);
        for layer in [&mut picture, &mut bustup] {
            // A threefold layer scale makes a one-pixel authored fragment cover
            // two logical pixels after the native device conversion.
            for (index, value) in [(1, 360.0), (12, 3000.0), (13, 3000.0)] {
                layer
                    .properties
                    .control(index, value, Ticks::ZERO, 0)
                    .unwrap();
            }
        }
        for size in [(1280, 720), (1920, 1080)] {
            let mut tile = HigurashiUserLayer::tile(
                shin_core::primitives::color::FloatColor4::WHITE,
                glam::vec4(-640.0, -720.0, 1280.0, 720.0),
                [31, 32, 33, 34],
            );
            let pixels = render_higurashi(&device, &queue, &tile, size);
            assert!(
                pixels.chunks_exact(4).all(|p| p == [255, 255, 255, 255]),
                "full logical Tile at{size:?}"
            );
            for index in 6..9 {
                tile.properties.control(index, 0.0, Ticks::ZERO, 0).unwrap();
            }
            let pixels = render_higurashi(&device, &queue, &tile, size);
            assert!(
                pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]),
                "prologue black Tile at{size:?}"
            );
            for (index, value) in [(6, 1500.0), (7, 500.0), (8, 0.0), (21, 4.0)] {
                tile.properties
                    .control(index, value, Ticks::ZERO, 0)
                    .unwrap();
            }
            let pixels = render_higurashi(&device, &queue, &tile, size);
            assert!(
                pixels.chunks_exact(4).all(|p| p == [0, 127, 255, 255]),
                "pre-shader Tile tint clamp at{size:?}"
            );
            for (index, value) in [(6, 1000.0), (7, 1000.0), (8, 1000.0), (21, 0.0), (9, 500.0)] {
                tile.properties
                    .control(index, value, Ticks::ZERO, 0)
                    .unwrap();
            }
            let pixels = render_higurashi(&device, &queue, &tile, size);
            assert!(
                pixels.chunks_exact(4).all(|p| p == [127, 127, 127, 255]),
                "transparent Tile at{size:?}"
            );
            println!(
                "Higurashi Tile {size:?}: full logical viewport, prologue black, pre-shader tint clamp and transparent pass"
            );
            let sample = |pixels: &[u8], x: u32| {
                let x = ((640 + 2 * x) * size.0 / 1280) + 1;
                let y = (360 * size.1 / 720) + 1;
                let at = ((y * size.0 + x) * 4) as usize;
                <[u8; 4]>::try_from(&pixels[at..at + 4]).unwrap()
            };
            let pixels = render_higurashi(&device, &queue, &picture, size);
            assert_eq!(sample(&pixels, 0), [100, 150, 200, 255]);
            for level in [0u8, 1, 2, 3, 255] {
                // Older p27 is a waveform mode, not Umineko's lip-sync gate.
                bustup.properties.control(27, 0.0, Ticks::ZERO, 0).unwrap();
                bustup.advance(
                    Ticks::ZERO,
                    Some(&|character, pan| {
                        assert_eq!(character, 123);
                        assert_eq!(pan, 0.0);
                        Some(level)
                    }),
                );
                let pixels = render_higurashi(&device, &queue, &bustup, size);
                assert_eq!(
                    sample(&pixels, 0),
                    match level {
                        1 => [0, 0, 0, 255],
                        2 => [10, 10, 200, 255],
                        _ => [200, 10, 10, 255],
                    },
                    "mouth{level} at{size:?}"
                );
                for (x, color) in [
                    (10, [80, 90, 100, 255]),
                    (20, [20, 30, 40, 255]),
                    (30, [40, 50, 60, 255]),
                    (40, [60, 70, 80, 255]),
                ] {
                    assert_eq!(sample(&pixels, x), color, "block{x} at{size:?}");
                }
            }
            println!(
                "Higurashi drawable {size:?}: native asset scale/projection, body/aux/face/static eye, three sparse mouths, voice callback pan"
            );
        }
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    });
    std::fs::remove_dir_all(dir).unwrap();
}

fn render_layer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layer: &mut impl Layer,
    parent: glam::Mat4,
) -> Vec<u8> {
    render_to_texture(device, queue, (384, 216), |context, view| {
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let transform = crate::layer::render_params::TransformParams {
            transform: parent,
            ..Default::default()
        };
        layer.pre_render(context, &transform);
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Higurashi automated layer render",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            for kind in [
                shin_render::PassKind::Opaque,
                shin_render::PassKind::Transparent,
            ] {
                layer.render(&mut pass, &transform, 1, kind);
            }
        }
    })
}

fn render_to_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    render: impl FnOnce(&mut crate::render::PreRenderContext, &wgpu::TextureView),
) -> Vec<u8> {
    use shin_render::{
        dynamic_buffer::DynamicBuffer,
        pipelines::PipelineStorage,
        resize::{SurfaceResizeSource, ViewportParams},
        shaders::types::{
            buffer::BytesAddress,
            texture::{DepthStencilTarget, TextureSamplerStore},
        },
    };
    let samplers = TextureSamplerStore::new(device);
    let mut pipelines = PipelineStorage::new(device.clone(), wgpu::TextureFormat::Rgba8Unorm);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        size.0, size.1,
    )));
    let make = |format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let target = make(
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let depth = make(
        shin_render::DEPTH_STENCIL_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    );
    let view = target.create_view(&Default::default());
    let depth = depth.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut context = crate::render::PreRenderContext {
        device,
        queue,
        breakup_texture: None,
        resize_source: &resize,
        sampler_store: &samplers,
        depth_stencil: DepthStencilTarget { view: &depth },
        pipeline_storage: &mut pipelines,
        dynamic_buffer: &mut dynamic,
        encoder: &mut encoder,
    };
    render(&mut context, &view);
    crate::layer::user::readback_test_texture(&mut context, &target)
}

#[test]
#[ignore = "loads the updated prologue and executes older layer commands on offscreen Metal"]
fn higurashi_prologue_layer_commands_drive_tile_and_preserve_front() {
    use crate::{adv::higurashi::layers::HigurashiLayers, asset::system::locate_assets_for_game};
    use shin_core::time::Ticks;
    let root = PathBuf::from(std::env::var("SHIN_HIGURASHI_ASSETS").unwrap());
    let (device, queue) = gpu();
    let server = AssetServer::new(
        locate_assets_for_game(Game::Higurashi, &root)
            .unwrap()
            .into(),
        context(Game::Higurashi, &device, &queue),
    );
    shin_tasks::block_on(async {
        let scenario = server
            .load::<HigurashiScenario, _>("/main.snr")
            .await
            .unwrap();
        let mut vm = HigurashiScripter::new(&scenario, 0, 42).unwrap();
        let mut result = CommandResult::None;
        let mut layers = HigurashiLayers::new(|| [1, 2, 3, 4]);
        let mut dispatched = 0;
        let mut reached_wipe = false;
        for _ in 0..64 {
            let command = vm.run(result, 100000).unwrap();
            result = CommandResult::None;
            if let HigurashiRuntimeCommand::SGET { dest, slot: 2 } = command {
                result = CommandResult::WriteMemory(dest, 0);
            }
            if matches!(command, HigurashiRuntimeCommand::WIPE { .. }) {
                // WIPE must remain pending, not silently consume an unimplemented group command.
                assert!(
                    !layers
                        .dispatch(&command, &server, &scenario, || [1, 2, 3, 4])
                        .await
                        .unwrap()
                );
                reached_wipe = true;
                break;
            }
            dispatched += usize::from(
                layers
                    .dispatch(&command, &server, &scenario, || [1, 2, 3, 4])
                    .await
                    .unwrap(),
            );
        }
        assert!(reached_wipe);
        assert_eq!(dispatched, 22);
        let target = layers.state.target(1).unwrap();
        assert_eq!((target.slot, target.bank), (0, 1));
        assert!(
            layers
                .layer(crate::adv::higurashi::layers::Target { bank: 0, ..target })
                .is_none()
        );
        for p in [6, 7, 8] {
            assert_eq!(layers.state.record(target).property(p), Some(0));
        }
        for size in [(1280, 720), (1920, 1080)] {
            let pixels = render_higurashi(&device, &queue, layers.layer(target).unwrap(), size);
            assert!(pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
            println!(
                "Higurashi dispatched prologue Tile at {}x{}: all {} pixels black after authored RGB controls",
                size.0,
                size.1,
                size.0 * size.1
            );
        }

        let mut layers = HigurashiLayers::new(|| [1, 2, 3, 4]);
        let load = |flags| HigurashiRuntimeCommand::LAYERLOAD {
            args: [1, 1, flags],
            params: (65535, -640, -720, 1280, 720, 0, 0, 0),
        };
        let ctrl = |p, v, duration| HigurashiRuntimeCommand::LAYERCTRL {
            args: [1, p],
            params: (v, duration, 0, 0, 0, 0, 0, 0),
        };
        for command in [
            load(0),
            ctrl(6, 0, 0),
            HigurashiRuntimeCommand::LAYERBACK { argument: 1 },
            ctrl(7, 0, 0),
            HigurashiRuntimeCommand::LAYERBACK { argument: 1 },
        ] {
            assert!(
                layers
                    .dispatch(&command, &server, &scenario, || [11, 12, 13, 14])
                    .await
                    .unwrap()
            );
        }
        let back = layers.state.target(1).unwrap();
        let front = crate::adv::higurashi::layers::Target { bank: 0, ..back };
        for size in [(1280, 720), (1920, 1080)] {
            for (target, color) in [(front, [0, 255, 255, 255]), (back, [0, 0, 255, 255])] {
                let pixels = render_higurashi(&device, &queue, layers.layer(target).unwrap(), size);
                assert!(pixels.chunks_exact(4).all(|p| p == color));
            }
        }
        // Native flag 1 keeps the live property queue/seeds, flag 2 resets it.
        layers
            .dispatch(&ctrl(0, 100, 10), &server, &scenario, || [1, 2, 3, 4])
            .await
            .unwrap();
        layers.advance(Ticks::from_f32(5.0), None);
        assert_eq!(layers.layer(back).unwrap().properties.values()[0], 50.0);
        layers
            .dispatch(&load(1), &server, &scenario, || [21, 22, 23, 24])
            .await
            .unwrap();
        let props = &layers.layer(back).unwrap().properties;
        assert_eq!(props.values()[0], 50.0);
        assert_eq!(props.oscillators.map(|o| o.seed), [11, 12, 13, 14]);
        layers.advance(Ticks::from_f32(5.0), None);
        assert_eq!(layers.layer(back).unwrap().properties.values()[0], 100.0);
        layers
            .dispatch(&load(2), &server, &scenario, || [21, 22, 23, 24])
            .await
            .unwrap();
        assert_eq!(layers.layer(back).unwrap().properties.values()[0], 0.0);
        assert_eq!(
            layers
                .layer(back)
                .unwrap()
                .properties
                .oscillators
                .map(|o| o.seed),
            [21, 22, 23, 24]
        );
        assert_eq!(layers.layer(front).unwrap().properties.values()[6], 0.0);
        layers
            .dispatch(
                &HigurashiRuntimeCommand::LAYERUNLOAD { args: [1, 0] },
                &server,
                &scenario,
                || [1, 2, 3, 4],
            )
            .await
            .unwrap();
        assert!(layers.layer(back).is_none());
        assert!(layers.layer(front).is_some());
        assert!(
            layers.state.target(1).is_some(),
            "back removal retains the allocated front slot until commit"
        );

        // Failed preparation cannot publish a newly allocated logical layer.
        let invalid = HigurashiRuntimeCommand::LAYERLOAD {
            args: [99, 2, 0],
            params: (-1, 0, 0, 0, 0, 0, 0, 0),
        };
        assert!(
            layers
                .dispatch(&invalid, &server, &scenario, || [1, 2, 3, 4])
                .await
                .is_err()
        );
        assert!(layers.state.target(99).is_none());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        println!(
            "Higurashi layer command publication: front/back copies, repeated BACK, older load flags, queued properties, deferred slot retirement and failed preparation pass"
        );
    });
}

#[test]
#[ignore = "loads the updated prologue and checks bank publication on offscreen Metal"]
fn higurashi_prologue_default_wipe_publishes_pictures_and_retains_previous_front() {
    use crate::{adv::higurashi::layers::HigurashiLayers, asset::system::locate_assets_for_game};
    let root = PathBuf::from(std::env::var("SHIN_HIGURASHI_ASSETS").unwrap());
    let (device, queue) = gpu();
    let server = AssetServer::new(
        locate_assets_for_game(Game::Higurashi, &root)
            .unwrap()
            .into(),
        context(Game::Higurashi, &device, &queue),
    );
    shin_tasks::block_on(async {
        let scenario = server
            .load::<HigurashiScenario, _>("/main.snr")
            .await
            .unwrap();
        let mut vm = HigurashiScripter::new(&scenario, 0, 42).unwrap();
        let mut result = CommandResult::None;
        let mut layers = HigurashiLayers::new(|| [1, 2, 3, 4]);
        let mut commits = 0;
        for _ in 0..100 {
            let command = vm.run(result, 100000).unwrap();
            result = CommandResult::None;
            if let HigurashiRuntimeCommand::SGET { dest, slot: 2 } = command {
                result = CommandResult::WriteMemory(dest, 0);
            }
            if let HigurashiRuntimeCommand::WIPE { args, .. } = command {
                // Explicit scheduler boundary; unrelated services and full group
                // composition are outside this bounded visual transition test.
                use crate::adv::higurashi::wipe::HigurashiDefaultTransition;
                assert_eq!(args[0], 0);
                let mut transition =
                    HigurashiDefaultTransition::begin(&mut layers, args[2], args[1] as u32, false);
                assert!(transition.is_running());
                assert!(!transition.command_complete());
                assert!(!transition.wait_complete());
                transition.advance(Ticks::from_f32(args[2] as f32 / 2.0));
                assert!(transition.is_running());
                let previous = &transition.previous;
                commits += 1;
                let front = layers.state.target(1).unwrap();
                assert_eq!(front.bank, 0);
                let layer = layers.layer(front).unwrap();
                assert_eq!(
                    layer.layer_type(),
                    if commits == 1 {
                        LayerType::Tile
                    } else {
                        LayerType::Picture
                    }
                );
                if commits == 3 {
                    assert!(
                        layers.state.target(15).is_none(),
                        "unloaded back frees its promoted slot"
                    );
                    assert!(
                        previous.layer(1).is_some(),
                        "old picture stays owned across slot retirement"
                    );
                }
                for size in [(1280, 720), (1920, 1080)] {
                    let pixels = render_higurashi(&device, &queue, layer, size);
                    if commits == 1 {
                        assert!(pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
                        assert!(previous.layer(front.slot).is_none());
                    } else {
                        assert!(
                            pixels.chunks_exact(4).all(|p| p[..3] != [0, 0, 0]),
                            "authored PIC 424 covers the promoted front"
                        );
                        let old = previous.layer(front.slot).unwrap();
                        if commits == 2 {
                            assert_eq!(old.layer_type(), LayerType::Tile);
                            let old_pixels = render_higurashi(&device, &queue, old, size);
                            assert!(old_pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
                        } else {
                            assert_eq!(old.layer_type(), LayerType::Picture);
                            assert_eq!(render_higurashi(&device, &queue, old, size), pixels);
                            // Previous layer 15 remains renderable after its slot is freed.
                            let removed = previous.layer(1).unwrap();
                            assert_eq!(removed.layer_type(), LayerType::Picture);
                            assert_eq!(
                                render_higurashi(&device, &queue, removed, size).len(),
                                pixels.len()
                            );
                        }
                    }
                    let outgoing = if let Some(old) = previous.layer(front.slot) {
                        render_higurashi(&device, &queue, old, size)
                    } else {
                        vec![0, 0, 0, 255].repeat((size.0 * size.1) as usize)
                    };
                    assert_default_transition_pixels(
                        &device,
                        &queue,
                        &transition,
                        size,
                        &pixels,
                        &outgoing,
                        0.5,
                    );
                    println!(
                        "Higurashi prologue bank commit {commits} at {}x{}: promoted front and retained previous resources pass",
                        size.0, size.1
                    );
                }
                transition.fast_forward();
                assert!(transition.command_complete());
                assert!(transition.wait_complete());
                assert!(!transition.is_running());
                // Slot reuse cannot change the previous-front resource owner.
                if commits == 3 {
                    let load = HigurashiRuntimeCommand::LAYERLOAD {
                        args: [99, 0, 0],
                        params: (0, 0, 0, 0, 0, 0, 0, 0),
                    };
                    layers
                        .dispatch(&load, &server, &scenario, || [5, 6, 7, 8])
                        .await
                        .unwrap();
                    assert_eq!(layers.state.target(99).unwrap().slot, 1);
                    assert_eq!(
                        transition.previous.layer(1).unwrap().layer_type(),
                        LayerType::Picture
                    );
                }
            } else {
                layers
                    .dispatch(&command, &server, &scenario, || [1, 2, 3, 4])
                    .await
                    .unwrap();
            }
        }
        assert_eq!(commits, 3);
        use crate::adv::higurashi::wipe::HigurashiDefaultTransition;
        // Older no-wait is bit0; the later bit1 must keep waiting.
        for flags in [0, 1, 2, 3] {
            let transition = HigurashiDefaultTransition::begin(&mut layers, 60, flags, false);
            assert_eq!(transition.command_complete(), flags & 1 != 0);
            assert!(!transition.wait_complete());
        }
        // Separate RGBA patterns check the embedded older shader formula at
        // endpoints and fractional progress, including nonopaque alpha.
        let size = (64, 64);
        let mut incoming = Vec::new();
        let mut outgoing = Vec::new();
        for y in 0..size.1 {
            for x in 0..size.0 {
                outgoing.extend([16 + x as u8 * 2, 32 + y as u8 * 2, 128, 160]);
                incoming.extend([170, 15 + x as u8, 40 + y as u8 * 2, 128]);
            }
        }
        for progress in [0.0, 0.125, 0.25, 0.37, 0.5, 0.71, 0.9999, 1.0] {
            let mut transition = HigurashiDefaultTransition::begin(&mut layers, 1000, 0, false);
            transition.advance(Ticks::from_f32(progress * 1000.0));
            assert_default_transition_pixels(
                &device,
                &queue,
                &transition,
                size,
                &incoming,
                &outgoing,
                progress,
            );
        }
        println!("Higurashi default shader: eight endpoint/fractional RGBA patterns pass");
        for (duration, skip) in [(0, false), (-60, false), (60, true)] {
            let transition = HigurashiDefaultTransition::begin(&mut layers, duration, 0, skip);
            assert!(transition.command_complete() && transition.wait_complete());
        }
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        println!(
            "Higurashi authored default wipe: three prologue commits, 720p/1080p midpoint RGBA, older waits/skip, removed-slot reuse and retained front owners pass"
        );
    });
}

fn render_higurashi(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layer: &HigurashiUserLayer,
    size: (u32, u32),
) -> Vec<u8> {
    render_to_texture(device, queue, size, |context, view| {
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let mut pass = context.begin_pass(
            TextureTarget {
                view,
                kind: TextureTargetKind::RenderTexture,
            },
            Some(context.depth_stencil),
            "Higurashi native drawable render",
        );
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(0),
            Some(1.0),
        );
        for kind in [
            shin_render::PassKind::Opaque,
            shin_render::PassKind::Transparent,
        ] {
            layer
                .render(&mut pass, &Default::default(), 1, kind)
                .unwrap();
        }
    })
}

fn assert_default_transition_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    transition: &crate::adv::higurashi::wipe::HigurashiDefaultTransition,
    size: (u32, u32),
    incoming: &[u8],
    outgoing: &[u8],
    progress: f32,
) {
    use shin_render::shaders::types::texture::{
        TextureSampler, TextureSource, TextureTarget, TextureTargetKind,
    };
    let upload = |bytes: &[u8]| {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Higurashi prepared screen texture"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.0 * 4),
                rows_per_image: Some(size.1),
            },
            texture.size(),
        );
        texture.create_view(&Default::default())
    };
    let incoming_view = upload(incoming);
    let outgoing_view = upload(outgoing);
    let result = render_to_texture(device, queue, size, |context, view| {
        let mut pass = context.begin_pass(
            TextureTarget {
                view,
                kind: TextureTargetKind::RenderTexture,
            },
            Some(context.depth_stencil),
            "Higurashi default wipe",
        );
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(0),
            Some(1.0),
        );
        transition.render(
            &mut pass,
            shin_render::RenderRequestBuilder::new().depth_stencil_shorthand(1, true, false),
            TextureSource {
                view: &incoming_view,
                sampler: TextureSampler::Linear,
            },
            TextureSource {
                view: &outgoing_view,
                sampler: TextureSampler::Linear,
            },
        );
    });
    for (i, ((actual, incoming), outgoing)) in result.iter().zip(incoming).zip(outgoing).enumerate()
    {
        let expected = f32::from(*outgoing) * (1.0 - progress) + f32::from(*incoming) * progress;
        assert!(
            (f32::from(*actual) - expected).abs() <= 1.0,
            "byte{i}: {actual} vs {expected}"
        );
    }
}

#[test]
#[ignore = "loads updated prologue into complete screen textures on offscreen Metal; no window/audio/saves"]
fn higurashi_prologue_group_textures_include_both_pictures_and_wipe() {
    use crate::{
        adv::higurashi::{
            layers::HigurashiLayers,
            scene::{CommandStatus, HigurashiScene},
            wipe::HigurashiDefaultTransition,
        },
        asset::system::locate_assets_for_game,
    };
    let root = PathBuf::from(std::env::var("SHIN_HIGURASHI_ASSETS").unwrap());
    let (device, queue) = gpu();
    let server = AssetServer::new(
        locate_assets_for_game(Game::Higurashi, &root)
            .unwrap()
            .into(),
        context(Game::Higurashi, &device, &queue),
    );
    shin_tasks::block_on(async {
        let scenario = server
            .load::<HigurashiScenario, _>("/main.snr")
            .await
            .unwrap();
        let mut vm = HigurashiScripter::new(&scenario, 0, 42).unwrap();
        let mut result = CommandResult::None;
        let mut scene = HigurashiScene::new(HigurashiLayers::new(|| [1, 2, 3, 4]));
        let sizes = [(1280, 720), (1920, 1080)];
        let mut last: [Option<Vec<u8>>; 2] = [None, None];
        let mut commits = 0;
        for _ in 0..100 {
            let command = vm.run(result, 100000).unwrap();
            result = CommandResult::None;
            if let HigurashiRuntimeCommand::SGET { dest, slot: 2 } = command {
                result = CommandResult::WriteMemory(dest, 0);
            }
            if let HigurashiRuntimeCommand::WIPE { args, .. } = command {
                assert_eq!(args[0], 0);
                assert_eq!(
                    scene.dispatch(&command, false).unwrap(),
                    Some(CommandStatus::Pending)
                );
                commits += 1;
                let initial: Vec<_> = sizes
                    .iter()
                    .map(|&size| render_group_scene(&device, &queue, &scene, size))
                    .collect();
                for (i, pixels) in initial.iter().enumerate() {
                    if let Some(last) = &last[i] {
                        assert_eq!(pixels, last, "outgoing full group at commit{commits}");
                    } else {
                        assert!(pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
                    }
                }
                scene.advance(Ticks::from_f32(args[2] as f32 / 2.0), true, false, None);
                let midpoint: Vec<_> = sizes
                    .iter()
                    .map(|&size| render_group_scene(&device, &queue, &scene, size))
                    .collect();
                scene.advance(Ticks::from_f32(args[2] as f32 / 2.0), true, false, None);
                assert!(scene.transition().is_none());
                assert_eq!(scene.poll_command(false), CommandStatus::Complete);
                assert_eq!(
                    scene
                        .dispatch(&HigurashiRuntimeCommand::WIPEWAIT {}, false)
                        .unwrap(),
                    Some(CommandStatus::Complete)
                );
                for (i, &size) in sizes.iter().enumerate() {
                    let final_pixels = render_group_scene(&device, &queue, &scene, size);
                    for (byte, ((&mid, &old), &new)) in midpoint[i]
                        .iter()
                        .zip(&initial[i])
                        .zip(&final_pixels)
                        .enumerate()
                    {
                        assert!(
                            (f32::from(mid) - (f32::from(old) + f32::from(new)) * 0.5).abs() <= 1.0,
                            "commit{commits} midpoint byte{byte}"
                        );
                    }
                    let background = render_higurashi(
                        &device,
                        &queue,
                        scene
                            .layers()
                            .layer(scene.layers().state.target(1).unwrap())
                            .unwrap(),
                        size,
                    );
                    if commits == 2 {
                        assert_eq!(scene.layers().group_children().len(), 2);
                        assert_ne!(
                            final_pixels, background,
                            "PIC750 must contribute to the whole screen"
                        );
                        let order =
                            crate::adv::higurashi::group::ordered(scene.layers().group_children());
                        assert_eq!(
                            order.iter().map(|c| c.owner_id).collect::<Vec<_>>(),
                            [1, 15]
                        );
                    } else {
                        assert_eq!(final_pixels, background);
                    }
                    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_GROUP_CAPTURE_DIR") {
                        std::fs::create_dir_all(&dir).unwrap();
                        std::fs::write(
                            PathBuf::from(dir)
                                .join(format!("commit{commits}-{}x{}.rgba", size.0, size.1)),
                            &final_pixels,
                        )
                        .unwrap();
                    }
                    last[i] = Some(final_pixels);
                    println!(
                        "Higurashi screen group commit{commits} {}x{}: complete incoming/outgoing textures and midpoint RGBA pass",
                        size.0, size.1
                    );
                }
            } else if scene.dispatch(&command, false).unwrap().is_none() {
                scene
                    .layers_mut()
                    .unwrap()
                    .dispatch(&command, &server, &scenario, || [1, 2, 3, 4])
                    .await
                    .unwrap();
            }
        }
        assert_eq!(commits, 3);
        println!(
            "Higurashi scene scheduler: three authored WIPE commands, WIPEWAIT and post-retirement steady frames pass at720p/1080p"
        );
        // Deliberately allocate in reverse ID order to distinguish logical tie
        // sorting from physical slot order, then test priority and transparency.
        let mut tiles = HigurashiLayers::new(|| [1, 2, 3, 4]);
        for (id, color) in [(15, 0xf0f0), (1, 0xff00)] {
            tiles
                .dispatch(
                    &HigurashiRuntimeCommand::LAYERLOAD {
                        args: [id, 1, 0],
                        params: (color, -640, -720, 1280, 720, 0, 0, 0),
                    },
                    &server,
                    &scenario,
                    || [1, 2, 3, 4],
                )
                .await
                .unwrap();
        }
        let control = |id, property, value| HigurashiRuntimeCommand::LAYERCTRL {
            args: [id, property],
            params: (value, 0, 0, 0, 0, 0, 0, 0),
        };
        for (kind, control_command) in [
            (0, None),
            (1, Some(control(15, 5, 1001))),
            (2, Some(control(15, 5, 1000))),
        ] {
            if let Some(command) = control_command {
                tiles
                    .dispatch(&command, &server, &scenario, || [1, 2, 3, 4])
                    .await
                    .unwrap();
            }
            if kind == 2 {
                tiles
                    .dispatch(&control(15, 9, 500), &server, &scenario, || [1, 2, 3, 4])
                    .await
                    .unwrap();
            }
            let transition = HigurashiDefaultTransition::begin(&mut tiles, 0, 0, false);
            for size in sizes {
                let pixels = render_group_transition(&device, &queue, &tiles, &transition, size);
                let expected: [u8; 4] = match kind {
                    0 => [0, 255, 0, 255],
                    1 => [255, 0, 0, 255],
                    _ => [128, 128, 0, 255],
                };
                for p in pixels.chunks_exact(4) {
                    for c in 0..4 {
                        assert!(
                            (i16::from(p[c]) - i16::from(expected[c])).abs() <= 1,
                            "group kind{kind}: {p:?}"
                        );
                    }
                }
            }
        }
        for command in [control(-1, 0, 320), control(-2, 0, -160)] {
            tiles
                .dispatch(&command, &server, &scenario, || [1, 2, 3, 4])
                .await
                .unwrap();
        }
        let transition = HigurashiDefaultTransition::begin(&mut tiles, 0, 0, false);
        for size in sizes {
            let pixels = render_group_transition(&device, &queue, &tiles, &transition, size);
            for (i, p) in pixels.chunks_exact(4).enumerate() {
                let expected: [u8; 4] = if (i as u32 % size.0) < size.0 / 8 {
                    [0, 0, 0, 255]
                } else {
                    [128, 128, 0, 255]
                };
                for c in 0..4 {
                    assert!(
                        (i16::from(p[c]) - i16::from(expected[c])).abs() <= 1,
                        "parent transform pixel{i}: {p:?}"
                    );
                }
            }
        }
        println!(
            "Higurashi screen group: physical-slot reversal, logical-ID ties, live priority, transparent foreground and root/screen transforms pass at both sizes"
        );
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    });
}

fn render_group_transition(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layers: &crate::adv::higurashi::layers::HigurashiLayers,
    transition: &crate::adv::higurashi::wipe::HigurashiDefaultTransition,
    size: (u32, u32),
) -> Vec<u8> {
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    render_to_texture(device, queue, size, |context, view| {
        let mut screens =
            crate::adv::higurashi::scene_textures::HigurashiSceneTextures::new(context);
        screens.prepare(context, layers, transition).unwrap();
        let mut pass = context.begin_pass(
            TextureTarget {
                view,
                kind: TextureTargetKind::RenderTexture,
            },
            Some(context.depth_stencil),
            "Higurashi whole-screen default wipe",
        );
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(0),
            Some(1.0),
        );
        screens.render(&mut pass, transition);
    })
}

fn render_group_scene(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &crate::adv::higurashi::scene::HigurashiScene,
    size: (u32, u32),
) -> Vec<u8> {
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    render_to_texture(device, queue, size, |context, view| {
        let mut screens =
            crate::adv::higurashi::scene_textures::HigurashiSceneTextures::new(context);
        screens.prepare_scene(context, scene).unwrap();
        let mut pass = context.begin_pass(
            TextureTarget {
                view,
                kind: TextureTargetKind::RenderTexture,
            },
            Some(context.depth_stencil),
            "Higurashi whole-screen default wipe",
        );
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(0),
            Some(1.0),
        );
        screens.render_scene(&mut pass, scene);
    })
}
