//! Actual native prepared-batch/Effect owner observations and Group scheduling.
use super::*;
use crate::{
    asset::system::{AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache},
    layer::user::{EffectLayer, TileLayer},
    wiper::DefaultWiper,
};
use shin_core::primitives::update::FrameId;
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{buffer::BytesAddress, texture::TextureSamplerStore},
};
use std::path::PathBuf;

fn tile(id: usize) -> UserLayer {
    let i = id as f32;
    TileLayer::new(
        FloatColor4::from_rgba(0.125 * i, 0.25, 0.5, 1.0),
        glam::vec4(-32.0 * i, 16.0 * i, 96.0, 48.0),
    )
    .into()
}
fn wipe() -> AnyWiper {
    let mut wipe = DefaultWiper::new(shin_core::time::Ticks::from_f32(30.0));
    wipe.advance_ticks(7.5);
    wipe.into()
}
fn invisible() -> UserLayer {
    let mut effect = EffectLayer::new();
    effect
        .properties_mut()
        .property_tweener_mut(LayerProperty::ShowLayer)
        .fast_forward_to(0.0);
    effect.into()
}
fn pending(owner: &LayerOwner<UserLayer>) -> Vec<RetainedLayer> {
    let layer = owner.read();
    let UserLayer::Effect(effect) = &*layer else {
        panic!("actual Effect")
    };
    effect.reference_pending_children().to_vec()
}
struct TileProbe {
    identity: usize,
    count: Box<dyn Fn() -> usize>,
}
fn probe(group: &LayerGroup, bank: LayerbankId, probes: &mut Vec<TileProbe>) {
    let owner = group.layer_owner(bank).unwrap();
    probes.push(TileProbe {
        identity: owner.reference_identity(),
        count: Box::new(owner.reference_weak()),
    });
}
fn clocks(owner: &LayerOwner<AnyWiper>) -> [u32; 2] {
    let wipe = owner.read();
    let AnyWiper::Default(wipe) = &*wipe else {
        panic!("Default wipe")
    };
    wipe.reference_time_words()
}
fn observe(
    id: usize,
    phase: &str,
    group: &LayerGroup,
    batch: &Option<LayerOwner<PreparedSlotBatch>>,
    effect: &Option<LayerOwner<UserLayer>>,
    copy: &Option<LayerOwner<PreparedSlotBatch>>,
    effect_copy: &Option<LayerOwner<UserLayer>>,
    probes: &[TileProbe],
    wipe_identity: usize,
    wipe_count: &dyn Fn() -> usize,
) -> String {
    let children = effect.as_ref().map_or_else(Vec::new, pending);
    let owned = batch
        .as_ref()
        .map(LayerOwner::retain)
        .or_else(|| children.first().cloned());
    let held = owned.as_ref().map(|l| {
        l.reference_read::<PreparedSlotBatch>()
            .expect("actual prepared batch cell")
    });
    let copied = copy.as_ref().map(LayerOwner::read);
    let mut words = vec![
        "B".into(),
        id.to_string(),
        phase.into(),
        usize::from(batch.is_some()).to_string(),
        usize::from(effect.is_some()).to_string(),
        usize::from(copy.is_some()).to_string(),
        usize::from(effect_copy.is_some()).to_string(),
        held.as_ref().map_or(0, |b| b.pairs.len()).to_string(),
        children.len().to_string(),
        probes.len().to_string(),
    ];
    words.extend(probes.iter().map(|p| (p.count)().to_string()));
    let observed = if let Some(b) = &held {
        clocks(b.wiper.as_ref().unwrap())
    } else if !group.layers.is_empty() {
        clocks(group.layers[0].layer.read().wiper.as_ref().unwrap())
    } else {
        [0, 0]
    };
    let cw = copied.as_ref().and_then(|b| b.wiper.as_ref());
    let copied_clocks = cw.map_or([0, 0], clocks);
    words.extend([
        wipe_count().to_string(),
        format!("{:08x}", observed[0]),
        format!("{:08x}", observed[1]),
        usize::from(
            held.as_ref()
                .is_some_and(|b| b.wiper.as_ref().unwrap().reference_identity() == wipe_identity),
        )
        .to_string(),
        usize::from(cw.is_some_and(|w| w.reference_identity() != wipe_identity)).to_string(),
        format!("{:08x}", copied_clocks[0]),
        format!("{:08x}", copied_clocks[1]),
        copied.as_ref().map_or(0, |b| b.pairs.len()).to_string(),
        effect_copy
            .as_ref()
            .map_or(0, |e| pending(e).len())
            .to_string(),
    ]);
    let identity = |owner: Option<&RetainedLayer>| {
        owner.map_or(0, |owner| {
            probes
                .iter()
                .position(|p| p.identity == owner.reference_identity())
                .unwrap()
                + 1
        })
    };
    if let Some(b) = held {
        for pair in &b.pairs {
            words.extend([
                identity(pair.source.as_ref()).to_string(),
                identity(Some(&pair.target)).to_string(),
            ]);
        }
    }
    words.join(" ")
}

