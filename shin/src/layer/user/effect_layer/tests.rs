use super::*;
use crate::{
    asset::system::AssetServer,
    layer::{
        LayerGroup,
        user::{TileLayer, UserLayer},
    },
};
use glam::vec4;
use shin_core::{
    primitives::{color::FloatColor4, update::FrameId},
    time::Ticks,
    vm::command::types::{LayerProperty, LayerbankId, MaskFlags},
};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::sync::Arc;

impl EffectLayer {
    pub(crate) fn assert_reference_capture(
        &mut self,
        context: &mut PreRenderContext,
        server: &Arc<AssetServer>,
    ) {
        // Actual guarded drawable update must skip the absent property object,
        // then initialize it through the base effect accessor in the same body.
        let mut absent_group = LayerGroup::<UserLayer>::new(None);
        absent_group.add_layer(LayerbankId::new(0), UserLayer::Effect(EffectLayer::new()));
        let group_seed_calls = crate::engine_random::seed_call_count();
        let absent_group_copy = absent_group.render_clone(&mut context.render_clone_ctx());
        for group in [&absent_group, &absent_group_copy] {
            assert_eq!(group.properties().reference_initialized_frame(), None);
            assert_eq!(
                group
                    .get_layer(LayerbankId::new(0))
                    .unwrap()
                    .properties()
                    .reference_initialized_frame(),
                None
            );
        }
        assert_eq!(crate::engine_random::seed_call_count(), group_seed_calls);
        let seed_calls = crate::engine_random::seed_call_count();
        let mut lazy = EffectLayer::new();
        assert_eq!(crate::engine_random::seed_call_count(), seed_calls + 1);
        assert_eq!(lazy.props.reference_initialized_frame(), None);
        let mut lazy_frame = FrameId::default();
        let mut lazy_update = AdvUpdateContext {
            frame_id: lazy_frame,
            delta_ticks: Ticks::from_f32(1.0),
            asset_server: server,
            device: context.device,
            queue: context.queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        };
        lazy.update(&lazy_update);
        lazy.fast_forward();
        assert_eq!(lazy.props.reference_initialized_frame(), None);
        let absent_copy = lazy.render_clone(&mut context.render_clone_ctx());
        assert_eq!(absent_copy.props.reference_initialized_frame(), None);
        assert_eq!(crate::engine_random::seed_call_count(), seed_calls + 1);
        lazy_frame.advance();
        lazy_update.frame_id = lazy_frame;
        lazy.update(&lazy_update);
        assert_eq!(
            lazy.props.reference_initialized_frame(),
            Some(FrameId::default())
        );
        assert_eq!(crate::engine_random::seed_call_count(), seed_calls + 8);
        lazy.update(&lazy_update);
        assert_eq!(
            lazy.props.reference_initialized_frame(),
            Some(FrameId::default())
        );
        lazy_frame.advance();
        lazy_update.frame_id = lazy_frame;
        lazy.update(&lazy_update);
        assert_eq!(lazy.props.reference_initialized_frame(), Some(lazy_frame));
        assert_eq!(crate::engine_random::seed_call_count(), seed_calls + 8);
        println!(
            "Native lazy drawable properties: framezero/copy/fast-forward retain absence; first body initializes after skipped property update; next frame advances existing state, exact RNG draw counts pass"
        );
        assert!(self.state.get_prerendered_tex().is_none());
        let mut frame = FrameId::default();
        self.props
            .property_tweener_mut(LayerProperty::RasterHorizontalTPeriod)
            .fast_forward_to(10.0);
        let mut update = AdvUpdateContext {
            frame_id: frame,
            delta_ticks: Ticks::from_f32(1.0),
            asset_server: server,
            device: context.device,
            queue: context.queue,
            are_animations_allowed: false,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        };
        self.update(&update);
        assert_eq!(self.state.reference_effect_phases(), [0.0; 3]);
        frame.advance();
        update.frame_id = frame;
        self.update(&update);
        assert_eq!(self.state.reference_effect_phases()[0], 0.1);
        self.update(&update);
        assert_eq!(self.state.reference_effect_phases()[0], 0.1);
        self.fast_forward();
        assert_eq!(self.state.reference_effect_phases()[0], 0.1);

        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("EffectLayer capture probe"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let draw = |layer: &dyn Layer, context: &mut PreRenderContext| {
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "EffectLayer capture probe",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), None);
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    layer.render(&mut pass, &TransformParams::default(), 1, kind);
                }
            }
            super::super::movie_layer::readback_test_texture(context, &target)
        };
        let tile = |color, rect, position| {
            let mut tile = TileLayer::new(color, rect);
            tile.properties_mut()
                .property_tweener_mut(LayerProperty::RenderPosition)
                .fast_forward_to(position);
            UserLayer::Tile(tile)
        };
        let mut group = LayerGroup::<UserLayer>::new(Some("EffectLayer capture".into()));
        group.add_layer(
            LayerbankId::new(2),
            tile(
                FloatColor4::from_rgba(0.0, 0.0, 1.0, 1.0),
                vec4(-960.0, -540.0, 1920.0, 1080.0),
                3000.0,
            ),
        );
        group.add_layer(
            LayerbankId::new(1),
            tile(
                FloatColor4::from_rgba(1.0, 0.0, 0.0, 1.0),
                vec4(-100.0, -50.0, 200.0, 100.0),
                2000.0,
            ),
        );
        self.props
            .property_tweener_mut(LayerProperty::MulColorRed)
            .fast_forward_to(500.0);
        // Native passes the parent to captured siblings, not this translation.
        self.props
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(300.0);
        let effect = std::mem::replace(self, Self::new());
        group.add_layer(LayerbankId::new(0), UserLayer::Effect(effect));
        group.pre_render(context, &TransformParams::default());
        assert_eq!(
            group.get_stencil_bump(),
            2,
            "Two siblings consumed, only Effect remains in outer batch"
        );
        let pixels = draw(&group, context);
        let center = (540 * 1920 + 960) * 4;
        assert!((127..=129).contains(&pixels[center]));
        assert_eq!(&pixels[center + 1..center + 3], &[0, 0]);
        assert_eq!(&pixels[0..3], &[0, 0, 255]);
        let shifted = (540 * 1920 + 1260) * 4;
        assert_eq!(
            &pixels[shifted..shifted + 3],
            &[0, 0, 255],
            "Effect transform does not move captured siblings"
        );

        let mut copy = {
            let layer = group.get_layer(LayerbankId::new(0)).unwrap();
            let UserLayer::Effect(effect) = &*layer else {
                panic!("Effect")
            };
            effect.render_clone(&mut context.render_clone_ctx())
        };
        copy.pre_render(context, &TransformParams::default());
        assert!(
            pixels == draw(&copy, context),
            "Copy retains prepared texture without recapturing children"
        );
        assert!(copy.pre_render_with_lower(context, &TransformParams::default(), &[]));
        assert!(
            draw(&copy, context)
                .chunks_exact(4)
                .all(|p| p[..3] == [0, 0, 0]),
            "New empty capture replaces copied result, no retained sibling vector"
        );

        let mut next = Self::new();
        next.props
            .property_tweener_mut(LayerProperty::RenderPosition)
            .fast_forward_to(500.0);
        next.props
            .property_tweener_mut(LayerProperty::MulColorRed)
            .fast_forward_to(500.0);
        group.add_layer(LayerbankId::new(3), UserLayer::Effect(next));
        group.pre_render(context, &TransformParams::default());
        assert_eq!(group.get_stencil_bump(), 2);
        let twice = draw(&group, context);
        assert!(
            (63..=65).contains(&twice[center]),
            "Successive effect consumes previous composite exactly once"
        );

        // Real source and target Effect captures inside a replacement wrapper.
        let mut replacement = Self::new();
        replacement
            .props
            .property_tweener_mut(LayerProperty::RenderPosition)
            .fast_forward_to(500.0);
        group.add_layer_with_wiper(
            LayerbankId::new(3),
            UserLayer::Effect(replacement),
            Some(crate::wiper::AnyWiper::Default(
                crate::wiper::DefaultWiper::new(Ticks::from_f32(10.0)),
            )),
        );
        group.pre_render(context, &TransformParams::default());
        let start = draw(&group, context);
        assert!((63..=65).contains(&start[center]));
        frame.advance();
        update.frame_id = frame;
        update.delta_ticks = Ticks::from_f32(10.0);
        update.are_animations_allowed = true;
        group.update(&update);
        group.pre_render(context, &TransformParams::default());
        let finish = draw(&group, context);
        assert!((127..=129).contains(&finish[center]));
        // Compare the captured route with independently tinted ordinary tiles.
        // Linear tint commutes with mask multiplication, up to RGBA8 rounding.
        let make_masked_group = |captured: bool| {
            let mut group = LayerGroup::<UserLayer>::new(Some("Effect/mask composition".into()));
            let factor = if captured {
                [1.0, 1.0, 1.0]
            } else {
                [0.5, 0.75, 0.25]
            };
            group.add_layer(
                LayerbankId::new(2),
                tile(
                    FloatColor4::from_rgba(0.0, 0.2 * factor[1], 0.8 * factor[2], 1.0),
                    vec4(-960.0, -540.0, 1920.0, 1080.0),
                    3000.0,
                ),
            );
            group.add_layer(
                LayerbankId::new(1),
                tile(
                    FloatColor4::from_rgba(0.8 * factor[0], 0.4 * factor[1], 0.2 * factor[2], 0.5),
                    vec4(-700.0, -400.0, 1400.0, 800.0),
                    2000.0,
                ),
            );
            if captured {
                let mut effect = Self::new();
                for (property, value) in [
                    (LayerProperty::MulColorRed, 500.0),
                    (LayerProperty::MulColorGreen, 750.0),
                    (LayerProperty::MulColorBlue, 250.0),
                ] {
                    effect
                        .props
                        .property_tweener_mut(property)
                        .fast_forward_to(value);
                }
                group.add_layer(LayerbankId::new(0), UserLayer::Effect(effect));
            }
            group
        };
        let mut captured = make_masked_group(true);
        let mut ordinary = make_masked_group(false);
        let mut worst = 0;
        for path in ["/mask/test.msk", "/mask/cut.msk", "/mask/c.msk"] {
            let mask: Arc<crate::asset::mask::MaskTexture> =
                shin_tasks::block_on(server.load(path)).unwrap();
            for raw in 0..32 {
                let flags = MaskFlags::from_bits_retain(raw);
                captured.set_mask_texture(mask.clone(), flags);
                ordinary.set_mask_texture(mask.clone(), flags);
                captured.pre_render(context, &TransformParams::default());
                ordinary.pre_render(context, &TransformParams::default());
                assert!(!captured.is_rendered_opaquely());
                assert!(!ordinary.is_rendered_opaquely());
                let actual = draw(&captured, context);
                let expected = draw(&ordinary, context);
                let error = actual
                    .iter()
                    .zip(&expected)
                    .map(|(&a, &b)| a.abs_diff(b))
                    .max()
                    .unwrap();
                assert!(
                    error <= 2,
                    "Effect/mask composition {path} flags{raw:x}: worst channel error{error}"
                );
                worst = worst.max(error);
            }
        }
        println!(
            "Effect/mask composition:96 cases across supplied test/cut/c masks, all32 flag combinations, captured versus independently tinted ordinary tiles, full RGBA comparison worst error{worst}"
        );
        // Move the existing stable groups into a Page. Its parent preparation
        // and two-plane stencil/composition must preserve the owned capture.
        let mut captured_page = crate::layer::PageLayer::new(2);
        let mut ordinary_page = crate::layer::PageLayer::new(2);
        let plane = shin_core::vm::command::types::PlaneId::new(1);
        *captured_page.get_plane_mut(plane) = captured;
        *ordinary_page.get_plane_mut(plane) = ordinary;
        for page in [&mut captured_page, &mut ordinary_page] {
            page.get_plane_mut(shin_core::vm::command::types::PlaneId::new(0))
                .add_layer(
                    LayerbankId::new(0),
                    tile(
                        FloatColor4::from_rgba(0.1, 0.3, 0.2, 1.0),
                        vec4(-960.0, -540.0, 1920.0, 1080.0),
                        3000.0,
                    ),
                );
        }
        let mut page_worst = 0;
        for path in ["/mask/test.msk", "/mask/cut.msk", "/mask/c.msk"] {
            let mask: Arc<crate::asset::mask::MaskTexture> =
                shin_tasks::block_on(server.load(path)).unwrap();
            for raw in 0..32 {
                for page in [&mut captured_page, &mut ordinary_page] {
                    page.get_plane_mut(plane)
                        .set_mask_texture(mask.clone(), MaskFlags::from_bits_retain(raw));
                    page.pre_render(context, &TransformParams::default());
                }
                let actual = draw(&captured_page, context);
                let expected = draw(&ordinary_page, context);
                let error = actual
                    .iter()
                    .zip(&expected)
                    .map(|(&a, &b)| a.abs_diff(b))
                    .max()
                    .unwrap();
                assert!(
                    error <= 2,
                    "Page/Effect mask {path} flags{raw:x}:error{error}"
                );
                page_worst = page_worst.max(error);
            }
        }
        println!(
            "Page/owned Effect/mask composition:96 cases, two real planes, all32 flags, full RGBA comparison worst error{page_worst}"
        );
        println!(
            "Native Effect type5: captured lower batch, base-only/duplicate clocks, translated-parent preservation, tint/successive effects, copied texture/empty recapture and replacement-wipe source/target pass"
        );
    }
}

