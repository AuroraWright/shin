//! Actual native capture/copy snapshots, including stable owned child handles.
use super::*;
use crate::{
    asset::system::{AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache},
    layer::{
        LayerGroup,
        owner::LayerOwner,
        user::{TileLayer, UserLayer},
    },
    wiper::DefaultWiper,
};
use shin_core::{
    primitives::{color::FloatColor4, update::FrameId},
    time::{Ticks, Tween},
    vm::command::types::{LayerProperty, LayerbankId},
};
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{RenderCloneCtx, buffer::BytesAddress, texture::TextureSamplerStore},
};
use std::{collections::BTreeSet, path::Path, sync::Arc};

enum Root {
    Effect(UserLayer),
    Group(LayerGroup),
}
enum Children {
    Borrowed(Vec<UserLayer>),
    Owned(Vec<LayerOwner<UserLayer>>),
}
impl Children {
    fn new(owned: bool) -> Self {
        let tiles = [FloatColor4::BLUE, FloatColor4::RED]
            .into_iter()
            .map(|color| TileLayer::new(color, glam::vec4(-8.0, -8.0, 16.0, 16.0)));
        if owned {
            Self::Owned(tiles.map(UserLayer::from).map(LayerOwner::new).collect())
        } else {
            Self::Borrowed(tiles.map(UserLayer::from).collect())
        }
    }
    fn clear(&mut self) {
        match self {
            Self::Borrowed(c) => c.clear(),
            Self::Owned(c) => c.clear(),
        }
    }
    fn capture(&self, l: &mut dyn Layer, context: &mut PreRenderContext, empty: bool) {
        match self {
            Self::Borrowed(c) => {
                let lower: Vec<&dyn Layer> = if empty {
                    Vec::new()
                } else {
                    c.iter().map(|l| l as &dyn Layer).collect()
                };
                assert!(l.pre_render_with_lower(context, &TransformParams::default(), &lower));
            }
            Self::Owned(c) => {
                let lower = if empty {
                    Vec::new()
                } else {
                    c.iter().map(LayerOwner::retain).collect::<Vec<_>>()
                };
                assert!(l.pre_render_with_owned_lower(
                    context,
                    &TransformParams::default(),
                    &lower
                ));
            }
        }
    }
    fn prepare(&self, l: &mut EffectLayer, context: &mut PreRenderContext, mode: usize) {
        if let Self::Owned(c) = self {
            for indices in [vec![0], vec![0, 1, 0], vec![1, 0], vec![0], vec![0, 1]] {
                let before = c
                    .iter()
                    .map(LayerOwner::reference_strong_count)
                    .collect::<Vec<_>>();
                let old = c
                    .iter()
                    .map(|child| {
                        l.pending_children
                            .iter()
                            .filter(|p| p.reference_identity() == child.reference_identity())
                            .count()
                    })
                    .collect::<Vec<_>>();
                let lower = indices.iter().map(|&i| c[i].retain()).collect::<Vec<_>>();
                l.assign_owned_lower(&lower);
                drop(lower);
                for (i, child) in c.iter().enumerate() {
                    assert_eq!(
                        child.reference_strong_count(),
                        before[i] - old[i] + indices.iter().filter(|&&j| j == i).count()
                    );
                }
            }
        }
        if mode != 0 {
            if mode == 2 {
                l.props
                    .property_tweener_mut(LayerProperty::ShowLayer)
                    .fast_forward_to(0.0);
            }
            self.capture(l, context, false);
        }
    }
}
impl Root {
    fn leaves(&self) -> Vec<crate::layer::layer_group::ReferenceLayerLeaf<'_, UserLayer>> {
        match self {
            Self::Effect(l) => vec![crate::layer::layer_group::ReferenceLayerLeaf::borrowed(l)],
            Self::Group(g) => g.reference_owner_leaves(),
        }
    }
    fn target(&mut self) -> crate::layer::owner::LayerWrite<'_, UserLayer> {
        match self {
            Self::Effect(l) => crate::layer::owner::LayerWrite::Direct(l),
            Self::Group(g) => g.get_layer_mut(LayerbankId::new(0)).unwrap(),
        }
    }
    fn copied(&self, ctx: &mut RenderCloneCtx) -> Self {
        match self {
            Self::Effect(l) => Self::Effect(l.render_clone(ctx)),
            Self::Group(g) => Self::Group(g.render_clone(ctx)),
        }
    }
    fn forward(&mut self) {
        match self {
            Self::Effect(l) => l.fast_forward(),
            Self::Group(g) => g.fast_forward(),
        }
    }
    fn slots(&self) -> usize {
        match self {
            Self::Effect(_) => 0,
            Self::Group(g) => g.reference_transition_words().len(),
        }
    }
}
fn effect(l: &UserLayer) -> &EffectLayer {
    let UserLayer::Effect(l) = l else {
        panic!("Expected actual Effect variant")
    };
    l
}
fn make_effect(
    populated: bool,
    children: &Children,
    context: &mut PreRenderContext,
    mode: usize,
) -> UserLayer {
    let mut l = EffectLayer::new();
    if populated {
        for i in 0..90 {
            let p = <LayerProperty as enum_map::Enum>::from_usize(i);
            let initial = p.initial_value() as f32;
            let mut t = l.props.property_tweener_mut(p);
            t.fast_forward_to(initial);
            for j in 1..=2 {
                t.enqueue(
                    initial + j as f32 * 0.25,
                    Tween::linear(Ticks::from_f32(10.0)),
                );
            }
        }
    }
    children.prepare(&mut l, context, mode);
    l.into()
}
fn prepare_root(
    populated: bool,
    graph: usize,
    children: &Children,
    context: &mut PreRenderContext,
    mode: usize,
) -> Root {
    // The native first Effect is constructed/captured before the Group factory.
    let first = make_effect(populated, children, context, mode);
    if graph == 0 {
        return Root::Effect(first);
    }
    let mut group = LayerGroup::new(None);
    group.add_layer(LayerbankId::new(0), first);
    if graph == 2 {
        let mut next = make_effect(populated, children, context, mode);
        if populated {
            next.set_properties(
                group
                    .get_layer(LayerbankId::new(0))
                    .unwrap()
                    .properties()
                    .share(),
            );
        }
        group.add_layer_with_wiper(
            LayerbankId::new(0),
            next,
            Some(DefaultWiper::new(Ticks::from_f32(30.0)).into()),
        );
    }
    Root::Group(group)
}
fn snapshot(id: usize, phase: &str, roots: &[Option<Root>; 3], last_rng: &mut usize) -> String {
    let now = crate::engine_random::seed_call_count();
    let mut words = vec![
        "E".into(),
        id.to_string(),
        phase.into(),
        (now - *last_rng).to_string(),
    ];
    *last_rng = now;
    let mut owners = Vec::new();
    let mut views = BTreeSet::new();
    let mut slots = 0;
    for root in roots {
        let leaves = root.as_ref().map_or_else(Vec::new, Root::leaves);
        words.extend([
            usize::from(root.is_some()).to_string(),
            leaves.len().to_string(),
        ]);
        if let Some(root) = root {
            slots += root.slots();
        }
        for l in leaves {
            let f = effect(&l);
            let props = l.properties();
            let owner = props.initialized_identity();
            owners.push(owner);
            let tex = f.state.get_prerendered_tex();
            if let Some(tex) = &tex {
                assert!(
                    views.insert(tex.render_texture.view as *const _ as usize),
                    "Separate live capture view objects"
                );
            }
            words.extend([
                usize::from(owner.is_some()).to_string(),
                format!("{:08x}", f.state.reference_dissolve_seed()),
                usize::from(tex.is_some()).to_string(),
                f.pending_children.len().to_string(),
            ]);
            for phase in f.state.reference_effect_phases() {
                words.push(format!("{:08x}", phase.to_bits()));
            }
            if owner.is_some() {
                for i in 0..90 {
                    let t = props
                        .reference_tweener_snapshot(<LayerProperty as enum_map::Enum>::from_usize(
                            i,
                        ))
                        .unwrap();
                    words.extend([
                        format!("{:08x}", t.value().to_bits()),
                        usize::from(!t.is_idle()).to_string(),
                    ]);
                }
            }
        }
    }
    for a in &owners {
        for b in &owners {
            words.push(usize::from(a.is_some() && a == b).to_string());
        }
    }
    words.push(slots.to_string());
    words.join(" ")
}

