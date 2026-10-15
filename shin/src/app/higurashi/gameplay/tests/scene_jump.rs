use super::*;
use crate::adv::higurashi::{
    audio::HigurashiAudio, host::browser::Notice, music_commands::MusicCommands,
    reference_gpu_assets, reference_render_to_texture,
};
use shin_core::{format::scenario::higurashi::HigurashiScenario, primitives::color::UnormColor};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::sync::atomic::Ordering;

fn update(slot: &mut title_slot::TitleSlot, number: &mut u32, pressed: u32, repeat: u32) {
    *number += 1;
    shin_tasks::block_on(slot.update_game(
        Ticks::from_f32(3.0),
        true,
        Buttons {
            pressed,
            repeat,
            ..Default::default()
        },
        &PointerFrame {
            events: &[],
            number: *number,
            time_seconds: *number as f64 / 60.0,
        },
        policy(),
        audio(),
    ))
    .unwrap();
}
fn marker(pass: &mut RenderPass) {
    use shin_render::{
        DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
        shaders::types::{buffer::VertexSource, vertices::PosColVertex},
    };
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

#[test]
#[ignore = "actual Gameplay backlog Yes/deferred whole-scene replacement/First gate/global owners/masked fade at720p/1080p;offscreen Metal/exact Loopback Audio"]
fn higurashi_gameplay_browser_jump_preserves_loaded_globals_and_first_gate() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let authored: Arc<HigurashiScenario> = server.load_sync("/main.snr").unwrap();
    let mut code = authored.raw()[0x2545e..0x25465].to_vec(); // Existing native-audited BGM command.
    for (i, c) in [b'A', b'B', b'C'].into_iter().enumerate() {
        code.push(0x86);
        code.extend(((i + 1) as u32).to_le_bytes()[..3].iter());
        code.extend([1, 0x7f, 4, 0, b'@', b'r', c, 0]);
    }
    code.extend([0, 0, 0]);
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let common = Arc::new(CommonAssets {
        scenario: scenario.clone(),
        font: server.load_sync("/newrodin.fnt").unwrap(),
        system: server.load_sync("/systex.txa").unwrap(),
        message: server.load_sync("/msgtex.txa").unwrap(),
        effects: server.load_sync("/effects.txa").unwrap(),
        sysse: server.load_sync("/sysse.bin").unwrap(),
    });
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
    let mut comparisons = 0;
    for size in [(1280, 720), (1920, 1080)] {
        let date = SaveDate {
            year: 2025,
            month: 4,
            day: 3,
            hour: 2,
            minute: 1,
            second: 19,
        };
        let mut data = fresh();
        data.date = date;
        data.extra_word = 77;
        data.entry.random_seed = 42;
        data.entry.save_position = 2;
        data.entry.replay_target = 2;
        data.entry.response_data = vec![0x23; 64];
        data.primary_entry = Some(GameDataEntry {
            scenario_id: 10,
            random_seed: 91,
            save_position: 99,
            replay_target: 99,
            response_data: vec![0x75; 67],
        });
        data.trailing_values = vec![4, 0x1234, 0xfffe];
        data.byte_variables = vec![3; 64];
        let mut saved = SaveData::default();
        saved.manual_save_slots[2] = Some(data.clone());
        saved.save_vectors.select_seen_choices = vec![0x123, 0, 0x8000];
        saved.save_vectors.seen_messages = vec![0, 0, 0x123];
        saved.save_vectors.select_last_choices = vec![0, 3, 0, 15];
        saved.save_vectors.extra_words_1 = vec![0x10, 0, 0x80000000];
        saved.settings.common.v11_disallowskipunread = false;
        saved.settings.common.v3_sysvol = 53;
        saved.settings.common.v2_voicevol = 37;
        saved.settings.extra_flag = true;
        let globals = Globals {
            storage: None,
            runtime_saves: None,
            common: common.clone(),
            data: saved.clone(),
            clock: PlayClock::from_saved_seconds(123),
            platform_flag: true,
            last_title_mode: 7,
            audio: HigurashiAudio::new(manager.clone(), server.clone(), authored.clone()),
            server: server.clone(),
            manager: manager.clone(),
            sounds: sounds.clone(),
        };
        let mut game = None;
        reference_render_to_texture(&device, &queue, size, |context, _| {
            let mut initial = context.new_render_texture("Initial Gameplay platform frame".into());
            {
                let mut pass = context.begin_pass(
                    initial.as_texture_target(),
                    Some(context.depth_stencil),
                    "Initial Gameplay",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            }
            game = Some(Gameplay::new(globals, data.clone(), 0, initial, || [1, 2, 3, 4]).unwrap());
        });
        let mut game = game.unwrap();
        let copies = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = copies.clone();
        game.set_clock_source(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(date)
        });
        let shared_read = game.bindings.history.clone();
        let shared_chart = game.bindings.progress.chart_unlocks.clone();
        let mut slot = title_slot::TitleSlot::from_game(game);
        let mut number = 0;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            update(&mut slot, &mut number, 0, 0);
            let game = slot.game.as_ref().unwrap();
            if game
                .host()
                .unwrap()
                .runner
                .frame()
                .message
                .playback
                .message_id
                == 1
                && game.host().unwrap().runner.reconstruction_phase().is_none()
            {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        for _ in 0..20 {
            update(&mut slot, &mut number, 0, 0);
        }
        assert_eq!(
            slot.game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .runner
                .native_message_timeline()
                .unwrap()
                .boundary,
            2
        );
        update(&mut slot, &mut number, 1 << 12, 0);
        assert_eq!(
            slot.game.as_ref().unwrap().pending_input_request(),
            Some(crate::adv::higurashi::input::Request::MessageBrowser)
        );
        reference_render_to_texture(&device, &queue, size, |context, _| {
            slot.game.as_mut().unwrap().prepare(context).unwrap();
        });
        for _ in 0..80 {
            update(&mut slot, &mut number, 0, 0);
            std::thread::sleep(Duration::from_millis(1));
        }
        update(&mut slot, &mut number, 0, 1 << 4); // Previous row, then scene jump.
        assert_eq!(
            slot.game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .reference_browser_selected(),
            Some(0)
        );
        let lifetime = slot
            .game
            .as_ref()
            .unwrap()
            .host()
            .unwrap()
            .reference_browser_lifetime()
            .unwrap();
        update(&mut slot, &mut number, 1 << 15, 0);
        loop {
            update(&mut slot, &mut number, 0, 0);
            if slot
                .game
                .as_mut()
                .unwrap()
                .host_mut()
                .unwrap()
                .reference_browser_question_ready()
            {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        for _ in 0..14 {
            update(&mut slot, &mut number, 0, 0);
        }
        update(&mut slot, &mut number, 0, 1 << 4); // No -> Yes.
        update(&mut slot, &mut number, 1 << 24, 0);
        loop {
            update(&mut slot, &mut number, 0, 0);
            if slot.pending_gameplay_replacement() {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let old = slot.game.as_mut().unwrap();
        assert_eq!(old.host().unwrap().modal_pause_depth(), 1);
        let notices = old.host_mut().unwrap().take_browser_notifications();
        assert_eq!(notices.len(), 1);
        assert!(matches!(notices[0].notice, Notice::Availability(true)));
        let old_data = old.game_data().unwrap();
        old.bindings.autosave.progress.extra_word = 0xdeadbeef;
        old.bindings.autosave.progress.entries[1].response_data = vec![0xf1, 0x55, 0x23, 0xee];
        let old_autosave = old.bindings.autosave.clone();
        let old_autosave_words = old.bindings.autosave.progress.entries[1]
            .response_data
            .as_ptr();
        let old_ticks = old.globals.clock.ticks();
        let old_music = old.host().unwrap().audio.bgm_clock();
        let ids = old.host().unwrap().next_mode_ids();
        let old_job = old.job.as_ref().unwrap().retirement.clone();
        let old_read_words = old.bindings.history.bits().words().as_ptr();
        let old_choice_words = old.bindings.choices.seen().words().as_ptr();
        let old_choice_last = old.bindings.choices.last().as_ptr();
        let draw_old =
            |game: &mut Gameplay, context: &mut PreRenderContext, target: TextureTarget| {
                game.screens = None;
                game.prepare(context).unwrap();
                let mut pass = context.begin_pass(
                    target,
                    Some(context.depth_stencil),
                    "Composed Gameplay backlog",
                );
                game.render(&mut pass).unwrap();
                marker(&mut pass);
            };
        let expected = reference_render_to_texture(&device, &queue, size, |context, target| {
            draw_old(
                slot.game.as_mut().unwrap(),
                context,
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
            );
        });
        let actual = reference_render_to_texture(&device, &queue, size, |context, target| {
            let mut composed = context.new_render_texture("Full old Gameplay frame".into());
            draw_old(
                slot.game.as_mut().unwrap(),
                context,
                composed.as_texture_target(),
            );
            assert!(
                slot.dispatch_gameplay_replacement(context, &composed)
                    .unwrap()
            );
            let new = slot.game.as_mut().unwrap();
            new.prepare(context).unwrap();
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Fresh Gameplay captured frame",
            );
            new.render(&mut pass).unwrap();
        });
        assert_eq!(
            actual, expected,
            "Full Gameplay snapshot changed at{size:?}"
        );
        comparisons += 1;
        assert!(lifetime.0.load(Ordering::Acquire));
        assert!(lifetime.1.iter().all(|h| h.is_finished()));
        assert!(old_job.is_finished());
        assert!(!slot.pending_gameplay_replacement());
        let new = slot.game.as_ref().unwrap();
        assert_eq!(new.bindings.autosave, old_autosave);
        assert_eq!(
            new.bindings.autosave.progress.entries[1]
                .response_data
                .as_ptr(),
            old_autosave_words
        );
        assert_eq!(new.globals.clock.ticks(), old_ticks);
        assert!(Arc::ptr_eq(&new.globals.common, &common));
        assert!(Arc::ptr_eq(&new.globals.sounds, &sounds));
        assert!(new.globals.platform_flag);
        assert_eq!(new.globals.last_title_mode, 7);
        assert_eq!(new.host().unwrap().next_mode_ids(), ids);
        assert_eq!(new.bindings.history.bits().words().as_ptr(), old_read_words);
        assert_eq!(
            new.bindings.choices.seen().words().as_ptr(),
            old_choice_words
        );
        assert_eq!(new.bindings.choices.last().as_ptr(), old_choice_last);
        assert_eq!(
            new.bindings.choices.seen().words(),
            saved.save_vectors.select_seen_choices
        );
        assert_eq!(
            new.bindings.choices.last(),
            saved.save_vectors.select_last_choices
        );
        assert_eq!(new.host().unwrap().modal_pause_depth(), 0);
        assert!(new.host().unwrap().runner.native_read_state().is_none());
        assert!(new.atlas.is_none() && new.job.is_some() && new.snapshot.is_some());
        assert_eq!(new.game_data().unwrap().entry.save_position, 1);
        assert_eq!(new.game_data().unwrap().date, old_data.date);
        assert_eq!(
            new.game_data().unwrap().primary_entry,
            old_data.primary_entry
        );
        assert_eq!(
            new.game_data().unwrap().trailing_values,
            old_data.trailing_values
        );
        assert_eq!(
            new.game_data().unwrap().byte_variables,
            old_data.byte_variables
        );
        assert_eq!(new.globals.data.manual_save_slots[2], Some(data.clone()));
        let calls = copies.load(Ordering::SeqCst);
        loop {
            update(&mut slot, &mut number, 0, 0);
            let new = slot.game.as_ref().unwrap();
            if new.host().unwrap().runner.reconstruction_phase().is_none()
                && new
                    .host()
                    .unwrap()
                    .runner
                    .frame()
                    .message
                    .playback
                    .message_id
                    == 0
            {
                break;
            }
            assert!(!new.input_permission);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let new = slot.game.as_mut().unwrap();
        assert_eq!(
            new.host()
                .unwrap()
                .runner
                .native_message_timeline()
                .unwrap()
                .boundary,
            1
        );
        assert_eq!(
            new.host()
                .unwrap()
                .runner
                .native_playback_descriptors()
                .unwrap()
                .message
                .text
                .as_deref(),
            Some("@rA")
        );
        assert!(new.globals.clock.ticks() > old_ticks);
        assert!(
            copies.load(Ordering::SeqCst) > calls,
            "Original clock owner must survive replacement"
        );
        assert_eq!(
            shared_chart.saved_words(),
            new.bindings.progress.chart_unlocks.saved_words()
        );
        new.bindings
            .progress
            .chart_unlocks
            .construct(1, &[31], &mut Vec::new());
        assert_eq!(shared_chart.saved_words()[0], 0x80000010);
        if old_music > 0 {
            assert!(new.host().unwrap().audio.bgm_clock() >= old_music);
        }
        for _ in 0..15 {
            update(&mut slot, &mut number, 0, 0);
        }
        update(&mut slot, &mut number, 1 << 24, 0);
        update(&mut slot, &mut number, 0, 0);
        update(&mut slot, &mut number, 1 << 24, 0);
        assert!(
            slot.game.as_ref().unwrap().bindings.history.contains(0),
            "Retained read-bank owner must observe new completion"
        );
        assert_eq!(
            slot.game
                .as_ref()
                .unwrap()
                .globals
                .data
                .save_vectors
                .seen_messages[0]
                & 1,
            1
        );
        assert!(
            !shared_read.contains(0),
            "Worker history snapshots stay isolated"
        );
    }
    eprintln!(
        "Gameplay browser jumps:{comparisons} byte-identical720p/1080p full-frame transfers;actual Yes/deferred ownership,old workers retired,new First atlas/no-VM gate/mode0 fade,loaded date/u16/bytes/inactive response tails/choices/manual slot/global font/audio/clock/chart/read banks survive and first selected message resumes"
    );
}