#[test]
#[ignore = "focused supplied-ROM Group/Page/owned Effect mask composition; offscreen Metal, no audio/windows/saves"]
fn reference_owned_effect_group_page_mask_composition() {
    use crate::asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache};
    use shin_render::{
        dynamic_buffer::DynamicBuffer,
        pipelines::PipelineStorage,
        resize::{SurfaceResizeSource, ViewportParams},
        shaders::types::{buffer::BytesAddress, texture::TextureSamplerStore},
    };
    crate::init_test_task_pools();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let mut io = LayeredAssetIo::new();
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
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Owned Effect/Page mask depth"),
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
    let view = depth.create_view(&Default::default());
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), wgpu::TextureFormat::Rgba8Unorm);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut context = PreRenderContext {
        breakup_texture: None,
        device: &device,
        queue: &queue,
        resize_source: &resize,
        sampler_store: &samplers,
        depth_stencil: DepthStencilTarget { view: &view },
        pipeline_storage: &mut pipelines,
        dynamic_buffer: &mut dynamic,
        encoder: &mut encoder,
    };
    EffectLayer::new().assert_reference_capture(&mut context, &server);
    drop(context);
    dynamic.finish(&mut encoder);
    queue.submit([encoder.finish()]);
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU work must complete before readback or retirement");
    dynamic.recall();
}

