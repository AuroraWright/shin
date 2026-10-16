use super::*;
use crate::{
    adv::{Adv, AdvAssets, chapter_navigation::Direction},
    app::AppAction,
    asset::system::AssetServer,
    layer::{DrawableLayer, Layer},
    render::PreRenderContext,
    update::UpdateContext,
};
use enum_map::EnumMap;
use glam::{Vec2, vec2};
use shin_core::{
    format::save::{GameData, RouteData},
    primitives::update::FrameId,
    time::Ticks,
};
use shin_input::ActionState;
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

// Focused actual ADV confirmation/input and mode0 publication checks. Reuses
// the supplied-resource offscreen fixture without repeating its other tests.
#[test]
#[ignore = "requires supplied ROMs, Metal and exact Loopback Audio"]
fn reference_chapter_confirmation_and_scene_installation() {
    assert!(std::env::var_os("SHIN_REFERENCE_CHAPTER_NAVIGATION_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-chapter-navigation".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

fn game(boundary: u32) -> GameData {
    let route: RouteData = serde_json::from_value(serde_json::json!(vec![0u8; 1024])).unwrap();
    let mut game = GameData::from_entry(GameDataEntry {
        scenario_id: 0,
        random_seed: 12345,
        save_position: boundary,
        route_data: route,
    });
    game.primary_entry = Some(GameDataEntry {
        scenario_id: 3,
        random_seed: 0xfeed1234,
        save_position: 77,
        route_data: serde_json::from_value(serde_json::json!([0x93, 0x51, 0x72, 0xa8, 6]))
            .unwrap(),
    });
    game
}
fn action(action: AppAction) -> EnumMap<AppAction, ActionState> {
    let mut input: EnumMap<AppAction, ActionState> = EnumMap::default();
    input[action].is_clicked = true;
    input[action].is_clicked_or_repeated = true;
    input
}
fn flush(context: &mut PreRenderContext) {
    let encoder = std::mem::replace(
        context.encoder,
        context.device.create_command_encoder(&Default::default()),
    );
    let mut upload = context.device.create_command_encoder(&Default::default());
    context.dynamic_buffer.finish(&mut upload);
    context.queue.submit([upload.finish(), encoder.finish()]);
    context.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
}
fn navigate<'a>(
    live: &mut Adv,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<AssetServer>,
    frame: &mut FrameId,
    input: &EnumMap<AppAction, ActionState>,
    ticks: f32,
) -> bool {
    frame.advance();
    let changed = live.update_chapter_navigation(
        &mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: *frame,
            delta_ticks: Ticks::from_f32(ticks),
            asset_server: server,
            pre_render: context,
        },
        input,
    );
    flush(context);
    changed
}
fn finish_load<'a>(
    live: &mut Adv,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<AssetServer>,
    frame: &mut FrameId,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while live.pending_restore.is_some() {
        assert!(Instant::now() < deadline, "Chapter load timeout");
        frame.advance();
        live.update_saved_scene_load(&mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: *frame,
            delta_ticks: Ticks::ZERO,
            asset_server: server,
            pre_render: context,
        });
        flush(context);
        if live.pending_restore.is_some() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    assert!(
        live.saved_scene_load_error().is_none(),
        "{:?}",
        live.saved_scene_load_error()
    );
}
fn pointer(live: &mut Adv, position: Vec2, held: bool, elapsed: f64) {
    live.sample_modal_pointer(position, (1920, 1080), held, elapsed);
    live.handle_pointer_state(position, false, held, elapsed);
    live.handle_input(Default::default(), true);
}
fn open<'a>(
    live: &mut Adv,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<AssetServer>,
    frame: &mut FrameId,
    direction: Direction,
) {
    live.handle_input(
        action(match direction {
            Direction::Next => AppAction::NextChapter,
            Direction::Previous => AppAction::PreviousChapter,
        }),
        true,
    );
    assert_eq!(live.pending_chapter, Some(direction));
    assert!(!navigate(
        live,
        context,
        server,
        frame,
        &Default::default(),
        1.0
    ));
    assert!(live.chapter_dialog.is_some());
    assert_eq!(
        live.adv_state.audio_pause_count, 0,
        "Native chapter dialog leaves audio running"
    );
    assert_eq!(live.adv_state.ui_hide_count, 0);
    assert!(!live.adv_state.playback_modes.skip && !live.adv_state.playback_modes.autoplay);
    for _ in 0..15 {
        navigate(live, context, server, frame, &Default::default(), 1.0);
    }
    assert!(live.chapter_dialog.as_ref().unwrap().1.reference_is_open());
    // Exercise the ordinary ADV frame path as well as the task callback:
    // CTRL held behind a chapter dialog must neither skip nor advance it.
    let source = live.vm_state.save_position;
    if source == 326 {
        let mut held: EnumMap<AppAction, ActionState> = EnumMap::default();
        held[AppAction::HoldSkip].is_held = true;
        live.handle_input(held.clone(), true);
        frame.advance();
        live.update(
            &mut UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: *frame,
                delta_ticks: Ticks::from_f32(1.0),
                asset_server: server,
                pre_render: context,
            },
            held,
        );
        flush(context);
        assert_eq!(live.vm_state.save_position, source);
        assert!(!live.held_skip);
        assert!(!live.skip_indicator_requested());
    }
}
fn verify_terminal_chapter<'a>(
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<AssetServer>,
    assets: &AdvAssets,
    scenario: &Arc<Scenario>,
) {
    let mut assets = assets.clone();
    assets.scenario = scenario.clone();
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut frame = FrameId::default();
    let mut globals = SaveData::default();
    globals.settings.v11_disallowskipunread = false;
    for boundary in [7149, 7150, 7151] {
        let mut live = Adv::new(
            audio.clone(),
            server.clone(),
            assets.clone(),
            Scripter::new(scenario, 0, 0),
        );
        live.load_game_data(&globals, &game(boundary), server.clone())
            .unwrap();
        finish_load(&mut live, context, server, &mut frame);
        live.update_saved_scene_load(&mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(30.0),
            asset_server: server,
            pre_render: context,
        });
        flush(context);
        let deadline = Instant::now() + Duration::from_secs(20);
        while boundary == 7150
            && !matches!(
                live.current_command,
                Some(crate::adv::command::ExecutingCommand::LAYERWAIT(_))
            )
        {
            assert!(
                Instant::now() < deadline,
                "Credits did not reach live LAYERWAIT"
            );
            frame.advance();
            live.update(
                &mut UpdateContext {
                    frame_started_at: Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(1.0),
                    asset_server: server,
                    pre_render: context,
                },
                Default::default(),
            );
            flush(context);
        }
        assert_eq!(live.vm_state.save_position, boundary);
        assert_eq!(live.vm_state.current_event, (boundary == 7150).then_some(0));
        assert!(live.vm_state.system_controls_allowed);
        assert_eq!(live.adv_state.ui_hide_count, 0);
        assert_eq!(live.adv_state.audio_pause_count, 0);
        assert!(
            ReconstructedScene::next_chapter_game_data(
                scenario,
                &game(boundary),
                &globals,
                1_000_000
            )
            .unwrap()
            .is_none(),
            "Canonical replay agrees with native no-target"
        );
        let source = serde_json::to_value((
            &live.vm_state.snapshot_game_data().entry,
            &live.vm_state.primary_entry,
        ))
        .unwrap();
        let globals_before = live.save_menu_data().encode_raw();
        let source_pc = live.scripter.position();
        let availability_started = Instant::now();
        for _ in 0..10 {
            assert!(!live.has_next_chapter().unwrap());
        }
        let availability_micros = availability_started.elapsed().as_micros() / 10;
        assert_eq!(live.scripter.position(), source_pc);
        assert_eq!(live.save_menu_data().encode_raw(), globals_before);
        live.handle_input(action(AppAction::NextChapter), true);
        assert_eq!(
            live.pending_chapter,
            Some(Direction::Next),
            "Chapter input is reachable at the terminal boundary"
        );
        let start = Instant::now();
        navigate(
            &mut live,
            context,
            server,
            &mut frame,
            &Default::default(),
            1.0,
        );
        let micros = start.elapsed().as_micros();
        assert!(
            live.chapter_dialog.is_none(),
            "No-next terminal credits must not open a confirmation"
        );
        assert_eq!(
            serde_json::to_value((
                &live.vm_state.snapshot_game_data().entry,
                &live.vm_state.primary_entry
            ))
            .unwrap(),
            source
        );
        assert!(!live.is_loading_saved_scene());
        assert!(live.persistence.is_none());
        println!(
            "Live authored terminal{boundary}: SYS1/event={:?}; next input reachable, no popup, availability={availability_micros}us, preflight+GPU={micros}us",
            live.vm_state.current_event
        );
    }
}

