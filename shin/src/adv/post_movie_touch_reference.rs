//! Authored Episode2 event, actual movie loading/long-press skip and reveal.
//! Starts at the exact CPU-replayed authored Movie event, not the whole Start UI.
use super::{
    Adv, ExecutingCommand, StartableCommand, VmState,
    assets::AdvAssets,
    command,
    reconstruction::{ReconstructedScene, ReconstructionStop},
};
use crate::{
    app::AppAction, asset::system::AssetServer, render::PreRenderContext, update::UpdateContext,
};
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::{
    format::scenario::Scenario,
    primitives::update::FrameId,
    time::Ticks,
    vm::{
        Scripter,
        command::{CommandResult, RuntimeCommand, types::LayerType},
    },
};
use shin_input::ActionState;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn cpu(next: RuntimeCommand, state: &mut VmState) -> CommandResult {
    match next {
        RuntimeCommand::SGET(get) => {
            let value = get.apply_state(state);
            get.token.finish(value)
        }
        RuntimeCommand::SELECT(_) | RuntimeCommand::QUIZ(_) => {
            panic!("Interactive before Episode2 movie")
        }
        cmd => {
            command::apply_command_state_ref(&cmd, state).unwrap();
            cmd.execute_dummy().expect("Episode2 exited before movie")
        }
    }
}
fn movie(scenario: &Scenario) -> ReconstructedScene {
    // Locate the exact event that owns Episode2's first eligible Movie.
    let mut script = Scripter::new(scenario, 1, 42);
    let mut state = VmState::for_scenario(1, 42);
    state.settings.v11_disallowskipunread = false;
    let mut result = CommandResult::None;
    let mut event = None;
    let event_step = (0..500_000)
        .find_map(|step| {
            let next = script
                .run(std::mem::replace(&mut result, CommandResult::None))
                .unwrap();
            if matches!(&next, RuntimeCommand::EVBEGIN(_)) {
                event = Some(step);
            }
            if matches!(&next, RuntimeCommand::LAYERLOAD(load) if load.layer_type==LayerType::Movie)
                && state.current_event.is_some()
                && state.system_controls_allowed
            {
                println!(
                    "Episode2 first skippable Movie boundary{} pc{} event step{} movie step{}",
                    state.save_position,
                    script.position().0,
                    event.unwrap(),
                    step
                );
                return event;
            }
            result = cpu(next, &mut state);
            None
        })
        .expect("Episode2 movie missing");
    // Replay the original path again, retaining the genuine supported EVBEGIN
    // token and its constructor state. Normal live commands load the Movie.
    let mut script = Scripter::new(scenario, 1, 42);
    let mut state = VmState::for_scenario(1, 42);
    state.settings.v11_disallowskipunread = false;
    let mut result = CommandResult::None;
    for step in 0..=event_step {
        let next = script.run(result).unwrap();
        if step == event_step {
            assert!(matches!(&next, RuntimeCommand::EVBEGIN(_)));
            command::apply_command_state_ref(&next, &mut state).unwrap();
            assert_eq!(state.snapshot_entry().scenario_id, 1);
            return ReconstructedScene {
                scripter: script,
                state,
                stop: ReconstructionStop::Command(next),
            };
        }
        result = cpu(next, &mut state);
    }
    unreachable!()
}

struct Driver {
    frame: FrameId,
    number: u32,
}
impl Driver {
    fn step<'a>(
        &mut self,
        live: &mut Adv,
        ctx: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        point: Vec2,
        extent: (u16, u16),
        held: bool,
        delta: f32,
    ) {
        self.number += 1;
        live.sample_modal_pointer(point, extent, held, 1.0 / 60.0);
        live.handle_pointer_state(
            point * Vec2::new(1920.0 / f32::from(extent.0), 1080.0 / f32::from(extent.1)),
            false,
            held,
            1.0 / 60.0,
        );
        let mut input = EnumMap::<AppAction, ActionState>::default();
        input[AppAction::PointerClick].is_held = held;
        live.update(
            &mut UpdateContext {
                frame_id: self.frame,
                frame_started_at: Instant::now(),
                delta_ticks: Ticks::from_f32(delta),
                asset_server: server,
                pre_render: ctx,
            },
            input,
        );
        self.frame.advance();
    }
}

