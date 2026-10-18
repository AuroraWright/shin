use super::*;
use crate::{
    adv::{
        higurashi::{audio::HigurashiAudio, reference_gpu_assets, reference_render_to_texture},
        input_area::{PointerEvent, PointerKind},
    },
    app::higurashi::gameplay::tests::{audio, fresh, policy},
};
use shin_core::{
    format::{
        save::higurashi::SaveDate,
        scenario::higurashi::{HigurashiScenario, HigurashiVariant},
    },
    primitives::color::UnormColor,
};
use shin_render::{
    DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    shaders::types::{
        buffer::VertexSource,
        texture::{TextureTarget, TextureTargetKind},
        vertices::PosColVertex,
    },
};
use std::{
    io::Write,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

fn words(data: &RuntimeGameData) -> Vec<u64> {
    let mut out = vec![
        bindings::packed_date(data.date),
        data.progress.active_index() as u64,
        data.progress.extra_word as u64,
    ];
    for e in &data.progress.entries {
        out.extend([
            e.scenario_id as u32 as u64,
            e.random_seed as u64,
            e.save_position as u64,
            e.replay_target as u64,
            e.response_data.len() as u64,
        ]);
        out.extend(e.response_data.iter().map(|&v| v as u64));
    }
    out.push(data.trailing_values.len() as u64);
    out.extend(data.trailing_values.iter().map(|&v| v as u64));
    out.push(data.byte_variables.len() as u64);
    out.extend(data.byte_variables.iter().map(|&v| v as u64));
    out
}
fn protocol_data(data: &RuntimeGameData) -> String {
    let mut out = format!(
        "{:x} {} {:x}",
        bindings::packed_date(data.date),
        data.progress.active_index(),
        data.progress.extra_word
    );
    for e in &data.progress.entries {
        out += &format!(
            " {:x} {:x} {:x} {:x} {}",
            e.scenario_id as u32,
            e.random_seed,
            e.save_position,
            e.replay_target,
            e.response_data.len()
        );
        for b in &e.response_data {
            out += &format!(" {b}");
        }
    }
    out += &format!(" {}", data.trailing_values.len());
    for v in &data.trailing_values {
        out += &format!(" {v}");
    }
    out += &format!(" {}", data.byte_variables.len());
    for v in &data.byte_variables {
        out += &format!(" {v}");
    }
    out
}
fn state(slot: &TitleSlot) -> (u32, f64, u32, RuntimeGameData) {
    if let Some(game) = &slot.game {
        (
            2,
            game.globals.clock.ticks(),
            game.globals.clock.saved_seconds(),
            game.runtime_game_data().unwrap(),
        )
    } else {
        let menu = slot.kakera.as_ref().unwrap();
        (
            1,
            menu.globals.clock.ticks(),
            menu.globals.clock.saved_seconds(),
            menu.motion.model.data.clone(),
        )
    }
}
fn pending(slot: &TitleSlot) -> (u32, i32, RuntimeGameData) {
    match slot.pending.as_ref().unwrap() {
        Pending::Kakera(r) => (1, r.mode, r.data.clone()),
        Pending::Gameplay(r) => (2, r.flags as i32, r.data.clone()),
        _ => panic!("unexpected scene factory"),
    }
}
fn snapshot(
    pending: u32,
    kind: u32,
    param: i32,
    active_param: i32,
    delta: f32,
    clock: f64,
    seen: f64,
    seconds: u32,
    events: &[u32],
    data: &RuntimeGameData,
) -> Vec<u64> {
    let mut v = vec![
        pending as u64,
        kind as u64,
        param as u32 as u64,
        active_param as u32 as u64,
        delta.to_bits() as u64,
        clock.to_bits(),
        seen.to_bits(),
        seconds as u64,
        events.len() as u64,
    ];
    v.extend(events.iter().map(|&v| v as u64));
    v.extend(words(data));
    v
}
struct Evidence {
    protocol: String,
    expected: Vec<Vec<u64>>,
    captures: usize,
}
impl Evidence {
    fn queued(&mut self, slot: &mut TitleSlot) {
        let (kind, clock, seconds, data) = state(slot);
        let (next, param, payload) = pending(slot);
        self.protocol += &format!(
            "N {kind} {:x} {}\nQ {next} {param} {}\n",
            clock.to_bits(),
            protocol_data(&data),
            protocol_data(&payload)
        );
        self.expected.push(snapshot(
            next,
            kind,
            param,
            0,
            0.0,
            clock,
            clock,
            seconds,
            &[],
            &payload,
        ));
        slot.reference_scene_events.clear();
    }
    fn dispatched(&mut self, slot: &mut TitleSlot, elapsed: f32, before: f64, param: i32) {
        assert!(slot.pending.is_none());
        let (kind, clock, seconds, data) = state(slot);
        self.protocol += &format!("U {elapsed}\n");
        self.expected.push(snapshot(
            0,
            kind,
            0,
            param,
            elapsed.min(3.0),
            clock,
            before,
            seconds,
            &slot.reference_scene_events,
            &data,
        ));
        assert_eq!(std::mem::take(&mut slot.reference_scene_events), [1, 2, 3]);
        self.protocol += "D\n";
    }
    fn verify(self) {
        let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_KAKERA_SCENE").unwrap())
            .args([
                "game-data/higurashi/uncompressed_executable_update",
                "/tmp/higurashi-newrodin.fnt",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let protocol = self.protocol.clone();
        let writer = std::thread::spawn(move || stdin.write_all(protocol.as_bytes()).unwrap());
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output_text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(output_text.lines().count(), self.expected.len());
        for (i, (line, expected)) in output_text.lines().zip(&self.expected).enumerate() {
            let mut fields = line.split_whitespace();
            assert_eq!(fields.next(), Some("K"));
            assert_eq!(
                fields
                    .map(|v| v.parse::<u64>().unwrap())
                    .collect::<Vec<_>>(),
                *expected,
                "native scene observation{i}"
            );
        }
        let path = "/tmp/higurashi-kakera-scene-capture";
        std::fs::create_dir_all(path).unwrap();
        for (name, bytes) in [
            ("input.txt", self.protocol.as_bytes()),
            ("output.txt", output_text.as_bytes()),
            ("stderr.txt", &output.stderr),
        ] {
            std::fs::write(format!("{path}/{name}"), bytes).unwrap();
        }
        println!(
            "Kakera scene:{} native observations/{} exact words,{} byte-identical whole-frame transfers;actual deferred factories/retirement/new-scene update/full runtime records/seed-clock-history continuation pass",
            self.expected.len(),
            self.expected.iter().map(Vec::len).sum::<usize>(),
            self.captures
        );
    }
}
fn marker(pass: &mut RenderPass) {
    let vertices = shin_render::quad_vertices::build_quad_vertices(|uv| PosColVertex {
        position: (glam::vec2(-0.5, 0.5) + uv * glam::vec2(0.25, -0.25)).extend(0.0),
        color: UnormColor::from_rgba(32, 96, 160, 255),
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
struct Frames {
    number: u32,
    size: (u32, u32),
    composed: Option<RenderTexture>,
}
impl Frames {
    fn step(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: &mut TitleSlot,
        elapsed: f32,
        pressed: u32,
        events: &[PointerEvent],
    ) -> Vec<u8> {
        self.number += 1;
        let mut composed = None;
        let image = reference_render_to_texture(device, queue, self.size, |context, target| {
            if let Some(previous) = &self.composed {
                shin_tasks::block_on(slot.update_frame(
                    context,
                    previous,
                    (self.size.0 as u16, self.size.1 as u16),
                    Ticks::from_f32(elapsed),
                    true,
                    Buttons {
                        pressed,
                        ..Default::default()
                    },
                    &PointerFrame {
                        number: self.number,
                        time_seconds: self.number as f64 / 60.0,
                        events,
                    },
                    policy(),
                    audio(),
                ))
                .unwrap();
            }
            slot.prepare(context).unwrap();
            let mut frame = context.new_render_texture("Kakera platform composed frame".into());
            for output in [
                frame.as_texture_target(),
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
            ] {
                let mut pass =
                    context.begin_pass(output, Some(context.depth_stencil), "Kakera scene frame");
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                slot.render(&mut pass).unwrap();
                marker(&mut pass);
            }
            composed = Some(frame);
        });
        self.composed = composed;
        image
    }
    fn ready(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, slot: &mut TitleSlot) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while slot.kakera.as_ref().unwrap().motion.first_phase().is_some() {
            self.step(device, queue, slot, 3.0, 0, &[]);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        slot.reference_scene_events.clear();
    }
    fn queue_again(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: &mut TitleSlot,
    ) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let image = self.step(device, queue, slot, 3.0, 0, &[]);
            if slot.pending_kakera() {
                return image;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
#[test]
#[ignore = "actual owned Gameplay/Kakera/ADV frame dispatcher,original factories/clock,full records,old producer retirement and whole composed capture at720p/1080p;offscreen Metal/exact Loopback Audio,no window/saves"]
fn higurashi_kakera_scene_roundtrips_preserve_runtime_owners_and_native_dispatch_order() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let authored: Arc<HigurashiScenario> = server.load_sync("/main.snr").unwrap();
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    let code = &[
        0x82, 7, 1, 0x86, 1, 0, 0, 0, 0x7f, 4, 0, b'@', b'r', b'A', 0, 0xd5, 0x82, 9, 2, 0x83, 0,
        0x83, 0xe8, 0, 0, 0,
    ];
    raw[start..start + code.len()].copy_from_slice(code);
    let common = Arc::new(CommonAssets {
        scenario: Arc::new(HigurashiScenario::new(raw.into()).unwrap()),
        font: server.load_sync("/newrodin.fnt").unwrap(),
        system: server.load_sync("/systex.txa").unwrap(),
        message: server.load_sync("/msgtex.txa").unwrap(),
        effects: server.load_sync("/effects.txa").unwrap(),
        sysse: server.load_sync("/sysse.bin").unwrap(),
    });
    let mut evidence = Evidence {
        protocol: String::new(),
        expected: vec![],
        captures: 0,
    };
    for size in [(1280, 720), (1920, 1080)] {
        let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
        let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
        let mut saved = SaveData::default();
        saved.persist_data = shin_core::format::save::PersistData(vec![1; 192]);
        saved.settings.common.v3_sysvol = 53;
        let mut disk = fresh();
        disk.trailing_values = vec![0x1234, 0xffff];
        disk.byte_variables = vec![9, 8];
        saved.auto_save_slot = Some(disk.clone());
        saved.manual_save_slots[2] = Some(disk.clone());
        let globals = Globals {
            storage: None,
            runtime_saves: None,
            common: common.clone(),
            data: saved,
            clock: PlayClock::from_saved_seconds(123),
            platform_flag: true,
            last_title_mode: 7,
            audio: HigurashiAudio::new(manager.clone(), server.clone(), authored.clone()),
            server: server.clone(),
            manager,
            sounds: sounds.clone(),
        };
        let mut portable = fresh();
        portable.date = SaveDate {
            year: 2025,
            month: 1,
            day: 2,
            hour: 3,
            minute: 4,
            second: 5,
        };
        portable.entry.random_seed = 42;
        portable.entry.response_data = vec![0x23, 0xee];
        portable.trailing_values = vec![7, 0xffff];
        portable.byte_variables = vec![2, 255];
        let mut runtime = RuntimeGameData::from(portable);
        runtime.progress.extra_word = 0xdeadbeef;
        runtime.progress.entries[1] = crate::adv::higurashi::autosave::ProgressEntry {
            scenario_id: 2084,
            random_seed: 0x87654321,
            save_position: 19,
            replay_target: 23,
            response_data: vec![0xf1, 0x55, 0x23, 0xee],
        };
        let seeds = Arc::new(AtomicUsize::new(0));
        let seed_counter = seeds.clone();
        let mut game = None;
        reference_render_to_texture(&device, &queue, size, |context, _| {
            let mut initial = context.new_render_texture("Initial Kakera ADV".into());
            {
                let mut pass = context.begin_pass(
                    initial.as_texture_target(),
                    Some(context.depth_stencil),
                    "Initial source",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            }
            game = Some(
                Gameplay::new(globals, runtime.clone(), 0, initial, move || {
                    seed_counter.fetch_add(1, Ordering::SeqCst);
                    [1, 2, 3, 4]
                })
                .unwrap(),
            );
        });
        let mut game = game.unwrap();
        let clock_calls = Arc::new(AtomicUsize::new(0));
        let calls = clock_calls.clone();
        game.set_clock_source(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(SaveDate {
                year: 2026,
                month: 10,
                day: 12,
                hour: 20,
                minute: 21,
                second: 22,
            })
        });
        game.bindings.history.record_completion(true, 73);
        let history_words = game.bindings.history.bits().words().as_ptr();
        let mut slot = TitleSlot::from_game(game);
        let mut frames = Frames {
            number: 0,
            size,
            composed: None,
        };
        frames.step(&device, &queue, &mut slot, 0.0, 0, &[]);
        let before = frames.queue_again(&device, &queue, &mut slot);
        assert!(slot.game.is_some());
        assert!(slot.kakera.is_none());
        let mut job = crate::app::higurashi::gameplay::selection::character::Job::new(
            server.clone(),
            &common,
            5,
        );
        let retirement = job.retirement.clone();
        let task = job.task.take().unwrap();
        let (sender, receiver) = futures::channel::oneshot::channel::<()>();
        job.task = Some(shin_tasks::async_io::spawn(async move {
            receiver.await.unwrap();
            task.await
        }));
        slot.game.as_mut().unwrap().selection.character.job = Some(job);
        evidence.queued(&mut slot);
        let before_clock = state(&slot).1;
        let after = frames.step(&device, &queue, &mut slot, 0.0, 0, &[]);
        assert!(retirement.is_finished());
        drop(sender);
        assert_eq!(before, after);
        evidence.captures += 1;
        evidence.dispatched(&mut slot, 0.0, before_clock, 0);
        for cycle in 0..3 {
            frames.ready(&device, &queue, &mut slot);
            let before_clock = state(&slot).1;
            let counter = slot.kakera.as_ref().unwrap().motion.counter;
            frames.step(&device, &queue, &mut slot, 59.5, 0, &[]);
            assert_eq!(state(&slot).1, before_clock + 59.5);
            assert_eq!(slot.kakera.as_ref().unwrap().motion.counter, counter + 1);
            if size.0 == 1920 {
                let point = glam::ivec2(350, 380);
                for kind in [PointerKind::Down, PointerKind::Up] {
                    frames.step(
                        &device,
                        &queue,
                        &mut slot,
                        3.0,
                        0,
                        &[PointerEvent {
                            kind,
                            id: 0,
                            position: point,
                        }],
                    );
                }
            } else {
                frames.step(&device, &queue, &mut slot, 3.0, 1 << 24, &[]);
            }
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut last = None;
            while !slot.pending_kakera_return() {
                last = Some(frames.step(&device, &queue, &mut slot, 3.0, 0, &[]));
                assert!(Instant::now() < deadline);
            }
            let payload = pending(&slot).2;
            assert_eq!(payload.progress.active_index(), 1);
            assert_eq!(payload.progress.extra_word, 0xdeadbeef);
            assert_eq!(payload.progress.entries[1].scenario_id, 2000);
            assert_eq!(
                payload.progress.entries[1].response_data,
                runtime.progress.entries[1].response_data
            );
            assert_eq!(payload.progress.entries[0].save_position, 2);
            assert!(slot.game.is_none());
            assert!(slot.kakera.is_some());
            let old_common = slot.kakera.as_ref().unwrap().globals.common.clone();
            if cycle == 0 {
                let mut invalid =
                    HigurashiScenario::new(old_common.scenario.raw().to_vec().into()).unwrap();
                invalid.variant = HigurashiVariant::Original;
                slot.kakera.as_mut().unwrap().globals.common = Arc::new(CommonAssets {
                    scenario: Arc::new(invalid),
                    font: old_common.font.clone(),
                    system: old_common.system.clone(),
                    message: old_common.message.clone(),
                    effects: old_common.effects.clone(),
                    sysse: old_common.sysse.clone(),
                });
                let before_seeds = seeds.load(Ordering::SeqCst);
                reference_render_to_texture(&device, &queue, size, |context, _| {
                    assert!(
                        slot.dispatch_kakera_return(context, frames.composed.as_ref().unwrap())
                            .is_err()
                    )
                });
                assert!(slot.kakera.is_some());
                assert!(slot.game.is_none());
                assert_eq!(pending(&slot).2, payload);
                assert_eq!(seeds.load(Ordering::SeqCst), before_seeds);
                slot.kakera.as_mut().unwrap().globals.common = old_common;
            }
            evidence.queued(&mut slot);
            let autosave = slot.continuation.as_ref().unwrap().autosave.clone();
            let autosave_words = slot
                .continuation
                .as_ref()
                .unwrap()
                .autosave
                .progress
                .entries[1]
                .response_data
                .as_ptr();
            let clock = state(&slot).1;
            let image = frames.step(&device, &queue, &mut slot, 0.0, 0, &[]);
            assert_eq!(last.unwrap(), image);
            evidence.captures += 1;
            assert_eq!(
                slot.game.as_ref().unwrap().runtime_game_data().unwrap(),
                payload
            );
            assert_eq!(slot.game.as_ref().unwrap().bindings.autosave, autosave);
            assert_eq!(
                slot.game
                    .as_ref()
                    .unwrap()
                    .bindings
                    .autosave
                    .progress
                    .entries[1]
                    .response_data
                    .as_ptr(),
                autosave_words
            );
            evidence.dispatched(&mut slot, 0.0, clock, 0);
            let game = slot.game.as_ref().unwrap();
            assert_eq!(game.globals.data.manual_save_slots[2], Some(disk.clone()));
            assert_eq!(game.globals.data.settings.common.v3_sysvol, 53);
            assert!(Arc::ptr_eq(&game.globals.common, &common));
            assert!(Arc::ptr_eq(&game.globals.sounds, &sounds));
            assert_eq!(game.globals.last_title_mode, 7);
            let before = frames.queue_again(&device, &queue, &mut slot);
            evidence.queued(&mut slot);
            let clock = state(&slot).1;
            let image = frames.step(&device, &queue, &mut slot, 0.0, 0, &[]);
            assert_eq!(before, image);
            evidence.captures += 1;
            evidence.dispatched(&mut slot, 0.0, clock, 0);
        }
        frames.ready(&device, &queue, &mut slot);
        frames.step(&device, &queue, &mut slot, 3.0, 1 << 25, &[]);
        let deadline = Instant::now() + Duration::from_secs(10);
        for _ in 0..100 {
            frames.step(&device, &queue, &mut slot, 3.0, 0, &[]);
            std::thread::sleep(Duration::from_millis(1));
            assert!(Instant::now() < deadline);
        }
        // Standard question starts on No. Left then A confirms Yes.
        frames.number += 1;
        slot.update_kakera(
            Ticks::from_f32(3.0),
            true,
            Buttons {
                repeat: 1 << 7,
                ..Default::default()
            },
            &PointerFrame {
                number: frames.number,
                time_seconds: 0.0,
                events: &[],
            },
        )
        .unwrap();
        frames.step(&device, &queue, &mut slot, 3.0, 1 << 24, &[]);
        let mut last = None;
        while !slot.pending_kakera_return() {
            last = Some(frames.step(&device, &queue, &mut slot, 3.0, 0, &[]));
            assert!(Instant::now() < deadline);
        }
        let payload = pending(&slot).2;
        assert_eq!(payload.progress.active_index(), 0);
        assert_eq!(payload.progress.entries[1].scenario_id, 2000);
        assert_eq!(
            payload.progress.entries[1].response_data,
            runtime.progress.entries[1].response_data
        );
        evidence.queued(&mut slot);
        let clock = state(&slot).1;
        let image = frames.step(&device, &queue, &mut slot, 0.0, 0, &[]);
        assert_eq!(last.unwrap(), image);
        evidence.captures += 1;
        evidence.dispatched(&mut slot, 0.0, clock, 0);
        for _ in 0..100 {
            frames.step(&device, &queue, &mut slot, 3.0, 0, &[]);
            if slot
                .game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .runner
                .pending_command()
                .is_some_and(|c| {
                    matches!(
                        c,
                        shin_core::vm::higurashi::command::HigurashiRuntimeCommand::WAIT { .. }
                    )
                })
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(slot.pending.is_none());
        assert!(slot.kakera.is_none());
        assert_eq!(
            slot.game
                .as_ref()
                .unwrap()
                .runtime_game_data()
                .unwrap()
                .progress
                .entries[1],
            payload.progress.entries[1]
        );
        assert!(clock_calls.load(Ordering::SeqCst) > 2);
        assert!(seeds.load(Ordering::SeqCst) > 1);
        assert!(slot.game.as_ref().unwrap().bindings.history.contains(73));
        assert_eq!(
            slot.game
                .as_ref()
                .unwrap()
                .bindings
                .history
                .bits()
                .words()
                .as_ptr(),
            history_words
        );
    }
    evidence.verify();
}
