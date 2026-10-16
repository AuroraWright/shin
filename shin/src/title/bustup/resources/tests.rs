use super::*;
use shin_core::time::Tween;
#[test]
fn publication_installs_inherited_target_and_retains_native_replacement_wiper() {
    use shin_core::vm::command::types::LayerbankId;
    let slot = LayerbankId::new(10);
    let mut group = LayerGroup::new(None);
    initial_layer(10, UserLayer::Null(NullLayer::new())).install(&mut group);
    assert!(!group.has_wiper_for_layerbank(slot));
    let mut clock = group
        .get_layer_mut(slot)
        .unwrap()
        .properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX);
    clock.enqueue(100.0, Tween::linear(Ticks::from_f32(10.0)));
    clock.update(Ticks::from_f32(5.0));
    drop(clock);
    replacement(10, UserLayer::Null(NullLayer::new())).install(&mut group);
    assert!(group.has_wiper_for_layerbank(slot));
    let mut layer = group.get_layer_mut(slot).unwrap();
    assert_eq!(
        layer.properties().get_value(LayerProperty::TranslateX),
        50.0
    );
    assert_eq!(
        layer.properties().get_value(LayerProperty::TranslateZ),
        1000.0
    );
    layer
        .properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX)
        .update(Ticks::from_f32(5.0));
    assert_eq!(
        layer.properties().get_value(LayerProperty::TranslateX),
        100.0
    );
}

#[test]
fn initial_slots_keep_native_depth_order_and_background_scale() {
    for slot in [0, 1, 10] {
        let p = initial_layer(slot, UserLayer::Null(NullLayer::new()));
        assert_eq!(p.transition_ticks, Ticks::ZERO);
        assert!(!p.inherit_properties);
        let props = p.layer.properties();
        assert_eq!(
            props.get_value(LayerProperty::RenderPosition),
            if slot < 2 {
                1000.0 + slot as f32
            } else {
                1000.0
            }
        );
        assert_eq!(
            props.get_value(LayerProperty::TranslateZ),
            if slot < 2 { 2000.0 } else { 1000.0 }
        );
        assert_eq!(
            props.get_value(LayerProperty::ScaleX),
            if slot < 2 { 2000.0 } else { 1000.0 }
        );
        assert_eq!(
            props.get_value(LayerProperty::ScaleY),
            if slot < 2 { 2000.0 } else { 1000.0 }
        );
    }
}
#[test]
fn replacement_copies_current_properties_and_outstanding_tween_queue() {
    let mut old = UserLayer::Null(NullLayer::new());
    let mut clock = old
        .properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX);
    clock.enqueue(100.0, Tween::linear(Ticks::from_f32(10.0)));
    clock.enqueue(200.0, Tween::linear(Ticks::from_f32(10.0)));
    clock.update(Ticks::from_f32(5.0));
    drop(clock);
    let mut p = replacement(10, UserLayer::Null(NullLayer::new()));
    assert_eq!(p.transition_ticks, Ticks::from_f32(10.0));
    p.inherit_from(&old);
    assert_eq!(
        p.layer.properties().get_value(LayerProperty::TranslateX),
        50.0
    );
    p.layer
        .properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX)
        .update(Ticks::from_f32(10.0));
    assert_eq!(
        p.layer.properties().get_value(LayerProperty::TranslateX),
        150.0
    );
    assert_eq!(old.properties().get_value(LayerProperty::TranslateX), 50.0);
}

