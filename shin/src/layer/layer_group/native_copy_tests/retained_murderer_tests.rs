//! Real native Top construction and last-owner destruction, compared with
//! production retained Murderer models, properties and capture-holder lifetime.
use super::*;
use crate::{
    asset::system::{AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache},
    layer::user::murderer_accusation::{MurdererAccusationView, PreparedMurdererLayer},
    wiper::DefaultWiper,
};
use shin_core::time::Ticks;
use shin_render::resize::{SurfaceResizeSource, ViewportParams};

fn tile(index: usize) -> UserLayer {
    let i = index as f32;
    TileLayer::new(
        FloatColor4::from_vec4(glam::vec4(0.125 * i, 0.25, 0.5, 1.0)),
        glam::vec4(-32.0 * i, 16.0 * i, 96.0, 48.0),
    )
    .into()
}

fn snapshot(
    fixture: usize,
    phase: &str,
    roots: &[Option<LayerGroup>; 2],
    probes: &[Box<dyn Fn() -> bool>],
) -> String {
    let mut words = vec!["A".to_owned(), fixture.to_string(), phase.to_owned()];
    let mut leaves = [Vec::new(), Vec::new()];
    for (g, root) in roots.iter().enumerate() {
        words.push(root.as_ref().map_or(0, |r| r.layers.len()).to_string());
        if let Some(root) = root {
            for entry in &root.layers {
                let bank = entry.layerbank_id.map_or(-1, |b| b.raw() as i32);
                let kind = if entry.layer.wiper.is_some() {
                    2
                } else {
                    usize::from(matches!(entry.layer.target, UserLayer::Murderer(_)))
                };
                words.extend([
                    bank.to_string(),
                    format!(
                        "{:08x}",
                        if bank < 0 {
                            entry.removal_delay.as_f32().to_bits()
                        } else {
                            0
                        }
                    ),
                    kind.to_string(),
                ]);
            }
            leaves[g] = root.reference_owner_leaves();
        }
    }
    words.extend(leaves.iter().map(|v| v.len().to_string()));
    let all: Vec<_> = leaves.iter().flatten().copied().collect();
    for leaf in &all {
        let props = leaf.properties().reference_copy_words();
        words.extend([
            usize::from(matches!(leaf, UserLayer::Murderer(_))).to_string(),
            usize::from(props.is_some()).to_string(),
            format!("{:08x}", props.map_or(0, |v| v[0])),
        ]);
    }
    for a in &all {
        for b in &all {
            let id = a.properties().initialized_identity();
            words.push(
                usize::from(id.is_some() && id == b.properties().initialized_identity())
                    .to_string(),
            );
        }
    }
    for a in &all {
        for b in &all {
            let same = match (a, b) {
                (UserLayer::Murderer(a), UserLayer::Murderer(b)) => {
                    let id = a.reference_retained_render_identity().unwrap();
                    Some(id) == b.reference_retained_render_identity()
                }
                _ => std::ptr::eq(*a, *b),
            };
            words.push(usize::from(same).to_string());
        }
    }
    words.push(probes.iter().filter(|probe| probe()).count().to_string());
    words.join(" ")
}

fn murderer(
    prepared: &PreparedMurdererLayer,
    device: &wgpu::Device,
    resize: &SurfaceResizeSource,
    extra: bool,
    child_kind: usize,
    probes: &mut Vec<Box<dyn Fn() -> bool>>,
) -> UserLayer {
    let layer = prepared.construct(false);
    if child_kind != 0 {
        layer.reference_open_menu(child_kind - 1);
    }
    probes.push(layer.reference_retirement_probe(device, resize, extra));
    layer.into()
}