#[test]
#[ignore = "native Effect traces and offscreen Metal;no audio/window/saves"]
fn reference_native_effect_visible_capture_clone_and_retirement() {
    run_reference(false);
}

#[test]
#[ignore = "native Effect traces and offscreen Metal;no audio/window/saves"]
fn reference_native_effect_owned_capture_clone_and_retirement() {
    run_reference(true);
}

fn run_reference(owned: bool) {
    crate::init_test_task_pools();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output = std::path::PathBuf::from(std::env::var_os("SHIN_EFFECT_OWNER_OUTPUT").unwrap());
    let request = std::fs::read_to_string(output.join("request.txt")).unwrap();
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let resize =
        SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(16, 16)));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Effect owner reference depth"),
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
    let depth_view = depth.create_view(&Default::default());
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), wgpu::TextureFormat::Rgba8Unorm);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut total = 0;
    for version in ["original", "1.03"] {
        let mut io = LayeredAssetIo::new();
        io.try_with_rom(root.join("umineko_switch_files/data.rom"))
            .unwrap();
        if version == "1.03" {
            io.try_with_rom(root.join("umineko_switch_files/patch.rom"))
                .unwrap();
        }
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
        if owned {
            assert_retained_user_mutation_and_explicit_copy(&server, &device, &queue);
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
            LayerGroup::reference_integrated_owned_capture(&mut context, &server);
            drop(context);
            dynamic.finish(&mut encoder);
            queue.submit(Some(encoder.finish()));
            device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
            dynamic.recall();
        }
        let mut actual = Vec::new();
        let mut frame = FrameId::default();
        for (id, line) in request.lines().enumerate() {
            let p: Vec<usize> = line
                .split_whitespace()
                .map(|w| w.parse().unwrap())
                .collect();
            if !owned && p[3] != 1 {
                continue;
            }
            let (populated, graph, order, mode) = (p[0] != 0, p[1], p[2], p[3]);
            crate::engine_random::with_reference_seed(5489, || {
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
                let mut last_rng = crate::engine_random::seed_call_count();
                let mut children = Children::new(owned);
                let weak = match &children {
                    Children::Owned(c) => {
                        c.iter().map(LayerOwner::reference_weak).collect::<Vec<_>>()
                    }
                    Children::Borrowed(_) => Vec::new(),
                };
                let mut roots = [
                    Some(prepare_root(
                        populated,
                        graph,
                        &children,
                        &mut context,
                        mode,
                    )),
                    None,
                    None,
                ];
                actual.push(snapshot(id, "source", &roots, &mut last_rng));
                for g in 1..3 {
                    roots[g] = Some(
                        roots[g - 1]
                            .as_ref()
                            .unwrap()
                            .copied(&mut context.render_clone_ctx()),
                    );
                }
                actual.push(snapshot(id, "copies", &roots, &mut last_rng));
                if mode != 0 {
                    let mut target = roots[1].as_mut().unwrap().target();
                    target
                        .properties_mut()
                        .property_tweener_mut(LayerProperty::ShowLayer)
                        .fast_forward_to(1.0);
                    children.capture(&mut *target, &mut context, false);
                    drop(target);
                    actual.push(snapshot(id, "recapture-copy", &roots, &mut last_rng));
                    children.capture(
                        &mut *roots[1].as_mut().unwrap().target(),
                        &mut context,
                        true,
                    );
                    actual.push(snapshot(id, "empty-recapture-copy", &roots, &mut last_rng));
                }
                frame.advance();
                roots[1]
                    .as_mut()
                    .unwrap()
                    .target()
                    .update(&AdvUpdateContext {
                        frame_id: frame,
                        delta_ticks: Ticks::from_f32(5.0),
                        asset_server: &server,
                        device: &device,
                        queue: &queue,
                        are_animations_allowed: true,
                        voice_callback: None,
                        movie_volume_settings: Default::default(),
                    });
                actual.push(snapshot(id, "advance-copy", &roots, &mut last_rng));
                roots[1]
                    .as_mut()
                    .unwrap()
                    .target()
                    .properties_mut()
                    .property_tweener_mut(LayerProperty::TranslateY)
                    .fast_forward_to(123.0);
                actual.push(snapshot(id, "mutate-copy", &roots, &mut last_rng));
                roots[1].as_mut().unwrap().forward();
                actual.push(snapshot(id, "forward-copy", &roots, &mut last_rng));
                children.clear();
                if owned {
                    for owner in &weak {
                        assert_eq!(
                            owner(),
                            if mode == 1 {
                                0
                            } else if graph == 2 {
                                2
                            } else {
                                1
                            }
                        );
                    }
                }
                actual.push(snapshot(id, "drop-children", &roots, &mut last_rng));
                for i in 0..3 {
                    roots[if order != 0 { 2 - i } else { i }] = None;
                    actual.push(snapshot(id, "drop", &roots, &mut last_rng));
                    if owned {
                        let pending = roots
                            .iter()
                            .flatten()
                            .flat_map(Root::leaves)
                            .map(|l| effect(&l).pending_children.len())
                            .sum::<usize>();
                        for owner in &weak {
                            assert_eq!(owner(), pending / 2);
                        }
                    }
                }
                assert!(roots.iter().all(Option::is_none));
                assert!(weak.iter().all(|owner| owner() == 0));
                drop(context);
                dynamic.finish(&mut encoder);
                queue.submit(Some(encoder.finish()));
                device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
                dynamic.recall();
            });
            total += 1;
        }
        std::fs::write(
            output.join(format!("{version}.port.trace")),
            actual.join("\n") + "\n",
        )
        .unwrap();
        let expected: Vec<String> =
            std::fs::read_to_string(output.join(format!("{version}.trace")))
                .unwrap()
                .lines()
                .filter(|l| l.starts_with("E "))
                .filter(|l| {
                    let id: usize = l.split_whitespace().nth(1).unwrap().parse().unwrap();
                    owned
                        || request
                            .lines()
                            .nth(id)
                            .unwrap()
                            .split_whitespace()
                            .nth(3)
                            .unwrap()
                            == "1"
                })
                .map(str::to_owned)
                .collect();
        assert_eq!(actual.len(), if owned { 372 } else { 132 });
        for (i, (got, want)) in actual.iter().zip(&expected).enumerate() {
            assert_eq!(got, want, "{version} row{i}");
        }
    }
    assert_eq!(total, if owned { 72 } else { 24 });
    println!(
        "Effect capture parity:owned={owned},cases={total};real child owner counts/final retirement,two production render-copy generations,recapture,empty recapture,properties and root drops;Group scheduling adoption remains separate"
    );
}

