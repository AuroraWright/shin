use super::{
    tests::{Voice, policy},
    *,
};
use binrw::BinWrite;
use shin_core::format::scenario::instruction_elements::UntypedNumberSpec;
use shin_core::vm::command::types::LayerType;
use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
#[ignore = "executes original updated LAYERLOAD callback readiness and animation gates"]
fn higurashi_layer_loading_matches_native_readiness_and_animation_gate() {
    let mut input = String::new();
    let mut expected = Vec::new();
    for flags in 0..=255 {
        for gate in [0, 1] {
            input.push_str(&format!("N {flags} {gate}\n"));
            expected.push(format!("N {gate} 0 0 0 1"));
            let owner = Owner::AssetFault {
                message: String::new(),
                animate: flags & 2 != 0,
            };
            let pending_gate = gate != 0 && owner.animations_enabled();
            for poll in 1..=8 {
                let ready = poll >= 7;
                input.push_str(&format!("P {}\n", u8::from(ready)));
                expected.push(format!(
                    "P {} {} 1 {poll} 1 0",
                    u8::from(ready),
                    u8::from(ready || pending_gate)
                ));
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_LAYER_LOADING").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).unwrap();
    assert_eq!(actual.lines().count(), expected.len());
    for (i, (actual, expected)) in actual.lines().zip(&expected).enumerate() {
        assert_eq!(actual, expected, "native loading row{i}");
    }
    println!(
        "Higurashi layer loading:512 native sequences/4608 snapshots match; one factory request, readiness, bit1 animation gate and balanced task references"
    );
}

fn number(code: &mut Vec<u8>, n: i32) {
    let mut encoded = std::io::Cursor::new(Vec::new());
    UntypedNumberSpec::Constant(n)
        .write_le(&mut encoded)
        .unwrap();
    code.extend(encoded.into_inner());
}

/// Hold the real loader's output behind an explicit test gate, even on cache
/// hits. No delays, hooks or alternate asset path enter production dispatch.
fn hold(
    runner: &mut HigurashiRunner,
) -> (
    futures::channel::oneshot::Sender<()>,
    Arc<std::sync::atomic::AtomicBool>,
) {
    let pending = runner.pending.as_mut().unwrap();
    let Owner::Asset { task, animate } = std::mem::replace(&mut pending.owner, Owner::New) else {
        panic!("not loading")
    };
    let (sender, receiver) = futures::channel::oneshot::channel();
    let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ready_worker = ready.clone();
    pending.owner = Owner::Asset {
        animate,
        task: shin_tasks::async_io::spawn(async move {
            receiver.await.unwrap();
            let result = task.await;
            ready_worker.store(true, std::sync::atomic::Ordering::Release);
            result
        }),
    };
    (sender, ready)
}

#[test]
#[ignore = "real PIC4/BUP4 workers, retained VM and live frame clocks on offscreen Metal"]
fn higurashi_runner_polls_assets_without_blocking_live_frame_or_replaying_vm() {
    let (device, queue, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let bustup = authored
        .bustup_info
        .iter()
        .position(|i| {
            i.name.as_str().eq_ignore_ascii_case("org/m/kei7") && i.emotion.as_str() == "Def1"
        })
        .unwrap() as i32;
    let mut cases = 0;
    let mut rendered = 0;
    for (kind, resource) in [(2, 424), (3, bustup), (2, -1)] {
        for flags in 0..4 {
            let mut raw = authored.raw().to_vec();
            let entry = 0x25491;
            let mut code = vec![0xc1, 1, kind as u8, flags as u8, 1];
            number(&mut code, resource);
            code.extend([0, 0, 0]);
            raw[entry..entry + code.len()].copy_from_slice(&code);
            let mut scenario = HigurashiScenario::new(raw.into()).unwrap();
            scenario.header.code_offset = entry as u32;
            let scenario = Arc::new(scenario);
            let mut runner = shin_tasks::block_on(HigurashiRunner::load(
                assets.clone(),
                scenario.clone(),
                0,
                42,
                || [1, 2, 3, 4],
            ))
            .unwrap();
            let old = Cmd::LAYERLOAD {
                args: [1, 1, 0],
                params: (65535, -640, -720, 1280, 720, 0, 0, 0),
            };
            shin_tasks::block_on(runner.frame.scene.layers_mut().unwrap().dispatch(
                &old,
                &assets,
                &scenario,
                || [1, 2, 3, 4],
            ))
            .unwrap();
            let tween = Cmd::LAYERCTRL {
                args: [1, 0],
                params: (100, 128, 0, 0, 0, 0, 0, 0),
            };
            shin_tasks::block_on(runner.frame.scene.layers_mut().unwrap().dispatch(
                &tween,
                &assets,
                &scenario,
                || [1, 2, 3, 4],
            ))
            .unwrap();
            let target = runner.frame.scene.layers().state.target(1).unwrap();
            let mut voice = Voice::default();
            let mut seeds = 0;
            assert_eq!(
                shin_tasks::block_on(runner.pump(
                    policy(),
                    1,
                    100,
                    || {
                        seeds += 1;
                        [5, 6, 7, 8]
                    },
                    &mut voice
                ))
                .unwrap(),
                Status::Waiting
            );
            assert_eq!(seeds, 1);
            let (gate, ready) = hold(&mut runner);
            let retained = (
                runner.position(),
                runner.next_position(),
                runner.random_state(),
                runner.completed_commands(),
            );
            assert!(runner.complete_external(CommandResult::None).is_err());
            for poll in 0..32 {
                let mut loading_policy = policy();
                loading_policy.fast_skip = poll % 2 == 0;
                assert_eq!(
                    shin_tasks::block_on(runner.pump(
                        loading_policy,
                        1,
                        100,
                        || panic!("load retry consumed seeds"),
                        &mut voice
                    ))
                    .unwrap(),
                    Status::Waiting
                );
                assert_eq!(
                    retained,
                    (
                        runner.position(),
                        runner.next_position(),
                        runner.random_state(),
                        runner.completed_commands()
                    )
                );
                assert_eq!(
                    runner
                        .frame
                        .scene
                        .layers()
                        .layer(target)
                        .unwrap()
                        .layer_type(),
                    LayerType::Tile
                );
                assert_eq!(
                    runner
                        .frame
                        .scene
                        .layers()
                        .state
                        .record(target)
                        .layer_type(),
                    1
                );
            }
            runner.advance(Ticks::from_f32(32.0), policy(), &mut voice, None);
            let x = if flags & 2 != 0 { 25.0 } else { 0.0 };
            assert_eq!(
                runner
                    .frame
                    .scene
                    .layers()
                    .layer(target)
                    .unwrap()
                    .properties
                    .values()[0],
                x
            );
            let mut paused = policy();
            paused.paused = true;
            runner.advance(Ticks::from_f32(128.0), paused, &mut voice, None);
            let mut disabled = policy();
            disabled.animations_enabled = false;
            runner.advance(Ticks::from_f32(128.0), disabled, &mut voice, None);
            assert_eq!(
                runner
                    .frame
                    .scene
                    .layers()
                    .layer(target)
                    .unwrap()
                    .properties
                    .values()[0],
                x
            );
            // The pending worker leaves the old drawable available to actual
            // offscreen frame submission at both output resolutions.
            for size in [(1280, 720), (1920, 1080)] {
                let pixels = super::super::frame::tests::gpu::render_to_texture(
                    &device,
                    &queue,
                    size,
                    |context, view| {
                        let mut textures =
                            super::super::scene_textures::HigurashiSceneTextures::new(context);
                        textures
                            .prepare_scene(context, &runner.frame.scene)
                            .unwrap();
                        let mut pass = context.begin_pass(shin_render::shaders::types::texture::TextureTarget { view, kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture }, Some(context.depth_stencil), "Pending Higurashi load");
                        pass.clear(
                            Some(shin_core::primitives::color::UnormColor::BLACK),
                            Some(0),
                            Some(1.0),
                        );
                        textures.render_frame(&mut pass, &runner.frame).unwrap();
                    },
                );
                assert!(pixels.chunks_exact(4).any(|p| p == [255, 255, 255, 255]));
                rendered += 1;
            }
            gate.send(()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            while !ready.load(std::sync::atomic::Ordering::Acquire) {
                assert!(Instant::now() < deadline, "worker timed out {kind}/{flags}");
                std::thread::sleep(Duration::from_millis(1));
            }
            // Even ready loads cannot publish while the ADV is paused.
            for _ in 0..4 {
                assert_eq!(
                    shin_tasks::block_on(runner.pump(
                        paused,
                        1,
                        100,
                        || panic!("paused consumed seeds"),
                        &mut voice
                    ))
                    .unwrap(),
                    Status::Waiting
                );
                assert_eq!(
                    runner
                        .frame
                        .scene
                        .layers()
                        .layer(target)
                        .unwrap()
                        .layer_type(),
                    LayerType::Tile
                );
            }
            let deadline = Instant::now() + Duration::from_secs(30);
            let completion = loop {
                assert!(Instant::now() < deadline, "loading timeout {kind}/{flags}");
                let result = shin_tasks::block_on(runner.pump(
                    policy(),
                    1,
                    100,
                    || panic!("publication consumed seeds"),
                    &mut voice,
                ));
                if !matches!(result, Ok(Status::Waiting)) {
                    break result;
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            if resource < 0 {
                let error = completion.unwrap_err().to_string();
                assert!(error.contains("Negative Higurashi picture ID"));
                for _ in 0..4 {
                    assert_eq!(
                        shin_tasks::block_on(runner.pump(
                            policy(),
                            1,
                            100,
                            || panic!("failed load consumed seeds"),
                            &mut voice
                        ))
                        .unwrap_err()
                        .to_string(),
                        error
                    );
                }
                assert_eq!(
                    runner
                        .frame
                        .scene
                        .layers()
                        .layer(target)
                        .unwrap()
                        .layer_type(),
                    LayerType::Tile
                );
                assert_eq!(runner.completed_commands(), 0);
            } else {
                assert_eq!(completion.unwrap(), Status::CommandBudget);
                assert_eq!(
                    (
                        runner.position(),
                        runner.next_position(),
                        runner.random_state()
                    ),
                    (retained.0, retained.1, retained.2)
                );
                let owner = runner.frame.scene.layers().layer(target).unwrap();
                assert_eq!(
                    owner.layer_type(),
                    if kind == 2 {
                        LayerType::Picture
                    } else {
                        LayerType::Bustup
                    }
                );
                assert_eq!(
                    owner.properties.values()[0],
                    if flags & 1 != 0 { x } else { 0.0 }
                );
                assert_eq!(
                    owner.properties.oscillators[0].seed,
                    if flags & 1 != 0 { 1 } else { 5 }
                );
                assert_eq!(runner.completed_commands(), 1);
                assert_eq!(
                    shin_tasks::block_on(runner.pump(
                        policy(),
                        1,
                        100,
                        || panic!("EXIT consumed seeds"),
                        &mut voice
                    ))
                    .unwrap(),
                    Status::Exited
                );
                assert_eq!(runner.completed_commands(), 2);
            }
            if resource < 0 {
                assert_eq!(
                    retained,
                    (
                        runner.position(),
                        runner.next_position(),
                        runner.random_state(),
                        runner.completed_commands()
                    )
                );
            }
            cases += 1;
        }
    }
    println!(
        "Higurashi nonblocking loads: {cases} real PIC/BUP/failure cases, {rendered} pending frames at720p/1080p; VM/seeds retained, animation flags, pause, keep/reset and fault ownership pass"
    );
}
