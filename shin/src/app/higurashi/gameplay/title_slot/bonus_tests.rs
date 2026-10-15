//! Source-only acceptance candidates for the original EXIT12049 Bonus factory.
//! Parent owns compilation and real offscreen Metal/exact Loopback execution.
//! Frame markers test whole-composition transport, not native fragment math.
use super::*;
use crate::{
    adv::higurashi::{
        audio::HigurashiAudio,
        autosave::{ProgressEntry, SaveProgress},
        reference_gpu_assets, reference_render_to_texture,
    },
    app::higurashi::{
        gameplay::tests::{audio, fresh, policy},
        load_game::{Source, Storage},
        runtime_game_data::RuntimeGameData,
        runtime_saves::Slots,
    },
};
use shin_core::{
    format::save::higurashi::SaveDate, primitives::color::UnormColor,
    vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};
use shin_render::{
    DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        texture::{TextureTarget, TextureTargetKind},
        vertices::PosColVertex,
    },
};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

const FIRST_MT: u32 = 3_499_211_612; // Frozen native MT seed5489 first result.

#[derive(Default)]
struct NoWrites(AtomicUsize);
impl Storage for NoWrites {
    fn trial_token(&self) -> bool {
        false
    }
    fn trial_exists(&self) -> Result<bool> {
        Ok(false)
    }
    fn read(&self, _: Source) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
    fn write(&self, _: &[u8]) -> Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        bail!("Bonus must not create a progress writer")
    }
}

