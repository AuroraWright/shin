use super::*;
use crate::layer::{LayerProperties, PropertyPolicy};
use anyhow::ensure;
use shin_core::vm::IntoRuntimeForm;
use shin_core::{
    format::scenario::instruction_elements::NumberSpec,
    vm::{
        VmCtx,
        command::{CompiletimeCommand, compiletime, types::LayerbankId},
    },
};

fn swap(a: i32, b: i32) -> Cmd {
    Cmd::Shared(
        CompiletimeCommand::LAYERSWAP(compiletime::LAYERSWAP {
            arg1: NumberSpec::constant(a),
            arg2: NumberSpec::constant(b),
        })
        .into_runtime_form(&VmCtx::new(0, 42)),
    )
}
fn clear() -> Cmd {
    Cmd::Shared(
        CompiletimeCommand::PLANECLEAR(compiletime::PLANECLEAR {})
            .into_runtime_form(&VmCtx::new(0, 42)),
    )
}
#[derive(Default)]
struct Services {
    entries: Vec<(i32, f32, usize)>,
    properties: Vec<LayerProperties>,
    plane: u8,
    copies: u32,
    fail: bool,
    mask: bool,
    flags: u32,
}
impl Services {
    fn new(plane: u8, n: usize, flags: u32) -> Self {
        Self {
            entries: (0..n).map(|i| (i as i32, 0.0, i)).collect(),
            properties: (0..n)
                .map(|i| {
                    let mut p = LayerProperties::with_policy(PropertyPolicy::Gerokasu2);
                    p.set_layer_id(LayerId::new(i as u16));
                    p
                })
                .collect(),
            plane,
            mask: true,
            flags,
            ..Default::default()
        }
    }
    fn words(&self, visits: u32) -> Vec<u32> {
        let mut result = vec![
            1,
            self.copies,
            visits,
            self.entries.len() as u32,
            self.mask as u32,
            self.flags,
            0,
        ];
        for &(bank, time, owner) in &self.entries {
            result.extend([
                bank as u32,
                if bank < 0 { time.to_bits() } else { 0 },
                owner as u32,
                self.properties[owner].get_layer_id().raw() as u32,
                0,
            ]);
        }
        result
    }
}
impl LayerEditServices for Services {
    fn edit_layers(&mut self, edit: &LayerEdit) -> Result<()> {
        ensure!(!self.fail, "injected edit failure");
        ensure!(edit.plane().raw() == self.plane, "captured plane");
        if let LayerEdit::Swap { targets, .. } = edit {
            for t in targets {
                ensure!(
                    self.entries
                        .iter()
                        .any(|e| e.0 == i32::from(t.layerbank.raw())),
                    "missing bank"
                );
            }
        }
        self.copies = 1;
        match edit {
            LayerEdit::Unload { targets, delay, .. } => {
                for t in targets {
                    if let Some(i) = self
                        .entries
                        .iter()
                        .position(|e| e.0 == i32::from(t.layerbank.raw()))
                    {
                        let mut entry = self.entries.remove(i);
                        if *delay != 0 {
                            entry.0 = -1;
                            entry.1 = *delay as f32;
                            self.entries.insert(0, entry);
                        }
                    }
                }
            }
            LayerEdit::Swap { targets, .. } => {
                for t in targets {
                    let i = self
                        .entries
                        .iter()
                        .find(|e| e.0 == i32::from(t.layerbank.raw()))
                        .unwrap()
                        .2;
                    self.properties[i].set_layer_id(t.layer);
                }
            }
            LayerEdit::Clear { .. } => {
                self.entries.clear();
                self.mask = false;
            }
        }
        Ok(())
    }
}
fn initial(plane: u8, n: usize) -> Gerokasu2Layers {
    let mut l = Gerokasu2Layers::default();
    l.select_plane(i32::from(plane)).unwrap();
    for i in 0..n {
        l.load(i as i32, 1, 0, [0; 8]).unwrap();
    }
    l
}
fn row(edit: &LayerEdit) -> Vec<u32> {
    let p = u32::from(edit.plane().raw());
    match edit {
        LayerEdit::Unload { targets, delay, .. } => {
            let mut out = vec![1, p, *delay as u32, targets.len() as u32, 0, 0, 0, 0];
            for t in targets {
                out.extend([u32::from(t.layer.raw()), u32::from(t.layerbank.raw())]);
            }
            out
        }
        LayerEdit::Swap { targets, .. } => {
            let mut out = vec![2, p, 0, 0, 48, 48, 0, 0];
            for (i, t) in targets.iter().enumerate() {
                out[2 + i] = u32::from(t.layer.raw());
                out[4 + i] = u32::from(t.layerbank.raw());
            }
            out
        }
        LayerEdit::Clear { .. } => vec![3, p, 0, 0, 0, 0, 0, 0],
    }
}
#[test]
fn ger_layer_edits_capture_addresses_and_fail_before_committing_integer_state() {
    let layers = initial(2, 2);
    let mut prepared = layers.clone();
    let edit = LayerEdit::prepare(&mut prepared, &swap(0, 1)).unwrap();
    let mut services = Services::new(2, 2, 17);
    services.fail = true;
    assert!(services.edit_layers(&edit).is_err());
    assert_eq!(
        layers.target(LayerId::new(0)).unwrap().layerbank,
        LayerbankId::new(0)
    );
    assert_eq!(services.copies, 0);
    prepared.select_plane(3).unwrap();
    services.fail = false;
    services.edit_layers(&edit).unwrap();
    assert_eq!(services.properties[0].get_layer_id(), LayerId::new(1));
    assert_eq!(services.properties[1].get_layer_id(), LayerId::new(0));
    services.entries.pop();
    assert!(services.edit_layers(&edit).is_err());
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original unload/swap/plane-clear bodies with prepared owners;CPU only"]
fn ger_native_layer_edit_callbacks_and_retained_vectors() {
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("gerokasu2/.local/edits");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-layer-edits");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-layer-edits.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut request = Vec::new();
    let mut expected = Vec::new();
    fn append(
        request: &mut Vec<u8>,
        expected: &mut Vec<Vec<u32>>,
        row: &[u32],
        state: &Services,
        visits: u32,
    ) {
        request.extend(row.iter().flat_map(|w| w.to_le_bytes()));
        expected.push(state.words(visits));
    }
    for plane in 0..4u8 {
        for n in [0, 1, 2, 8, 48] {
            for delay in [0, 1, -1, 3, 600, 16777217, i32::MIN, i32::MAX] {
                for id in [-6, -4, -1, 0, 1, 7, 47, 255] {
                    let mut layers = initial(plane, n);
                    layers.select(0, 47);
                    let flags = 0x80000000 | u32::from(plane);
                    let mut state = Services::new(plane, n, flags);
                    append(
                        &mut request,
                        &mut expected,
                        &[0, u32::from(plane), n as u32, flags, 1, 0, 0, 0],
                        &state,
                        0,
                    );
                    let edit = LayerEdit::prepare(
                        &mut layers,
                        &Cmd::LayerUnload {
                            layer_id: id,
                            delay_time: delay,
                        },
                    )
                    .unwrap();
                    state.edit_layers(&edit).unwrap();
                    append(&mut request, &mut expected, &row(&edit), &state, 1);
                    // Repeating the already prepared request exercises lookup after
                    // delayed entries lose their bank IDs, and one capture per ADV.
                    state.edit_layers(&edit).unwrap();
                    append(&mut request, &mut expected, &row(&edit), &state, 1);
                    let edit = LayerEdit::prepare(&mut layers, &clear()).unwrap();
                    state.edit_layers(&edit).unwrap();
                    append(&mut request, &mut expected, &row(&edit), &state, 1);
                }
            }
            for (a, b) in [
                (0, 1),
                (1, 47),
                (0, 255),
                (255, 0),
                (0, 0),
                (-1, 0),
                (0, 256),
                (255, 255),
            ] {
                let mut layers = initial(plane, n);
                let mut state = Services::new(plane, n, 0xffff);
                append(
                    &mut request,
                    &mut expected,
                    &[0, u32::from(plane), n as u32, 0xffff, 1, 0, 0, 0],
                    &state,
                    0,
                );
                let edit = LayerEdit::prepare(&mut layers, &swap(a, b)).unwrap();
                state.edit_layers(&edit).unwrap();
                append(&mut request, &mut expected, &row(&edit), &state, 1);
            }
        }
    }
    let input = folder.join("request.bin");
    fs::write(&input, request).unwrap();
    let output = Command::new(binary)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("gerokasu2/.local/newrodin-medium.fnt"))
        .arg(root.join("gerokasu2/.local/system.fnt"))
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
    let mut offset = 0;
    let mut words = 0;
    for (i, expected) in expected.iter().enumerate() {
        assert_eq!(&output.stdout[offset..offset + 4], b"GED1");
        offset += 4;
        let actual = output.stdout[offset..offset + expected.len() * 4]
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(&actual, expected, "layer edit snapshot{i}");
        offset += expected.len() * 4;
        words += expected.len();
    }
    assert_eq!(offset, output.stdout.len());
    println!(
        "GEROKASU2_LAYER_EDITS_NATIVE snapshots={} words={} complete_callbacks_vectors_lookup_mask_raw_delay_capture=verified",
        expected.len(),
        words
    );
}
