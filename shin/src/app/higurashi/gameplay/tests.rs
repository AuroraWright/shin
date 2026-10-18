use super::*;
use bindings::{Bindings, packed_date};
use shin_core::format::save::higurashi::{GameDataEntry, SaveDate};
use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub(in crate::app::higurashi::gameplay) mod character;
mod command_tail;
mod exit;
mod kakera;
mod message_copy;
mod prologue_bars;
mod quiz;
mod retained_cache;
mod scenario_select;
mod scene_jump;
mod selection;
mod temporary_notifications;
mod touch;
pub(super) fn fresh() -> GameData {
    GameData {
        date: SaveDate {
            year: 0,
            month: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
        },
        extra_word: 0,
        entry: GameDataEntry {
            scenario_id: 0,
            random_seed: 0,
            save_position: 0,
            replay_target: 0,
            response_data: Vec::new(),
        },
        primary_entry: None,
        trailing_values: Vec::new(),
        byte_variables: Vec::new(),
    }
}
fn payload(d: &GameData) -> Vec<u64> {
    let date = d.date;
    let mut v = vec![
        1,
        u64::from(date.year),
        u64::from(date.month),
        u64::from(date.day),
        u64::from(date.hour),
        u64::from(date.minute),
        u64::from(date.second),
        d.primary_entry.is_some() as u64,
        u64::from(d.extra_word),
    ];
    for e in d.primary_entry.iter().chain(std::iter::once(&d.entry)) {
        v.extend([
            e.scenario_id as u32 as u64,
            u64::from(e.random_seed),
            u64::from(e.save_position),
            u64::from(e.replay_target),
            e.response_data.len() as u64,
        ]);
        v.extend(e.response_data.iter().map(|v| u64::from(*v)));
    }
    v.push(d.trailing_values.len() as u64);
    v.extend(d.trailing_values.iter().map(|v| u64::from(*v)));
    v.push(d.byte_variables.len() as u64);
    v.extend(d.byte_variables.iter().map(|v| u64::from(*v)));
    v
}
#[test]
#[ignore = "original First ctor/copied native decoded SaveData/progress/date/u16/response/readiness/mode0 transfer"]
fn higurashi_gameplay_loaded_bindings_match_native_first() {
    let golden: serde_json::Value = serde_json::from_slice(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/higurashi/native-save-codec-golden.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let mut protocol = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for p in golden["profiles"].as_array().unwrap() {
        let n = p["profile"].as_u64().unwrap();
        let raw = p["raw"].as_str().unwrap();
        let bytes = (0..raw.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&raw[i..i + 2], 16).unwrap())
            .collect();
        let saved = SaveData::decode_raw(bytes).unwrap();
        for slot in [0, 2, 99] {
            let game = if slot == 0 {
                saved.auto_save_slot.clone()
            } else {
                saved.manual_save_slots[slot - 1].clone()
            }
            .unwrap_or_else(fresh);
            let bindings = Bindings::new(&saved, &game).unwrap();
            let imported = bindings.game_data().unwrap();
            assert_eq!(imported, game);
            assert_eq!(bindings.progress.persistent().0, saved.persist_data.0);
            assert_eq!(
                bindings.progress.chart_unlocks.saved_words(),
                saved.save_vectors.extra_words_1
            );
            assert_eq!(
                bindings.history.bits().words(),
                saved.save_vectors.seen_messages
            );
            assert_eq!(
                bindings.choices.seen().words(),
                saved.save_vectors.select_seen_choices
            );
            assert_eq!(
                bindings.choices.last(),
                saved.save_vectors.select_last_choices
            );
            let mut published = saved.clone();
            bindings.publish_globals(&mut published);
            assert_eq!(published.save_vectors, saved.save_vectors);
            assert_eq!(bindings.autosave.valid, saved.auto_save_slot.is_some());
            if let Some(auto) = &saved.auto_save_slot {
                assert_eq!(bindings.autosave.packed_time, packed_date(auto.date));
            }
            protocol.push_str(&format!("N {n} {slot} 0\n"));
            owners += 1;
            let mut complete = false;
            for status in [None, Some(0), Some(1), Some(3), Some(2), Some(2)] {
                if let Some(s) = status {
                    protocol.push_str(&format!("U {s}\n"));
                    if s == 2 {
                        complete = true;
                    }
                }
                let mut v = vec![
                    1,
                    complete as u64,
                    complete as u64,
                    complete as u64,
                    complete as u64,
                    1,
                ];
                v.extend(payload(&imported));
                if complete {
                    v.extend(payload(&imported));
                } else {
                    v.push(0);
                }
                v.push(packed_date(imported.date));
                expected.push(v);
            }
            protocol.push_str("D\n");
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_GAMEPLAY_FIRST").unwrap())
        .args([
            "higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || input.write_all(protocol.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output_text = String::from_utf8(output.stdout).unwrap();
    let actual = output_text
        .lines()
        .filter_map(|l| l.strip_prefix("T "))
        .map(|l| {
            l.split_whitespace()
                .map(|v| v.parse::<u64>().unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    let mut words = 0;
    for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(a, e, "Native First loaded observation{i}");
        words += e.len();
    }
    assert_eq!(output_text.lines().filter(|l| *l == "D 1").count(), owners);
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    eprintln!(
        "Gameplay bindings:{owners} native owners/{} observations/{words} exact values;33 decoded save profiles/3 slots,both entries/all response tails,packed date and high u16 trailing values,loaded physical read/persist vectors,status exactly2,one-shot mode0 handoff and First retirement pass",
        expected.len()
    );
}
#[test]
#[ignore = "original11780/a9790 against full decoded save profiles, copied two-entry progress and retained auto-slot vectors"]
fn higurashi_live_autosave_publishes_native_slot_without_discarding_saved_data() {
    use crate::adv::higurashi::{
        autosave::{AutosaveCommand, AutosaveMemory, AutosaveServices},
        save_metadata::SaveMetadata,
    };
    struct Publication<'a> {
        saved: &'a mut SaveData,
        date: SaveDate,
        events: Vec<&'static str>,
    }
    impl AutosaveServices for Publication<'_> {
        fn packed_time(&mut self) -> Result<u64> {
            self.events.push("clock");
            Ok(packed_date(self.date))
        }
        fn captured(&mut self, memory: &AutosaveMemory) -> Result<()> {
            self.events.push("publish");
            bindings::publish_autosave(memory, self.saved);
            Ok(())
        }
        fn notify(&mut self, text: &str) -> Result<()> {
            assert_eq!(self.events, ["clock", "publish"]);
            assert_eq!(text, " ");
            assert_eq!(self.saved.auto_save_slot.as_ref().unwrap().date, self.date);
            self.events.push("notify");
            Ok(())
        }
    }
    let golden: serde_json::Value = serde_json::from_slice(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/higurashi/native-save-codec-golden.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let mut protocol = String::new();
    let mut expected = Vec::new();
    for profile in golden["profiles"].as_array().unwrap() {
        let n = profile["profile"].as_u64().unwrap() as u32;
        if n == 0 {
            continue;
        }
        let raw = profile["raw"].as_str().unwrap();
        let decoded = SaveData::decode_raw(
            (0..raw.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&raw[i..i + 2], 16).unwrap())
                .collect(),
        )
        .unwrap();
        let source = decoded.manual_save_slots[1].clone().unwrap();
        for boundary in [0, 1, 200, u32::MAX] {
            let mut saved = decoded.clone();
            let mut current = Bindings::new(&saved, &source).unwrap();
            let mut progress = current.progress.progress().clone();
            let date = SaveDate {
                year: (2026 + n % 3) as u16,
                month: (1 + n % 12) as u8,
                day: (1 + n % 28) as u8,
                hour: (n % 24) as u8,
                minute: (n % 60) as u8,
                second: ((n * 7) % 60) as u8,
            };
            let mut services = Publication {
                saved: &mut saved,
                date,
                events: Vec::new(),
            };
            let mut command = AutosaveCommand::new();
            assert!(
                command
                    .poll_live(
                        &mut progress,
                        &mut current.autosave,
                        boundary,
                        &SaveMetadata::new(),
                        &mut services
                    )
                    .unwrap()
            );
            assert_eq!(services.events, ["clock", "publish", "notify"]);
            drop(services);
            assert_eq!(current.game_data().unwrap().date, source.date);
            assert_eq!(saved.manual_save_slots, decoded.manual_save_slots);
            assert_eq!(saved.save_vectors, decoded.save_vectors);
            assert_eq!(saved.persist_data.0, decoded.persist_data.0);
            let slot = saved.auto_save_slot.as_ref().unwrap();
            assert_eq!(slot.entry.save_position, boundary);
            assert_eq!(slot.entry.replay_target, boundary);
            expected.push(payload(slot));
            protocol.push_str(&format!("{n} {boundary}\n"));
            let roundtrip = SaveData::decode(&saved.encode().unwrap()).unwrap();
            let mut encoded_slot = slot.clone();
            if encoded_slot.primary_entry.is_none() {
                encoded_slot.trailing_values.clear();
            }
            assert_eq!(roundtrip.auto_save_slot, Some(encoded_slot));
            assert_eq!(roundtrip.manual_save_slots, saved.manual_save_slots);
            assert_eq!(roundtrip.save_vectors, saved.save_vectors);
            assert_eq!(roundtrip.persist_data.0, saved.persist_data.0);
        }
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_LIVE_AUTOSAVE_SLOT").unwrap())
            .args([
                "higurashi/uncompressed_executable_update",
                "/tmp/higurashi-newrodin.fnt",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(protocol.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            line.strip_prefix("A ")
                .unwrap()
                .split_whitespace()
                .map(|v| v.parse::<u64>().unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    let mut words = 0;
    for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(a, e, "native auto-slot case{i}");
        words += e.len();
    }
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    eprintln!(
        "Live autosave:{} native cases/{words} exact values;32 full save profiles/4 boundaries,system calendar packing,both active entries/response tails,retained target u16/bytes,publication before notification,unchanged current date/manual slots/progress and native codec roundtrip pass",
        expected.len()
    );
}
pub(super) fn policy() -> Policy {
    Policy {
        animations_enabled: true,
        paused: false,
        fast_skip: false,
        suppress_transient_audio: false,
        read_flags: 0,
        text_speed: 100,
        outline_mode: 0,
    }
}
fn reads() -> ReadPolicy {
    ReadPolicy {
        modes: 0,
        forced: false,
        read_only: false,
    }
}
pub(super) fn audio() -> AudioFramePolicy {
    AudioFramePolicy {
        focus_mode_active: true,
        voice_focus_enabled: true,
        effects_update_enabled: true,
        music_rate: 1.0,
    }
}
#[test]
#[ignore = "actual boot/title/story request to fresh ADV,owned composed capture/adv atlas/no-VM resource gate/masked30 fade/real authored command;offscreen Metal and exact Loopback Audio"]
fn higurashi_gameplay_real_title_handoff_initializes_authored_vm() {
    use crate::{
        adv::higurashi::{
            reference_gpu_assets as gpu_assets, reference_render_to_texture as render_to_texture,
        },
        app::higurashi::{load_game::Paths, scene_host::SceneHost},
    };
    use shin_core::primitives::update::FrameId;
    let (device, queue, server) = gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
    let mut startup = SceneHost::new(
        server.clone(),
        manager,
        sounds,
        Arc::new(Paths {
            current: None,
            trial: None,
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut number = 0;
    let mut id = FrameId::default();
    let pointer = |number| PointerFrame {
        events: &[],
        number,
        time_seconds: number as f64 / 60.0,
    };
    let transition = loop {
        number += 1;
        id.advance();
        if let Some(t) = startup
            .update(
                id,
                Ticks::from_f32(3.0),
                Buttons {
                    pressed: 1 << 24,
                    ..Default::default()
                },
                &pointer(number),
                &queue,
            )
            .unwrap()
        {
            break t;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    let mut slot = title_slot::TitleSlot::new(transition, None);
    for _ in 0..300 {
        number += 1;
        slot.update_title(
            Ticks::from_f32(3.0),
            true,
            Buttons::default(),
            &pointer(number),
        )
        .unwrap();
        if slot.title.as_ref().unwrap().frame().logo_alpha == 1.0
            && slot.title.as_mut().unwrap().take_menu_request().is_some()
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..30 {
        number += 1;
        slot.update_title(
            Ticks::from_f32(3.0),
            true,
            Buttons::default(),
            &pointer(number),
        )
        .unwrap();
    }
    // Two actual A confirmations:Top story category,then its first story row.
    for _ in 0..2 {
        number += 1;
        slot.update_title(
            Ticks::ZERO,
            true,
            Buttons {
                pressed: 1 << 24,
                ..Default::default()
            },
            &pointer(number),
        )
        .unwrap();
        for _ in 0..70 {
            number += 1;
            slot.update_title(
                Ticks::from_f32(3.0),
                true,
                Buttons::default(),
                &pointer(number),
            )
            .unwrap();
            if slot.pending_story() {
                break;
            }
        }
    }
    while !slot.pending_story() {
        number += 1;
        slot.update_title(
            Ticks::from_f32(3.0),
            true,
            Buttons::default(),
            &pointer(number),
        )
        .unwrap();
        assert!(Instant::now() < deadline);
    }
    let retained_font = slot.title.as_ref().unwrap().globals.common.font.clone();
    let play_before = slot.title.as_ref().unwrap().globals.clock.ticks();
    let mut comparisons = 0;
    let mut composed = None;
    let black = render_to_texture(&device, &queue, (1280, 720), |context, _| {
        let mut frame = context.new_render_texture("Actual final title frame".into());
        {
            let mut pass = context.begin_pass(
                frame.as_texture_target(),
                Some(context.depth_stencil),
                "Title story blackout",
            );
            slot.title.as_ref().unwrap().render(&mut pass);
        }
        composed = Some(frame);
    });
    // Dispatch after the title's completed blackout frame,at the next platform boundary.
    render_to_texture(&device, &queue, (1280, 720), |context, _| {
        assert!(
            slot.dispatch_story(context, composed.as_ref().unwrap(), || [1, 2, 3, 4])
                .unwrap()
        );
    });
    assert!(slot.title.is_none());
    let game = slot.game.as_mut().unwrap();
    let autosave_date = SaveDate {
        year: 2026,
        month: 10,
        day: 12,
        hour: 11,
        minute: 42,
        second: 17,
    };
    let clock_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted_clock = clock_calls.clone();
    game.set_clock_source(move || {
        counted_clock.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(autosave_date)
    });
    assert!(Arc::ptr_eq(&game.globals.common.font, &retained_font));
    assert_eq!(game.flags, 0);
    assert!(!game.quick_return);
    assert!(game.host().unwrap().runner.native_read_state().is_none());
    assert!(game.atlas.is_none());
    for size in [(1280, 720), (1920, 1080)] {
        // Each offscreen helper owns a distinct resize source;recreate caches
        // when replacing that source,as a new platform render context would.
        game.screens = None;
        let pixels = render_to_texture(&device, &queue, size, |context, target| {
            game.prepare(context).unwrap();
            let mut pass = context.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    view: target,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Pending story capture",
            );
            game.render(&mut pass).unwrap();
        });
        assert!(pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
        comparisons += 1;
    }
    let _ = black;
    let mut notification_comparisons = 0;
    let verify_notification = |game: &mut Gameplay| {
        assert!(game.notification_busy());
        assert!(game.host().unwrap().ui.indicator.transition.value() > 0.0);
        for size in [(1280, 720), (1920, 1080)] {
            let mut images = Vec::new();
            for enabled in [false, true] {
                game.globals.data.settings.common.v17_showscenetitle = enabled;
                game.screens = None;
                images.push(render_to_texture(&device,&queue,size,|context,target| {
                    game.prepare(context).unwrap();
                    let mut pass=context.begin_pass(shin_render::shaders::types::texture::TextureTarget {view:target,kind:shin_render::shaders::types::texture::TextureTargetKind::RenderTexture},Some(context.depth_stencil),"Actual authored notification");
                    game.render(&mut pass).unwrap();
                }));
                assert!(game.notification_busy());
            }
            assert!(
                images[0] != images[1],
                "real autosave title must draw at{size:?}"
            );
        }
        2
    };
    let mut constructing = true;
    let mut fading = false;
    loop {
        number += 1;
        let result = shin_tasks::block_on(game.update(
            Ticks::from_f32(3.0),
            true,
            Buttons {
                pressed: 1 << 24,
                ..Default::default()
            },
            &pointer(number),
            policy(),
            audio(),
        ))
        .unwrap();
        if notification_comparisons == 0 && game.globals.data.auto_save_slot.is_some() {
            notification_comparisons = verify_notification(game);
        }
        if constructing && game.atlas.is_some() {
            constructing = false;
            assert!(game.host().unwrap().runner.native_read_state().is_none());
            assert_eq!(game.host().unwrap().runner.completed_commands(), 0);
        }
        if let Some(overlay) = game.host().unwrap().runner.reconstruction_overlay() {
            fading |= overlay.clock.active;
        }
        if game.host().unwrap().runner.native_read_state().is_some() {
            assert!(!game.input_permission);
            assert!(matches!(
                result,
                Status::Waiting | Status::External | Status::CommandBudget
            ));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(fading);
    assert!(game.globals.clock.ticks() > play_before);
    assert_eq!(game.game_data().unwrap().entry.scenario_id, 0);
    assert_eq!(game.game_data().unwrap().date, fresh().date);
    for _ in 0..300 {
        number += 1;
        let status = shin_tasks::block_on(game.update(
            Ticks::from_f32(3.0),
            true,
            Buttons::default(),
            &pointer(number),
            policy(),
            audio(),
        ))
        .unwrap();
        if notification_comparisons == 0 && game.globals.data.auto_save_slot.is_some() {
            notification_comparisons = verify_notification(game);
        }
        if status == Status::External
            || game
                .host()
                .unwrap()
                .runner
                .frame()
                .message
                .glyphs()
                .is_some()
        {
            eprintln!(
                "Actual authored story boundary:{status:?} pending{:?}",
                game.host().unwrap().runner.pending_command()
            );
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(game.host().unwrap().runner.completed_commands() > 0);
    // AUTOSAVE and first MSGSET each call11780; only AUTOSAVE shows a notice.
    assert_eq!(clock_calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    let auto = game.globals.data.auto_save_slot.as_ref().unwrap().clone();
    assert_eq!(auto.date, autosave_date);
    assert_eq!(auto.entry.scenario_id, 0);
    assert_eq!(auto.entry.save_position, auto.entry.replay_target);
    assert!(auto.entry.save_position > 0);
    assert_eq!(
        game.bindings.autosave.packed_time,
        packed_date(autosave_date)
    );
    assert_eq!(
        auto.entry.save_position,
        game.bindings.autosave.progress.active_entry().save_position
    );
    assert_eq!(
        auto.entry.response_data,
        game.bindings.autosave.progress.active_entry().response_data
    );
    let labels = game
        .host()
        .unwrap()
        .runner
        .native_save_metadata()
        .unwrap()
        .saved();
    assert_eq!(
        game.notification.text,
        format!("{} {}", labels[0], labels[1])
    );
    assert_eq!(notification_comparisons, 2);
    comparisons += notification_comparisons;
    assert!(game.bindings.memory.extra_words.contains(&0));
    assert_eq!(game.globals.data.save_vectors.extra_words_1[0] & 1, 1);
    assert_eq!(
        game.globals.data.save_vectors.extra_words_1,
        game.bindings.progress.chart_unlocks.saved_words()
    );
    assert_eq!(
        game.host()
            .unwrap()
            .runner
            .frame()
            .message
            .playback
            .message_id,
        1578
    );
    assert!(matches!(
        game.host().unwrap().runner.pending_command(),
        Some(shin_core::vm::higurashi::command::HigurashiRuntimeCommand::MSGSET { .. })
    ));
    for _ in 0..45 {
        number += 1;
        assert_eq!(
            shin_tasks::block_on(game.update(
                Ticks::from_f32(3.0),
                true,
                Buttons::default(),
                &pointer(number),
                policy(),
                audio()
            ))
            .unwrap(),
            Status::Waiting
        );
    }
    assert_eq!(
        game.host()
            .unwrap()
            .runner
            .frame()
            .message
            .playback
            .message_id,
        1578
    );
    for size in [(1280, 720), (1920, 1080)] {
        game.screens = None;
        let pixels = render_to_texture(&device, &queue, size, |context, target| {
            game.prepare(context).unwrap();
            let mut pass = context.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    view: target,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Actual first story message",
            );
            game.render(&mut pass).unwrap();
        });
        assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
        assert!(
            pixels
                .chunks_exact(4)
                .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0),
            "actual message scene must draw at{size:?}"
        );
        comparisons += 1;
    }
    // Both root requests attach their existing modal owner during preparation.
    // They must retain commands while queued and keep the loaded game/global
    // owners unchanged through opening, B cancellation and pruning.
    use crate::adv::higurashi::{host::browser::Notice, input::Request};
    let before_menus = game.host().unwrap().runner.completed_commands();
    let saved_before_menus = game.game_data().unwrap();
    for (bit, request, opening_frames) in [
        (15, Request::SystemMenu, 60),
        (12, Request::MessageBrowser, 200),
    ] {
        number += 1;
        shin_tasks::block_on(game.update(
            Ticks::from_f32(3.0),
            true,
            Buttons {
                pressed: 1 << bit,
                ..Default::default()
            },
            &pointer(number),
            policy(),
            audio(),
        ))
        .unwrap();
        assert_eq!(game.pending_input_request(), Some(request));
        assert_eq!(
            game.host().unwrap().runner.completed_commands(),
            before_menus
        );
        assert_eq!(game.host().unwrap().modal_pause_depth(), 0);
        game.controls.state.temporary_indicator = 7;
        let opening_cues = game.globals.sounds.lock().sounds.len();
        game.screens = None;
        render_to_texture(&device, &queue, (1280, 720), |context, _| {
            game.prepare(context).unwrap();
        });
        assert_eq!(game.pending_input_request(), None);
        assert_eq!(game.controls.state.temporary_indicator, 0);
        assert_eq!(game.globals.sounds.lock().sounds.len(), opening_cues + 1);
        assert!(game.host().unwrap().has_modes());
        assert_eq!(game.host().unwrap().modal_pause_depth(), 1);
        assert!(game.host().unwrap().audio.modal_effects_paused());
        assert!(!game.input_permission);
        assert_eq!(game.game_data().unwrap(), saved_before_menus);
        for _ in 0..opening_frames {
            number += 1;
            shin_tasks::block_on(game.update(
                Ticks::from_f32(3.0),
                true,
                Buttons::default(),
                &pointer(number),
                policy(),
                audio(),
            ))
            .unwrap();
            assert_eq!(
                game.host().unwrap().runner.completed_commands(),
                before_menus
            );
            assert_eq!(game.host().unwrap().modal_pause_depth(), 1);
            if request == Request::MessageBrowser {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert!(game.take_input_flush());
        for size in [(1280, 720), (1920, 1080)] {
            game.screens = None;
            let pixels = render_to_texture(&device, &queue, size, |context, target| {
                game.prepare(context).unwrap();
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Actual Gameplay root menu",
                );
                game.render(&mut pass).unwrap();
            });
            assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
            assert!(
                pixels
                    .chunks_exact(4)
                    .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
            );
            comparisons += 1;
        }
        if request == Request::MessageBrowser {
            let notices = game.host_mut().unwrap().take_browser_notifications();
            assert_eq!(notices.len(), 1);
            assert!(matches!(notices[0].notice, Notice::Availability(true)));
        }
        number += 1;
        shin_tasks::block_on(game.update(
            Ticks::from_f32(3.0),
            true,
            Buttons {
                pressed: 1 << 25,
                ..Default::default()
            },
            &pointer(number),
            policy(),
            audio(),
        ))
        .unwrap();
        for _ in 0..80 {
            number += 1;
            shin_tasks::block_on(game.update(
                Ticks::from_f32(3.0),
                true,
                Buttons::default(),
                &pointer(number),
                policy(),
                audio(),
            ))
            .unwrap();
            if !game.host().unwrap().has_modes() && game.host().unwrap().modal_snapshot().is_none()
            {
                break;
            }
        }
        assert!(
            !game.host().unwrap().has_modes(),
            "{request:?} must finish B cancellation"
        );
        assert_eq!(game.host().unwrap().modal_pause_depth(), 0);
        assert!(!game.host().unwrap().audio.modal_effects_paused());
        assert!(game.host().unwrap().modal_snapshot().is_none());
        assert_eq!(game.game_data().unwrap(), saved_before_menus);
        assert_eq!(
            game.host().unwrap().runner.completed_commands(),
            before_menus
        );
        assert!(Arc::ptr_eq(&game.globals.common.font, &retained_font));
    }
    eprintln!(
        "Actual Gameplay root menus:SystemMenu/MessageBrowser requests,prepared capture,counted pause/SE,lower input flush,720p/1080p output,B cancel and full owner/global return pass"
    );
    let before_confirm = game.host().unwrap().runner.completed_commands();
    number += 1;
    let mut last_status = shin_tasks::block_on(game.update(
        Ticks::from_f32(3.0),
        true,
        Buttons {
            pressed: 1 << 24,
            ..Default::default()
        },
        &pointer(number),
        policy(),
        audio(),
    ))
    .unwrap();
    if !game.bindings.history.contains(1578) {
        eprintln!(
            "Actual first A retained message:read{:?}/flags{}/time{}",
            game.host().unwrap().runner.native_read_state(),
            game.host().unwrap().runner.frame().message.playback.flags,
            game.host()
                .unwrap()
                .runner
                .frame()
                .message
                .playback
                .current_time
        );
        number += 1;
        shin_tasks::block_on(game.update(
            Ticks::from_f32(3.0),
            true,
            Buttons::default(),
            &pointer(number),
            policy(),
            audio(),
        ))
        .unwrap();
        number += 1;
        last_status = shin_tasks::block_on(game.update(
            Ticks::from_f32(3.0),
            true,
            Buttons {
                pressed: 1 << 24,
                ..Default::default()
            },
            &pointer(number),
            policy(),
            audio(),
        ))
        .unwrap();
    }
    assert!(game.bindings.history.contains(1578));
    assert!(game.host().unwrap().runner.completed_commands() > before_confirm);
    let mut control_frames = 0;
    if last_status != Status::External {
        for bit in [10, 25, 11] {
            number += 1;
            last_status = shin_tasks::block_on(game.update(
                Ticks::from_f32(3.0),
                true,
                Buttons {
                    pressed: 1 << bit,
                    ..Default::default()
                },
                &pointer(number),
                policy(),
                audio(),
            ))
            .unwrap();
            control_frames += 1;
            assert_eq!(
                game.controls.state.modes,
                match bit {
                    10 => 1,
                    25 => 0,
                    _ => 2,
                }
            );
        }
        if let Some(vm) = game.host().unwrap().runner.native_read_state() {
            if vm.is_read == 0 {
                assert!(!crate::adv::higurashi::input::suppress_transient_audio(
                    Some(vm),
                    2,
                    false,
                    true
                ));
            }
        }
        game.globals.data.settings.common.v11_disallowskipunread = false;
        let before_skip = game.host().unwrap().runner.completed_commands();
        for _ in 0..320 {
            if last_status == Status::External {
                break;
            }
            number += 1;
            last_status = shin_tasks::block_on(game.update(
                Ticks::from_f32(3.0),
                true,
                Buttons::default(),
                &pointer(number),
                policy(),
                audio(),
            ))
            .unwrap();
            control_frames += 1;
            if game.host().unwrap().runner.completed_commands() > before_skip + 100 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            game.host().unwrap().runner.completed_commands() > 57,
            "Real story must resume beyond the supported picture750 loader"
        );
    }
    eprintln!(
        "Actual controller continuation:{control_frames} frames/status{last_status:?}/completed{} pending{:?}",
        game.host().unwrap().runner.completed_commands(),
        game.host().unwrap().runner.pending_command()
    );
    let auto = game.globals.data.auto_save_slot.as_ref().unwrap().clone();
    let clock = game.globals.clock.ticks();
    let data = game.game_data().unwrap();
    let globals = slot.game.take().unwrap().into_globals();
    assert_eq!(globals.clock.ticks(), clock);
    assert_eq!(globals.data.auto_save_slot, Some(auto));
    assert_eq!(globals.data.save_vectors.extra_words_1[0] & 1, 1);
    assert_eq!(data.entry.scenario_id, 0);
    // Independent command stream checks that native variable continuations
    // finish before WAIT gets its single clamped frame tick.
    let mut raw = globals.common.scenario.raw().to_vec();
    let start = globals.common.scenario.header.code_offset as usize;
    let code = [0x82, 1, 0xb0, 0x81, 31, 0, 1, 0x83, 1, 6, 0, 0, 0];
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(
        shin_core::format::scenario::higurashi::HigurashiScenario::new(raw.into()).unwrap(),
    );
    let runner = shin_tasks::block_on(HigurashiRunner::load(
        server.clone(),
        scenario.clone(),
        42,
        7,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    let mut host = HigurashiHost::new(
        runner,
        crate::adv::higurashi::audio::HigurashiAudio::new(
            globals.manager.clone(),
            server.clone(),
            scenario,
        ),
    );
    let mut input = fresh();
    input.entry.scenario_id = 42;
    let mut bindings = Bindings::new(&SaveData::default(), &input).unwrap();
    let status = shin_tasks::block_on(host.update_native_with_external(
        Ticks::from_f32(120.0),
        policy(),
        reads(),
        &mut bindings,
        audio(),
        8,
        100000,
        || [1, 2, 3, 4],
        |runner, bindings, _| bindings.poll_variables(runner),
    ))
    .unwrap();
    assert_eq!(status, Status::Waiting);
    // WAIT is still pending;only its two variable predecessors are complete.
    assert_eq!(host.runner.completed_commands(), 2);
    assert_eq!(bindings.progress.pending_persistent(), [(1, 42)]);
    assert!(matches!(
        host.runner.pending_command(),
        Some(shin_core::vm::higurashi::command::HigurashiRuntimeCommand::WAIT { amount: 6, .. })
    ));
    for expected in [Status::Waiting, Status::Exited] {
        let status = shin_tasks::block_on(host.update_native_with_external(
            Ticks::from_f32(3.0),
            policy(),
            reads(),
            &mut bindings,
            audio(),
            8,
            100000,
            || [1, 2, 3, 4],
            |runner, bindings, _| bindings.poll_variables(runner),
        ))
        .unwrap();
        assert_eq!(status, expected);
    }
    // Dropping an unpublished reconstruction owns its compute/decoder scope.
    let mut snapshot = None;
    render_to_texture(&device, &queue, (1280, 720), |context, _| {
        let mut frame = context.new_render_texture("Pending story retirement capture".into());
        {
            let mut pass = context.begin_pass(
                frame.as_texture_target(),
                Some(context.depth_stencil),
                "Black capture",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
        }
        snapshot = Some(frame);
    });
    let mut pending =
        Gameplay::new(globals, fresh(), 0, snapshot.unwrap(), || [1, 2, 3, 4]).unwrap();
    while pending.atlas.is_none() {
        number += 1;
        shin_tasks::block_on(pending.update(
            Ticks::ZERO,
            true,
            Buttons::default(),
            &pointer(number),
            policy(),
            audio(),
        ))
        .unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(pending.host().unwrap().runner.native_read_state().is_none());
    assert!(
        pending
            .host()
            .unwrap()
            .runner
            .reconstruction_phase()
            .is_some()
    );
    drop(pending);
    eprintln!(
        "Gameplay title handoff:{comparisons}720p/1080p captures/notification setting pairs/first authored message1578 outputs,retired title/common ownership,adv atlas gate/no initial VM,mode0 masked30 fade/input exclusion,real AUTOSAVE and CHART node/global unlock publication,first Japanese MSGSET retained after135 native ticks,loaded auto-slot/global clock/audio return,variable continuations before one clamped WAIT tick and unpublished reconstruction retirement pass"
    );
}