struct Fixture {
    device: wgpu::Device,
    queue: wgpu::Queue,
    server: Arc<AssetServer>,
}
struct SourceOwner {
    slot: TitleSlot,
    number: u32,
    io: Arc<NoWrites>,
    os_calls: Arc<AtomicUsize>,
    date_calls: Arc<AtomicUsize>,
    setter_value: u8,
}
impl Fixture {
    fn new() -> Self {
        let (device, queue, server) = reference_gpu_assets();
        Self {
            device,
            queue,
            server: Arc::new(server),
        }
    }
    fn source(
        &self,
        extent: (u32, u32),
        scenario: i32,
        active: usize,
        committed: bool,
        setter_value: u8,
    ) -> SourceOwner {
        // Real decoded WAIT/SSET/DB/EXIT commands. The terminal WAIT prevents
        // this controlled fixture from running unrelated authored gameplay.
        let code = [
            0x83,
            1,
            0x83,
            0xe8,
            0x82,
            18,
            2,
            0xdb,
            52,
            setter_value,
            0,
            0,
            1,
            0x83,
            0,
            1,
        ];
        let (_, common) =
            crate::app::higurashi::gameplay::tests::character::common(&self.server, &code);
        let manager = Arc::new(shin_audio::AudioManager::new_for_tests()); // Exact Loopback Audio.
        let io = Arc::new(NoWrites::default());
        let mut saved = SaveData::default();
        saved.persist_data.0 = vec![1; 192];
        saved.settings.common.v3_sysvol = 37;
        saved.save_menu_position = 29;
        saved.save_vectors.seen_messages = vec![0xa5, 0x8765_4321];
        saved.save_vectors.select_seen_choices = vec![0xdead_beef, 0x2345_6789];
        saved.save_vectors.select_last_choices = vec![7, 255, 3];
        saved.save_vectors.extra_words_0 = vec![0x1234_5678; 20];
        saved.auto_save_slot = Some(fresh());
        let mut slots = Slots::loaded(&saved);
        for (index, row) in std::iter::once(&mut slots.auto)
            .chain(slots.manual.iter_mut())
            .enumerate()
        {
            row.valid = index % 3 != 2;
            row.data = RuntimeGameData::from(fresh());
            row.data.progress.extra_word = 0xdead_0000 | index as u32;
            row.data.progress.entries[0].scenario_id = 2000 + index as i32;
            row.data.progress.entries[0].random_seed = 0x1234_5678 ^ index as u32;
            row.data.progress.entries[0].response_data = vec![index as u8, 0xee];
            row.data.progress.entries[1] = ProgressEntry {
                scenario_id: 12049,
                random_seed: 0xfedc_ba98 ^ index as u32,
                save_position: 123 + index as u32,
                replay_target: 789 + index as u32,
                response_data: vec![0xf1, index as u8, 0x55],
            };
            row.data.progress.set_active_index(index & 1).unwrap();
            row.data.trailing_values = vec![0xffff, 0x1234, index as u16];
            row.data.byte_variables = vec![2, index as u8, 0xff];
        }
        let globals = Globals {
            common: common.clone(),
            data: saved,
            storage: Some(io.clone()),
            runtime_saves: Some(slots),
            clock: PlayClock::from_saved_seconds(17),
            platform_flag: true,
            last_title_mode: 7,
            audio: HigurashiAudio::new(
                manager.clone(),
                self.server.clone(),
                common.scenario.clone(),
            ),
            server: self.server.clone(),
            manager: manager.clone(),
            sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager))),
        };
        let mut data = RuntimeGameData::from(fresh());
        data.date = SaveDate {
            year: 2025,
            month: 12,
            day: 31,
            hour: 23,
            minute: 58,
            second: 57,
        };
        data.progress.extra_word = 0xdead_beef;
        data.progress.entries[0].scenario_id = 100;
        data.progress.entries[0].random_seed = 0x2468_acf1;
        data.progress.entries[0].response_data = vec![0x23, 0xee];
        data.progress.entries[1] = ProgressEntry {
            scenario_id: 2084,
            random_seed: 0x8765_4321,
            save_position: 17,
            replay_target: 19,
            response_data: vec![0xf1, 0x55],
        };
        data.progress.set_active_index(active).unwrap();
        data.progress.entries[active].scenario_id = scenario;
        data.progress.entries[active].save_position = 0;
        data.progress.entries[active].replay_target = 0;
        data.trailing_values = vec![7, 0xffff, 0x1234];
        data.byte_variables = vec![0; 64];
        let source_date = data.date;
        let os_calls = Arc::new(AtomicUsize::new(0));
        let calls = os_calls.clone();
        let mut game = None;
        reference_render_to_texture(&self.device, &self.queue, extent, |context, _| {
            let mut initial = context.new_render_texture("Bonus source initial".into());
            context
                .begin_pass(initial.as_texture_target(), None, "Bonus initial black")
                .clear(Some(UnormColor::BLACK), None, None);
            game = Some(
                Gameplay::new(globals, data, 0, initial, move || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    [1, 2, 3, 4]
                })
                .unwrap(),
            );
        });
        let date_calls = Arc::new(AtomicUsize::new(0));
        let dates = date_calls.clone();
        let mut game = game.unwrap();
        game.set_clock_source(move || {
            dates.fetch_add(1, Ordering::SeqCst);
            Ok(source_date)
        });
        let mut source = SourceOwner {
            slot: TitleSlot::from_game(game),
            number: 0,
            io,
            os_calls,
            date_calls,
            setter_value,
        };
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            self.step(&mut source, 3.0).unwrap();
            let game = source.slot.game.as_ref().unwrap();
            if game.input_permission
                && !game.host().unwrap().has_modes()
                && matches!(
                    game.host().unwrap().runner.pending_command(),
                    Some(Cmd::WAIT { amount: 1000, .. })
                )
            {
                break;
            }
            assert!(
                Instant::now() < end,
                "Bonus source First did not reach WAIT"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        source
            .slot
            .game
            .as_mut()
            .unwrap()
            .bindings
            .memory
            .commits_enabled = committed;
        source
    }
    fn step(&self, source: &mut SourceOwner, elapsed: f32) -> Result<Status> {
        source.number += 1;
        shin_tasks::block_on(source.slot.update_game(
            Ticks::from_f32(elapsed),
            true,
            Buttons::default(),
            &PointerFrame {
                number: source.number,
                time_seconds: source.number as f64 / 60.0,
                events: &[],
            },
            policy(),
            audio(),
        ))
    }
    fn exit(&self, source: &mut SourceOwner, expect_bonus: bool) {
        let os = source.os_calls.load(Ordering::SeqCst);
        let dates = source.date_calls.load(Ordering::SeqCst);
        let mt = crate::engine_random::seed_call_count();
        let source_date = source.slot.game.as_ref().unwrap().bindings.date;
        assert!(
            source
                .slot
                .game
                .as_mut()
                .unwrap()
                .host_mut()
                .unwrap()
                .runner
                .interrupt_wait()
        );
        assert_eq!(self.step(source, 0.0).unwrap(), Status::External);
        let game = source.slot.game.as_ref().unwrap();
        assert_eq!(
            game.host().unwrap().runner.exit_command().unwrap().phase(),
            1
        );
        assert_eq!(game.bindings.progress.bytes()[52], 0);
        assert_eq!(
            game.bindings.progress.pending_bytes(),
            [(52, i32::from(source.setter_value))]
        );
        assert!(!source.slot.pending_scene_factory());
        assert_eq!(crate::engine_random::seed_call_count(), mt);
        // EXIT's30-tick clock advances after each command poll; reaching its
        // endpoint alone must not emit the factory until the following poll.
        for _ in 0..2 {
            assert_eq!(self.step(source, 0.0).unwrap(), Status::External);
            assert!(!source.slot.pending_scene_factory());
        }
        for _ in 0..10 {
            assert_eq!(self.step(source, 3.0).unwrap(), Status::External);
            assert_eq!(
                source
                    .slot
                    .game
                    .as_ref()
                    .unwrap()
                    .host()
                    .unwrap()
                    .runner
                    .exit_command()
                    .unwrap()
                    .phase(),
                1
            );
            assert!(!source.slot.pending_scene_factory());
            assert_eq!(crate::engine_random::seed_call_count(), mt);
        }
        assert_eq!(self.step(source, 0.0).unwrap(), Status::External);
        assert_eq!(
            source
                .slot
                .game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .runner
                .exit_command()
                .unwrap()
                .phase(),
            -1
        );
        assert_eq!(
            crate::engine_random::seed_call_count(),
            mt + usize::from(expect_bonus)
        );
        assert_eq!(source.os_calls.load(Ordering::SeqCst), os);
        assert_eq!(source.date_calls.load(Ordering::SeqCst), dates);
        assert_eq!(
            source.slot.game.as_ref().unwrap().bindings.date,
            source_date
        );
        assert_eq!(source.io.0.load(Ordering::SeqCst), 0);
        assert!(
            source.slot.game.is_some(),
            "Factory emission must not retire the source"
        );
    }
    fn capture(&self, slot: &mut TitleSlot, size: (u32, u32)) -> (RenderTexture, Vec<u8>) {
        let mut composed = None;
        let pixels = reference_render_to_texture(&self.device, &self.queue, size, |ctx, target| {
            slot.prepare(ctx).unwrap();
            let mut color = ctx.new_render_texture("Bonus whole composed source".into());
            for output in [
                color.as_texture_target(),
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
            ] {
                let mut pass =
                    ctx.begin_pass(output, Some(ctx.depth_stencil), "Bonus source composition");
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                slot.render(&mut pass).unwrap();
                marker(&mut pass); // Separate platform-overlay transport fixture.
            }
            composed = Some(color);
        });
        assert!(pixels.chunks_exact(4).any(|p| p == [31, 95, 159, 255]));
        assert!(pixels.chunks_exact(4).any(|p| p == [191, 63, 127, 255]));
        (composed.unwrap(), pixels)
    }
    fn copied(&self, snapshot: &RenderTexture, size: (u32, u32)) -> Vec<u8> {
        reference_render_to_texture(&self.device, &self.queue, size, |ctx, target| {
            let overlay = crate::adv::higurashi::runner::reconstruction_mode::Overlay {
                clock: OverlayClock::captured(true),
                snapshot: snapshot.copy_snapshot(&mut ctx.render_clone_ctx()),
                mask: None,
            };
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(ctx.depth_stencil),
                "Bonus pure snapshot",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            overlay.render(&mut pass);
        })
    }
    fn boundary(
        &self,
        source: &mut SourceOwner,
        composed: &RenderTexture,
        extent: (u32, u32),
    ) -> Result<Option<Status>> {
        source.number += 1;
        let mut result = None;
        reference_render_to_texture(&self.device, &self.queue, extent, |ctx, _| {
            result = Some(shin_tasks::block_on(source.slot.update_frame(
                ctx,
                composed,
                (extent.0 as u16, extent.1 as u16),
                Ticks::ZERO,
                true,
                Buttons::default(),
                &PointerFrame {
                    number: source.number,
                    time_seconds: source.number as f64 / 60.0,
                    events: &[],
                },
                policy(),
                audio(),
            )));
        });
        result.unwrap()
    }
}