fn assert_retained_user_mutation_and_explicit_copy(
    server: &Arc<AssetServer>,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) {
    crate::engine_random::with_reference_seed(1234, || {
        let owner = LayerOwner::new(UserLayer::Tile(TileLayer::new(
            FloatColor4::RED,
            glam::vec4(-8.0, -8.0, 16.0, 16.0),
        )));
        let weak = owner.reference_weak();
        let before = crate::engine_random::seed_call_count();
        let typed = owner.clone();
        let mut retained = owner.retain();
        assert_eq!(owner.reference_identity(), retained.reference_identity());
        assert_eq!(
            crate::engine_random::seed_call_count(),
            before,
            "Retain never deep-copies state or consumes RNG"
        );
        assert_eq!(weak(), 3);
        {
            let mut layer = typed.write();
            let mut t = layer
                .properties_mut()
                .property_tweener_mut(LayerProperty::TranslateX);
            t.fast_forward_to(13.0);
            t.enqueue(23.0, Tween::linear(Ticks::from_f32(10.0)));
        }
        let mut frame = FrameId::default();
        frame.advance();
        retained.update(&AdvUpdateContext {
            frame_id: frame,
            delta_ticks: Ticks::from_f32(5.0),
            asset_server: server,
            device,
            queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        });
        assert_eq!(
            owner
                .read()
                .properties()
                .get_value(LayerProperty::TranslateX),
            18.0
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        let before_copy = crate::engine_random::seed_call_count();
        let copied = owner.render_clone(&mut RenderCloneCtx {
            device,
            encoder: &mut encoder,
        });
        assert_eq!(crate::engine_random::seed_call_count(), before_copy + 6);
        assert_ne!(owner.reference_identity(), copied.reference_identity());
        let copied_weak = copied.reference_weak();
        copied
            .write()
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(-55.0);
        retained.fast_forward();
        assert_eq!(
            typed
                .read()
                .properties()
                .get_value(LayerProperty::TranslateX),
            23.0
        );
        assert_eq!(
            copied
                .read()
                .properties()
                .get_value(LayerProperty::TranslateX),
            -55.0
        );
        drop(owner);
        drop(typed);
        assert_eq!(
            weak(),
            1,
            "Erased retained handle owns the actual mutable UserLayer"
        );
        drop(retained);
        assert_eq!(weak(), 0);
        drop(copied);
        assert_eq!(copied_weak(), 0);
        queue.submit(Some(encoder.finish()));
    });
}
