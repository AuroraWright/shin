use super::super::{
    checkpoints::CheckpointServices,
    forward_search::{Kind, Status as SearchStatus},
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
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
#[derive(Default)]
struct Ui {
    resets: usize,
    state: super::super::ui_state::HigurashiUiState,
}
impl ReconstructionUi for Ui {
    fn reset_blackout_and_indicators(&mut self) {
        self.resets += 1;
        self.state.reset_blackout_and_indicators();
    }
}
struct Retired(Arc<AtomicBool>);
impl Drop for Retired {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
#[test]
#[ignore = "offscreen Metal reconstruction mode, actual worker retirement and VM fade handoff"]
fn higurashi_runner_reconstruction_mode_freezes_retires_and_installs_after_native_fade() {
    let (device, queue, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    for mode in [0, 1] {
        for extent in [(1280, 720), (1920, 1080)] {
            let script = scenario(&[
                0x86, 1, 0, 0, 1, 0x7f, 2, 0, b'A', 0, 0x82, 2, 1, 0x86, 2, 0, 0, 1, 0x7f, 2, 0,
                b'B', 0, 0, 0, 0,
            ]);
            let mut initial = session(script.clone(), 0, 0, 42);
            let mut runner = shin_tasks::block_on(HigurashiRunner::load(
                assets.clone(),
                script.clone(),
                0,
                42,
                || [1; 4],
            ))
            .unwrap();
            // These old720/1080 component profiles explicitly supply native
            // constructor dimensions. Normal Gameplay always uses1920x1080.
            runner
                .frame
                .cache
                .reference_set_extent([extent.0, extent.1])
                .unwrap();
            let mut history = MessageReadHistory::new();
            history.record_completion(true, 0);
            let mut audio = Audio::default();
            let mut ui = Ui::default();
            ui.state.pulse_hint();
            ui.state.indicator.set_hidden(1);
            ui.state
                .indicator
                .transition
                .control(1.0, Ticks::from_f32(30.0), 0)
                .unwrap();
            ui.state.menu.indicator.set_hidden(1);
            ui.state.menu.elapsed = 19.0;
            let policy = ReadPolicy {
                modes: 0,
                forced: false,
                read_only: true,
            };
            assert_eq!(
                shin_tasks::block_on(runner.pump_native(
                    super::tests::policy(),
                    policy,
                    &mut CheckpointServices {
                        memory: &mut initial.memory,
                        progress: &mut initial.progress,
                        history: &mut history
                    },
                    32,
                    10000,
                    || [1; 4],
                    &mut audio
                ))
                .unwrap(),
                Status::Waiting
            );
            let mut search = runner
                .begin_forward_search(&initial.memory, &initial.progress, Kind::ReadAhead, true)
                .unwrap()
                .unwrap();
            while search.pump(&history, 32, 10000, || false).unwrap() != SearchStatus::Complete {}
            let result = search.take_result().unwrap().unwrap();
            // Finish the native ten-tick window slide before capturing.
            // The retained MSGSET command still owns the source VM boundary.
            for _ in 0..4 {
                runner.advance(
                    Ticks::from_f32(3.0),
                    super::tests::policy(),
                    &mut audio,
                    None,
                );
            }
            let old = (
                runner.position(),
                runner.next_position(),
                runner.random_state(),
            );
            let glyphs = runner.frame.message.glyphs().unwrap() as *const _;
            let before_time = runner.frame.message.playback.current_time;
            let retired = Arc::new(AtomicBool::new(false));
            let retirement = Retired(retired.clone());
            runner.pending.as_mut().unwrap().owner = Owner::Asset {
                task: shin_tasks::async_io::spawn_loader(async move {
                    let _retirement = retirement;
                    std::future::pending::<Result<super::super::layers::LoadedLayers>>().await
                }),
                animate: true,
            };
            let resize = SurfaceResizeSource::new(ViewportParams::both(
                winit::dpi::PhysicalSize::new(extent.0, extent.1),
            ));
            use super::super::frame::tests::gpu::render_to_texture;
            use super::super::scene_textures::HigurashiSceneTextures;
            use shin_core::primitives::color::UnormColor;
            use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
            let expected_pixels = render_to_texture(&device, &queue, extent, |context, view| {
                let mut screens = HigurashiSceneTextures::new(context);
                screens
                    .prepare_frame_with_overlay(context, &runner.frame, None)
                    .unwrap();
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Source composed frame",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                screens.render_cached_frame(&mut pass, runner.frame(), 1);
            });
            let captured_pixels = render_to_texture(&device, &queue, extent, |context, view| {
                let mut screens = HigurashiSceneTextures::new(context);
                let mut previous = context.new_render_texture("Old separate overlay".into());
                {
                    let mut pass = context.begin_pass(
                        previous.as_texture_target(),
                        None,
                        "Old overlay pixels",
                    );
                    pass.clear(Some(UnormColor::from_rgba(255, 0, 0, 255)), None, None);
                }
                runner.reconstruction_overlay = Some(super::reconstruction_mode::Overlay {
                    clock: super::super::reconstruction_mode::OverlayClock::captured(true),
                    snapshot: previous,
                    mask: None,
                });
                runner
                    .capture_and_begin_reconstruction_mode(
                        context,
                        &mut screens,
                        result,
                        mode,
                        history.clone(),
                        || [2; 4],
                    )
                    .unwrap();
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Retained composed frame",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                runner.reconstruction_overlay().unwrap().render(&mut pass);
            });
            assert_eq!(
                captured_pixels, expected_pixels,
                "Composed capture mode{mode} {extent:?}"
            );
            assert!(
                captured_pixels.chunks_exact(4).any(|p| p != [0, 0, 0, 255]),
                "Composed capture must include real message pixels"
            );
            assert_eq!(runner.reconstruction_phase(), Some(Phase::Preparing));
            assert!(!retired.load(Ordering::Acquire));
            runner.advance(
                Ticks::from_f32(3.0),
                super::tests::policy(),
                &mut audio,
                None,
            );
            assert_eq!(runner.frame.message.playback.current_time, before_time);
            assert_eq!(
                (
                    runner.position(),
                    runner.next_position(),
                    runner.random_state()
                ),
                old
            );
            assert_eq!(runner.frame.message.glyphs().unwrap() as *const _, glyphs);
            assert!(!runner.click_message(&mut audio));
            assert!(!runner.interrupt_wait());
            assert_eq!(
                shin_tasks::block_on(runner.pump_native(
                    super::tests::policy(),
                    policy,
                    &mut CheckpointServices {
                        memory: &mut initial.memory,
                        progress: &mut initial.progress,
                        history: &mut history
                    },
                    32,
                    10000,
                    || [3; 4],
                    &mut audio
                ))
                .unwrap(),
                Status::Waiting
            );
            audio.fail_slot = Some(5);
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                match runner.poll_reconstruction_mode(&mut audio, &mut ui) {
                    Ok(Phase::Preparing) => {}
                    Err(error) => {
                        assert!(
                            error.to_string().contains("effect unavailable"),
                            "Unexpected worker/publication failure: {error:#}"
                        );
                        break;
                    }
                    other => panic!("Unexpected publication{other:?}"),
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(runner.reconstruction_phase(), Some(Phase::Preparing));
            assert!(runner.vm.is_some());
            assert!(!retired.load(Ordering::Acquire));
            assert_eq!(ui.resets, 0);
            assert!(runner.cancel_reconstruction_mode().is_err());
            assert!(
                shin_tasks::block_on(runner.pump_native(
                    super::tests::policy(),
                    policy,
                    &mut CheckpointServices {
                        memory: &mut initial.memory,
                        progress: &mut initial.progress,
                        history: &mut history
                    },
                    32,
                    10000,
                    || [3; 4],
                    &mut audio
                ))
                .is_err()
            );
            audio.fail_slot = None;
            assert_eq!(
                runner
                    .poll_reconstruction_mode(&mut audio, &mut ui)
                    .unwrap(),
                Phase::Fading
            );
            assert!(runner.vm.is_none());
            assert!(retired.load(Ordering::Acquire));
            assert_eq!(ui.resets, 1);
            assert!(!ui.state.hint.is_waiting());
            assert!(!ui.state.indicator.hidden);
            assert_eq!(ui.state.indicator.opacity, 1.0);
            assert!(!ui.state.indicator.transition.is_waiting());
            assert!(!ui.state.menu.indicator.hidden);
            assert_eq!(ui.state.menu.indicator.opacity, 1.0);
            assert_eq!(ui.state.menu.elapsed, 0.0);
            assert_eq!(
                (
                    runner.position(),
                    runner.next_position(),
                    runner.random_state()
                ),
                old
            );
            let overlay = runner.reconstruction_overlay().unwrap();
            assert_eq!(overlay.snapshot.dimensions(), extent);
            assert_eq!(overlay.clock.duration, if mode == 0 { 30.0 } else { 4.0 });
            assert_eq!(overlay.mask.is_some(), mode == 0);
            if mode == 0 {
                assert!(runner.frame.message.glyphs().is_none());
            } else {
                assert_eq!(runner.frame.message.glyphs().unwrap() as *const _, glyphs);
            }
            assert!(runner.cancel_reconstruction_mode().is_err());
            let duration = runner.reconstruction_overlay().unwrap().clock.duration;
            runner.advance(
                Ticks::from_f32(duration - 0.5),
                super::tests::policy(),
                &mut audio,
                None,
            );
            assert_eq!(
                runner
                    .poll_reconstruction_mode(&mut audio, &mut ui)
                    .unwrap(),
                Phase::Fading
            );
            let mut paused = super::tests::policy();
            paused.paused = true;
            runner.advance(Ticks::from_f32(100.0), paused, &mut audio, None);
            assert_eq!(
                runner
                    .poll_reconstruction_mode(&mut audio, &mut ui)
                    .unwrap(),
                Phase::Fading
            );
            runner.advance(
                Ticks::from_f32(0.5),
                super::tests::policy(),
                &mut audio,
                None,
            );
            assert!(
                runner.vm.is_none(),
                "clock update precedes VM publication poll"
            );
            assert_eq!(
                runner
                    .poll_reconstruction_mode(&mut audio, &mut ui)
                    .unwrap(),
                Phase::Complete
            );
            assert!(runner.vm.is_some());
            assert!(runner.reconstruction_phase().is_none());
            let mut completed = runner.take_reconstruction_result().unwrap();
            assert_eq!(completed.status(), super::super::replay::Status::Published);
            assert_eq!(completed.progress.persistent().get(2), 1);
            assert_eq!(initial.progress.persistent().get(2), 0);
            assert_eq!(
                runner.native_read_state().unwrap().flags,
                completed.read.state().flags
            );
            assert!(!runner.reconstruction_overlay().unwrap().clock.visible());
            assert_eq!(
                shin_tasks::block_on(runner.pump_native(
                    super::tests::policy(),
                    policy,
                    &mut CheckpointServices {
                        memory: &mut completed.memory,
                        progress: &mut completed.progress,
                        history: &mut history
                    },
                    32,
                    10000,
                    || [4; 4],
                    &mut audio
                ))
                .unwrap(),
                Status::Waiting
            );
            assert_eq!(runner.read_state.as_ref().unwrap().timeline().boundary, 2);
            // A second preparation can cancel and drain its worker while retaining
            // the now-live VM, then resume with the same message owner.
            let fork = completed
                .memory
                .fork_for_restore(2, 2, &completed.progress)
                .unwrap();
            let next = super::super::replay::ReplaySession::new(script, fork).unwrap();
            let before = (
                runner.position(),
                runner.next_position(),
                runner.random_state(),
            );
            let snapshot = RenderTexture::new(
                device.clone(),
                resize.canvas_handle(),
                "Higurashi cancellation capture".into(),
            );
            runner
                .begin_reconstruction_mode(next, mode, snapshot, history.clone(), || [5; 4])
                .unwrap();
            runner.cancel_reconstruction_mode().unwrap();
            assert!(runner.reconstruction_phase().is_none());
            assert!(runner.reconstruction_overlay().is_none());
            assert_eq!(
                (
                    runner.position(),
                    runner.next_position(),
                    runner.random_state()
                ),
                before
            );
        }
    }
    println!(
        "Runner reconstruction modes:4 real-resource publication/fade/retirement/cancellation pipelines at720p/1080p"
    );
}
