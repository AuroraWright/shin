use super::*;
use crate::{adv::vm_state::layers::LayerOperationTarget, layer::PropertyPolicy};
use shin_core::vm::command::types::LayerId;

#[derive(Default)]
struct Services {
    owners: Vec<(PropertyTarget, LayerProperties)>,
    wipes: Vec<(PlaneId, LayerbankId)>,
    fail: bool,
}
impl Services {
    fn owner(&mut self, target: PropertyTarget) -> &mut LayerProperties {
        &mut self
            .owners
            .iter_mut()
            .find(|(t, _)| *t == target)
            .unwrap()
            .1
    }
    fn insert(&mut self, target: PropertyTarget) {
        self.owners.push((
            target,
            LayerProperties::with_policy(PropertyPolicy::Gerokasu2),
        ));
    }
}
impl LayerControlServices for Services {
    fn supports_root(&self) -> bool {
        true
    }
    fn visit_properties(
        &mut self,
        targets: &[PropertyTarget],
        action: &mut dyn FnMut(&mut LayerProperties),
    ) -> Result<()> {
        ensure!(!self.fail, "injected owner failure");
        for target in targets {
            ensure!(
                self.owners.iter().any(|(t, _)| t == target),
                "missing owner"
            );
        }
        for &target in targets {
            action(self.owner(target));
        }
        Ok(())
    }
    fn bank_wiping(&mut self, plane: PlaneId, bank: LayerbankId) -> Result<bool> {
        Ok(self.wipes.contains(&(plane, bank)))
    }
}
fn command(id: i32, value: i32, duration: i32, flags: i32) -> Cmd {
    Cmd::LayerCtrl {
        layer_id: id,
        property_id: LayerProperty::TranslateX.into_usize() as i32,
        params: (value, duration, flags, 0, 0, 0, 0, 0),
    }
}
#[test]
fn ger_message_layer_init_rejects_unsupported_bank_address_without_resetting_message() {
    let mut layers = Gerokasu2Layers::default();
    layers.load(3, 1, 0, [0; 8]).unwrap();
    let before = layers.reference_bytes();
    let mut services = Services::default();
    services.insert(PropertyTarget::Message);
    services
        .owner(PropertyTarget::Message)
        .property_tweener_mut(LayerProperty::TranslateX)
        .gerokasu2_control(91.0, Ticks::from_f32(20.0), 0, Easing::Linear, false);
    let props = services
        .owner(PropertyTarget::Message)
        .reference_copy_words()
        .unwrap();
    services.fail = true; // Any property visit would fail before this guard.
    assert!(initialize(&mut layers, -5, &mut services).is_err());
    assert_eq!(layers.reference_bytes(), before);
    assert_eq!(
        services
            .owner(PropertyTarget::Message)
            .reference_copy_words()
            .unwrap(),
        props
    );
    assert!(layers.apply(&Cmd::LayerInit { layer_id: -5 }).is_err());
    assert_eq!(layers.reference_bytes(), before);
}
fn apply(layers: &mut Gerokasu2Layers, services: &mut Services, command: Cmd) {
    let mut prepared = layers.clone();
    let plan = LayerControlPlan::prepare(&mut prepared, &command).unwrap();
    plan.apply(services).unwrap();
    *layers = prepared;
}

#[test]
fn gerokasu2_selected_controls_waits_capture_plane_targets_and_retry_once() {
    let mut layers = Gerokasu2Layers::default();
    layers.load(3, 1, 0, [0; 8]).unwrap();
    layers.load(5, 1, 0, [0; 8]).unwrap();
    let targets: Vec<LayerOperationTarget> = layers.targets(-6).unwrap().iter().copied().collect();
    assert!(targets.is_empty());
    layers.select(3, 5);
    let first = layers.target(LayerId::new(3)).unwrap().layerbank;
    let second = layers.target(LayerId::new(5)).unwrap().layerbank;
    let plane = PlaneId::new(0);
    let mut services = Services::default();
    for bank in [first, second] {
        services.insert(PropertyTarget::Bank { plane, bank });
    }
    let original = layers.record(first).property(LayerProperty::TranslateX);
    let mut prepared = layers.clone();
    let plan = LayerControlPlan::prepare(&mut prepared, &command(-6, 100, 10, 0)).unwrap();
    services.fail = true;
    assert!(plan.apply(&mut services).is_err());
    assert_eq!(
        layers.record(first).property(LayerProperty::TranslateX),
        original
    );
    services.fail = false;
    plan.apply(&mut services).unwrap();
    layers = prepared;
    apply(&mut layers, &mut services, command(-6, 50, 10, 128));
    let wait = LayerWait::capture(
        &mut layers,
        -6,
        &[LayerProperty::TranslateX.into_usize() as i32],
    )
    .unwrap();
    let transition = TransitionWait::capture(&mut layers, -6).unwrap();
    services.wipes.push((plane, first));
    layers.select_plane(1).unwrap();
    layers.select(9, 9);
    assert!(!wait.poll(&mut services, false).unwrap());
    assert!(!transition.poll(&mut services).unwrap());
    for (_, props) in &mut services.owners {
        props
            .property_tweener_mut(LayerProperty::TranslateX)
            .update(Ticks::from_f32(15.0));
        assert_eq!(props.get_value(LayerProperty::TranslateX), 125.0);
    }
    assert!(!wait.poll(&mut services, false).unwrap());
    assert!(wait.poll(&mut services, true).unwrap());
    services.wipes.clear();
    assert!(transition.poll(&mut services).unwrap());
}

