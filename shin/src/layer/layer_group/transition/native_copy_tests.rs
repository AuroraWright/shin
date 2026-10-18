use super::*;
use crate::{layer::user::TileLayer, wiper::DefaultWiper};
use shin_render::shaders::types::{RenderClone, RenderCloneCtx};
use std::{fs, process::Command};

fn collect<'a>(slot: &'a SlotLayer<TileLayer>, nodes: &mut Vec<ReferenceLayerLeaf<'a, TileLayer>>) {
    slot.reference_owner_leaves(nodes);
}

#[test]
#[ignore = "requires Metal for production RenderCloneCtx; executes guarded original/1.03 nested slot copies; no audio"]
fn reference_native_nested_slot_property_aliases() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = std::env::temp_dir().join("shin-native-slot-copy-results");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("oracle");
    let compiled = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
        .arg(root.join("reverse-engineering/native-slot-copy.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, _queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut context = RenderCloneCtx {
        device: &device,
        encoder: &mut encoder,
    };
    let mut expected = Vec::new();
    for mask in 0..8 {
        for (partition, indices) in [[0, 0, 0], [0, 0, 1], [0, 1, 0], [0, 1, 1], [0, 1, 2]]
            .into_iter()
            .enumerate()
        {
            let props: [_; 3] = std::array::from_fn(|i| {
                let mut props = LayerProperties::new();
                props
                    .property_tweener_mut(LayerProperty::TranslateX)
                    .fast_forward_to((i + 1) as f32 * 10.0);
                props
            });
            let mut children = Vec::new();
            for i in 0..3 {
                // Exercise the actual wrapper's two ownership modes; artwork
                // is irrelevant here and no textures are allocated/copied.
                let mut child = TileLayer::reference_with_retained_owner(mask & (1 << i) != 0);
                child.set_properties(props[indices[i]].share());
                children.push(child);
            }
            let mut iter = children.into_iter();
            let mut original = SlotLayer::new(iter.next().unwrap(), None, None);
            for target in iter {
                original = SlotLayer::new(
                    target,
                    Some(original),
                    Some(AnyWiper::Default(DefaultWiper::new(Ticks::from_f32(30.0)))),
                );
            }
            let mut copied = original.render_clone(&mut context);
            let mut nodes = Vec::new();
            collect(&original, &mut nodes);
            collect(&copied, &mut nodes);
            let mut words = vec!["G".into(), mask.to_string(), partition.to_string()];
            for a in &nodes {
                for b in &nodes {
                    words.push(
                        u32::from(a.properties().identity() == b.properties().identity())
                            .to_string(),
                    );
                }
            }
            for a in &nodes {
                for b in &nodes {
                    let identity = |n: &TileLayer| {
                        n.reference_retained_render_identity()
                            .unwrap_or(n as *const _ as usize)
                    };
                    words.push(u32::from(identity(a) == identity(b)).to_string());
                }
            }
            drop(nodes);
            copied
                .source
                .as_mut()
                .unwrap()
                .write()
                .target
                .properties_mut()
                .property_tweener_mut(LayerProperty::TranslateX)
                .fast_forward_to(999.0);
            let mut nodes = Vec::new();
            collect(&original, &mut nodes);
            collect(&copied, &mut nodes);
            for node in nodes {
                words.push(format!(
                    "{:08x}",
                    node.properties()
                        .get_value(LayerProperty::TranslateX)
                        .to_bits()
                ));
            }
            words.push(u32::from(copied.is_transition_active()).to_string());
            words.push(
                u32::from(
                    copied
                        .source
                        .as_ref()
                        .unwrap()
                        .read()
                        .is_transition_active(),
                )
                .to_string(),
            );
            expected.push(words.join(" "));
        }
    }
    fs::write(folder.join("port.trace"), expected.join("\n") + "\n").unwrap();
    let mut mismatches = Vec::new();
    for (version, name) in [
        ("original", "uncompressed_executable"),
        ("1.03", "uncompressed_executable_update"),
    ] {
        let output = Command::new(&binary)
            .arg(root.join("game-data/umineko").join(name))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{version}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::write(folder.join(format!("{version}.trace")), &output.stdout).unwrap();
        let trace = String::from_utf8(output.stdout).unwrap();
        let rows: Vec<_> = trace.lines().collect();
        assert_eq!(rows.len(), expected.len());
        for (row, (native, port)) in rows.into_iter().zip(&expected).enumerate() {
            let native: Vec<_> = native.split_whitespace().collect();
            let port: Vec<_> = port.split_whitespace().collect();
            assert_eq!(native.len(), 83);
            assert_eq!(port.len(), 83);
            let fields: Vec<_> = native
                .iter()
                .zip(&port)
                .enumerate()
                .filter_map(|(i, (a, b))| (a != b).then_some(i))
                .collect();
            if !fields.is_empty() {
                mismatches.push(format!(
                    "{version} row{row} mask{} partition{}: fields{fields:?}",
                    native[1], native[2]
                ));
            }
        }
    }
    fs::write(
        folder.join("comparison.txt"),
        format!(
            "mismatched_graphs={}\n{}\n",
            mismatches.len(),
            mismatches.join("\n")
        ),
    )
    .unwrap();
    assert!(
        mismatches.is_empty(),
        "{} mismatched graphs: {:?}",
        mismatches.len(),
        &mismatches[..mismatches.len().min(8)]
    );
}
