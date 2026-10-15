//! Genuine Group/Page prepared-list traces, including removal before recapture.
use super::*;
use crate::{
    asset::system::{AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache},
    layer::{
        PageLayer,
        owner::LayerOwner,
        user::{EffectLayer, TileLayer},
    },
    wiper::DefaultWiper,
};
use shin_core::vm::command::types::PlaneId;
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{buffer::BytesAddress, texture::TextureSamplerStore},
};
use std::path::PathBuf;

enum Scene {
    Group(LayerGroup),
    Page(PageLayer),
}
impl Scene {
    fn group(&self) -> &LayerGroup {
        match self {
            Self::Group(g) => g,
            Self::Page(p) => p.get_plane(PlaneId::new(0)),
        }
    }
    fn group_mut(&mut self) -> &mut LayerGroup {
        match self {
            Self::Group(g) => g,
            Self::Page(p) => p.get_plane_mut(PlaneId::new(0)),
        }
    }
    fn prepare(&mut self, context: &mut PreRenderContext) {
        match self {
            Self::Group(g) => g.pre_render(context, &Default::default()),
            Self::Page(p) => p.pre_render(context, &Default::default()),
        }
    }
    fn copied(&self, context: &mut RenderCloneCtx) -> Self {
        match self {
            Self::Group(g) => Self::Group(g.render_clone(context)),
            Self::Page(p) => Self::Page(p.render_clone(context)),
        }
    }
    fn page_state(&self) -> (usize, u8) {
        match self {
            Self::Group(_) => (0, 0),
            Self::Page(p) => p.reference_prepared_plane_state(),
        }
    }
}
struct TileProbe {
    identity: usize,
    count: Box<dyn Fn() -> usize>,
}
fn set(layer: &mut UserLayer, property: LayerProperty, value: f32) {
    layer
        .properties_mut()
        .property_tweener_mut(property)
        .fast_forward_to(value);
}
fn add_tile(
    scene: &mut Scene,
    bank: u8,
    id: usize,
    position: f32,
    wiping: bool,
    probes: &mut Vec<TileProbe>,
) {
    let i = id as f32;
    let mut tile: UserLayer = TileLayer::new(
        FloatColor4::from_rgba(0.125 * i, 0.25, 0.5, 1.0),
        glam::vec4(-32.0 * i, 16.0 * i, 96.0, 48.0),
    )
    .into();
    set(&mut tile, LayerProperty::RenderPosition, position);
    let bank = LayerbankId::new(bank);
    scene.group_mut().add_layer_with_wiper(
        bank,
        tile,
        wiping.then(|| AnyWiper::Default(DefaultWiper::new(Ticks::from_f32(30.0)))),
    );
    let owner = scene.group().layer_owner(bank).unwrap();
    probes.push(TileProbe {
        identity: owner.reference_identity(),
        count: Box::new(owner.reference_weak()),
    });
}
fn portable_kind(owner: &RetainedLayer, probes: &[TileProbe]) -> usize {
    if let Some(layer) = owner.reference_read::<UserLayer>() {
        match &*layer {
            UserLayer::Tile(_) => {
                probes
                    .iter()
                    .position(|p| p.identity == owner.reference_identity())
                    .unwrap()
                    + 1
            }
            UserLayer::Effect(_) => 100,
            _ => panic!("unexpected actual user variant"),
        }
    } else if owner.reference_read::<PreparedSlotBatch>().is_some() {
        200
    } else {
        assert!(owner.reference_read::<SlotLayer<UserLayer>>().is_some());
        300
    }
}
fn observation(
    id: usize,
    phase: &str,
    scene: &Option<Scene>,
    copy: &Option<Scene>,
    effect: &Option<LayerOwner<UserLayer>>,
    probes: &[TileProbe],
) -> String {
    let mut words = vec![
        "S".to_owned(),
        id.to_string(),
        phase.to_owned(),
        usize::from(scene.is_some()).to_string(),
        probes.len().to_string(),
    ];
    words.extend(probes.iter().map(|p| (p.count)().to_string()));
    if let Some(effect) = effect {
        let layer = effect.read();
        let UserLayer::Effect(effect) = &*layer else {
            panic!("Effect")
        };
        let pending = effect.reference_pending_children();
        words.push(pending.len().to_string());
        words.extend(pending.iter().map(|p| portable_kind(p, probes).to_string()));
    } else {
        words.push("0".to_owned());
    }
    if let Some(scene) = scene {
        let group = scene.group();
        words.extend([
            group.layers_to_render.len().to_string(),
            group.stencil_bump.to_string(),
        ]);
        for item in &group.layers_to_render {
            let LayerRenderSource::Retained(owner) = &item.source else {
                panic!("user planes must retain selected owners")
            };
            words.extend([
                portable_kind(owner, probes).to_string(),
                item.stencil_ref_relative.to_string(),
            ]);
        }
        let (count, bump) = scene.page_state();
        words.extend([count.to_string(), bump.to_string()]);
    } else {
        words.extend(["0", "0", "0", "0"].map(str::to_owned));
    }
    words.push(usize::from(copy.is_some()).to_string());
    words.push(
        copy.as_ref()
            .map_or(0, |c| c.group().layers_to_render.len())
            .to_string(),
    );
    words.push(copy.as_ref().map_or(0, |c| c.page_state().0).to_string());
    words.join(" ")
}
fn copied_probes(slot: &SlotLayer<UserLayer>, probes: &mut Vec<Box<dyn Fn() -> usize>>) {
    if let Some(source) = &slot.source {
        copied_probes(&source.read(), probes);
    }
    let owner = slot.target.shared_owner().unwrap();
    probes.push(Box::new(owner.reference_weak()));
}