pub(super) fn run<'a>(ctx: &mut PreRenderContext<'a, '_, '_, '_>, server: &'a Arc<AssetServer>) {
    crate::init_test_task_pools();
    let assets = shin_tasks::block_on(AdvAssets::load(server)).unwrap();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    for extent in [(1280, 720), (1920, 1080)] {
        for release in ["after-reveal", "skip-frame", "reveal"] {
            let mut live = Adv::new(
                manager.clone(),
                server.clone(),
                assets.clone(),
                Scripter::new(&assets.scenario, 1, 42),
            );
            live.queue_reconstructed_scene(Box::new(movie(&assets.scenario)), server.clone(), None);
            let point = Vec2::new(extent.0 as f32 * 0.3, extent.1 as f32 * 0.7);
            let mut d = Driver {
                frame: FrameId::default(),
                number: 0,
            };
            let deadline = Instant::now() + Duration::from_secs(40);
            while !matches!(live.current_command, Some(ExecutingCommand::MOVIEWAIT(_)))
                || live.is_loading_saved_scene()
            {
                assert!(
                    Instant::now() < deadline,
                    "Episode2 movie load: error={:?} pc={:?} save={} loading={} event={:?}",
                    live.saved_scene_load_error(),
                    live.scripter.position(),
                    live.vm_state.save_position,
                    live.is_loading_saved_scene(),
                    live.vm_state.current_event
                );
                d.step(&mut live, ctx, server, point, extent, false, 1.0);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(live.vm_state.current_event.is_some());
            assert!(live.story_touch_eligibility().input && live.story_touch_eligibility().event);
            // Same public pointer sampler and actual ADV long-press callback as the app.
            d.step(&mut live, ctx, server, point, extent, true, 0.0);
            for _ in 0..75 {
                d.step(&mut live, ctx, server, point, extent, true, 0.0);
                if live.is_loading_saved_scene() {
                    break;
                }
            }
            assert!(
                live.is_loading_saved_scene(),
                "real long press must start event skip"
            );
            println!(
                "Episode2 skip accepted, worker_pending={} release={release}",
                live.pending_restore.is_some()
            );
            if release == "skip-frame" {
                d.step(&mut live, ctx, server, point, extent, false, 0.0);
            }
            while live.pending_restore.is_some() {
                assert!(Instant::now() < deadline, "Episode2 restore timeout");
                d.step(
                    &mut live,
                    ctx,
                    server,
                    point,
                    extent,
                    release != "skip-frame",
                    1.0,
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(live.adv_state.scene_load_fade.is_some());
            assert!(live.vm_state.current_event.is_none());
            if release == "reveal" {
                d.step(&mut live, ctx, server, point, extent, true, 10.0);
                assert!(
                    live.adv_state.scene_load_fade.is_some(),
                    "mid-reveal release requires active fade"
                );
                d.step(&mut live, ctx, server, point, extent, false, 1.0);
            }
            while live.is_loading_saved_scene() {
                assert!(Instant::now() < deadline, "Episode2 reveal timeout");
                d.step(
                    &mut live,
                    ctx,
                    server,
                    point,
                    extent,
                    release == "after-reveal",
                    1.0,
                );
            }
            if release == "after-reveal" {
                d.step(&mut live, ctx, server, point, extent, false, 0.0);
            }
            while !live.adv_state.message_layer().is_interested_in_input()
                || !live.story_touch_eligibility().advance
            {
                assert!(
                    Instant::now() < deadline,
                    "Episode2 post-movie message timeout"
                );
                d.step(&mut live, ctx, server, point, extent, false, 1.0);
            }
            assert!(live.saved_scene_load_error().is_none());
            assert!(!live.story_touch.blocks_input());
            let e = live.story_touch_eligibility();
            assert!(e.input && e.system && e.advance);
            assert!(!live.adv_state.playback_modes.skip && !live.adv_state.playback_modes.autoplay);
            let neutral = live.scripter.position();
            for _ in 0..8 {
                d.step(&mut live, ctx, server, point, extent, false, 1.0);
            }
            assert_eq!(
                live.scripter.position(),
                neutral,
                "neutral frames cannot advance message"
            );
            d.step(&mut live, ctx, server, point, extent, false, 0.0);
            assert!(
                !live.process_story_pointer().0,
                "{release} {extent:?}: released movie contact must not consume idle dialogue frames"
            );
            let before = live.scripter.position();
            for _ in 0..4 {
                d.step(&mut live, ctx, server, point, extent, true, 0.0);
                d.step(&mut live, ctx, server, point, extent, false, 1.0);
                d.step(&mut live, ctx, server, point, extent, false, 1.0);
            }
            assert_ne!(
                live.scripter.position(),
                before,
                "fresh post-movie taps advance real story"
            );
            // A new deliberate swipe must still open the toolbar after several taps.
            let start = Vec2::new(extent.0 as f32 * 0.7, point.y);
            d.step(&mut live, ctx, server, start, extent, true, 0.0);
            d.step(
                &mut live,
                ctx,
                server,
                start - Vec2::new(extent.0 as f32 * 0.2, 0.0),
                extent,
                true,
                0.0,
            );
            d.step(
                &mut live,
                ctx,
                server,
                start - Vec2::new(extent.0 as f32 * 0.3, 0.0),
                extent,
                false,
                0.0,
            );
            assert!(
                live.story_touch.reference_toolbar_open(),
                "fresh post-movie swipe opens controls"
            );
            println!(
                "Episode2 {extent:?} release={release}: actual long-press movie skip/reveal, idle unconsumed, fresh taps advance and swipe opens controls; no user saves"
            );
        }
    }
}
