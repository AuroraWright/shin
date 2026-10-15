//! Real interactive render clones, independent controllers and final weak owners.
use super::*;
use crate::{
    asset::system::{AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache},
    layer::DrawableLayer,
};
use shin_core::{
    time::{Ticks, Tween, Tweener},
    vm::command::types::LayerProperty,
};
use shin_render::shaders::types::{RenderClone, RenderCloneCtx};
use std::{path::Path, sync::Arc};

pub(super) fn clock_command(
    t: &mut Tweener,
    command: &str,
    index: usize,
    queued: usize,
    delta: f32,
) {
    if command == "populate" || (command == "mutate" && index == 0) {
        let value = if command == "mutate" {
            999.0
        } else {
            100.0 + index as f32
        };
        t.fast_forward_to(value);
        if command == "populate" {
            for j in 0..queued {
                t.enqueue(
                    value + (j + 1) as f32 * 0.25,
                    Tween::linear(Ticks::from_f32(10.0)),
                );
            }
        }
    } else if command == "advance" {
        t.update(Ticks::from_f32(delta));
    }
}
fn clocks(layer: &UserLayer, cmd: &str, queued: usize, dt: f32) -> Vec<(u32, bool)> {
    match layer {
        UserLayer::Quiz(l) => l.reference_plain_owner_clocks(cmd, queued, dt),
        UserLayer::Cake(l) => l.reference_plain_owner_clocks(cmd, queued, dt),
        UserLayer::Last(l) => l.reference_plain_owner_clocks(cmd, queued, dt),
        _ => unreachable!(),
    }
}
fn owner(layer: &UserLayer) -> (usize, Box<dyn Fn() -> usize>) {
    match layer {
        UserLayer::Quiz(l) => l.reference_plain_owner_probe(),
        UserLayer::Cake(l) => l.reference_plain_owner_probe(),
        UserLayer::Last(l) => l.reference_plain_owner_probe(),
        _ => unreachable!(),
    }
}
fn selected(layer: &UserLayer) -> i32 {
    match layer {
        UserLayer::Quiz(l) => l.reference_controller_state().2,
        UserLayer::Cake(l) => l.reference_controller_state().2,
        UserLayer::Last(l) => l.reference_controller_state().2,
        _ => unreachable!(),
    }
}
fn complete(layer: &UserLayer) -> bool {
    match layer {
        UserLayer::Quiz(l) => l.result().is_some(),
        UserLayer::Cake(l) => l.result().is_some(),
        UserLayer::Last(l) => l.result().is_some(),
        _ => unreachable!(),
    }
}
enum Textures {
    Quiz(Arc<quiz_layer::QuizTextures>),
    Cake(Arc<cake_layer::CakeTextures>),
    Last(Arc<last_layer::LastTextures>),
}
impl Textures {
    fn new_layer(&self, mask: i32) -> UserLayer {
        match self {
            Self::Quiz(t) => quiz_layer::QuizLayer::new(t.clone(), mask).into(),
            Self::Cake(t) => cake_layer::CakeLayer::new(t.clone()).into(),
            Self::Last(t) => last_layer::LastLayer::new(t.clone()).into(),
        }
    }
    fn count(&self) -> usize {
        match self {
            Self::Quiz(t) => Arc::strong_count(t),
            Self::Cake(t) => Arc::strong_count(t),
            Self::Last(t) => Arc::strong_count(t),
        }
    }
    fn probe(&self) -> Box<dyn Fn() -> usize> {
        match self {
            Self::Quiz(t) => {
                let w = Arc::downgrade(t);
                Box::new(move || w.strong_count())
            }
            Self::Cake(t) => {
                let w = Arc::downgrade(t);
                Box::new(move || w.strong_count())
            }
            Self::Last(t) => {
                let w = Arc::downgrade(t);
                Box::new(move || w.strong_count())
            }
        }
    }
}
fn state(
    id: usize,
    phase: &str,
    layers: &[Option<UserLayer>; 3],
    textures: &Textures,
    probes: &[(usize, Box<dyn Fn() -> usize>)],
) -> String {
    let live = layers.iter().flatten().count();
    assert_eq!(
        textures.count(),
        live + 1,
        "Only the explicit fixture atlas and real layers own textures"
    );
    for (i, (_, weak)) in probes.iter().enumerate() {
        assert_eq!(weak(), usize::from(layers[i].is_some()));
    }
    let mut words = vec![
        "I".into(),
        id.to_string(),
        phase.into(),
        usize::from(live != 0).to_string(),
    ];
    for l in layers {
        words.push(usize::from(l.is_some()).to_string());
        let Some(l) = l else { continue };
        let p = l.properties();
        let populated = p.initialized_identity().is_some();
        words.extend([
            selected(l).to_string(),
            usize::from(complete(l)).to_string(),
            usize::from(populated).to_string(),
        ]);
        if populated {
            for i in 0..90 {
                let t = p
                    .reference_tweener_snapshot(<LayerProperty as enum_map::Enum>::from_usize(i))
                    .unwrap();
                words.extend([
                    format!("{:08x}", t.value().to_bits()),
                    usize::from(!t.is_idle()).to_string(),
                ]);
            }
        }
        let c = clocks(l, "observe", 0, 0.0);
        words.push(c.len().to_string());
        for (bits, busy) in c {
            words.extend([format!("{bits:08x}"), usize::from(busy).to_string()]);
        }
    }
    words.join(" ")
}
fn advance(layer: &mut UserLayer, dt: f32) {
    clocks(layer, "advance", 0, dt);
    if layer.properties().initialized_identity().is_some() {
        for i in 0..90 {
            layer
                .properties_mut()
                .property_tweener_mut(<LayerProperty as enum_map::Enum>::from_usize(i))
                .update(Ticks::from_f32(dt));
        }
    }
}
#[test]
#[ignore = "supplied ROM,native ownership traces and offscreen Metal;no audio/window/saves"]
fn reference_native_plain_controller_clone_and_final_retirement() {
    crate::init_test_task_pools();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output = std::path::PathBuf::from(std::env::var_os("SHIN_PLAIN_OWNER_OUTPUT").unwrap());
    let requests = std::fs::read_to_string(output.join("request.txt")).unwrap();
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let mut total = 0;
    for version in ["original", "1.03"] {
        let expected = std::fs::read_to_string(output.join(format!("{version}.trace"))).unwrap();
        let expected: Vec<_> = expected.lines().filter(|s| s.starts_with("I ")).collect();
        let mut rows = Vec::new();
        for kind in 0..3 {
            let mut io = LayeredAssetIo::new();
            io.try_with_rom(root.join("umineko_switch_files/data.rom"))
                .unwrap();
            if version == "1.03" {
                io.try_with_rom(root.join("umineko_switch_files/patch.rom"))
                    .unwrap();
            }
            let server = AssetServer::new(
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
            );
            let textures = match kind {
                0 => Textures::Quiz(shin_tasks::block_on(server.load("/quiz.txa")).unwrap()),
                1 => Textures::Cake(shin_tasks::block_on(server.load("/cake.txa")).unwrap()),
                2 => Textures::Last(shin_tasks::block_on(server.load("/last.txa")).unwrap()),
                _ => unreachable!(),
            };
            drop(server);
            let texture_probe = textures.probe();
            assert_eq!(texture_probe(), 1);
            for (id, line) in requests.lines().enumerate() {
                let p: Vec<usize> = line
                    .split_whitespace()
                    .map(|n| n.parse().unwrap())
                    .collect();
                if p[0] != kind {
                    continue;
                };
                let (_, mask, populated, queued, completed, order) =
                    (p[0], p[1], p[2], p[3], p[4], p[5]);
                let mut layers = [Some(textures.new_layer(mask as i32)), None, None];
                let source = layers[0].as_mut().unwrap();
                if completed != 0 {
                    match source {
                        UserLayer::Quiz(l) => l.restore_completed_answer(
                            (usize::BITS - 1 - mask.leading_zeros()) as i32,
                        ),
                        UserLayer::Cake(l) => l.restore_completed_answer(3),
                        UserLayer::Last(l) => l.restore_completed_answer(3),
                        _ => unreachable!(),
                    }
                }
                clocks(source, "selection", 0, 0.0);
                clocks(source, "populate", queued, 0.0);
                if populated != 0 {
                    for i in 0..90 {
                        clock_command(
                            &mut source.properties_mut().property_tweener_mut(
                                <LayerProperty as enum_map::Enum>::from_usize(i),
                            ),
                            "populate",
                            i,
                            3,
                            0.0,
                        );
                    }
                }
                let mut probes = vec![owner(source)];
                rows.push(state(id, "source", &layers, &textures, &probes));
                let mut encoder = device.create_command_encoder(&Default::default());
                let mut ctx = RenderCloneCtx {
                    device: &device,
                    encoder: &mut encoder,
                };
                for g in 1..3 {
                    layers[g] = Some(layers[g - 1].as_ref().unwrap().render_clone(&mut ctx));
                    probes.push(owner(layers[g].as_ref().unwrap()));
                }
                for i in 0..3 {
                    for j in 0..i {
                        assert_ne!(
                            probes[i].0, probes[j].0,
                            "Copies own distinct mutable models"
                        );
                        if populated != 0 {
                            assert_ne!(
                                layers[i]
                                    .as_ref()
                                    .unwrap()
                                    .properties()
                                    .initialized_identity(),
                                layers[j]
                                    .as_ref()
                                    .unwrap()
                                    .properties()
                                    .initialized_identity()
                            );
                        }
                    }
                }
                queue.submit(Some(encoder.finish()));
                rows.push(state(id, "copies", &layers, &textures, &probes));
                advance(layers[1].as_mut().unwrap(), 5.0);
                rows.push(state(id, "advance-copy", &layers, &textures, &probes));
                clocks(layers[1].as_ref().unwrap(), "mutate", 0, 0.0);
                rows.push(state(id, "mutate-copy", &layers, &textures, &probes));
                for i in 0..3 {
                    drop(layers[(order + i) % 3].take());
                    rows.push(state(id, "drop", &layers, &textures, &probes));
                    for l in layers.iter_mut().flatten() {
                        advance(l, 10.0);
                    }
                    rows.push(state(id, "survivors", &layers, &textures, &probes));
                }
                assert!(probes.iter().all(|(_, w)| w() == 0));
                total += 1;
            }
            drop(textures);
            assert_eq!(texture_probe(), 0, "Final decoded atlas owner retired");
            device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
        }
        assert_eq!(rows.len(), expected.len());
        for (i, (a, b)) in rows.iter().zip(expected).enumerate() {
            assert_eq!(a, b, "{version} native row{i}");
        }
        std::fs::write(
            output.join(format!("{version}.port.trace")),
            rows.join("\n") + "\n",
        )
        .unwrap();
    }
    println!(
        "Plain controller ownership:{total} cases;actual render copies,controller weak final release and all six decoded atlas owners retired"
    );
}

mod group_owner_tests;
