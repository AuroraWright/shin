//! Actual runner/loader/Publication ownership for source WIPE reconstruction.
//! The independent48 original command/mode observations live in the CPU
//! corpus test. Here snapshots are supplied at the capture-service boundary;
//! cached ADV rendering/capture pixels and OS audio are separate acceptance.
use super::super::{
    live_reconstruction::tests::Audio,
    read_history::MessageReadHistory,
    reconstruction_mode::Phase,
    replay::tests::{scenario, session},
};
use super::reconstruction_mode::ReconstructionUi;
use super::*;
use shin_render::{
    render_texture::RenderTexture,
    resize::{SurfaceResizeSource, ViewportParams},
};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

struct Reads;
impl ReadServices for Reads {
    fn checkpoint(
        &mut self,
        _: super::super::read_state::Checkpoint,
        _: super::super::input::ReadState,
    ) -> Result<()> {
        Ok(())
    }
    fn message_is_read(&self, _: u32) -> bool {
        false
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
}
#[derive(Default)]
struct Ui(usize);
impl ReconstructionUi for Ui {
    fn reset_blackout_and_indicators(&mut self) {
        self.0 += 1;
    }
}
fn pump(runner: &mut HigurashiRunner, audio: &mut Audio) -> Status {
    shin_tasks::block_on(runner.pump_native(
        super::tests::policy(),
        ReadPolicy {
            modes: 0,
            forced: false,
            read_only: true,
        },
        &mut Reads,
        1,
        10000,
        || [1; 4],
        audio,
    ))
    .unwrap()
}
fn groups(runner: &HigurashiRunner) -> [Vec<(i32, u32, String)>; 2] {
    std::array::from_fn(|bank| {
        runner
            .frame
            .scene
            .layers()
            .reference_group_entries(bank)
            .into_iter()
            .map(|(key, remaining, owner)| (key, remaining, format!("{owner:?}")))
            .collect()
    })
}
fn vm(runner: &HigurashiRunner) -> (CodeAddress, CodeAddress, u32) {
    (
        runner.position(),
        runner.next_position(),
        runner.random_state(),
    )
}
fn snapshot(device: &wgpu::Device, resize: &SurfaceResizeSource) -> RenderTexture {
    RenderTexture::new(
        device.clone(),
        resize.canvas_handle(),
        "Source-command capture boundary".into(),
    )
}

#[test]
#[ignore = "offscreen Metal loaded runner/source WIPE, actual reconstruction workers and audio-failure retry; controlled audio, no window/saves"]
fn higurashi_runner_reconstruction_retires_pending_wipe_only_after_publication() {
    let (device, queue, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let mut cases = 0;
    for kind in 0..4 {
        for mode in [0, 1] {
            for cancel in [true, false] {
                let mut code = match kind {
                    2 => vec![0x8f],              // WIPEWAIT retains the preexisting Screen wiper.
                    3 => vec![0x8e, 0, 1, 60, 0], // bit0 releases command, retains wiper.
                    _ => vec![0x8e, 0, 0, 60, 0], // phase0 or phase2 depends on prior wiper.
                };
                // Replay constructs a real Tile, then stops at the saved MSGSET.
                code.extend([
                    0xc1, 7, 1, 0, 0, 0xc5, 7, 0xc3, 7, 0, 1, 12, 0x8e, 0, 0, 0, 0,
                ]);
                code.extend([0x86, 1, 0, 0, 1, 0x7f, 2, 0, b'A', 0, 0, 0, 0]);
                let script = scenario(&code);
                let mut runner = shin_tasks::block_on(HigurashiRunner::load(
                    assets.clone(),
                    script.clone(),
                    0,
                    42,
                    || [1; 4],
                ))
                .unwrap();
                runner
                    .frame
                    .scene
                    .bind_history(&device, &queue, [1920, 1080])
                    .unwrap();
                runner
                    .frame
                    .scene
                    .layers_mut()
                    .unwrap()
                    .reference_physical_properties_mut(0)
                    .unwrap()
                    .tweener_mut(39)
                    .unwrap()
                    .fast_forward_to(752.);
                if kind == 0 || kind == 2 {
                    // Explicit source fixture: direct original b919c plane transfer.
                    runner.frame.scene.reference_native_wipe(1000).unwrap();
                }
                let mut audio = Audio::default();
                assert_eq!(
                    pump(&mut runner, &mut audio),
                    if kind == 3 {
                        Status::CommandBudget
                    } else {
                        Status::Waiting
                    }
                );
                assert_eq!(runner.frame.scene.command_pending(), kind != 3);
                assert!(runner.frame.scene.transition().is_some());
                for _ in 0..2 {
                    runner.advance(Ticks::ZERO, super::tests::policy(), &mut audio, None);
                }
                let old_vm = vm(&runner);
                let history = runner.frame.scene.history().reference_snapshot();
                assert_eq!(history.0, [1920, 1080]);
                assert!(history.2.is_some());
                let weak = runner.frame.scene.history().weak();
                let before_groups = groups(&runner);
                let before_banks =
                    super::super::layers::replay_snapshot(&runner.frame.scene.layers().state);
                let seeds = Arc::new(AtomicUsize::new(0));
                let seed_calls = seeds.clone();
                runner
                    .begin_reconstruction_mode(
                        session(script, 1, 1, 42),
                        mode,
                        snapshot(&device, &resize),
                        MessageReadHistory::new(),
                        move || {
                            seed_calls.fetch_add(1, Ordering::Relaxed);
                            [11, 12, 13, 14]
                        },
                    )
                    .unwrap();
                runner.advance(
                    Ticks::from_f32(100.),
                    super::tests::policy(),
                    &mut audio,
                    None,
                );
                assert_eq!(vm(&runner), old_vm);
                assert_eq!(runner.frame.scene.history().reference_snapshot(), history);
                assert_eq!(groups(&runner), before_groups);
                if cancel {
                    runner.cancel_reconstruction_mode().unwrap();
                    assert!(runner.vm.is_some());
                    assert_eq!(vm(&runner), old_vm);
                    assert_eq!(runner.frame.scene.command_pending(), kind != 3);
                    assert!(runner.frame.scene.transition().is_some());
                    assert_eq!(groups(&runner), before_groups);
                    assert_eq!(
                        super::super::layers::replay_snapshot(&runner.frame.scene.layers().state),
                        before_banks
                    );
                    assert_eq!(runner.frame.scene.history().reference_snapshot(), history);
                    assert!(runner.reconstruction_overlay().is_none());
                    assert_eq!(seeds.load(Ordering::Relaxed), 0);
                } else {
                    let mut ui = Ui::default();
                    audio.fail_slot = Some(5);
                    let deadline = Instant::now() + Duration::from_secs(30);
                    loop {
                        match runner.poll_reconstruction_mode(&mut audio, &mut ui) {
                            Ok(Phase::Preparing) => {}
                            Err(error) => {
                                assert!(format!("{error:#}").contains("effect unavailable"));
                                break;
                            }
                            other => {
                                panic!("source{kind}/mode{mode}: unexpected publication {other:?}")
                            }
                        }
                        assert!(Instant::now() < deadline);
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    // Native visual prefix is already published, source command
                    // remains held during the explicit desktop audio retry.
                    assert!(runner.vm.is_some());
                    assert_eq!(vm(&runner), old_vm);
                    assert_eq!(runner.frame.scene.command_pending(), kind != 3);
                    assert!(runner.frame.scene.transition().is_none());
                    assert_eq!(runner.frame.scene.history().reference_snapshot(), history);
                    assert!(std::sync::Weak::ptr_eq(
                        &weak,
                        &runner.frame.scene.history().weak()
                    ));
                    assert_eq!(ui.0, 0);
                    assert!(runner.cancel_reconstruction_mode().is_err());
                    let after_groups = groups(&runner);
                    assert!(after_groups.iter().any(|rows| !rows.is_empty()));
                    let after_banks =
                        super::super::layers::replay_snapshot(&runner.frame.scene.layers().state);
                    let constructors = seeds.load(Ordering::Relaxed);
                    assert!(constructors > 0, "real target Tile constructor must run");
                    audio.fail_slot = None;
                    assert_eq!(
                        runner
                            .poll_reconstruction_mode(&mut audio, &mut ui)
                            .unwrap(),
                        Phase::Fading
                    );
                    assert!(runner.vm.is_none());
                    assert!(runner.pending.is_none());
                    assert!(!runner.frame.scene.command_pending());
                    assert!(runner.frame.scene.layers_mut().is_ok());
                    assert_eq!(groups(&runner), after_groups);
                    assert_eq!(
                        super::super::layers::replay_snapshot(&runner.frame.scene.layers().state),
                        after_banks
                    );
                    assert_eq!(runner.frame.scene.history().reference_snapshot(), history);
                    assert_eq!(
                        seeds.load(Ordering::Relaxed),
                        constructors,
                        "audio retry cannot reconstruct twice"
                    );
                    assert_eq!(ui.0, 1);
                    let duration = runner.reconstruction_overlay().unwrap().clock.duration;
                    assert_eq!(duration, if mode == 0 { 30. } else { 4. });
                    runner.advance(
                        Ticks::from_f32(duration),
                        super::tests::policy(),
                        &mut audio,
                        None,
                    );
                    assert_eq!(
                        runner
                            .poll_reconstruction_mode(&mut audio, &mut ui)
                            .unwrap(),
                        Phase::Complete
                    );
                    let _completed = runner.take_reconstruction_result().unwrap();
                    assert!(runner.vm.is_some());
                    assert!(runner.frame.scene.layers_mut().is_ok());
                    assert!(runner.frame.scene.transition().is_none());
                    // Actual retained MSGSET now takes the live command owner.
                    assert_eq!(pump(&mut runner, &mut audio), Status::Waiting);
                    assert!(matches!(runner.pending_command(), Some(Cmd::MSGSET { .. })));
                }
                drop(runner);
                assert!(
                    weak.upgrade().is_none(),
                    "no Screen ownership leaks after source{kind}/mode{mode}/cancel{cancel}"
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 16);
    println!(
        "Source-command runner integration:16 actual loaded runner/worker profiles,4 source states,8 cancellations and8 visual-prefix/audio-retry/fade/VM handoffs; same Screen history and target owners survive command retirement; controlled snapshot/audio services, cached ADV pixels remain pending."
    );
}