#[test]
#[ignore = "genuine native prepared-batch/Effect traces and offscreen Metal;no audio/window/saves"]
fn reference_native_prepared_batch_effect_owner_retirement() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_BATCH_EFFECT_OWNER_OUTPUT").unwrap());
    let request = std::fs::read_to_string(output.join("request.txt")).unwrap();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let resize =
        SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(16, 16)));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Prepared batch owner depth"),
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
    let server = Arc::new(AssetServer::new(
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
    let mut frame = FrameId::default();
    let mut actual = Vec::new();
    let mut weak_retirements = 0;
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
    for (id, line) in request.lines().enumerate() {
        let p = line
            .split_whitespace()
            .map(|w| w.parse::<usize>().unwrap())
            .collect::<Vec<_>>();
        let (source, n, mode, order) = (p[0], p[1], p[2], p[3]);
        let mut group = LayerGroup::new(None);
        let bank = LayerbankId::new(0);
        let mut probes = Vec::new();
        if source != 0 {
            group.add_layer(bank, tile(1));
            probe(&group, bank, &mut probes);
        }
        group.add_layer_with_wiper(bank, tile(2), Some(wipe()));
        probe(&group, bank, &mut probes);
        let wiper = group.layers[0].layer.read().wiper.as_ref().unwrap().clone();
        let wipe_identity = wiper.reference_identity();
        let wipe_count = wiper.reference_weak();
        drop(wiper);
        let mut batch: Option<LayerOwner<PreparedSlotBatch>> = None;
        for i in 1..n {
            let bank = LayerbankId::new(i as u8);
            group.add_layer(bank, tile(i * 2 + 1));
            probe(&group, bank, &mut probes);
            if mode != 0 {
                group.add_layer_with_wiper(bank, tile(i * 2 + 2), Some(wipe()));
                probe(&group, bank, &mut probes);
            }
            if let Some(batch) = &mut batch {
                batch.write().append(group.prepared_pair(i));
            } else {
                batch = Some(group.build_slot_batch(&[0, 1]));
            }
            actual.push(observe(
                id,
                "append",
                &group,
                &batch,
                &None,
                &None,
                &None,
                &probes,
                wipe_identity,
                &wipe_count,
            ));
        }
        let batch_weak = batch.as_ref().unwrap().reference_weak();
        let mut effect = Some(LayerOwner::new(invisible()));
        let effect_weak = effect.as_ref().unwrap().reference_weak();
        effect.as_mut().unwrap().pre_render_with_owned_lower(
            &mut context,
            &TransformParams::default(),
            &[batch.as_ref().unwrap().retain()],
        );
        actual.push(observe(
            id,
            "capture",
            &group,
            &batch,
            &effect,
            &None,
            &None,
            &probes,
            wipe_identity,
            &wipe_count,
        ));
        let mut copy = Some(
            batch
                .as_ref()
                .unwrap()
                .render_clone(&mut context.render_clone_ctx()),
        );
        let mut effect_copy = Some(
            effect
                .as_ref()
                .unwrap()
                .render_clone(&mut context.render_clone_ctx()),
        );
        let copied_weak = copy.as_ref().unwrap().reference_weak();
        let effect_copy_weak = effect_copy.as_ref().unwrap().reference_weak();
        actual.push(observe(
            id,
            "copies",
            &group,
            &batch,
            &effect,
            &copy,
            &effect_copy,
            &probes,
            wipe_identity,
            &wipe_count,
        ));
        frame.advance();
        let update = AdvUpdateContext {
            frame_id: frame,
            delta_ticks: shin_core::time::Ticks::from_f32(5.0),
            asset_server: &server,
            device: &device,
            queue: &queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        };
        group.update(&update);
        batch.as_mut().unwrap().update(&update);
        actual.push(observe(
            id,
            "advance",
            &group,
            &batch,
            &effect,
            &copy,
            &effect_copy,
            &probes,
            wipe_identity,
            &wipe_count,
        ));
        let orders = [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        for what in orders[order] {
            let phase = match what {
                0 => {
                    group.clear_layers();
                    "drop-graph"
                }
                1 => {
                    batch = None;
                    "drop-batch"
                }
                _ => {
                    effect = None;
                    "drop-effect"
                }
            };
            actual.push(observe(
                id,
                phase,
                &group,
                &batch,
                &effect,
                &copy,
                &effect_copy,
                &probes,
                wipe_identity,
                &wipe_count,
            ));
        }
        copy = None;
        effect_copy = None;
        actual.push(observe(
            id,
            "final",
            &group,
            &batch,
            &effect,
            &copy,
            &effect_copy,
            &probes,
            wipe_identity,
            &wipe_count,
        ));
        assert_eq!(batch_weak(), 0);
        assert_eq!(copied_weak(), 0);
        assert_eq!(effect_weak(), 0);
        assert_eq!(effect_copy_weak(), 0);
        assert_eq!(wipe_count(), 0);
        assert!(probes.iter().all(|p| (p.count)() == 0));
        weak_retirements += probes.len() + 5;
    }
    for version in ["original", "1.03"] {
        let native = std::fs::read_to_string(output.join(format!("{version}.trace"))).unwrap();
        let expected = native
            .lines()
            .filter(|l| l.starts_with("B "))
            .collect::<Vec<_>>();
        assert_eq!(actual.len(), expected.len());
        for (i, (got, want)) in actual.iter().zip(expected).enumerate() {
            assert_eq!(got, want, "{version} row{i}");
        }
        std::fs::write(
            output.join(format!("{version}.port.trace")),
            actual.join("\n") + "\n",
        )
        .unwrap();
    }
    assert_scheduler_retains_peer_pairs(&mut context);
    drop(context);
    dynamic.finish(&mut encoder);
    queue.submit(Some(encoder.finish()));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU work must complete before readback or retirement");
    dynamic.recall();
    println!(
        "Prepared batch/Effect:48 desktop cases match both native images;{} exact rows/image;{weak_retirements} final Weak probes; shared wipe/frame guard, empty copied pairs/captures and six destruction orders",
        actual.len()
    );
}

fn assert_scheduler_retains_peer_pairs(context: &mut PreRenderContext) {
    for recapture in [false, true] {
        let mut group = LayerGroup::new(None);
        let mut probes = Vec::new();
        let mut wrapper_probes = Vec::new();
        for i in 0..2 {
            let bank = LayerbankId::new(i);
            group.add_layer(bank, tile(1));
            probe(&group, bank, &mut probes);
            group.add_layer_with_wiper(bank, tile(2), Some(wipe()));
            probe(&group, bank, &mut probes);
            wrapper_probes.push(
                group.layers[i as usize]
                    .layer
                    .shared_owner()
                    .unwrap()
                    .reference_weak(),
            );
        }
        // Stable ordering: two matching wrappers followed by an invisible Effect.
        for bank in [0, 1] {
            group
                .get_layer_mut(LayerbankId::new(bank))
                .unwrap()
                .properties_mut()
                .property_tweener_mut(LayerProperty::RenderPosition)
                .fast_forward_to(2000.0 - bank as f32 * 100.0);
        }
        let mut effect = invisible();
        effect
            .properties_mut()
            .property_tweener_mut(LayerProperty::RenderPosition)
            .fast_forward_to(1000.0);
        group.add_layer(LayerbankId::new(2), effect);
        let effect = group.layer_owner(LayerbankId::new(2)).unwrap();
        group.pre_render(context, &TransformParams::default());
        {
            let children = pending(&effect);
            assert_eq!(children.len(), 1);
            let batch = children[0]
                .reference_read::<PreparedSlotBatch>()
                .expect("scheduler captured distinct actual batch");
            assert_eq!(batch.pairs.len(), 2);
            assert!(batch.source_texture.get().is_some() && batch.target_texture.get().is_some());
        }
        group.clear_layers();
        assert!(
            wrapper_probes.iter().all(|p| p() == 0),
            "batch does not retain semantic wrapper objects"
        );
        assert!(
            probes.iter().all(|p| (p.count)() == 1),
            "every detached old/new peer survives in captured batch"
        );
        if recapture {
            effect
                .write()
                .pre_render_with_owned_lower(context, &TransformParams::default(), &[]);
            assert!(probes.iter().all(|p| (p.count)() == 0));
        }
        drop(effect);
        drop(group); // Its native prepared list also owns the selected Effect.
        assert!(probes.iter().all(|p| (p.count)() == 0));
    }
    println!(
        "Actual Group prepared-batch capture:2 peer-removal/recapture/destruction cases;8 detached old/new peer final Weak releases; semantic wrapper owners retire independently"
    );
}