#[test]
#[ignore = "genuine native Group/Page scheduling traces; offscreen Metal, no audio/windows/saves"]
fn reference_native_group_page_prepared_list_retirement() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_GROUP_PAGE_SCHEDULER_OUTPUT").unwrap());
    let request = std::fs::read_to_string(output.join("request.txt")).unwrap();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let resize =
        SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(16, 16)));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Native scheduler depth"),
        size: wgpu::Extent3d {
            width: 16,
            height: 16,
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
    let _server = Arc::new(AssetServer::new(
        LayeredAssetIo::new().into(),
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
    let mut actual = Vec::new();
    let mut final_probes = 0;
    for (id, line) in request.lines().enumerate() {
        let p = line
            .split_whitespace()
            .map(|s| s.parse::<usize>().unwrap())
            .collect::<Vec<_>>();
        let (page, layout, mode) = (p[0] != 0, p[1], p[2]);
        let mut root = if page {
            Scene::Page(PageLayer::new(2))
        } else {
            Scene::Group(LayerGroup::new(None))
        };
        let mut probes = Vec::new();
        if layout != 0 {
            add_tile(&mut root, 0, 1, 2000.0, false, &mut probes);
        }
        add_tile(&mut root, 0, 2, 2000.0, layout != 0, &mut probes);
        if layout == 2 {
            add_tile(&mut root, 0, 5, 2000.0, true, &mut probes);
        }
        if layout != 0 {
            add_tile(&mut root, 1, 3, 1900.0, false, &mut probes);
        }
        add_tile(&mut root, 1, 4, 1900.0, layout != 0, &mut probes);
        let mut effect = if mode != 0 {
            let mut effect: UserLayer = EffectLayer::new().into();
            set(&mut effect, LayerProperty::RenderPosition, 1000.0);
            if mode == 1 {
                set(&mut effect, LayerProperty::ShowLayer, 0.0);
            }
            root.group_mut().add_layer(LayerbankId::new(2), effect);
            root.group().layer_owner(LayerbankId::new(2))
        } else {
            None
        };
        let effect_weak = effect.as_ref().map(LayerOwner::reference_weak);
        let mut scene = Some(root);
        let mut copy = None;
        actual.push(observation(id, "initial", &scene, &copy, &effect, &probes));
        scene.as_mut().unwrap().prepare(&mut context);
        actual.push(observation(id, "prepared", &scene, &copy, &effect, &probes));
        copy = Some(
            scene
                .as_ref()
                .unwrap()
                .copied(&mut context.render_clone_ctx()),
        );
        let mut copy_weak = Vec::new();
        for item in &copy.as_ref().unwrap().group().layers {
            copied_probes(&item.layer.read(), &mut copy_weak);
        }
        actual.push(observation(id, "copies", &scene, &copy, &effect, &probes));
        for bank in [0, 1] {
            drop(
                scene
                    .as_mut()
                    .unwrap()
                    .group_mut()
                    .remove_layer(LayerbankId::new(bank), Ticks::ZERO),
            );
        }
        actual.push(observation(id, "removed", &scene, &copy, &effect, &probes));
        add_tile(scene.as_mut().unwrap(), 0, 7, 2000.0, false, &mut probes);
        actual.push(observation(
            id,
            "reused-bank",
            &scene,
            &copy,
            &effect,
            &probes,
        ));
        scene.as_mut().unwrap().prepare(&mut context);
        actual.push(observation(
            id,
            "recaptured",
            &scene,
            &copy,
            &effect,
            &probes,
        ));
        scene = None;
        actual.push(observation(
            id,
            "drop-scene",
            &scene,
            &copy,
            &effect,
            &probes,
        ));
        effect = None;
        actual.push(observation(
            id,
            "drop-effect",
            &scene,
            &copy,
            &effect,
            &probes,
        ));
        copy = None;
        actual.push(observation(
            id,
            "drop-copy",
            &scene,
            &copy,
            &effect,
            &probes,
        ));
        for probe in probes {
            assert_eq!((probe.count)(), 0);
            final_probes += 1;
        }
        for weak in copy_weak {
            assert_eq!(weak(), 0);
            final_probes += 1;
        }
        if let Some(weak) = effect_weak {
            assert_eq!(weak(), 0);
            final_probes += 1;
        }
    }
    for version in ["original", "1.03"] {
        let expected = std::fs::read_to_string(output.join(format!("{version}.trace")))
            .unwrap()
            .lines()
            .filter(|l| l.starts_with("S "))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        std::fs::write(
            output.join(format!("{version}.port.trace")),
            actual.join("\n") + "\n",
        )
        .unwrap();
        for (a, b) in actual.iter().zip(&expected) {
            assert_eq!(a, b);
        }
        assert_eq!(actual.len(), expected.len());
    }
    drop(context);
    dynamic.finish(&mut encoder);
    queue.submit([encoder.finish()]);
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU work must complete before readback or retirement");
    dynamic.recall();
    println!(
        "Native Group/Page scheduler:18 desktop cases match both images;{} exact rows/image;{final_probes} actual final Weak probes; ordinary/compatible/nested slots, visible/invisible capture, removal/recapture, empty explicit copies and final retirement",
        actual.len()
    );
}