#[test]
#[ignore = "requires Metal and supplied ROM;real retained Murderer final ownership;no audio/window/saves"]
fn reference_native_retained_murderer_copy_and_retirement() {
    crate::init_test_task_pools();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = std::env::var_os("SHIN_RETAINED_MURDERER_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("shin-native-retained-murderer"));
    fs::create_dir_all(&folder).unwrap();
    let oracle = folder.join("oracle");
    let oracle_source = std::env::var_os("SHIN_RETAINED_MURDERER_ORACLE_SOURCE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("reverse-engineering/native-retained-murderer-ownership.c"));
    let compile = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
        .arg(oracle_source)
        .arg("-o")
        .arg(&oracle)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    let mut io = LayeredAssetIo::new();
    // This task concerns CPU ownership, not changed ROM artwork. Prepare once;
    // reuse the same real resources for every independent model construction.
    io.try_with_rom(root.join("umineko_switch_files/data.rom"))
        .unwrap();
    let server = AssetServer::new(
        io.into(),
        AssetLoadContext {
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    );
    let prepared =
        [5, 7].map(|hint| shin_tasks::block_on(MurdererAccusationView::prepare(&server, hint)));
    let resize =
        SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(16, 16)));
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut clone_ctx = RenderCloneCtx {
        device: &device,
        encoder: &mut encoder,
    };
    let bank = LayerbankId::new(1);
    let child_kind = std::env::var("SHIN_RETAINED_MURDERER_CHILD")
        .map(|v| v.parse::<usize>().unwrap())
        .unwrap_or(0);
    assert!(child_kind < 7);
    let mut request = String::new();
    let mut expected = Vec::new();
    let mut fixture = 0;
    for scenario in 0..3 {
        for sharing in 0..if scenario == 0 { 1 } else { 2 } {
            for extra in [false, true] {
                for operation in 0..3 {
                    for order in 0..2 {
                        request += &format!(
                            "{scenario} {sharing} {} {operation} {order}\n",
                            usize::from(extra)
                        );
                        let mut roots = [Some(LayerGroup::new(None)), None];
                        let mut probes = Vec::new();
                        let group = roots[0].as_mut().unwrap();
                        group.add_layer(
                            bank,
                            murderer(
                                &prepared[0],
                                &device,
                                &resize,
                                extra,
                                child_kind,
                                &mut probes,
                            ),
                        );
                        if scenario != 0 {
                            let mut next = if scenario == 1 {
                                tile(2)
                            } else {
                                murderer(
                                    &prepared[1],
                                    &device,
                                    &resize,
                                    extra,
                                    child_kind,
                                    &mut probes,
                                )
                            };
                            if sharing == 1 {
                                next.set_properties(
                                    group.get_layer(bank).unwrap().properties().share(),
                                );
                            }
                            group.add_layer_with_wiper(
                                bank,
                                next,
                                Some(DefaultWiper::new(Ticks::from_f32(30.0)).into()),
                            );
                        }
                        assert!(group.remove_layer(bank, Ticks::from_f32(3.0)).is_none());
                        group.add_layer(bank, tile(4));
                        expected.push(snapshot(fixture, "retained", &roots, &probes));
                        roots[1] = Some(roots[0].as_ref().unwrap().render_clone(&mut clone_ctx));
                        expected.push(snapshot(fixture, "copied", &roots, &probes));
                        roots[1].as_ref().unwrap().reference_owner_leaves()[0]
                            .properties()
                            .share()
                            .property_tweener_mut(LayerProperty::TranslateX)
                            .fast_forward_to(999.0);
                        expected.push(snapshot(fixture, "mutated", &roots, &probes));
                        if operation != 0 {
                            roots[operation - 1].as_mut().unwrap().fast_forward();
                            expected.push(snapshot(
                                fixture,
                                if operation == 1 {
                                    "forward-original"
                                } else {
                                    "forward-copy"
                                },
                                &roots,
                                &probes,
                            ));
                        }
                        for g in 0..2 {
                            let at = if order == 1 { 1 - g } else { g };
                            roots[at] = None;
                            expected.push(snapshot(
                                fixture,
                                if at == 0 {
                                    "drop-original"
                                } else {
                                    "drop-copy"
                                },
                                &roots,
                                &probes,
                            ));
                        }
                        assert!(probes.iter().all(|p| !p()));
                        expected.push(snapshot(fixture, "final", &roots, &probes));
                        fixture += 1;
                    }
                }
            }
        }
    }
    let request_file = folder.join("request.txt");
    fs::write(&request_file, request).unwrap();
    fs::write(folder.join("port.trace"), expected.join("\n") + "\n").unwrap();
    let mut mismatches = Vec::new();
    for (version, name) in [
        ("original", "uncompressed_executable"),
        ("1.03", "uncompressed_executable_update"),
    ] {
        let run = Command::new(&oracle)
            .arg(root.join("umineko_switch_files").join(name))
            .stdin(Stdio::from(fs::File::open(&request_file).unwrap()))
            .output()
            .unwrap();
        fs::write(folder.join(format!("{version}.trace")), &run.stdout).unwrap();
        fs::write(folder.join(format!("{version}.stderr")), &run.stderr).unwrap();
        assert!(
            run.status.success(),
            "{version}:{}",
            String::from_utf8_lossy(&run.stderr)
        );
        let trace = String::from_utf8(run.stdout).unwrap();
        let rows: Vec<_> = trace.lines().filter(|l| l.starts_with("A ")).collect();
        assert_eq!(rows.len(), expected.len());
        for (row, (native, port)) in rows.iter().zip(&expected).enumerate() {
            if native != port {
                mismatches.push(format!("{version} row{row}\nnative:{native}\nport:{port}"));
            }
        }
    }
    fs::write(
        folder.join("comparison.txt"),
        format!(
            "profiles={fixture}\nrows={}\nmismatches={}\n{}\n",
            expected.len() * 2,
            mismatches.len(),
            mismatches
                .iter()
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        ),
    )
    .unwrap();
    assert!(
        mismatches.is_empty(),
        "{} mismatches: {:?}",
        mismatches.len(),
        mismatches.iter().take(4).collect::<Vec<_>>()
    );
}