fn marker(pass: &mut RenderPass) {
    for (origin, color) in [
        (
            glam::vec2(-0.75, 0.65),
            UnormColor::from_rgba(31, 95, 159, 255),
        ),
        (
            glam::vec2(0.25, -0.15),
            UnormColor::from_rgba(191, 63, 127, 255),
        ),
    ] {
        let vertices = shin_render::quad_vertices::build_quad_vertices(|uv| PosColVertex {
            position: (origin + uv * glam::vec2(0.25, -0.3)).extend(0.0),
            color,
        });
        pass.run(RenderRequestBuilder::new().build(
            RenderProgramWithArguments::Fill {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                transform: glam::Mat4::IDENTITY,
            },
            DrawPrimitive::TrianglesStrip,
        ));
    }
}
fn expected_bonus(seed: u32) -> RuntimeGameData {
    //4738..47e0 writes these fields. Date/dormant fixed scalar zeroing is the
    //explicit desktop initialization boundary, not native stack-byte parity.
    let empty = ProgressEntry {
        scenario_id: 0,
        random_seed: 0,
        save_position: 0,
        replay_target: 0,
        response_data: vec![],
    };
    RuntimeGameData::from_progress(
        SaveProgress::new(
            0,
            0,
            [
                ProgressEntry {
                    scenario_id: 10010,
                    random_seed: seed,
                    ..empty.clone()
                },
                empty,
            ],
        )
        .unwrap(),
        SaveDate {
            year: 0,
            month: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
        },
        vec![],
        vec![],
    )
}
fn rows_equal(actual: &Slots, expected: &Slots) {
    for (index, (a, e)) in std::iter::once(&actual.auto)
        .chain(actual.manual.iter())
        .zip(std::iter::once(&expected.auto).chain(expected.manual.iter()))
        .enumerate()
    {
        assert_eq!(a.valid, e.valid, "Full saved row{index} validity");
        assert_eq!(
            a.data, e.data,
            "Full saved row{index} date/both records/extra/four banks"
        );
    }
}