#[test]
fn gerokasu2_container_controls_protected_and_ignored_waits_validate_before_mutation() {
    let mut layers = Gerokasu2Layers::default();
    let mut services = Services::default();
    for target in [
        PropertyTarget::Root,
        PropertyTarget::Screen,
        PropertyTarget::Page,
        PropertyTarget::Plane(PlaneId::new(0)),
    ] {
        services.insert(target);
    }
    for id in [-1, -2, -3, -4] {
        apply(&mut layers, &mut services, command(id, 100, 10, 4096));
        let wait = LayerWait::capture(
            &mut layers,
            id,
            &[LayerProperty::TranslateX.into_usize() as i32],
        )
        .unwrap();
        assert!(!wait.poll(&mut services, true).unwrap());
        apply(&mut layers, &mut services, command(id, 25, -1, 128 | 256));
        assert!(wait.poll(&mut services, false).unwrap());
        apply(&mut layers, &mut services, command(id, 100, 10, 0x10000));
        assert!(wait.poll(&mut services, false).unwrap());
        initialize(&mut layers, id, &mut services).unwrap();
    }
    for (id, p, flags) in [
        (-7, 9, 0),
        (256, 9, 0),
        (-2, 90, 0),
        (-2, -1, 0),
        (-2, 9, 6),
    ] {
        let mut c = command(id, 5, 10, flags);
        if let Cmd::LayerCtrl { property_id, .. } = &mut c {
            *property_id = p;
        }
        assert!(LayerControlPlan::prepare(&mut layers.clone(), &c).is_err());
    }
    assert!(LayerWait::capture(&mut layers, -2, &[90]).is_err());
}