#[test]
#[ignore = "requires supplied executable/ROM; no GPU/audio"]
fn reference_native_effectable_rtti_implementors_and_mask_regions() {
    use std::io::Read;
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let relocs: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
        .step_by(24)
        .map(|o| {
            (
                u64::from_le_bytes(exe[o + 256..o + 264].try_into().unwrap()),
                u64::from_le_bytes(exe[o + 272..o + 280].try_into().unwrap()),
            )
        })
        .collect();
    let mut uses = relocs
        .iter()
        .filter_map(|(&a, &v)| (v == 0x1d3920).then_some(a))
        .collect::<Vec<_>>();
    uses.sort_unstable();
    assert_eq!(uses, [0x1d6f38, 0x1d7fd0, 0x2247b8]);
    for (info, expected) in [
        (0x1d6f18, b"N4shin11EffectLayerE".as_slice()),
        (0x1d7fb0, b"N4shin10LayerGroup15TransitionLayerE".as_slice()),
        (0x1d3928, b"N4shin5Layer10EffectableE".as_slice()),
    ] {
        let start = relocs[&info] as usize + 256;
        let end = start + exe[start..].iter().position(|&v| v == 0).unwrap();
        assert_eq!(&exe[start..end], expected);
    }
    let mut reader = shin_core::format::rom::RomReader::new(
        shin_core::primitives::stateless_reader::StatelessFile::new(
            std::fs::File::open(rom).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    for (path, counts, dimensions) in [
        ("/mask/test.msk", [134, 99, 225], (534, 308)),
        ("/mask/cut.msk", [47, 49, 53], (544, 308)),
        ("/mask/c.msk", [0, 0, 1], (512, 360)),
    ] {
        let entry = reader.find_file(path).unwrap();
        let mut bytes = Vec::new();
        reader
            .open_file(entry)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let mask = shin_core::format::mask::read_mask(&bytes).unwrap();
        assert_eq!(mask.texels.dimensions(), dimensions);
        assert_eq!(
            [
                mask.regions.black_regions.rect_count,
                mask.regions.white_regions.rect_count,
                mask.regions.transparent_regions.rect_count,
            ],
            counts
        );
    }
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU/audio"]
fn reference_native_effect_capture_clone_and_batch_consumption() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let relocs: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
        .step_by(24)
        .map(|o| {
            (
                u64::from_le_bytes(exe[o + 256..o + 264].try_into().unwrap()),
                u64::from_le_bytes(exe[o + 272..o + 280].try_into().unwrap()),
            )
        })
        .collect();
    let jump = u16::from_le_bytes(exe[0x1653ea + 256..0x1653ec + 256].try_into().unwrap());
    assert_eq!(0x20608 + u32::from(jump) * 4, 0x20a64);
    for (a, expected) in [
        (0x224c20, 0x1d6e10),
        (0x1d6f18, 0x16f734),
        (0x1d6e30, 0x6aa40),
        (0x1d6e38, 0x78820),
        (0x1d6e48, 0x63790),
        (0x1d6e50, 0x75d90),
        (0x1d6e60, 0x63810),
        (0x1d6e68, 0x638c0),
        (0x1d6e98, 0x638d0),
        (0x1d6ea0, 0x638e0),
        (0x1d6ea8, 0x79380),
        (0x1d6f08, 0x63870),
        (0x2247b8, 0x1d3920),
    ] {
        assert_eq!(relocs.get(&a), Some(&expected));
    }
    for (a, expected) in [
        (0x632fc, 0x94004ad5),
        (0x6330c, 0xa9077e7f),
        (0x63324, 0xa9067e68),
        (0x637b8, 0x94004a0e),
        (0x637c8, 0xa9077e9f),
        (0x637e0, 0xa9067e88),
        (0x63810, 0xd65f03c0),
        (0x63844, 0x9400007b),
        (0x63850, 0x94004acc),
        (0x63858, 0x320003e0),
        (0x638d0, 0x320003e0),
        (0x63964, 0x9400375f),
        (0x6398c, 0x9401af2d),
        (0x6399c, 0xd10022b5),
        (0x639b8, 0xd63f0100),
        (0x639c4, 0xf9003a74),
        (0x639e8, 0x320003e0),
        (0x766a4, 0xaa1603e3),
        (0x7c56c, 0xd63f0100),
        (0x7c570, 0x360001c0),
        (0x7c5a4, 0xf90013f3),
        (0x7f708, 0x2a1f03e0),
        (0x84134, 0x97ff7c57),
        (0x20aac, 0x94018d35),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
            expected,
            "Native{a:x}"
        );
    }
}
