//! Exact native Movie composed-owner rows, with explicit prepared fixtures.
use super::*;
use crate::{
    asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache},
    layer::{LayerGroup, properties::LayerProperties},
    wiper::DefaultWiper,
};
use shin_core::{
    time::{Ticks, Tween},
    vm::command::types::{LayerProperty, LayerbankId},
};
use shin_render::shaders::types::{RenderClone, RenderCloneCtx};
use std::{collections::BTreeSet, path::Path};
enum Root {
    Movie(UserLayer),
    Group(LayerGroup),
}
impl Root {
    fn leaves(&self) -> Vec<crate::layer::layer_group::ReferenceLayerLeaf<'_, UserLayer>> {
        match self {
            Self::Movie(l) => vec![crate::layer::layer_group::ReferenceLayerLeaf::borrowed(l)],
            Self::Group(g) => g.reference_owner_leaves(),
        }
    }
    fn target(&mut self) -> crate::layer::owner::LayerWrite<'_, UserLayer> {
        match self {
            Self::Movie(l) => crate::layer::owner::LayerWrite::Direct(l),
            Self::Group(g) => g.get_layer_mut(LayerbankId::new(0)).unwrap(),
        }
    }
    fn copy(&self, ctx: &mut RenderCloneCtx) -> Self {
        match self {
            Self::Movie(l) => Self::Movie(l.render_clone(ctx)),
            Self::Group(g) => Self::Group(g.render_clone(ctx)),
        }
    }
    fn forward(&mut self) {
        match self {
            Self::Movie(l) => l.fast_forward(),
            Self::Group(g) => g.fast_forward(),
        }
    }
    fn slots(&self) -> usize {
        match self {
            Self::Movie(_) => 0,
            Self::Group(g) => g.reference_transition_words().len(),
        }
    }
}
fn movie(l: &UserLayer) -> &movie_layer::MovieLayer {
    let UserLayer::Movie(m) = l else {
        panic!("Expected Movie")
    };
    m
}
fn movie_mut(l: &mut UserLayer) -> &mut movie_layer::MovieLayer {
    let UserLayer::Movie(m) = l else {
        panic!("Expected Movie")
    };
    m
}
fn fill(p: &mut LayerProperties, identity: usize) {
    for i in 0..90 {
        let value = (100 + identity * 128 + i) as f32;
        let mut t = p.property_tweener_mut(<LayerProperty as enum_map::Enum>::from_usize(i));
        t.fast_forward_to(value);
        for j in 0..3 {
            t.enqueue(
                value + (j + 1) as f32 * 0.25,
                Tween::linear(Ticks::from_f32(10.0)),
            );
        }
    }
}
fn root(
    prototype: &movie_layer::MovieLayer,
    graph: usize,
    populated: bool,
    ctx: &mut RenderCloneCtx,
) -> Root {
    let mut source: UserLayer = prototype.render_clone(ctx).into();
    if populated {
        fill(source.properties_mut(), 0);
        fill(&mut movie(&source).reference_still_properties(), 1);
    }
    if graph == 0 {
        return Root::Movie(source);
    }
    let mut group = LayerGroup::new(None);
    group.add_layer(LayerbankId::new(0), source);
    for _ in 1..graph {
        let next = {
            let leaves = group.reference_owner_leaves();
            let first = &leaves[0];
            let mut next = first.render_clone(ctx);
            if populated {
                next.set_properties(first.properties().share());
            }
            next
        };
        group.add_layer_with_wiper(
            LayerbankId::new(0),
            next,
            Some(DefaultWiper::new(Ticks::from_f32(30.0)).into()),
        );
    }
    Root::Group(group)
}
fn props_words(words: &mut Vec<String>, p: &LayerProperties) {
    let populated = p.initialized_identity().is_some();
    words.push(usize::from(populated).to_string());
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
}
fn state(
    fixture: usize,
    phase: &str,
    roots: &[Option<Root>; 3],
    owner: &movie_layer::ReferenceMovieOwner,
    picture: &std::sync::Weak<Picture>,
    fixture_picture_owners: usize,
) -> String {
    let count = roots
        .iter()
        .flatten()
        .map(|r| r.leaves().len())
        .sum::<usize>();
    assert_eq!(
        picture.strong_count(),
        count + fixture_picture_owners,
        "One linked picture asset owner per Movie plus prepared fixtures"
    );
    let p = picture.upgrade().unwrap();
    let blocks = p
        .blocks
        .iter_placements()
        .map(|(_, _, b)| b as *const _ as usize)
        .collect::<BTreeSet<_>>()
        .len();
    let header = [
        (p.origin_x as u16 as u32) | ((p.origin_y as u16 as u32) << 16),
        p.effective_width | (p.effective_height << 16),
        p.flags,
        p.scale.to_bits(),
    ];
    drop(p);
    let mut words = vec![
        "O".into(),
        fixture.to_string(),
        phase.into(),
        if count != 0 {
            blocks.to_string()
        } else {
            "0".into()
        },
    ];
    let mut properties = Vec::new();
    let mut slots = 0;
    let mut shared = 0;
    let mut still_models = BTreeSet::new();
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
            let m = movie(&l);
            let (transparency, repeat, back, still) = m.reference_composed_owner_flags();
            shared += usize::from(back);
            assert!(
                still_models.insert(still),
                "Every linked mutable Picture wrapper is independent"
            );
            if back {
                assert_eq!(m.reference_publication_owner().identity(), owner.identity());
            }
            assert_eq!(m.reference_still_owner().as_ptr(), picture.as_ptr());
            words.extend([
                transparency.to_string(),
                usize::from(repeat).to_string(),
                usize::from(back).to_string(),
                "1".into(),
            ]);
            let p = l.properties();
            properties.push(p.initialized_identity());
            props_words(&mut words, p);
            let p = m.reference_still_properties();
            properties.push(p.initialized_identity());
            props_words(&mut words, &p);
            words.extend(header.iter().map(|h| format!("{h:08x}")));
        }
    }
    for a in &properties {
        for b in &properties {
            words.push(usize::from(a.is_some() && a == b).to_string());
        }
    }
    words.extend([slots.to_string(), shared.to_string()]);
    assert_eq!(
        owner.owners(),
        shared + 1,
        "Only recorded Movie owners and explicit prepared fixture share playback"
    );
    words.join(" ")
}
#[test]
#[ignore = "supplied ROM,exact Loopback Audio,native Movie traces and Metal;no window/saves"]
fn reference_native_movie_composed_clone_and_final_owners() {
    crate::init_test_task_pools();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output = std::path::PathBuf::from(std::env::var_os("SHIN_MOVIE_OWNER_OUTPUT").unwrap());
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let audio = Arc::new(AudioManager::new_for_tests());
    audio.suspension().set_suspended(true);
    let mut cases = 0;
    for version in ["original", "1.03"] {
        let bytes = std::fs::read(
            Path::new("/private/tmp/shin-authored-saved-movie")
                .join(version)
                .join("main.snr"),
        )
        .unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        let mut io = LayeredAssetIo::new();
        io.try_with_rom(workspace.join("game-data/umineko/data.rom"))
            .unwrap();
        if version == "1.03" {
            io.try_with_rom(workspace.join("game-data/umineko/patch.rom"))
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
        let baseline =
            std::fs::read_to_string(output.join(format!("{version}-0-0-0-0.trace"))).unwrap();
        let mut prototypes = Vec::new();
        for fixture in 0..5 {
            let fields: Vec<_> = baseline
                .lines()
                .find(|l| {
                    l.starts_with(&format!("L {fixture} "))
                        && l.split_whitespace().nth(3) == Some("6")
                })
                .unwrap()
                .split_whitespace()
                .collect();
            let bank: usize = fields[2].parse().unwrap();
            let id: usize = fields[4].parse().unwrap();
            let info = &scenario.info_tables().movie_info[id];
            let movie_path = fields[6].to_string();
            let fields: Vec<_> = baseline
                .lines()
                .find(|l| l.starts_with(&format!("R {} 6 ", fixture * 48 + bank)))
                .unwrap()
                .split_whitespace()
                .collect();
            let gain = u32::from_str_radix(fields[5], 10).unwrap();
            let repeat = fields[6] == "1";
            let linked: usize = fields[7].parse().unwrap();
            let picture_path = scenario.info_tables().picture_info[linked].path();
            let assets = server.clone();
            let movie = shin_tasks::block_on(shin_tasks::async_io::spawn(async move {
                assets.load::<Movie, _>(movie_path).await.unwrap()
            }));
            let assets = server.clone();
            let still = shin_tasks::block_on(shin_tasks::async_io::spawn(async move {
                assets.load::<Picture, _>(picture_path).await.unwrap()
            }));
            let args = movie_layer::MovieArgs {
                volume_source: info.volume_source,
                transparency: info.transparency,
                local_volume: Volume(f32::from_bits(gain)),
                repeat,
            };
            let gpu = device.clone();
            let sound = audio.clone();
            let prototype = shin_tasks::block_on(shin_tasks::async_io::spawn(async move {
                movie_layer::MovieLayer::new(
                    &gpu,
                    &sound,
                    movie,
                    args,
                    Some(still),
                    Default::default(),
                )
                .await
            }));
            prototypes.push(prototype);
        }
        drop(server);
        for graph in 0..4 {
            for populated in 0..2 {
                for order in 0..2 {
                    for detach in 0..2 {
                        let label = format!("{version}-{graph}-{populated}-{order}-{detach}");
                        let native =
                            std::fs::read_to_string(output.join(format!("{label}.trace"))).unwrap();
                        let expected: Vec<_> =
                            native.lines().filter(|l| l.starts_with("O ")).collect();
                        let mut rows = Vec::new();
                        for (fixture, prototype) in prototypes.iter().enumerate() {
                            let owner = prototype.reference_publication_owner();
                            let picture = prototype.reference_still_owner();
                            let fixture_picture_owners = picture.strong_count();
                            let mut encoder = device.create_command_encoder(&Default::default());
                            let mut ctx = RenderCloneCtx {
                                device: &device,
                                encoder: &mut encoder,
                            };
                            let mut roots = [
                                Some(root(prototype, graph, populated != 0, &mut ctx)),
                                None,
                                None,
                            ];
                            rows.push(state(
                                fixture,
                                "source",
                                &roots,
                                &owner,
                                &picture,
                                fixture_picture_owners,
                            ));
                            for g in 1..3 {
                                roots[g] = Some(roots[g - 1].as_ref().unwrap().copy(&mut ctx));
                            }
                            queue.submit(Some(encoder.finish()));
                            rows.push(state(
                                fixture,
                                "copies",
                                &roots,
                                &owner,
                                &picture,
                                fixture_picture_owners,
                            ));
                            let mut p = roots[1].as_mut().unwrap().target();
                            p.properties_mut()
                                .property_tweener_mut(
                                    <LayerProperty as enum_map::Enum>::from_usize(0),
                                )
                                .fast_forward_to(999.0);
                            movie(&p)
                                .reference_still_properties()
                                .property_tweener_mut(
                                    <LayerProperty as enum_map::Enum>::from_usize(0),
                                )
                                .fast_forward_to(-123.0);
                            drop(p);
                            rows.push(state(
                                fixture,
                                "mutate-copy",
                                &roots,
                                &owner,
                                &picture,
                                fixture_picture_owners,
                            ));
                            if detach != 0 {
                                movie_mut(&mut roots[1].as_mut().unwrap().target())
                                    .reference_composed_owner_detach();
                                rows.push(state(
                                    fixture,
                                    "detach-copy",
                                    &roots,
                                    &owner,
                                    &picture,
                                    fixture_picture_owners,
                                ));
                            }
                            roots[1].as_mut().unwrap().forward();
                            rows.push(state(
                                fixture,
                                "forward-copy",
                                &roots,
                                &owner,
                                &picture,
                                fixture_picture_owners,
                            ));
                            drop(roots[if order != 0 { 2 } else { 0 }].take());
                            rows.push(state(
                                fixture,
                                "first-drop",
                                &roots,
                                &owner,
                                &picture,
                                fixture_picture_owners,
                            ));
                            rows.push(state(
                                fixture,
                                "task-dropped",
                                &roots,
                                &owner,
                                &picture,
                                fixture_picture_owners,
                            ));
                            drop(roots[1].take());
                            rows.push(state(
                                fixture,
                                "middle-drop",
                                &roots,
                                &owner,
                                &picture,
                                fixture_picture_owners,
                            ));
                            drop(roots[if order != 0 { 0 } else { 2 }].take());
                            rows.push(state(
                                fixture,
                                "final",
                                &roots,
                                &owner,
                                &picture,
                                fixture_picture_owners,
                            ));
                            assert_eq!(owner.owners(), 1);
                            assert_eq!(picture.strong_count(), fixture_picture_owners);
                            cases += 1;
                        }
                        std::fs::write(
                            output.join(format!("{label}.port.trace")),
                            rows.join("\n") + "\n",
                        )
                        .unwrap();
                        assert_eq!(rows.len(), expected.len());
                        for (i, (a, b)) in rows.iter().zip(expected).enumerate() {
                            assert_eq!(a, b, "{label} native composed Movie row{i}");
                        }
                    }
                }
            }
        }
        let fixture_pictures: Vec<_> = prototypes
            .iter()
            .map(|p| p.reference_still_owner())
            .collect();
        for prototype in prototypes {
            let owner = prototype.reference_publication_owner();
            drop(prototype);
            owner.assert_released();
        }
        for picture in fixture_pictures {
            assert_eq!(picture.strong_count(), 0);
        }
        device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
    }
    println!(
        "Movie ownership:{cases} cases;exact native composed rows;per-case graph owners released;10 final fixture playback/player retirements and all linked-Picture Weak retirements"
    );
}