#[test]
fn ger_message_controls_capture_outer_owner_wait_and_preserve_screen_targets() {
    let mut layers = Gerokasu2Layers::default();
    let screen = layers.special_property(
        super::super::layers::SpecialLayer::Screen,
        LayerProperty::TranslateX,
    );
    let mut services = Services::default();
    services.insert(PropertyTarget::Message);
    apply(&mut layers, &mut services, command(-5, 100, 10, 0));
    let wait = LayerWait::capture(
        &mut layers,
        -5,
        &[LayerProperty::TranslateX.into_usize() as i32],
    )
    .unwrap();
    assert!(!wait.poll(&mut services, false).unwrap());
    services
        .owner(PropertyTarget::Message)
        .property_tweener_mut(LayerProperty::TranslateX)
        .update(Ticks::from_f32(10.0));
    assert!(wait.poll(&mut services, false).unwrap());
    assert_eq!(
        services
            .owner(PropertyTarget::Message)
            .get_value(LayerProperty::TranslateX),
        100.0
    );
    assert_eq!(
        layers.special_property(
            super::super::layers::SpecialLayer::Screen,
            LayerProperty::TranslateX
        ),
        screen
    );
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "requires supplied executable/fonts;original LAYERCTRL/LAYERWAIT helpers;CPU only"]
fn gerokasu2_native_layer_control_and_wait_helpers() {
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/controls");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-layer-controls");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-layer-controls.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut layers = Gerokasu2Layers::default();
    let mut services = Services::default();
    services.insert(PropertyTarget::Root);
    let mut request = Vec::new();
    let mut expected = Vec::new();
    fn snapshot(services: &mut Services, done: bool, visits: &[u32]) -> Vec<u32> {
        let props = services.owner(PropertyTarget::Root);
        let mut words = vec![done as u32, visits.len() as u32];
        words.extend(visits);
        for id in 0..90 {
            let p = LayerProperty::from_usize(id);
            let value = props.get_value(p);
            let t = props.property_tweener(p);
            words.extend([
                value.to_bits(),
                t.target_value().to_bits(),
                (!t.is_idle()) as u32,
                t.is_waiting() as u32,
            ]);
        }
        words
    }
    fn emit(request: &mut Vec<u8>, words: impl IntoIterator<Item = u32>) {
        request.extend(words.into_iter().flat_map(u32::to_le_bytes));
    }
    for id in 0..90 {
        for flags in [
            0,
            128,
            64,
            192,
            256,
            384,
            512,
            640,
            768,
            896,
            4096,
            0x10000,
            0x11000,
            0xfffe0000u32 as i32,
        ] {
            *services.owner(PropertyTarget::Root) =
                LayerProperties::with_policy(PropertyPolicy::Gerokasu2);
            layers = Default::default();
            emit(&mut request, [0]);
            emit(
                &mut request,
                (0..90).map(|p| (LayerProperty::from_usize(p).initial_value() as f32).to_bits()),
            );
            expected.push(snapshot(&mut services, true, &[]));
            let ease = id % 6;
            let power = if id % 2 == 0 { 2 } else { -2 };
            for (value, duration, f) in [(100, 30, 0), (25, 8, 128), (-17, -1, 0), (41, 12, flags)]
            {
                let c = Cmd::LayerCtrl {
                    layer_id: -1,
                    property_id: id,
                    params: (value, duration, f | ease, power, 0, 0, 0, 0),
                };
                apply(&mut layers, &mut services, c);
                emit(
                    &mut request,
                    [
                        1,
                        id as u32,
                        value as u32,
                        duration as u32,
                        (f | ease) as u32,
                        power as u32,
                    ],
                );
                expected.push(snapshot(&mut services, true, &[id as u32]));
            }
            for (skip, delta) in [(false, 3.0), (true, 15.0), (true, 40.0)] {
                let list = [(id + 1) % 90, id];
                let mut visits = Vec::new();
                for &p in &list {
                    visits.push(p as u32);
                    let mut t = services
                        .owner(PropertyTarget::Root)
                        .property_tweener(LayerProperty::from_usize(p as usize));
                    if skip {
                        t.fast_forward_unless_prohibited();
                    }
                    if t.is_waiting() {
                        break;
                    }
                }
                let wait = LayerWait::capture(&mut layers, -1, &list).unwrap();
                let done = wait.poll(&mut services, skip).unwrap();
                emit(&mut request, [2, skip as u32, list.len() as u32]);
                emit(&mut request, list.map(|v| v as u32));
                expected.push(snapshot(&mut services, done, &visits));
                for p in 0..90 {
                    services
                        .owner(PropertyTarget::Root)
                        .property_tweener_mut(LayerProperty::from_usize(p))
                        .update(Ticks::from_f32(delta));
                }
                emit(&mut request, [3, delta.to_bits()]);
                expected.push(snapshot(&mut services, true, &[]));
            }
            let wait = LayerWait::capture(&mut layers, -1, &[]).unwrap();
            assert!(wait.poll(&mut services, true).unwrap());
            emit(&mut request, [2, 1, 0]);
            expected.push(snapshot(&mut services, true, &[]));
        }
    }
    let input = folder.join("request.bin");
    fs::write(&input, request).unwrap();
    let output = Command::new(&binary)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
        .stdin(Stdio::from(fs::File::open(input).unwrap()))
        .output()
        .unwrap();
    fs::write(folder.join("native.trace"), &output.stdout).unwrap();
    fs::write(folder.join("native.stderr"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut at = 0;
    let mut count = 0;
    let mut failures = Vec::new();
    for (row, words) in expected.iter().enumerate() {
        assert_eq!(&output.stdout[at..at + 4], b"GCP1");
        at += 4;
        for (field, &value) in words.iter().enumerate() {
            let native = u32::from_le_bytes(output.stdout[at..at + 4].try_into().unwrap());
            at += 4;
            let float = field >= 2 + words[1] as usize && (field - 2 - words[1] as usize) % 4 < 2;
            if native != value
                && !(float && f32::from_bits(native).is_nan() && f32::from_bits(value).is_nan())
            {
                failures.push(format!(
                    "row{row} field{field} native={native:08x} port={value:08x}"
                ));
            }
        }
        count += words.len();
    }
    assert_eq!(at, output.stdout.len());
    fs::write(
        folder.join("comparison.txt"),
        format!(
            "mismatches={}\n{}",
            failures.len(),
            failures
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        ),
    )
    .unwrap();
    assert!(
        failures.is_empty(),
        "{} mismatches: {:?}",
        failures.len(),
        failures.iter().take(6).collect::<Vec<_>>()
    );
    println!(
        "GEROKASU2_LAYER_CONTROLS_NATIVE snapshots={} words={} properties=90 helpers=LAYERCTRL_LayerWait",
        expected.len(),
        count
    );
}