fn verify_desktop_flick<'a>(
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<AssetServer>,
    assets: &AdvAssets,
    scenario: &Arc<Scenario>,
    target: &wgpu::TextureView,
) {
    use shin_input::{Action, ActionsState, RawInputAccumulator};
    use winit::{
        dpi::PhysicalPosition,
        event::{DeviceId, ElementState, MouseButton, WindowEvent},
    };
    fn frame<'a>(
        live: &mut Adv,
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        id: &mut FrameId,
        accumulator: &mut RawInputAccumulator,
        actions: &mut ActionsState<AppAction>,
        position: Vec2,
        held: bool,
        extent: (u16, u16),
        fps: f64,
    ) {
        let device_id = DeviceId::dummy();
        accumulator.on_winit_event(&WindowEvent::CursorMoved {
            device_id,
            position: PhysicalPosition::new(position.x as f64, position.y as f64),
        });
        accumulator.on_winit_event(&WindowEvent::MouseInput {
            device_id,
            state: if held {
                ElementState::Pressed
            } else {
                ElementState::Released
            },
            button: MouseButton::Left,
        });
        let raw = accumulator.start_frame();
        let elapsed = Duration::from_secs_f64(1.0 / fps);
        let input = actions.update(AppAction::lower(&raw), elapsed);
        // Same physical/canvas mapping and delivery order as App/SceneHost.
        let pointer =
            raw.mouse.position * vec2(1920.0 / extent.0 as f32, 1080.0 / extent.1 as f32);
        live.sample_modal_pointer(
            raw.mouse.position,
            extent,
            input[AppAction::PointerClick].is_held,
            elapsed.as_secs_f64(),
        );
        live.handle_pointer_state(
            pointer,
            input[AppAction::PointerClick].is_clicked,
            input[AppAction::PointerClick].is_held,
            elapsed.as_secs_f64(),
        );
        id.advance();
        live.update(
            &mut UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: *id,
                delta_ticks: Ticks::from_duration(elapsed),
                asset_server: server,
                pre_render: context,
            },
            input,
        );
        accumulator.finish_frame();
        flush(context);
    }
    let mut assets = assets.clone();
    assets.scenario = scenario.clone();
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut id = FrameId::default();
    let mut output = String::new();
    for width in [1920u16, 3840] {
        let extent = (width, width * 9 / 16);
        let scale = width as f32 / 1920.0;
        for fps in [60.0, 120.0] {
            for hide in [false, true] {
                if !hide && std::env::var_os("SHIN_REFERENCE_HIDDEN_TEXT_TAP_ONLY").is_some() {
                    continue;
                }
                let mut live = Adv::new(
                    audio.clone(),
                    server.clone(),
                    assets.clone(),
                    Scripter::new(scenario, 0, 0),
                );
                let mut globals = SaveData::default();
                globals.settings.v11_disallowskipunread = false;
                globals.settings.v17_showscenetitle = fps == 60.0;
                live.load_game_data(&globals, &game(326), server.clone())
                    .unwrap();
                finish_load(&mut live, context, server, &mut id);
                live.update_saved_scene_load(&mut UpdateContext {
                    frame_started_at: Instant::now(),
                    frame_id: id,
                    delta_ticks: Ticks::from_f32(30.0),
                    asset_server: server,
                    pre_render: context,
                });
                flush(context);
                assert!(!live.is_loading_saved_scene());
                assert!(live.vm_state.system_controls_allowed);
                let mut accumulator = RawInputAccumulator::new();
                let mut actions = ActionsState::new();
                let mut drag = |live: &mut Adv, direction: Vec2| {
                    let start = vec2(1000.0, 500.0) * scale;
                    // Hold before moving, then cover128 canvas pixels in200ms.
                    for _ in 0..(fps * 0.3) as usize {
                        frame(
                            live,
                            context,
                            server,
                            &mut id,
                            &mut accumulator,
                            &mut actions,
                            start,
                            true,
                            extent,
                            fps,
                        );
                    }
                    let steps = (fps * 0.2) as usize;
                    for step in 1..=steps {
                        let position =
                            start + direction * (128.0 * scale * step as f32 / steps as f32);
                        frame(
                            live,
                            context,
                            server,
                            &mut id,
                            &mut accumulator,
                            &mut actions,
                            position,
                            true,
                            extent,
                            fps,
                        );
                    }
                    let end = start + direction * 128.0 * scale;
                    // A stationary frame commonly separates mouse motion from release.
                    frame(
                        live,
                        context,
                        server,
                        &mut id,
                        &mut accumulator,
                        &mut actions,
                        end,
                        true,
                        extent,
                        fps,
                    );
                    frame(
                        live,
                        context,
                        server,
                        &mut id,
                        &mut accumulator,
                        &mut actions,
                        end,
                        false,
                        extent,
                        fps,
                    );
                    for _ in 0..(fps * 0.25) as usize {
                        frame(
                            live,
                            context,
                            server,
                            &mut id,
                            &mut accumulator,
                            &mut actions,
                            end,
                            false,
                            extent,
                            fps,
                        );
                    }
                };
                if hide {
                    drag(&mut live, vec2(0.0, 1.0));
                    assert!(
                        live.adv_state.hidden_ui_active,
                        "down drag width{width} fps{fps}"
                    );
                    assert_eq!(live.adv_state.ui_hide_count, 1);
                    // A drag release is not a tap and must leave text hidden.
                    drag(&mut live, vec2(-1.0, 0.0));
                    assert!(live.adv_state.hidden_ui_active);
                    drop(drag);
                    let source = live.vm_state.save_position;
                    let message_id = live.vm_state.messagebox_state.message_id;
                    let position = vec2(800.0, 500.0) * scale;
                    for _ in 0..(fps * 0.5) as usize {
                        frame(
                            &mut live,
                            context,
                            server,
                            &mut id,
                            &mut accumulator,
                            &mut actions,
                            position,
                            true,
                            extent,
                            fps,
                        );
                        assert!(
                            live.adv_state.hidden_ui_active,
                            "Mouse hold must not restore text"
                        );
                        assert_eq!(live.adv_state.ui_hide_count, 1);
                    }
                    frame(
                        &mut live,
                        context,
                        server,
                        &mut id,
                        &mut accumulator,
                        &mut actions,
                        position,
                        false,
                        extent,
                        fps,
                    );
                    assert!(
                        !live.adv_state.hidden_ui_active,
                        "Only the tap release restores text"
                    );
                    assert_eq!(live.adv_state.ui_hide_count, 0);
                    assert_eq!(
                        live.vm_state.save_position, source,
                        "Restoring must not advance the sentence"
                    );
                    assert_eq!(live.vm_state.messagebox_state.message_id, message_id);
                } else {
                    drag(&mut live, vec2(-1.0, 0.0));
                    assert!(
                        live.story_touch.reference_toolbar_open(),
                        "left drag width{width} fps{fps}"
                    );
                    drag(&mut live, vec2(1.0, 0.0));
                    assert!(
                        !live.story_touch.reference_toolbar_open(),
                        "right drag closes toolbar"
                    );
                    assert!(live.system_menu.is_none());
                    drag(&mut live, vec2(1.0, 0.0));
                    assert!(live.system_menu.is_some(), "right drag opens pause");
                }
                live.adv_state
                    .root_layer_group
                    .pre_render(context, &Default::default());
                flush(context);
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            view: target,
                            kind: TextureTargetKind::Screen,
                        },
                        Some(context.depth_stencil),
                        "Desktop mouse gestures",
                    );
                    live.render(&mut pass);
                }
                flush(context);
                assert!(live.persistence.is_none());
                assert!(live.saved_scene_load_error().is_none());
                output.push_str(&format!("width={width} fps={fps} hide={hide}: passed real ADV raw-input/update/render\n"));
            }
        }
    }
    let path =
        std::path::PathBuf::from(std::env::var_os("SHIN_REFERENCE_CHAPTER_OUTPUT").unwrap());
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("desktop-flick-integration.txt"), &output).unwrap();
    print!("{output}");
}

