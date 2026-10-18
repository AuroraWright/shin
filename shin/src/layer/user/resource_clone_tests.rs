//! Real ROM assets, virtual render copies and weak probes of final resources.
//! Compare bounded native decoded-resource fixtures; no audio/window/saves.
use super::*;
use crate::{
    asset::system::{AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache},
    layer::DrawableLayer,
};
use shin_core::{
    format::scenario::Scenario,
    time::{Ticks, Tween},
    vm::command::types::LayerProperty,
};
use shin_render::shaders::types::{RenderClone, RenderCloneCtx};
use std::{collections::BTreeSet, path::Path, sync::Weak};
mod composed;

enum Owner {
    Picture(Weak<Picture>),
    Bustup(Weak<Bustup>),
}
impl Owner {
    fn of(layer: &UserLayer) -> Self {
        match layer {
            UserLayer::Picture(p) => Self::Picture(p.reference_resource_owner()),
            UserLayer::Bustup(b) => Self::Bustup(b.reference_resource_owner()),
            _ => unreachable!(),
        }
    }
    fn live(&self) -> bool {
        match self {
            Self::Picture(p) => p.strong_count() != 0,
            Self::Bustup(b) => b.strong_count() != 0,
        }
    }
    fn identity(&self) -> usize {
        match self {
            Self::Picture(p) => p.as_ptr() as usize,
            Self::Bustup(b) => b.as_ptr() as usize,
        }
    }
}
fn snapshot(fixture: usize, phase: &str, layers: &[Vec<UserLayer>; 3], owners: &[Owner]) -> String {
    let mut blocks = BTreeSet::new();
    for o in owners {
        match o {
            Owner::Picture(p) => {
                if let Some(p) = p.upgrade() {
                    for (_, _, b) in p.blocks.iter_placements() {
                        blocks.insert(b as *const _ as usize);
                    }
                }
            }
            Owner::Bustup(b) => {
                if let Some(b) = b.upgrade() {
                    for block in b
                        .base_blocks
                        .iter()
                        .chain(b.face1.iter())
                        .chain(b.face2.iter())
                        .chain(&b.mouth_blocks)
                        .chain(&b.eye_blocks)
                    {
                        blocks.insert(Arc::as_ptr(block) as usize);
                    }
                }
            }
        }
    }
    let mut words = vec![
        "C".into(),
        fixture.to_string(),
        phase.into(),
        owners.len().to_string(),
        blocks.len().to_string(),
    ];
    for group in layers {
        for (n, o) in owners.iter().enumerate() {
            if group.is_empty() {
                words.push("0".into());
                continue;
            }
            let layer = &group[n];
            let actual = Owner::of(layer);
            assert_eq!(actual.identity(), o.identity());
            assert!(o.live());
            let props = layer.properties().reference_copy_words();
            words.extend([
                "1".into(),
                match layer {
                    UserLayer::Picture(_) => "2",
                    UserLayer::Bustup(_) => "3",
                    _ => unreachable!(),
                }
                .into(),
                usize::from(props.is_some()).to_string(),
            ]);
            for i in 0..90 {
                words.push(format!("{:08x}", props.as_ref().map_or(0, |v| v[i])));
            }
            match o {
                Owner::Picture(p) => {
                    let p = p.upgrade().unwrap();
                    for bits in [
                        (p.origin_x as u16 as u32) | ((p.origin_y as u16 as u32) << 16),
                        p.effective_width | (p.effective_height << 16),
                        p.flags,
                        p.scale.to_bits(),
                    ] {
                        words.push(format!("{bits:08x}"));
                    }
                }
                Owner::Bustup(_) => {
                    let UserLayer::Bustup(b) = layer else {
                        unreachable!()
                    };
                    let (seed, elapsed, wait, sequence) = b.reference_blink_state();
                    for bits in [b.reference_mouth_state(), 0, seed, elapsed, wait] {
                        words.push(format!("{bits:08x}"));
                    }
                    words.push(sequence.len().to_string());
                    words.extend(sequence.iter().map(u32::to_string));
                }
            }
        }
    }
    words.join(" ")
}
#[test]
#[ignore = "requires supplied ROM, native resource-clone traces and Metal;no audio/window/saves"]
fn reference_native_resource_layer_clone_and_final_retirement() {
    crate::init_test_task_pools();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output = std::path::PathBuf::from(std::env::var_os("SHIN_RESOURCE_CLONE_OUTPUT").unwrap());
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let mut comparisons = 0;
    for version in ["original", "1.03"] {
        let snr = std::fs::read(
            Path::new("/private/tmp/shin-authored-restoration")
                .join(version)
                .join("main.snr"),
        )
        .unwrap();
        let scenario = Scenario::new(snr.into()).unwrap();
        let mut io = LayeredAssetIo::new();
        io.try_with_rom(root.join("game-data/umineko/data.rom"))
            .unwrap();
        if version == "1.03" {
            io.try_with_rom(root.join("game-data/umineko/patch.rom"))
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
        for profile in 0..6 {
            let text =
                std::fs::read_to_string(output.join(format!("{version}-{profile}.trace"))).unwrap();
            let expected: Vec<_> = text.lines().filter(|l| l.starts_with("C ")).collect();
            let mut actual = Vec::new();
            for fixture in 0..2 {
                let mut layers: [Vec<UserLayer>; 3] = std::array::from_fn(|_| Vec::new());
                let mut owners = Vec::new();
                for line in text
                    .lines()
                    .filter(|l| l.starts_with(&format!("L {fixture} ")))
                {
                    let f: Vec<_> = line.split_whitespace().collect();
                    let id = f[4].parse::<usize>().unwrap();
                    let layer: UserLayer = if f[3] == "2" {
                        let asset = shin_tasks::block_on(server.load::<Picture, _>(f[6])).unwrap();
                        PictureLayer::new(asset, None).into()
                    } else {
                        let info = &scenario.info_tables().bustup_info[id];
                        let asset = shin_tasks::block_on(server.load_with_args::<Bustup, _>(
                            f[6],
                            BustupArgs {
                                expression: info.emotion.to_string(),
                                character_id: CharacterId::from_metadata(info.lipsync_character_id),
                                disable_animations: false,
                            },
                        ))
                        .unwrap();
                        let mut b = super::bustup_layer::BustupLayer::new(asset, None);
                        b.reference_resource_clone_blink();
                        b.into()
                    };
                    owners.push(Owner::of(&layer));
                    layers[0].push(layer);
                }
                let block_probes: Vec<_> = owners
                    .iter()
                    .flat_map(|o| match o {
                        Owner::Bustup(b) => {
                            let b = b.upgrade().unwrap();
                            b.base_blocks
                                .iter()
                                .chain(b.face1.iter())
                                .chain(b.face2.iter())
                                .chain(&b.mouth_blocks)
                                .chain(&b.eye_blocks)
                                .map(Arc::downgrade)
                                .collect::<Vec<_>>()
                        }
                        _ => Vec::new(),
                    })
                    .collect();
                let picture_probes: Vec<_> = owners
                    .iter()
                    .filter_map(|o| match o {
                        Owner::Picture(p) => Some(Arc::downgrade(&p.upgrade().unwrap().blocks)),
                        _ => None,
                    })
                    .collect();
                if profile >= 3 {
                    for (n, layer) in layers[0].iter_mut().enumerate() {
                        for i in 0..90 {
                            let p = <LayerProperty as enum_map::Enum>::from_usize(i);
                            layer
                                .properties_mut()
                                .property_tweener_mut(p)
                                .fast_forward_to((100 + n * 128 + i) as f32);
                        }
                        for i in 0..3 {
                            layer
                                .properties_mut()
                                .property_tweener_mut(LayerProperty::TranslateX)
                                .enqueue(
                                    0.25 * (i + 1) as f32,
                                    Tween::linear(Ticks::from_f32(10.0)),
                                );
                        }
                    }
                }
                let mut encoder = device.create_command_encoder(&Default::default());
                let mut ctx = RenderCloneCtx {
                    device: &device,
                    encoder: &mut encoder,
                };
                for g in 1..3 {
                    layers[g] = layers[g - 1]
                        .iter()
                        .map(|l| l.render_clone(&mut ctx))
                        .collect();
                    for n in 0..owners.len() {
                        let a = layers[g - 1][n].properties().initialized_identity();
                        let b = layers[g][n].properties().initialized_identity();
                        assert_eq!(a.is_some(), b.is_some());
                        if a.is_some() {
                            assert_ne!(a, b);
                        }
                    }
                }
                actual.push(snapshot(fixture, "cloned", &layers, &owners));
                if profile >= 3 {
                    for layer in &mut layers[1] {
                        layer
                            .properties_mut()
                            .property_tweener_mut(LayerProperty::TranslateX)
                            .fast_forward_unless_prohibited();
                        assert_eq!(
                            layer.properties().get_value(LayerProperty::TranslateX),
                            0.75
                        );
                    }
                    actual.push(snapshot(fixture, "copy-forwarded", &layers, &owners));
                }
                if profile % 3 == 1 {
                    layers[1].clear();
                    actual.push(snapshot(fixture, "copy-first", &layers, &owners));
                }
                if profile % 3 == 2 {
                    layers[2].clear();
                    layers[1].clear();
                    actual.push(snapshot(fixture, "copies-first", &layers, &owners));
                }
                layers[0].clear();
                actual.push(snapshot(fixture, "source-dropped", &layers, &owners));
                if !layers[1].is_empty() {
                    layers[1].clear();
                    actual.push(snapshot(fixture, "copy-dropped", &layers, &owners));
                }
                if !layers[2].is_empty() {
                    layers[2].clear();
                    actual.push(snapshot(fixture, "last-dropped", &layers, &owners));
                }
                assert!(owners.iter().all(|o| !o.live()), "asset owner leaked");
                assert!(
                    block_probes.iter().all(|p| p.strong_count() == 0),
                    "decoded Bustup block leaked"
                );
                assert!(
                    picture_probes.iter().all(|p| p.strong_count() == 0),
                    "decoded Picture block owner leaked"
                );
                device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
            }
            std::fs::write(
                output.join(format!("{version}-{profile}.port.trace")),
                actual.join("\n") + "\n",
            )
            .unwrap();
            assert_eq!(actual, expected, "{version} profile{profile}");
            comparisons += actual.len();
        }
    }
    println!(
        "Resource layer clones:24 profiles,{comparisons} native/port observations; real assets and decoded blocks retired"
    );
}
