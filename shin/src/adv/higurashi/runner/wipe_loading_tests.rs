//! NEW normal Runner WIPE branch + real MSK3 loader integration. The only
//! readiness seam holds a real decoded Result inside its original loader scope.
//! Pure offscreen GPU services; no window/audio/save initialization. Host's
//! already-proven epoch->prepare cadence is exercised through its real backend.
use super::{
    tests::{Voice, policy},
    *,
};
use crate::{
    asset::{
        mask::MaskTexture,
        system::{AssetLoadContext, LayeredAssetIo, locate_assets_for_game},
    },
    cli::Game,
};
use binrw::BinWrite;
use shin_core::format::{scenario::instruction_elements::UntypedNumberSpec, text::U8String};
use std::{
    path::PathBuf,
    sync::{
        Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

// Own only this uniquely created temporary override; no authored files change.
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "higurashi-wipe-mask-fault-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[derive(Debug)]
struct Decoded {
    owner: Weak<MaskTexture>,
    extent: [u32; 2],
    format: wgpu::TextureFormat,
    label: String,
}
#[derive(Default)]
struct Seen {
    called: AtomicBool,
    result: Mutex<Option<Result<Decoded, String>>>,
}
struct Gate {
    release: futures::channel::oneshot::Sender<()>,
    seen: Arc<Seen>,
}
fn install_gate(runner: &mut HigurashiRunner) -> Gate {
    let (release, receiver) = futures::channel::oneshot::channel();
    let seen = Arc::new(Seen::default());
    let observed = seen.clone();
    runner
        .reference_frame_mut()
        .scene
        .reference_gate_next_mask(Box::new(move |result| {
            let info = match result {
                Ok(mask) => {
                    let size = mask.texture.size();
                    Ok(Decoded {
                        owner: Arc::downgrade(mask),
                        extent: [size.width, size.height],
                        format: mask.texture.wgpu_texture().format(),
                        label: mask.label.clone(),
                    })
                }
                Err(error) => Err(format!("{error:#}")),
            };
            assert!(observed.result.lock().unwrap().replace(info).is_none());
            observed.called.store(true, Ordering::Release);
            Box::pin(async move {
                let _ = receiver.await;
            })
        }));
    Gate { release, seen }
}
fn wait_gate(gate: &Gate) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !gate.seen.called.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline, "actual MSK3 loader timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn number(code: &mut Vec<u8>, value: i32) {
    let mut out = std::io::Cursor::new(Vec::new());
    UntypedNumberSpec::Constant(value)
        .write_le(&mut out)
        .unwrap();
    code.extend(out.into_inner());
}
fn scenario_for_wipe(
    authored: &HigurashiScenario,
    flags: i32,
    duration: i32,
    id: i32,
) -> HigurashiScenario {
    let mut code = vec![0x8e];
    for value in [1, flags, duration] {
        number(&mut code, value);
    }
    code.push(7); // Only actual param0/1/2 are authored here.
    for value in [id, 100, 0] {
        number(&mut code, value);
    }
    code.extend([0, 0, 0]); // EXIT0,0 after this single WIPE.
    let mut raw = authored.raw().to_vec();
    let entry = 0x25491;
    raw[entry..entry + code.len()].copy_from_slice(&code);
    let mut scenario = HigurashiScenario::new(raw.into()).unwrap();
    scenario.header.code_offset = entry as u32;
    scenario
}
fn runner(assets: Arc<AssetServer>, scenario: Arc<HigurashiScenario>) -> HigurashiRunner {
    // Prove the authored instruction before actual Runner dispatch.
    assert!(matches!(
        HigurashiScripter::new(&scenario, 0, 42)
            .unwrap()
            .run(CommandResult::None, 100)
            .unwrap(),
        Cmd::WIPE {
            args: [1, _, _],
            ..
        }
    ));
    shin_tasks::block_on(HigurashiRunner::load(assets, scenario, 0, 42, || {
        [1, 2, 3, 4]
    }))
    .unwrap()
}
fn pump(runner: &mut HigurashiRunner, p: Policy, voice: &mut Voice) -> Result<Status> {
    shin_tasks::block_on(runner.pump(p, 1, 100, || panic!("WIPE consumed layer entropy"), voice))
}
fn retained(runner: &HigurashiRunner) -> (CodeAddress, CodeAddress, u32, u64) {
    (
        runner.position(),
        runner.next_position(),
        runner.random_state(),
        runner.completed_commands(),
    )
}
type Owners = [Vec<(i32, u32, i32, i32, Vec<u32>)>; 2];
fn owners(runner: &HigurashiRunner) -> Owners {
    std::array::from_fn(|bank| {
        runner
            .frame()
            .scene
            .layers()
            .reference_group_entries(bank)
            .into_iter()
            .map(|(key, remaining, layer)| {
                (
                    key,
                    remaining,
                    layer.layer_type().native_id(),
                    layer.owner_id(),
                    layer
                        .properties
                        .values()
                        .iter()
                        .map(|v| v.to_bits())
                        .collect(),
                )
            })
            .collect()
    })
}
fn request_count(assets: &AssetServer, path: &str) -> usize {
    assets
        .reference_requested_paths()
        .iter()
        .filter(|value| value.as_str() == path)
        .count()
}
fn layer_command(
    runner: &mut HigurashiRunner,
    command: Cmd,
    assets: &AssetServer,
    scenario: &HigurashiScenario,
) {
    assert!(
        shin_tasks::block_on(
            runner
                .reference_frame_mut()
                .scene
                .layers_mut()
                .unwrap()
                .dispatch(&command, assets, scenario, || [1, 2, 3, 4])
        )
        .unwrap()
    );
}
fn setup(
    runner: &mut HigurashiRunner,
    assets: &AssetServer,
    scenario: &HigurashiScenario,
    prior: bool,
) {
    let tile = |color| Cmd::LAYERLOAD {
        args: [1, 1, 0],
        params: (color, -640, -720, 1280, 720, 0, 0, 0),
    };
    layer_command(runner, tile(0xf800), assets, scenario);
    if prior {
        assert_eq!(
            runner
                .reference_frame_mut()
                .scene
                .dispatch(
                    &Cmd::WIPE {
                        args: [0, 1, 4],
                        params: (0, 0, 0, 0, 0, 0, 0, 0),
                    },
                    false
                )
                .unwrap(),
            Some(CommandStatus::Complete)
        );
        assert!(runner.frame().scene.transition().is_some());
    }
    layer_command(
        runner,
        Cmd::LAYERCTRL {
            args: [1, 0],
            params: (64, 64, 0, 0, 0, 0, 0, 0),
        },
        assets,
        scenario,
    );
    layer_command(runner, Cmd::LAYERBACK { argument: 1 }, assets, scenario);
    layer_command(runner, tile(0x001f), assets, scenario);
    layer_command(
        runner,
        Cmd::LAYERCTRL {
            args: [1, 0],
            params: (128, 128, 0, 0, 0, 0, 0, 0),
        },
        assets,
        scenario,
    );
}
fn advance_prepared(
    runner: &mut HigurashiRunner,
    renderer: &mut super::super::frame_renderer::FrameRenderer,
    p: Policy,
    voice: &mut Voice,
    delta: f32,
) {
    // Same production epoch boundary as HigurashiHost::prepare_admitted_frame.
    let before = runner.frame().visual_epoch();
    runner.advance(Ticks::from_f32(delta), p, voice, None);
    if runner.frame().visual_epoch() != before {
        renderer.prepare(runner.frame(), None).unwrap();
    }
}
fn release_poll(
    runner: &mut HigurashiRunner,
    release: futures::channel::oneshot::Sender<()>,
    p: Policy,
    voice: &mut Voice,
) -> Result<Status> {
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(
            Instant::now() < deadline,
            "WIPE publication/error timed out"
        );
        let result = pump(runner, p, voice);
        if !matches!(result, Ok(Status::Waiting)) {
            return result;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
#[ignore = "actual normal WIPE Runner/Frame, real supplied MSK3 decode, offscreen native1920 cache and scoped readiness; no audio"]
fn higurashi_runner_mask_wipe_waits_before_lookup_and_freezes_clocks_cache_until_ready() {
    let (device, queue, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let path = format!(
        "/mask/{}.msk",
        authored.mask_info[0].name.as_str().to_ascii_lowercase()
    );
    assert!(assets.contains(&path));
    let root = PathBuf::from(std::env::var("SHIN_HIGURASHI_ASSETS").unwrap());
    let source = locate_assets_for_game(Game::Higurashi, &root)
        .unwrap()
        .read_file(&path)
        .unwrap()
        .read_all_sync()
        .unwrap();
    assert_eq!(&source[..4], b"MSK3");
    let expected = [
        u16::from_le_bytes(source[12..14].try_into().unwrap()) as u32,
        u16::from_le_bytes(source[14..16].try_into().unwrap()) as u32,
    ];
    let mut profiles = 0;
    for no_wait in [false, true] {
        for fast_skip in [false, true] {
            let scenario = Arc::new(scenario_for_wipe(&authored, i32::from(no_wait), 8, 0));
            let mut runner = runner(assets.clone(), scenario.clone());
            setup(&mut runner, &assets, &scenario, true);
            let mut renderer =
                super::super::frame_renderer::FrameRenderer::new(&device, &queue, runner.frame())
                    .unwrap();
            renderer.prepare(runner.frame(), None).unwrap();
            let start_requests = request_count(&assets, &path);
            let gate = install_gate(&mut runner);
            let mut voice = Voice::default();
            let mut requested = policy();
            requested.fast_skip = fast_skip;
            assert_eq!(
                pump(&mut runner, requested, &mut voice).unwrap(),
                Status::Waiting
            );
            let saved = retained(&runner);
            // Valid Mask metadata/IO cannot start while the preceding no-wait
            // Screen wiper is still owned, even if new bit0 or skip are true.
            for step in 0..4 {
                assert_eq!(request_count(&assets, &path), start_requests);
                assert!(!gate.seen.called.load(Ordering::Acquire));
                assert!(runner.visual_updates_admitted(requested));
                assert_eq!(
                    pump(&mut runner, requested, &mut voice).unwrap(),
                    Status::Waiting
                );
                assert_eq!(retained(&runner), saved);
                advance_prepared(&mut runner, &mut renderer, requested, &mut voice, 1.);
                assert_eq!(runner.frame().scene.transition().is_none(), step == 3);
            }
            assert_eq!(request_count(&assets, &path), start_requests);
            assert_eq!(
                pump(&mut runner, requested, &mut voice).unwrap(),
                Status::Waiting
            );
            wait_gate(&gate);
            assert_eq!(request_count(&assets, &path), start_requests + 1);
            let weak = {
                let decoded = gate.seen.result.lock().unwrap();
                let decoded = decoded.as_ref().unwrap().as_ref().unwrap();
                assert_eq!(decoded.extent, expected);
                assert_eq!(decoded.format, wgpu::TextureFormat::R8Unorm);
                assert!(decoded.label.contains(&path));
                decoded.owner.clone()
            };
            assert!(weak.upgrade().is_some());
            let held_owners = owners(&runner);
            let held_epoch = runner.frame().visual_epoch();
            let held_cache = runner.frame().cache.reference_snapshot();
            for attempt in 0..16 {
                let mut p = requested;
                p.fast_skip = attempt % 2 == 0;
                assert_eq!(pump(&mut runner, p, &mut voice).unwrap(), Status::Waiting);
                assert!(!runner.visual_updates_admitted(p));
                advance_prepared(&mut runner, &mut renderer, p, &mut voice, 3.);
                assert_eq!(retained(&runner), saved);
                assert_eq!(owners(&runner), held_owners);
                assert_eq!(runner.frame().visual_epoch(), held_epoch);
                assert_eq!(runner.frame().cache.reference_snapshot(), held_cache);
            }
            // Ready results cannot publish on a paused callback either.
            let mut paused = requested;
            paused.paused = true;
            gate.release.send(()).unwrap();
            for _ in 0..4 {
                assert_eq!(
                    pump(&mut runner, paused, &mut voice).unwrap(),
                    Status::Waiting
                );
                advance_prepared(&mut runner, &mut renderer, paused, &mut voice, 3.);
                assert_eq!(owners(&runner), held_owners);
                assert_eq!(runner.frame().cache.reference_snapshot(), held_cache);
            }
            let deadline = Instant::now() + Duration::from_secs(30);
            while owners(&runner) == held_owners {
                assert!(Instant::now() < deadline);
                let status = pump(&mut runner, requested, &mut voice).unwrap();
                if owners(&runner) != held_owners {
                    assert_eq!(
                        status,
                        if no_wait || fast_skip {
                            Status::CommandBudget
                        } else {
                            Status::Waiting
                        }
                    );
                    break;
                }
                assert_eq!(status, Status::Waiting);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(runner.completed_commands(), u64::from(no_wait || fast_skip));
            assert_eq!(runner.frame().scene.transition().is_some(), !fast_skip);
            assert_eq!(weak.upgrade().is_some(), !fast_skip);
            assert_eq!(
                (
                    runner.position(),
                    runner.next_position(),
                    runner.random_state()
                ),
                (saved.0, saved.1, saved.2)
            );
            assert!(runner.visual_updates_admitted(requested));
            advance_prepared(&mut runner, &mut renderer, requested, &mut voice, 8.);
            assert!(runner.frame().scene.transition().is_none());
            // ActualU retires the Wiper's reference; phase2 still owns its
            // prepared Mask until the following P/6208 command retirement.
            assert_eq!(weak.upgrade().is_some(), !no_wait && !fast_skip);
            assert_eq!(
                runner.frame().cache.reference_snapshot().prepares,
                held_cache.prepares + 1
            );
            if !no_wait && !fast_skip {
                assert_eq!(
                    pump(&mut runner, requested, &mut voice).unwrap(),
                    Status::CommandBudget
                );
            }
            assert!(weak.upgrade().is_none());
            assert_eq!(runner.completed_commands(), 1);
            assert_eq!(request_count(&assets, &path), start_requests + 1);
            assert_eq!(
                pump(&mut runner, policy(), &mut voice).unwrap(),
                Status::Exited
            );
            profiles += 1;
        }
    }
    println!(
        "Higurashi normal WIPE: {profiles} real MSK3 nowait/skip profiles; precedingtransition wait before IO, one actual decode,16held callbacks/owner clocks/cache unchanged, pausedready publication, native1920 cache resume and decodedMask lifetime pass; no audio"
    );
}

#[test]
#[ignore = "normal WIPE decoded-result cancellation and genuine missing/corrupt MSK3 asset failures; offscreen device only"]
fn higurashi_runner_mask_wipe_cancellation_and_real_asset_faults_never_publish_banks() {
    let (device, queue, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let path = format!(
        "/mask/{}.msk",
        authored.mask_info[0].name.as_str().to_ascii_lowercase()
    );
    // Command6208 retirement boundary, without pretending to run reconstruction.
    let scenario = Arc::new(scenario_for_wipe(&authored, 1, 8, 0));
    let mut cancelled = runner(assets.clone(), scenario.clone());
    setup(&mut cancelled, &assets, &scenario, false);
    let gate = install_gate(&mut cancelled);
    let mut voice = Voice::default();
    assert_eq!(
        pump(&mut cancelled, policy(), &mut voice).unwrap(),
        Status::Waiting
    );
    wait_gate(&gate);
    let old = owners(&cancelled);
    let weak = gate
        .seen
        .result
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap()
        .owner
        .clone();
    assert!(weak.upgrade().is_some());
    cancelled
        .reference_frame_mut()
        .scene
        .retire_source_command();
    assert_eq!(owners(&cancelled), old);
    assert!(cancelled.frame().scene.transition().is_none());
    assert!(weak.upgrade().is_none());
    assert!(gate.release.send(()).is_err());
    assert!(!cancelled.frame().command_pending());
    drop(cancelled); // Do not pump a deliberately retired source continuation.

    let root = PathBuf::from(std::env::var("SHIN_HIGURASHI_ASSETS").unwrap());
    let scratch = Scratch::new();
    let corrupt = scratch.0.join(path.trim_start_matches('/'));
    std::fs::create_dir_all(corrupt.parent().unwrap()).unwrap();
    std::fs::write(&corrupt, b"MSK3").unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_dir(&scratch.0).unwrap();
    io.with(
        locate_assets_for_game(Game::Higurashi, &root)
            .unwrap()
            .into(),
    );
    let corrupt_assets = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: Game::Higurashi,
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: assets.context().font_cache.clone(),
        },
    ));
    for corrupt in [false, true] {
        let mut bad = scenario_for_wipe(&authored, 1, 8, 0);
        if !corrupt {
            bad.mask_info[0].name = U8String::new("__missing_wipe_msk");
        }
        let scenario = Arc::new(bad);
        let source = if corrupt {
            corrupt_assets.clone()
        } else {
            assets.clone()
        };
        let failed_path = format!(
            "/mask/{}.msk",
            scenario.mask_info[0].name.as_str().to_ascii_lowercase()
        );
        let mut failed = runner(source.clone(), scenario.clone());
        setup(&mut failed, &source, &scenario, false);
        let gate = install_gate(&mut failed);
        let mut skip = policy();
        skip.fast_skip = true;
        assert_eq!(
            pump(&mut failed, skip, &mut voice).unwrap(),
            Status::Waiting
        );
        wait_gate(&gate);
        let error = gate
            .seen
            .result
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap_err()
            .clone();
        assert!(error.contains(if corrupt {
            "Invalid Higurashi mask header"
        } else {
            "__missing_wipe_msk"
        }));
        let old = owners(&failed);
        let saved = retained(&failed);
        assert!(!failed.visual_updates_admitted(skip));
        assert_eq!(
            pump(&mut failed, skip, &mut voice).unwrap(),
            Status::Waiting
        );
        let error = release_poll(&mut failed, gate.release, skip, &mut voice)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Higurashi Mask wipe asset failed"));
        // Reporting failure is a desktop checked boundary; original general
        // exception/status transport remains unexecuted by command oracle.
        for _ in 0..3 {
            assert_eq!(
                pump(&mut failed, skip, &mut voice).unwrap_err().to_string(),
                error
            );
            assert_eq!(owners(&failed), old);
            assert_eq!(retained(&failed), saved);
            assert!(failed.frame().scene.transition().is_none());
            assert!(!failed.visual_updates_admitted(skip));
        }
        assert_eq!(request_count(&source, &failed_path), 1);
        assert_eq!(failed.completed_commands(), 0);
    }
    // Both metadata-absent cases and signed nonpositive duration avoid the
    // actual loader entirely; duration still governs default/no-effect outcome.
    for (id, duration) in [(-1, 8), (65535, 8), (0, 0), (0, -5)] {
        let scenario = Arc::new(scenario_for_wipe(&authored, 1, duration, id));
        let mut control = runner(assets.clone(), scenario);
        let before = request_count(&assets, &path);
        let gate = install_gate(&mut control);
        assert_eq!(
            pump(&mut control, policy(), &mut voice).unwrap(),
            Status::CommandBudget
        );
        assert_eq!(request_count(&assets, &path), before);
        assert!(!gate.seen.called.load(Ordering::Acquire));
        assert_eq!(control.frame().scene.transition().is_some(), duration > 0);
        control.advance(Ticks::from_f32(8.), policy(), &mut voice, None);
        assert!(control.frame().scene.transition().is_none());
        assert_eq!(
            pump(&mut control, policy(), &mut voice).unwrap(),
            Status::Exited
        );
    }
    println!(
        "Higurashi normal WIPE: real decodedMSK cancellation retires weakowner without bankpublication; missing/corrupt files retain VM and ownerbanks, one attempt/no fallback despite skip+nowait; four metadata/duration noIO controls pass; no audio"
    );
}