pub(super) fn verify<'a>(
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<AssetServer>,
    assets: &AdvAssets,
    scenario: &Arc<Scenario>,
    target: &wgpu::TextureView,
) {
    if std::env::var_os("SHIN_REFERENCE_CHAPTER_TERMINAL_ONLY").is_some() {
        verify_terminal_chapter(context, server, assets, scenario);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_DESKTOP_FLICK_ONLY").is_some() {
        verify_desktop_flick(context, server, assets, scenario, target);
        return;
    }
    let mut assets = assets.clone();
    assets.scenario = scenario.clone();
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut frame = FrameId::default();
    let mut output = String::new();
    for profile in [
        "default-no",
        "cancel",
        "system-revoked-opening",
        "system-revoked-after-yes",
        "previous-pointer",
        "next-recomputed",
        "unread-rejected",
        "unread-just-read",
        "toolbar-next",
    ] {
        let source = if matches!(profile, "next-recomputed" | "toolbar-next") {
            1
        } else {
            326
        };
        let mut live = Adv::new(
            audio.clone(),
            server.clone(),
            assets.clone(),
            Scripter::new(scenario, 0, 0),
        );
        let mut globals = SaveData::default();
        globals.settings.v11_disallowskipunread = false;
        live.load_game_data(&globals, &game(source), server.clone())
            .unwrap();
        finish_load(&mut live, context, server, &mut frame);
        // Complete the initial source fade through the production update
        // entry point before testing story controls.
        frame.advance();
        live.update_saved_scene_load(&mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(30.0),
            asset_server: server,
            pre_render: context,
        });
        flush(context);
        assert!(!live.is_loading_saved_scene());
        assert_eq!(live.vm_state.save_position, source);
        assert!(live.vm_state.system_controls_allowed);
        let canonical_available = ReconstructedScene::next_chapter_game_data(
            scenario,
            &live.vm_state.snapshot_game_data(),
            &live.save_menu_data(),
            1_000_000,
        )
        .unwrap()
        .is_some();
        let state_before = serde_json::to_value((
            &live.vm_state.snapshot_game_data().entry,
            &live.vm_state.primary_entry,
        ))
        .unwrap();
        let globals_before = live.save_menu_data().encode_raw();
        let source_pc = live.scripter.position();
        let start = Instant::now();
        assert_eq!(
            live.has_next_chapter().unwrap(),
            canonical_available,
            "{profile}"
        );
        println!(
            "{profile}: full availability={}us",
            start.elapsed().as_micros()
        );
        assert_eq!(live.scripter.position(), source_pc);
        assert_eq!(live.save_menu_data().encode_raw(), globals_before);
        assert_eq!(
            serde_json::to_value((
                &live.vm_state.snapshot_game_data().entry,
                &live.vm_state.primary_entry
            ))
            .unwrap(),
            state_before
        );
        let inactive = serde_json::to_value(&live.vm_state.primary_entry).unwrap();
        let old_owner = live.adv_state.root_layer_group.properties().identity();
        if matches!(profile, "unread-rejected" | "unread-just-read") {
            live.vm_state.settings.v11_disallowskipunread = true;
            if profile == "unread-just-read" {
                assert!(!live.vm_state.messagebox_state.was_read);
                live.vm_state
                    .mark_message_read(live.vm_state.messagebox_state.message_id);
                // The old replay preflight uses current read history. The
                // immediate guard must agree without replaying the route.
                assert!(
                    ReconstructedScene::next_chapter_game_data(
                        scenario,
                        &live.vm_state.snapshot_game_data(),
                        &live.save_menu_data(),
                        1_000_000,
                    )
                    .unwrap()
                    .is_some()
                );
            }
            live.handle_input(action(AppAction::NextChapter), true);
            assert!(!navigate(
                &mut live,
                context,
                server,
                &mut frame,
                &Default::default(),
                1.0
            ));
            assert!(!live.is_loading_saved_scene());
            if profile == "unread-rejected" {
                assert!(live.chapter_dialog.is_none());
            } else {
                assert!(
                    live.chapter_dialog.is_some(),
                    "Freshly marked read message permits the immediate prompt"
                );
                for _ in 0..15 {
                    navigate(
                        &mut live,
                        context,
                        server,
                        &mut frame,
                        &Default::default(),
                        1.0,
                    );
                }
                navigate(
                    &mut live,
                    context,
                    server,
                    &mut frame,
                    &action(AppAction::Cancel),
                    1.0,
                );
            }
        } else if profile == "toolbar-next" {
            pointer(&mut live, vec2(1000.0, 700.0), true, 0.01);
            pointer(&mut live, vec2(800.0, 700.0), true, 0.01);
            pointer(&mut live, vec2(800.0, 700.0), false, 0.01);
            live.story_touch.update(Ticks::from_f32(10.0));
            let next = vec2(1700.0, 350.0);
            pointer(&mut live, next, true, 0.01);
            assert!(live.pending_chapter.is_none());
            pointer(&mut live, next, false, 0.1);
            assert_eq!(live.pending_chapter, Some(Direction::Next));
            navigate(
                &mut live,
                context,
                server,
                &mut frame,
                &Default::default(),
                1.0,
            );
            for _ in 0..15 {
                navigate(
                    &mut live,
                    context,
                    server,
                    &mut frame,
                    &Default::default(),
                    1.0,
                );
            }
        } else if profile == "system-revoked-opening" {
            live.pending_chapter = Some(Direction::Previous);
            navigate(
                &mut live,
                context,
                server,
                &mut frame,
                &Default::default(),
                1.0,
            );
            live.vm_state.system_controls_allowed = false;
        } else {
            live.adv_state.playback_modes.toggle_skip();
            live.adv_state.playback_modes.toggle_autoplay();
            open(
                &mut live,
                context,
                server,
                &mut frame,
                if profile == "next-recomputed" {
                    Direction::Next
                } else {
                    Direction::Previous
                },
            );
        }
        // Render actual navi archive and generic dialog together; no live window.
        live.adv_state
            .root_layer_group
            .pre_render(context, &Default::default());
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "Chapter toolbar and confirmation",
            );
            live.render(&mut pass);
        }
        flush(context);
        let mut expected = None;
        match profile {
            "default-no" => {
                navigate(
                    &mut live,
                    context,
                    server,
                    &mut frame,
                    &action(AppAction::Enter),
                    1.0,
                );
            }
            "cancel" => {
                navigate(
                    &mut live,
                    context,
                    server,
                    &mut frame,
                    &action(AppAction::Cancel),
                    1.0,
                );
            }
            "previous-pointer" => {
                let yes = live
                    .chapter_dialog
                    .as_ref()
                    .unwrap()
                    .1
                    .reference_button_center(0);
                // An inherited held contact cannot confirm until a fresh press.
                live.chapter_dialog
                    .as_mut()
                    .unwrap()
                    .1
                    .set_pointer_extent((1920, 1080), true);
                pointer(&mut live, yes, true, 0.1);
                pointer(&mut live, yes, false, 0.1);
                assert!(live.chapter_dialog.as_ref().unwrap().1.reference_is_open());
                pointer(&mut live, yes, true, 0.1);
                pointer(&mut live, yes, false, 0.1);
                expected = Some(324);
            }
            "next-recomputed" | "system-revoked-after-yes" | "toolbar-next" => {
                navigate(
                    &mut live,
                    context,
                    server,
                    &mut frame,
                    &action(AppAction::Left),
                    1.0,
                );
                navigate(
                    &mut live,
                    context,
                    server,
                    &mut frame,
                    &action(AppAction::Enter),
                    1.0,
                );
                if profile == "next-recomputed" {
                    // Model a real decoded VM advancing behind the unpaused dialog.
                    // The final callback must search this new prefix, not its preflight.
                    let new_source = ReconstructedScene::from_game_data(
                        scenario,
                        &game(326),
                        &globals,
                        1_000_000,
                    )
                    .unwrap();
                    live.vm_state = new_source.state;
                    live.scripter = new_source.scripter;
                    expected = Some(614);
                } else if profile == "toolbar-next" {
                    expected = Some(324);
                } else {
                    live.vm_state.system_controls_allowed = false;
                }
            }
            _ => {}
        }
        // An intermediate Yes and the closing animation must leave old owners intact.
        for _ in 0..6 {
            navigate(
                &mut live,
                context,
                server,
                &mut frame,
                &Default::default(),
                1.0,
            );
            assert!(!live.is_loading_saved_scene());
            assert_eq!(
                live.adv_state.root_layer_group.properties().identity(),
                old_owner
            );
        }
        for _ in 0..60 {
            if live.chapter_dialog.is_none() {
                break;
            }
            navigate(
                &mut live,
                context,
                server,
                &mut frame,
                &Default::default(),
                1.0,
            );
        }
        assert!(
            live.chapter_dialog.is_none(),
            "{profile}: dialog failed to finish"
        );
        if let Some(boundary) = expected {
            finish_load(&mut live, context, server, &mut frame);
            assert_eq!(live.vm_state.save_position, boundary, "{profile}");
            assert!(
                live.adv_state.scene_load_fade.is_some(),
                "Mode0 retains the frozen-source fade"
            );
            assert_eq!(
                live.previous_rollback_position,
                Some(if profile == "next-recomputed" {
                    326
                } else if profile == "toolbar-next" {
                    1
                } else {
                    324
                })
            );
            let replay = ReconstructedScene::from_game_data(
                scenario,
                &game(boundary),
                &globals,
                1_000_000,
            )
            .unwrap();
            assert_eq!(live.scripter.position(), replay.scripter.position());
            assert_eq!(
                serde_json::to_value(&live.vm_state.primary_entry).unwrap(),
                inactive
            );
        } else {
            assert_eq!(
                live.vm_state.save_position, source,
                "{profile}: rejected confirmation changed story"
            );
            assert_eq!(
                live.adv_state.root_layer_group.properties().identity(),
                old_owner
            );
        }
        assert!(
            live.persistence.is_none(),
            "No working progress writer is configured"
        );
        assert!(live.saved_scene_load_error().is_none());
        output.push_str(&format!(
            "{profile}: pass source={source} target={expected:?}\n"
        ));
    }
    let output_path =
        std::path::PathBuf::from(std::env::var_os("SHIN_REFERENCE_CHAPTER_OUTPUT").unwrap());
    std::fs::create_dir_all(&output_path).unwrap();
    std::fs::write(output_path.join("integration.txt"), &output).unwrap();
    print!("{output}");
    println!("Nine chapter confirmation/publication profiles passed");
}