#[test]
#[ignore = "actual decoded EXIT12049/11a94/30tick gates/one global MT seed at dispatch,fresh10010 flags0,both active records and negative route controls;real assets/offscreen Metal/exact Loopback,no storage writes/window"]
fn higurashi_bonus_exit_emits_once_after_committed_byte_and_native_wait_gates() {
    let fixture = Fixture::new();
    let mut bonus = 0;
    let mut opposite = 0;
    for active in 0..2 {
        for (scenario, commit, setter, expected) in [
            (12049, true, 2, 0),
            (12049, false, 2, 1),
            (12049, true, 1, 1),
            (12051, true, 2, 2),
        ] {
            let mut source = fixture.source((1280, 720), scenario, active, commit, setter);
            let original = source
                .slot
                .game
                .as_ref()
                .unwrap()
                .runtime_game_data()
                .unwrap();
            let calls = crate::engine_random::seed_call_count();
            crate::engine_random::with_reference_seed(5489, || {
                fixture.exit(&mut source, expected == 0);
            });
            let game = source.slot.game.as_ref().unwrap();
            assert_eq!(
                game.runtime_game_data().unwrap().progress.entries,
                original.progress.entries
            );
            assert_eq!(
                game.bindings.progress.persistent().get(18),
                if commit { 2 } else { 1 }
            );
            assert_eq!(
                game.bindings.progress.bytes()[52],
                if commit { setter } else { 0 }
            );
            if expected == 0 {
                let Some(Pending::BonusAdv(transfer)) = &source.slot.pending else {
                    panic!("Native Bonus must queue a typed fresh ADV factory");
                };
                assert_eq!(transfer.data, expected_bonus(FIRST_MT));
                assert_eq!(transfer.flags, 0);
                assert!(
                    transfer.globals.is_none()
                        && transfer.bindings.is_none()
                        && transfer.snapshot.is_none()
                );
                assert!(source.slot.pending_bonus_adv() && source.slot.pending_scene_factory());
                assert!(game.pending_exit_request().is_none());
                assert!(!game.has_save_task());
                assert_eq!(crate::engine_random::seed_call_count(), calls + 1);
                bonus += 1;
            } else if expected == 1 {
                let Some(Pending::Kakera(request)) = &source.slot.pending else {
                    panic!("Uncommitted/subthreshold12049 must remain Kakera");
                };
                assert_eq!(request.mode, 49);
                assert_eq!(request.data, game.runtime_game_data().unwrap());
                assert!(!source.slot.pending_bonus_adv());
                assert_eq!(crate::engine_random::seed_call_count(), calls);
                opposite += 1;
            } else {
                assert!(source.slot.pending.is_none());
                assert!(
                    game.has_save_task(),
                    "12051 still needs native SaveThenTitle"
                );
                assert!(!source.slot.pending_bonus_adv());
                assert_eq!(crate::engine_random::seed_call_count(), calls);
                opposite += 1;
            }
            assert_eq!(source.io.0.load(Ordering::SeqCst), 0);
        }
    }
    assert_eq!((bonus, opposite), (2, 6));
    eprintln!(
        "Bonus EXIT:8 actual decoded owners/both active records,30tick post-poll gating/committed byte52/2 fresh10010 flags0 factories with exactlyone global MT seed;6 subthreshold/uncommitted/12051 controls;no writes/calendar calls or source projection"
    );
}

