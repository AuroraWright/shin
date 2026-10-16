use super::transitions::{Frames, advance, assets, new_backend};
use super::*;
use crate::adv::gerokasu2::{
    layer_edit::{LayerEdit, LayerEditServices},
    play_scene::Gerokasu2PlayScene,
    runner::FramePolicy,
};
use shin_core::vm::IntoRuntimeForm;
use shin_core::{
    primitives::update::FrameId,
    vm::{
        VmCtx,
        command::{CompiletimeCommand, compiletime},
    },
};
use std::{
    io::Write,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};
fn swap(a: i32, b: i32) -> Gerokasu2RuntimeCommand {
    Gerokasu2RuntimeCommand::Shared(
        CompiletimeCommand::LAYERSWAP(compiletime::LAYERSWAP {
            arg1: NumberSpec::constant(a),
            arg2: NumberSpec::constant(b),
        })
        .into_runtime_form(&VmCtx::new(0, 42)),
    )
}
fn clear() -> Gerokasu2RuntimeCommand {
    Gerokasu2RuntimeCommand::Shared(
        CompiletimeCommand::PLANECLEAR(compiletime::PLANECLEAR {})
            .into_runtime_form(&VmCtx::new(0, 42)),
    )
}
fn edit(backend: &mut BasicLoadBackend, layers: &mut Gerokasu2Layers, c: Gerokasu2RuntimeCommand) {
    let mut prepared = layers.clone();
    let edit = LayerEdit::prepare(&mut prepared, &c).unwrap();
    backend.edit_layers(&edit).unwrap();
    *layers = prepared;
}
fn tile_cmd(id: i32, color: i32) -> Gerokasu2RuntimeCommand {
    Gerokasu2RuntimeCommand::LayerLoad {
        layer_id: id,
        layer_type: 1,
        flags: 0,
        params: (color, -960, -540, 1920, 1080, 0, 0, 0),
    }
}
struct Native {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    snapshots: usize,
    words: usize,
}
impl Native {
    fn new() -> Self {
        let mut child = Command::new(std::env::var_os("SHIN_GEROKASU2_LAYER_EDIT_ORACLE").unwrap())
            .args([
                std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").unwrap(),
                std::env::var_os("SHIN_GEROKASU2_MEDIUM_FONT").unwrap(),
                std::env::var_os("SHIN_GEROKASU2_SYSTEM_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: child.stdout.take().unwrap(),
            child,
            snapshots: 0,
            words: 0,
        }
    }
    fn compare(
        &mut self,
        row: &[u32],
        backend: &BasicLoadBackend,
        plane: PlaneId,
        visits: u32,
        children: u32,
    ) {
        self.input
            .as_mut()
            .unwrap()
            .write_all(&row.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<_>>())
            .unwrap();
        let group = backend.screen().page_layer().get_plane(plane);
        let entries = group.reference_owner_entry_words();
        let leaves = group.reference_owner_leaves();
        assert_eq!(entries.len(), leaves.len());
        let (mask, flags) = group.reference_mask_state();
        let mut expected = vec![
            1,
            backend.has_previous_root() as u32,
            visits,
            entries.len() as u32,
            mask.is_some() as u32,
            flags.bits() as u32,
            children,
        ];
        for ((bank, time, wiping), leaf) in entries.iter().zip(leaves.iter()) {
            expected.extend([
                *bank as u32,
                *time,
                leaf.properties().get_value(LayerProperty::TranslateY) as u32,
                leaf.properties().get_layer_id().raw() as u32,
                *wiping as u32,
            ]);
        }
        let mut actual = vec![0; 4 + expected.len() * 4];
        self.output.read_exact(&mut actual).unwrap();
        assert_eq!(&actual[..4], b"GED1");
        let words = actual[4..]
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(
            words, expected,
            "real layer edit snapshot{}",
            self.snapshots
        );
        self.snapshots += 1;
        self.words += expected.len();
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        self.input.take();
        let status = self.child.wait().unwrap();
        if !std::thread::panicking() {
            assert!(status.success());
        }
    }
}

#[test]
#[ignore = "requires supplied ROM/native edit oracle,Metal and exact Loopback Audio"]
fn ger_layer_edits_real_groups_match_native_swap_unload_countdowns_clear_and_fast_forward() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let scenario = Arc::new(Gerokasu2Scenario::new(bytes().into()).unwrap());
    let mut native = Native::new();
    let mut frames = Frames::new(&gpu);
    for p in 0..4u8 {
        for delay in [0, 3, -3, i32::MAX] {
            let plane = PlaneId::new(p);
            let mut backend = new_backend(&assets, &scenario);
            let mut layers = Gerokasu2Layers::default();
            layers.select_plane(i32::from(p)).unwrap();
            for (id, color) in [(0, 0xff00), (1, 0xf00f)] {
                complete(
                    &mut load(&mut backend, &mut layers, tile_cmd(id, color)),
                    &mut backend,
                );
                let bank = layers.target(LayerId::new(id as u16)).unwrap().layerbank;
                backend
                    .layer_mut(plane, bank)
                    .unwrap()
                    .properties_mut()
                    .property_tweener_mut(LayerProperty::TranslateY)
                    .fast_forward_to(id as f32);
            }
            backend.previous_root = super::super::PreviousRoot::default();
            native.compare(&[0, u32::from(p), 2, 0, 0, 0, 0, 0], &backend, plane, 0, 0);
            edit(&mut backend, &mut layers, swap(0, 1));
            native.compare(&[2, u32::from(p), 0, 1, 1, 0, 0, 0], &backend, plane, 1, 0);
            assert_eq!(
                frames.draw(&gpu, &mut backend, plane)
                    [(90 * 320 + 160) * 4..(90 * 320 + 160) * 4 + 4],
                [255, 0, 0, 255]
            );
            edit(
                &mut backend,
                &mut layers,
                Gerokasu2RuntimeCommand::LayerUnload {
                    layer_id: 0,
                    delay_time: delay,
                },
            );
            assert!(layers.target(LayerId::new(0)).is_none());
            native.compare(
                &[1, u32::from(p), delay as u32, 1, 0, 0, 0, 0, 0, 1],
                &backend,
                plane,
                1,
                0,
            );
            let mut frame = FrameId::default();
            for (delta, allowed) in [(0.0, false), (1.0, false), (2.0, true)] {
                let children = backend
                    .screen()
                    .page_layer()
                    .get_plane(plane)
                    .reference_owner_entry_words()
                    .len() as u32;
                frame.advance();
                advance(&mut backend, &gpu, &assets, frame, delta, allowed);
                native.compare(
                    &[4, delta.to_bits(), allowed as u32, 0, 0, 0, 0, 0],
                    &backend,
                    plane,
                    0,
                    children,
                );
            }
            let children = backend
                .screen()
                .page_layer()
                .get_plane(plane)
                .reference_owner_entry_words()
                .len() as u32;
            backend.fast_forward();
            native.compare(&[5, 0, 0, 0, 0, 0, 0, 0], &backend, plane, 0, children);
            edit(&mut backend, &mut layers, clear());
            native.compare(&[3, u32::from(p), 0, 0, 0, 0, 0, 0], &backend, plane, 1, 0);
            assert!(
                frames
                    .draw(&gpu, &mut backend, plane)
                    .chunks_exact(4)
                    .all(|p| p == [0, 0, 0, 255])
            );
        }
    }
    // A delayed wrapper keeps its active wipe after the timer expires. Reusing
    // the freed bank installs a separate slot and retirement preserves it.
    let mut backend = new_backend(&assets, &scenario);
    let mut layers = Gerokasu2Layers::default();
    complete(
        &mut load(
            &mut backend,
            &mut layers,
            command(1, 0, [0xff00, -960, -540, 1920, 1080, 0, 0, 0]),
        ),
        &mut backend,
    );
    let bank = layers.target(LayerId::new(3)).unwrap().layerbank;
    backend.set_transition(LoadTransition {
        duration: 30,
        flags: 2,
        ..Default::default()
    });
    complete(
        &mut load(
            &mut backend,
            &mut layers,
            command(1, 4, [0xf00f, -960, -540, 1920, 1080, 0, 0, 0]),
        ),
        &mut backend,
    );
    assert!(
        backend
            .screen()
            .page_layer()
            .get_plane(PlaneId::new(0))
            .has_wiper_for_layerbank(bank)
    );
    edit(
        &mut backend,
        &mut layers,
        Gerokasu2RuntimeCommand::LayerUnload {
            layer_id: 3,
            delay_time: 3,
        },
    );
    let mut frame = FrameId::default();
    frame.advance();
    advance(&mut backend, &gpu, &assets, frame, 3.0, false);
    assert_eq!(
        backend
            .screen()
            .page_layer()
            .get_plane(PlaneId::new(0))
            .reference_retained_layer_count(),
        1
    );
    frames.draw(&gpu, &mut backend, PlaneId::new(0));
    complete(
        &mut load(&mut backend, &mut layers, tile_cmd(4, 0xf0f0)),
        &mut backend,
    );
    assert_eq!(layers.target(LayerId::new(4)).unwrap().layerbank, bank);
    frame.advance();
    advance(&mut backend, &gpu, &assets, frame, 27.0, true);
    assert_eq!(
        backend
            .screen()
            .page_layer()
            .get_plane(PlaneId::new(0))
            .reference_retained_layer_count(),
        1
    );
    frame.advance();
    advance(&mut backend, &gpu, &assets, frame, 3.0, true);
    assert_eq!(
        backend
            .screen()
            .page_layer()
            .get_plane(PlaneId::new(0))
            .reference_retained_layer_count(),
        0
    );
    assert!(backend.layer(PlaneId::new(0), bank).is_some());
    let mask: Arc<crate::asset::mask::MaskTexture> = shin_tasks::block_on(assets.load(&format!(
        "/mask/{}.msk",
        scenario.mask_info[0].name.as_str()
    )))
    .unwrap();
    let flags = shin_core::vm::command::types::MaskFlags::from_bits_retain(0x80000001u32 as i32);
    backend
        .screen_mut()
        .page_layer_mut()
        .get_plane_mut(PlaneId::new(0))
        .set_mask_texture(mask, flags);
    layers.set_mask(0, flags.bits());
    edit(&mut backend, &mut layers, clear());
    let (mask, retained_flags) = backend
        .screen()
        .page_layer()
        .get_plane(PlaneId::new(0))
        .reference_mask_state();
    assert!(mask.is_none());
    assert_eq!(retained_flags, flags);
    frames.draw(&gpu, &mut backend, PlaneId::new(0));
    println!(
        "GEROKASU2_LAYER_EDITS_GPU snapshots={} words={} real_shared_groups_native_vectors_delays_swap_ids_clear_forward=verified frames={} channels={} audio=Loopback Audio",
        native.snapshots, native.words, frames.frames, frames.channels
    );
}
fn context() -> MessageCommandContext {
    MessageCommandContext {
        was_read: false,
        event_mode: -1,
        skip: false,
        message_speed: 50,
    }
}
fn policy() -> FramePolicy {
    FramePolicy {
        paused: false,
        scene_changing: false,
        animations_allowed: true,
        autoplay: false,
        fast_skip: false,
    }
}
fn num(data: &mut Vec<u8>, v: i32) {
    let mut out = Cursor::new(Vec::new());
    NumberSpec::<i32>::constant(v).write_le(&mut out).unwrap();
    data.extend(out.into_inner());
}
fn op(data: &mut Vec<u8>, code: u8, args: &[i32]) {
    data.push(code);
    for &v in args {
        num(data, v);
    }
}
fn tile(data: &mut Vec<u8>, id: i32, color: i32) {
    op(data, 0xc1, &[id, 1, 0]);
    data.push(31);
    for v in [color, -960, -540, 1920, 1080] {
        num(data, v);
    }
}
fn wait(data: &mut Vec<u8>, v: i32) {
    data.extend([0x83, 0]);
    num(data, v);
}
fn scene(gpu: &Offscreen, scenario: Arc<Gerokasu2Scenario>) -> Gerokasu2PlayScene {
    shin_tasks::block_on(Gerokasu2PlayScene::load(
        assets(gpu),
        scenario,
        Arc::new(AudioManager::new_for_tests()),
        0,
        42,
        InitialMessage {
            style: 0,
            asynchronous: false,
            face: -1,
        },
        Default::default(),
        context(),
    ))
    .unwrap()
}
#[test]
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio"]
fn ger_layer_edits_decoded_commands_reuse_retained_bank_clear_other_plane_and_exit() {
    let gpu = Offscreen::new();
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut data = bytes();
    data.truncate(original.header.code_offset as usize);
    tile(&mut data, 0, 0xff00);
    tile(&mut data, 1, 0xf00f);
    op(&mut data, 0xc5, &[0, 1]);
    wait(&mut data, 1);
    op(&mut data, 0xc2, &[0, 3]);
    wait(&mut data, 1);
    tile(&mut data, 2, 0xf0f0);
    wait(&mut data, 3);
    op(&mut data, 0xcc, &[1]);
    tile(&mut data, 0, 0xf00f);
    data.push(0xcd);
    op(&mut data, 0xcc, &[0]);
    op(&mut data, 0xc6, &[0, 255]);
    op(&mut data, 0xc2, &[-6, -2]);
    wait(&mut data, 0);
    data.push(0xcd);
    data.extend([0, 0]);
    num(&mut data, 0);
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    let mut scene = scene(&gpu, Arc::new(Gerokasu2Scenario::new(data.into()).unwrap()));
    let mut frame = FrameId::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut retained = false;
    let mut reused = false;
    loop {
        frame.advance();
        let status = scene
            .step(frame, Ticks::from_f32(1.0), policy(), context())
            .unwrap();
        assert_ne!(
            status,
            RunnerStatus::External,
            "{:?}",
            scene.runner().pending_command()
        );
        if scene.runner().completed_commands() == 5 {
            assert!(scene.runner().layers().target(LayerId::new(0)).is_none());
            retained |= scene
                .backend()
                .screen()
                .page_layer()
                .get_plane(PlaneId::new(0))
                .reference_retained_layer_count()
                > 0;
        }
        if let Some(t) = scene.runner().layers().target(LayerId::new(2)) {
            reused |= t.layerbank.raw() == 1;
        }
        if status == RunnerStatus::Exited {
            break;
        }
        assert!(Instant::now() < deadline);
    }
    assert_eq!(scene.runner().completed_commands(), 17);
    assert!(retained && reused);
    for p in [0, 1] {
        assert!(
            scene
                .backend()
                .screen()
                .page_layer()
                .get_plane(PlaneId::new(p))
                .reference_owner_entry_words()
                .is_empty()
        );
    }
    let mut frames = Frames::new(&gpu);
    assert!(
        frames
            .draw_scene(&gpu, scene.backend_mut())
            .chunks_exact(4)
            .all(|p| p == [0, 0, 0, 255])
    );
    println!(
        "GEROKASU2_LAYER_EDITS_RUNNER commands=17 delayed_bank_reuse_swap_plane_clear_selected_negative_delay_EXIT=verified frames={} channels={} audio=Loopback Audio",
        frames.frames, frames.channels
    );
}
#[test]
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio"]
fn ger_layer_edits_supplied_opening_initializes_six_owned_banks() {
    let gpu = Offscreen::new();
    let scenario = Arc::new(Gerokasu2Scenario::new(bytes().into()).unwrap());
    let mut scene = scene(&gpu, scenario);
    let mut frame = FrameId::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        frame.advance();
        let status = scene
            .step(frame, Ticks::from_f32(1.0), policy(), context())
            .unwrap();
        assert_ne!(status, RunnerStatus::External);
        if scene.runner().completed_commands() >= 23 {
            break;
        }
        assert!(Instant::now() < deadline);
        assert_ne!(status, RunnerStatus::Exited);
    }
    assert!(scene.runner().completed_commands() >= 23);
    for id in 0..6 {
        let bank = scene
            .runner()
            .layers()
            .target(LayerId::new(id))
            .unwrap()
            .layerbank;
        assert!(scene.backend().layer(PlaneId::new(0), bank).is_some());
    }
    println!(
        "GEROKASU2_AUTHORED_INITIALIZATION six_owned_banks no_external_acknowledgements supplied_ROM_unchanged=verified audio=Loopback Audio"
    );
}