#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_original_async_resource_publication_barriers_and_inheritance() {
    use crate::asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache};
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

    use shin_core::{
        format::{rom::RomReader, scenario::Scenario},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(&path).unwrap()).unwrap()).unwrap();
    let mut bytes = Vec::new();
    rom.open_file(rom.find_file("/main.snr").unwrap())
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Arc::new(Scenario::new(bytes.into()).unwrap());
    let catalog = crate::title::bustup::catalog::BustupCatalog::new(
        &scenario.info_tables().character_box_info,
    )
    .unwrap();
    let initial_id = catalog.characters[0].outfits[0].poses[0].expressions[0];
    let mut resources = BustupResources::new(server, scenario, &catalog.backgrounds[0], initial_id);
    assert!(resources.has_blocking_loads());
    let mut order = Vec::new();
    use shin_core::vm::command::types::LayerbankId;
    let mut layers = LayerGroup::new(None);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while resources.has_blocking_loads() {
        assert!(
            std::time::Instant::now() < deadline,
            "Initial Bustup worker timeout"
        );
        let blocked = resources.has_blocking_loads();
        for event in resources.poll().unwrap() {
            assert!(blocked);
            match event {
                Publication::Atlas(atlas) => {
                    let _ = atlas.get("char01");
                    order.push(0);
                }
                Publication::Mask(_) => order.push(1),
                Publication::Layer(p) => {
                    order.push(match p.slot {
                        0 => 2,
                        1 => 3,
                        10 => 4,
                        _ => panic!(),
                    });
                    assert_eq!(p.transition_ticks, Ticks::ZERO);
                    p.install(&mut layers);
                }
                Publication::InitialComplete => order.push(5),
                Publication::InitialFailed(error) | Publication::ReplacementFailed(error) => {
                    panic!("Unexpected supplied resource failure: {error:#}")
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(order, [0, 1, 2, 3, 4, 5]);
    assert!(matches!(
        *layers.get_layer(LayerbankId::new(0)).unwrap(),
        UserLayer::Picture(_)
    ));
    assert!(matches!(
        *layers.get_layer(LayerbankId::new(1)).unwrap(),
        UserLayer::Null(_)
    ));
    assert!(matches!(
        *layers.get_layer(LayerbankId::new(10)).unwrap(),
        UserLayer::Bustup(_)
    ));
    if let UserLayer::Bustup(layer) = &*layers.get_layer(LayerbankId::new(10)).unwrap() {
        let (mouths, eyes) = layer.reference_frame_counts();
        assert!(
            mouths <= 1 && eyes <= 1,
            "Native gallery uses static portrait frames"
        );
    }
    // Select an authored pair, then an absent secondary. Both slots must be
    // returned in one publication poll, with native ten-tick transitions.
    let paired = catalog
        .backgrounds
        .iter()
        .find(|b| b.secondary.is_some())
        .unwrap();
    for background in [paired, &catalog.backgrounds[0]] {
        resources.request(Request::LoadBackground {
            primary: background.primary,
            secondary: background.secondary,
        });
        assert!(resources.has_blocking_loads());
        let mut published = false;
        while resources.has_blocking_loads() {
            assert!(
                std::time::Instant::now() < deadline,
                "Background worker timeout"
            );
            let blocked = resources.has_blocking_loads();
            let events = resources.poll().unwrap();
            if !events.is_empty() {
                assert!(blocked);
                assert_eq!(events.len(), 2);
                for (slot, event) in events.into_iter().enumerate() {
                    let Publication::Layer(mut p) = event else {
                        panic!()
                    };
                    assert_eq!(p.slot, slot as u8);
                    assert_eq!(p.transition_ticks, Ticks::from_f32(10.0));
                    p.inherit_from(&*layers.get_layer(LayerbankId::new(p.slot)).unwrap());
                    assert_eq!(
                        p.layer.properties().get_value(LayerProperty::TranslateZ),
                        2000.0
                    );
                    if slot == 1 {
                        assert_eq!(
                            matches!(p.layer, UserLayer::Null(_)),
                            background.secondary.is_none()
                        );
                    }
                    p.install(&mut layers);
                }
                published = true;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(published);
    }
    let id = catalog.characters[0].outfits[0].poses[0].expressions[1];
    resources.request(Request::LoadBustup(id));
    assert!(resources.has_blocking_loads());
    let mut replaced = false;
    while resources.has_blocking_loads() {
        assert!(std::time::Instant::now() < deadline, "BUP worker timeout");
        for event in resources.poll().unwrap() {
            let Publication::Layer(mut p) = event else {
                panic!()
            };
            assert_eq!(p.slot, 10);
            assert_eq!(p.transition_ticks, Ticks::from_f32(10.0));
            p.inherit_from(&*layers.get_layer(LayerbankId::new(10)).unwrap());
            assert!(matches!(p.layer, UserLayer::Bustup(_)));
            assert_eq!(
                p.layer
                    .properties()
                    .get_value(LayerProperty::RenderPosition),
                1000.0
            );
            p.install(&mut layers);
            replaced = true;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(replaced);
    for slot in [0, 1, 10] {
        assert!(layers.has_wiper_for_layerbank(LayerbankId::new(slot)));
    }
    assert!(resources.poll().unwrap().is_empty());
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_resource_worker_properties_pair_barrier_and_replacement() {
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (a, expected) in [
        (0x3f244, 0x940005fb),
        (0x3f270, 0x9101e108),
        (0x3f338, 0x940005be),
        (0x3f444, 0x97ffef1f),
        (0x3f570, 0x528000a1),
        (0x3f578, 0x1e220260),
        (0x3f590, 0x321f03e1),
        (0x3f5ac, 0x321e07e1),
        (0x3f5c8, 0x528001a1),
        (0x3f630, 0x2a1303e1),
        (0x3f690, 0x71000a7f),
        (0x3f6a4, 0x97ffedb3),
        (0x3f7f4, 0x528000a1),
        (0x3f814, 0x321f03e1),
        (0x3f87c, 0x52800141),
        (0x3f890, 0x9400eb94),
        (0x3b0f4, 0x7100043f),
        (0x3b104, 0x34000b09),
        (0x3b108, 0x0b080128),
        (0x3b114, 0x12003d01),
        (0x3b118, 0x9402bc52),
        (0x3b264, 0xaa1303e8),
        (0x3b27c, 0x14011c39),
        (0x3adf4, 0x79c02008),
        (0x3e268, 0xd63f0100),
        (0x3e26c, 0x360002a0),
        (0x3e308, 0x52800141),
        (0x3e314, 0x9400f3f3),
        (0x3e360, 0xf9403509),
        (0x3e370, 0xf9403108),
        (0x3e380, 0xd63f0100),
        (0x3e454, 0x1e249000),
        (0x3e474, 0x9400f09b),
        (0x3e4f4, 0x2a1f03e0),
        (0x3ebf4, 0xf9401108),
        (0x3ebfc, 0x360017a0),
        (0x3ec18, 0x1e249008),
        (0x3ed20, 0x9400f170),
        (0x3ed68, 0xf9403509),
        (0x3ed78, 0xf9403108),
        (0x3ee48, 0x94008d76),
        (0x3ee60, 0x9400ee20),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
            expected,
            "Native Bustup resource{a:x}"
        );
    }
    assert_eq!(&exe[0x15e59e + 256..0x15e59e + 268], b"/default.msk");
    for (a, value) in [(0x159578, 1000.0f32), (0x159500, 2000.0)] {
        assert_eq!(
            f32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
            value
        );
    }
}