#[test]
#[ignore = "actual EXIT Bonus next-frame fullADV/capture/all101 rows/preflight+post-retirement failure/retry/frozen MT+Globals/save snapshot/real First at720p1080p;offscreen Metal/exact Loopback,no user saves/window;Runtime quarantine evidence reused"]
fn higurashi_bonus_scene_transfer_retains_full_globals_and_frozen_seed_across_factory_retry() {
    let fixture = Fixture::new();
    let mut profiles = 0;
    for (size, active) in [((1280, 720), 0), ((1920, 1080), 1)] {
        let mut source = fixture.source(size, 12049, active, true, 2);
        let os_before = source.os_calls.load(Ordering::SeqCst);
        let dates_before = source.date_calls.load(Ordering::SeqCst);
        let mt_before = crate::engine_random::seed_call_count();
        crate::engine_random::with_reference_seed(5489, || fixture.exit(&mut source, true));
        let expected = expected_bonus(FIRST_MT);
        assert!(source.slot.pending_bonus_adv());
        assert_eq!(source.slot.active_scene().unwrap(), ActiveScene::Gameplay);
        assert!(
            fixture.step(&mut source, 0.0).is_err(),
            "Pending factory must reject another source update"
        );
        assert!(source.slot.game.as_ref().unwrap().fault.is_none());
        let (composed, image) = fixture.capture(&mut source.slot, size);
        let game = source.slot.game.as_ref().unwrap();
        let expected_rows = game.globals.runtime_saves.as_ref().unwrap().clone();
        let frozen_save = serde_json::to_value(source.slot.save_snapshot().unwrap()).unwrap();
        let global_clock = game.globals.clock.ticks();
        let old_provider = game.seeds.clone();
        let sounds = game.globals.sounds.clone();
        let manager = game.globals.manager.clone();
        let server = game.globals.server.clone();
        let common = game.globals.common.clone();
        let weak_atlas = Arc::downgrade(game.atlas.as_ref().unwrap());
        assert_eq!(crate::engine_random::seed_call_count(), mt_before + 1);

        // Deliberately invalid flags test preflight only, after the successful
        // request came from actual EXIT. It is never a synthetic success path.
        let Some(Pending::BonusAdv(transfer)) = &mut source.slot.pending else {
            unreachable!()
        };
        transfer.flags = 1;
        let error = fixture.boundary(&mut source, &composed, size).unwrap_err();
        assert!(format!("{error:#}").contains("flags0"));
        assert!(source.slot.game.is_some() && weak_atlas.upgrade().is_some());
        assert_eq!(source.slot.active_scene().unwrap(), ActiveScene::Gameplay);
        assert_eq!(source.slot.globals().unwrap().clock.ticks(), global_clock);
        assert_eq!(source.os_calls.load(Ordering::SeqCst), os_before);
        assert_eq!(crate::engine_random::seed_call_count(), mt_before + 1);
        let Some(Pending::BonusAdv(transfer)) = &mut source.slot.pending else {
            unreachable!()
        };
        assert_eq!(transfer.data, expected);
        assert!(transfer.globals.is_none() && transfer.snapshot.is_none());
        transfer.flags = 0;

        let event_count = source.slot.reference_scene_events.len();
        source.slot.reference_fail_next_constructor();
        let error = fixture.boundary(&mut source, &composed, size).unwrap_err();
        assert!(format!("{error:#}").contains("Injected retained scene constructor fault"));
        assert!(source.slot.game.is_none());
        assert!(
            weak_atlas.upgrade().is_none(),
            "Source worker/atlas must retire before actual constructor"
        );
        assert_eq!(
            source.slot.active_scene().unwrap(),
            ActiveScene::RetiredFactory
        );
        assert!(source.slot.pending_scene_factory());
        assert_eq!(&source.slot.reference_scene_events[event_count..], [1]);
        assert_eq!(source.os_calls.load(Ordering::SeqCst), os_before);
        assert_eq!(crate::engine_random::seed_call_count(), mt_before + 1);
        source.slot.cancel_input().unwrap(); // No clocks/acknowledgement on a vacant retained factory.
        assert_eq!(source.slot.globals().unwrap().clock.ticks(), global_clock);
        assert_eq!(
            serde_json::to_value(source.slot.save_snapshot().unwrap()).unwrap(),
            frozen_save
        );
        let Some(Pending::BonusAdv(transfer)) = &source.slot.pending else {
            unreachable!()
        };
        assert_eq!(transfer.data, expected);
        assert_eq!(transfer.flags, 0);
        assert_eq!(
            transfer
                .bindings
                .as_ref()
                .unwrap()
                .runtime_game_data()
                .unwrap(),
            expected
        );
        let globals = transfer.globals.as_ref().unwrap();
        rows_equal(globals.runtime_saves.as_ref().unwrap(), &expected_rows);
        assert!(Arc::ptr_eq(&globals.sounds, &sounds));
        assert!(Arc::ptr_eq(&globals.manager, &manager));
        assert!(Arc::ptr_eq(&globals.server, &server));
        assert!(Arc::ptr_eq(&globals.common, &common));
        assert_eq!(
            fixture.copied(transfer.snapshot.as_ref().unwrap(), size),
            image
        );

        // Actual update_frame owns the next boundary dispatch and updates the
        // incoming First exactly once; no direct Gameplay replacement is made.
        fixture.boundary(&mut source, &composed, size).unwrap();
        assert!(!source.slot.pending_bonus_adv() && !source.slot.pending_scene_factory());
        assert_eq!(
            &source.slot.reference_scene_events[event_count..],
            [1, 2, 3]
        );
        let game = source.slot.game.as_ref().unwrap();
        assert_eq!(game.runtime_game_data().unwrap(), expected);
        assert_eq!(game.flags, 0);
        assert!(Arc::ptr_eq(&game.seeds, &old_provider));
        assert!(Arc::ptr_eq(&game.globals.sounds, &sounds));
        assert!(Arc::ptr_eq(&game.globals.manager, &manager));
        assert!(
            game.bindings.memory.entries().is_empty()
                && game.bindings.memory.extra_words.is_empty()
        );
        assert!(
            game.bindings.progress.pending_persistent().is_empty()
                && game.bindings.progress.pending_bytes().is_empty()
        );
        assert_eq!(game.bindings.progress.responses().1, 0);
        assert_eq!(game.globals.clock.ticks(), global_clock);
        rows_equal(game.globals.runtime_saves.as_ref().unwrap(), &expected_rows);
        // A genuinely completed atlas worker may already transfer the capture
        // from outer First to its reconstruction child in this zero-delta
        // callback. Both are actual owners of the same retained composition.
        let first_capture = game
            .snapshot
            .as_ref()
            .or_else(|| game.host().unwrap().runner.reconstruction_overlay())
            .expect("Initial incoming First must retain the source composition");
        assert_eq!(fixture.copied(&first_capture.snapshot, size), image);
        assert!(source.os_calls.load(Ordering::SeqCst) > os_before);
        assert_eq!(crate::engine_random::seed_call_count(), mt_before + 1);

        let end = Instant::now() + Duration::from_secs(20);
        loop {
            fixture.step(&mut source, 3.0).unwrap();
            let game = source.slot.game.as_ref().unwrap();
            if game.input_permission
                && !game.host().unwrap().has_modes()
                && matches!(
                    game.host().unwrap().runner.pending_command(),
                    Some(Cmd::WAIT { amount: 1000, .. })
                )
            {
                break;
            }
            assert!(
                Instant::now() < end,
                "Real Bonus ADV First did not complete"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let game = source.slot.game.as_ref().unwrap();
        assert_eq!(game.runtime_game_data().unwrap(), expected);
        assert!(game.snapshot.is_none());
        rows_equal(game.globals.runtime_saves.as_ref().unwrap(), &expected_rows);
        assert_eq!(source.date_calls.load(Ordering::SeqCst), dates_before);
        assert_eq!(source.io.0.load(Ordering::SeqCst), 0);
        assert_eq!(crate::engine_random::seed_call_count(), mt_before + 1);
        let before = game.globals.clock.ticks();
        let visual_before = game.host().unwrap().ui_elapsed_seconds();
        fixture.step(&mut source, 15.0).unwrap();
        let game = source.slot.game.as_ref().unwrap();
        assert_eq!(game.globals.clock.ticks(), before + 15.0);
        assert_eq!(
            game.host().unwrap().ui_elapsed_seconds().to_bits(),
            (visual_before + 3.0 * f32::from_bits(0x3c88_8889)).to_bits()
        );
        profiles += 1;
    }
    assert_eq!(profiles, 2);
    eprintln!(
        "Bonus scene:2 actual EXIT factories at720p/1080p,full101 rows/Globals/providers,exact whole-composed marker captures,preflight source retention,source death before failed constructor,retained save_snapshot and retry without another MT draw,real flags0 First and fresh VM,clamped3 visual/full15 clock;Runtime quarantine/focus/window evidence reused,not newly exercised"
    );
}
