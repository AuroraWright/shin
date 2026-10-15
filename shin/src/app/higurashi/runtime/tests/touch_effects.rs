//! Runtime wiring proof. The unchanged shared renderer and the separately
//! frozen native Higurashi touch corpus supply particle/shader fidelity.
//! These comparisons exercise clean versus visible composition and real raw
//! input; their reference composition deliberately does not re-render a scene.
use super::*;
use crate::app::higurashi::runtime_game_data::RuntimeGameData;
use shin_core::format::save::higurashi::{GameData, GameDataEntry, SaveDate};
use std::path::PathBuf;

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        if self.0.exists() {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
struct Fixture {
    // Retire actual scenes, media and loaders before their temporary storage.
    runtime: Runtime,
    gpu: Graphics,
    store: Arc<Store>,
    file: Vec<u8>,
    rows: Vec<(bool, RuntimeGameData)>,
    _temporary: Temporary,
}
impl Fixture {
    fn new(label: &str) -> Self {
        Self::new_with_touch(label, true)
    }
    fn new_with_touch(label: &str, enabled: bool) -> Self {
        let temporary = Temporary(std::env::temp_dir().join(format!(
            "higurashi-runtime-touch-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
        let mut saved = SaveData::default();
        saved.play_seconds = 17;
        saved.settings.common.v16_showtoucheffect = enabled;
        saved.manual_save_slots = std::array::from_fn(|i| {
            Some(GameData {
                date: SaveDate {
                    year: 2025,
                    month: 10,
                    day: (i % 28 + 1) as u8,
                    hour: (i % 24) as u8,
                    minute: (i % 60) as u8,
                    second: ((i * 7) % 60) as u8,
                },
                extra_word: (i % 8) as u8,
                primary_entry: Some(GameDataEntry {
                    scenario_id: 2000,
                    random_seed: 0x1234_0000 + i as u32,
                    save_position: 0,
                    replay_target: 0,
                    response_data: vec![i as u8, 0x81, 0xfe],
                }),
                entry: GameDataEntry {
                    scenario_id: 2000,
                    random_seed: 0x8765_0000 + i as u32,
                    save_position: 0,
                    replay_target: 0,
                    response_data: vec![0xc7, i as u8, 0, 0xff],
                },
                trailing_values: vec![1023, i as u16, 0x123],
                byte_variables: vec![9, i as u8, 255],
            })
        });
        let expected = crate::app::higurashi::runtime_saves::Slots::loaded(&saved);
        let rows = std::iter::once(&expected.auto)
            .chain(expected.manual.iter())
            .map(|slot| (slot.valid, slot.data.clone()))
            .collect();
        let paths = Paths {
            current: Some(temporary.0.join("progress.dat")),
            trial: None,
        };
        let file = saved.encode().unwrap();
        paths.write(&file).unwrap();
        let store = Arc::new(Store {
            paths,
            fail: AtomicBool::new(false),
            gate: (StdMutex::new(true), Condvar::new()),
            writes: StdMutex::new(Vec::new()),
        });
        let (device, queue, server) = crate::adv::higurashi::reference_gpu_assets();
        let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
        let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
        let mut runtime = Runtime::new(Arc::new(server), manager, sounds, store.clone());
        runtime.set_focused(true);
        Self {
            runtime,
            gpu: Graphics::new(device, queue),
            store,
            file,
            rows,
            _temporary: temporary,
        }
    }
    fn step(&mut self, raw: &RawInputState, viewport: InputViewport) {
        self.step_elapsed(raw, viewport, Duration::from_millis(50));
    }
    fn step_elapsed(&mut self, raw: &RawInputState, viewport: InputViewport, elapsed: Duration) {
        assert!(matches!(
            self.gpu.update(&mut self.runtime, raw, viewport, elapsed),
            Frame::Advanced(_)
        ));
        assert!(self.runtime.failure().is_none());
    }
    fn tap(&mut self, code: KeyCode, viewport: InputViewport) {
        self.step(&RawInputState::new(), viewport);
        self.step(&key(code), viewport);
        self.step(&RawInputState::new(), viewport);
    }
    fn until(
        &mut self,
        viewport: InputViewport,
        deadline: Instant,
        description: &str,
        ready: impl Fn(&Runtime) -> bool,
    ) {
        while !ready(&self.runtime) {
            assert!(Instant::now() < deadline, "{description}");
            self.step(&RawInputState::new(), viewport);
        }
    }
    fn boot(&mut self, viewport: InputViewport, deadline: Instant) {
        self.until(viewport, deadline, "mandatory notice", |r| {
            notice(r).is_some_and(|(phase, _)| phase == 2)
        });
        self.tap(KeyCode::Enter, viewport);
        self.until(viewport, deadline, "opening movie", |r| {
            r.bootstrap
                .as_ref()
                .is_some_and(SceneHost::reference_demo_ready)
        });
        self.tap(KeyCode::Enter, viewport);
        self.until(viewport, deadline, "normal Title Top", |r| {
            ready_menu(r, "top")
        });
        assert_eq!(
            self.runtime.slot().unwrap().active_scene().unwrap(),
            ActiveScene::Title
        );
        assert_eq!(
            self.runtime
                .slot()
                .unwrap()
                .title
                .as_ref()
                .unwrap()
                .reference_top_selection(),
            Some(0)
        );
        assert!(
            self.runtime
                .globals()
                .unwrap()
                .unwrap()
                .common
                .system
                .contains("drag")
        );
        self.unchanged_rows_and_disk();
    }
    fn unchanged_rows_and_disk(&self) {
        let globals = self.runtime.globals().unwrap().unwrap();
        let slots = globals.runtime_saves.unwrap();
        let actual: Vec<_> = std::iter::once(&slots.auto)
            .chain(slots.manual.iter())
            .map(|slot| (slot.valid, slot.data.clone()))
            .collect();
        // Story owns and can refresh row0; every unrelated manual row survives.
        assert_eq!(actual.len(), 101);
        assert_eq!(&actual[1..], &self.rows[1..]);
        if self.runtime.slot().unwrap().game.is_none() {
            assert_eq!(actual[0], self.rows[0]);
        }
        assert!(self.store.writes.lock().unwrap().is_empty());
        assert_eq!(
            self.store.paths.read(Source::Current).unwrap().as_deref(),
            Some(self.file.as_slice())
        );
    }
    fn pointer(&mut self, viewport: InputViewport, at: Vec2, held: bool) {
        self.step(
            &mouse(
                viewport.offset + at / Vec2::new(1920., 1080.) * viewport.size,
                held,
            ),
            viewport,
        );
    }
    fn expire(&mut self, viewport: InputViewport) {
        for _ in 0..22 {
            self.step(&RawInputState::new(), viewport);
        }
        assert!(!self.runtime.touch_effect.has_ripples());
        assert!(!self.runtime.touch_effect.has_stars());
    }
}

fn story(runtime: &Runtime) -> &crate::app::higurashi::gameplay::Gameplay {
    runtime.slot().unwrap().game.as_ref().unwrap()
}
fn story_host(runtime: &Runtime) -> &crate::adv::higurashi::host::HigurashiHost {
    story(runtime).host().unwrap()
}
fn raw_pointer(viewport: InputViewport, at: Vec2, held: bool) -> RawInputState {
    mouse(
        viewport.offset + at / Vec2::new(1920., 1080.) * viewport.size,
        held,
    )
}

// Ordinary desktop gesture with a stationary hold, slow deliberate movement,
// and a stationary release gap. Native's last-frame velocity alone is zero
// here. Only the scoped desktop MouseRelease adaptation supplies the intended
// gesture; no synthetic native PointerEvent or direct request is injected.
fn ordinary_drag(f: &mut Fixture, viewport: InputViewport, hz: u32, displacement: Vec2) {
    let period = Duration::from_nanos(1_000_000_000 / u64::from(hz));
    let start = Vec2::new(900., 700.);
    let end = start + displacement;
    f.step_elapsed(&raw_pointer(viewport, start, false), viewport, period);
    f.step_elapsed(&raw_pointer(viewport, start, true), viewport, period);
    let mut hold = Duration::ZERO;
    while hold < Duration::from_millis(300) {
        let step = period.min(Duration::from_millis(300) - hold);
        f.step_elapsed(&raw_pointer(viewport, start, true), viewport, step);
        hold += step;
    }
    let count = hz / 5;
    let mut movement = Duration::ZERO;
    for index in 1..=count {
        let step = if index == count {
            Duration::from_millis(200) - movement
        } else {
            period
        };
        f.step_elapsed(
            &raw_pointer(
                viewport,
                start + displacement * (index as f32 / count as f32),
                true,
            ),
            viewport,
            step,
        );
        movement += step;
    }
    assert_eq!(movement, Duration::from_millis(200));
    let mut gap = Duration::ZERO;
    while gap + period < Duration::from_millis(40) {
        f.step_elapsed(&raw_pointer(viewport, end, true), viewport, period);
        gap += period;
    }
    f.step_elapsed(
        &raw_pointer(viewport, end, false),
        viewport,
        Duration::from_millis(40) - gap,
    );
}

fn short_tap(f: &mut Fixture, viewport: InputViewport, hz: u32, at: Vec2) {
    let period = Duration::from_nanos(1_000_000_000 / u64::from(hz));
    f.step_elapsed(&raw_pointer(viewport, at, false), viewport, period);
    f.step_elapsed(&raw_pointer(viewport, at, true), viewport, period);
    f.step_elapsed(&raw_pointer(viewport, at, false), viewport, period);
}

#[test]
#[ignore = "normal Runtime authored message with slow held raw-mouse toolbar/menu/hide/restore gestures at720p1080p and60/120Hz,feedback on/off;real assets/offscreen GPU/exact Loopback Audio;temporary progress only"]
fn higurashi_runtime_desktop_gestures_work_independently_of_visual_feedback() {
    use shin_core::vm::higurashi::command::HigurashiRuntimeCommand as Cmd;
    for enabled in [false, true] {
        let mut f = Fixture::new_with_touch(
            if enabled {
                "gesture-fx"
            } else {
                "gesture-no-fx"
            },
            enabled,
        );
        let deadline = Instant::now() + Duration::from_secs(90);
        let small = viewport((1280, 720));
        f.boot(small, deadline);
        f.tap(KeyCode::Enter, small);
        f.until(small, deadline, "normal new-story category", |r| {
            ready_menu(r, "cat")
        });
        f.tap(KeyCode::Enter, small);
        f.until(small, deadline, "authored first interested message", |r| {
            r.slot()
                .and_then(|s| s.game.as_ref())
                .and_then(|game| game.host())
                .is_some_and(|host| {
                    host.runner.native_vm_attached()
                        && host.runner.completed_commands() >= 10
                        && host.runner.reconstruction_phase().is_none()
                        && matches!(
                            host.runner.pending_command(),
                            Some(Cmd::MSGWAIT { .. } | Cmd::MSGSET { auto_wait: 1, .. })
                        )
                        && host.runner.frame().message.playback.interested_in_input()
                })
        });
        for _ in 0..10 {
            f.step(&RawInputState::new(), small);
        }
        assert!(
            story_host(&f.runtime)
                .runner
                .native_read_state()
                .unwrap()
                .flags
                & 0x8000_0000
                != 0
        );
        assert_eq!(story(&f.runtime).reference_play_modes(), 0);
        assert_eq!(f.runtime.globals().unwrap().unwrap().platform_flag, enabled);
        let commands = story_host(&f.runtime).runner.completed_commands();
        let position = story_host(&f.runtime).runner.position();
        let glyphs = story_host(&f.runtime)
            .runner
            .frame()
            .message
            .glyphs()
            .unwrap() as *const _;
        if !enabled {
            if let Some(path) = std::env::var_os("SHIN_HIGURASHI_GESTURE_SAVE_CAPTURE") {
                use std::io::Write;
                let saved = f.runtime.globals().unwrap().unwrap().data;
                assert!(
                    saved.auto_save_slot.is_some(),
                    "export only a real authored AUTOSAVE"
                );
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .unwrap();
                file.write_all(&saved.encode().unwrap()).unwrap();
                eprintln!(
                    "Actual first-dialogue temporary progress captured at{:?},PC{position:?},completed{commands};no VM/slot mutation",
                    path
                );
            }
        }
        for size in [(1280, 720), (1920, 1080)] {
            let v = viewport(size);
            // Resize/any modal return may quarantine the old sample; a full
            // neutral sample precedes every fresh deliberate gesture.
            f.step(&RawInputState::new(), v);
            for hz in [60, 120] {
                ordinary_drag(&mut f, v, hz, Vec2::new(-128., 0.));
                // The native indicator saturates in at most4 delta3 frames.
                // Fail this synchronous regression promptly; no async loader
                // can justify waiting through the full boot deadline here.
                for _ in 0..12 {
                    f.step(&RawInputState::new(), v);
                }
                assert_eq!(
                    story_host(&f.runtime).touch_state().opacity,
                    1.,
                    "left desktop gesture must open toolbar at{size:?}/{hz}Hz,effect{enabled}"
                );
                assert_eq!(story_host(&f.runtime).touch_state().selected, -1);
                if hz == 60 {
                    exact_presentation(&mut f, enabled);
                }

                // Actual native skip control owns release. This first authored
                // message is unread and the pristine preference forbids unread
                // skip, so toggling cannot advance the source command.
                assert_eq!(
                    story_host(&f.runtime)
                        .runner
                        .native_read_state()
                        .unwrap()
                        .is_read,
                    0
                );
                short_tap(&mut f, v, hz, Vec2::new(1620., 180.));
                assert_eq!(story(&f.runtime).reference_play_modes(), 2);
                short_tap(&mut f, v, hz, Vec2::new(1620., 180.));
                assert_eq!(story(&f.runtime).reference_play_modes(), 0);

                ordinary_drag(&mut f, v, hz, Vec2::new(128., 0.));
                f.until(v, deadline, "right gesture closes existing toolbar", |r| {
                    story_host(r).touch_state().opacity == 0.
                });
                assert_eq!(story_host(&f.runtime).modal_pause_depth(), 0);
                ordinary_drag(&mut f, v, hz, Vec2::new(128., 0.));
                f.until(v, deadline, "right gesture opens actual SystemMenu", |r| {
                    story_host(r)
                        .reference_menu_control()
                        .is_some_and(|(_, ready)| ready)
                });
                assert_eq!(story_host(&f.runtime).modal_pause_depth(), 1);
                f.tap(KeyCode::Escape, v);
                f.until(v, deadline, "B returns from actual SystemMenu", |r| {
                    story_host(r).modal_pause_depth() == 0 && !story_host(r).has_modes()
                });
                ordinary_drag(&mut f, v, hz, Vec2::new(0., 128.));
                f.until(v, deadline, "down gesture owns fully hidden textbox", |r| {
                    let message = &story_host(r).runner.frame().message.playback;
                    message.hidden && message.modal_slide == 0.
                });
                assert!(story_host(&f.runtime).has_modes());
                assert_eq!(
                    story_host(&f.runtime)
                        .runner
                        .frame()
                        .message
                        .glyphs()
                        .unwrap() as *const _,
                    glyphs
                );
                short_tap(&mut f, v, hz, Vec2::new(900., 700.));
                f.until(v, deadline, "fresh tap restores retained textbox", |r| {
                    let host = story_host(r);
                    let message = &host.runner.frame().message.playback;
                    !host.has_modes() && !message.hidden && message.modal_slide == 1.
                });
                assert_eq!(story_host(&f.runtime).runner.completed_commands(), commands);
                assert_eq!(story_host(&f.runtime).runner.position(), position);
                assert_eq!(
                    story_host(&f.runtime)
                        .runner
                        .frame()
                        .message
                        .glyphs()
                        .unwrap() as *const _,
                    glyphs
                );
                assert_eq!(story(&f.runtime).reference_play_modes(), 0);
                f.unchanged_rows_and_disk();
            }
        }
    }
    eprintln!(
        "Runtime desktop gestures:8 actual authored-message viewport/cadence/effect combinations,300ms hold+128logicalpx/200ms movement+40ms stationary release;left toolbar/real skip control/right close/right SystemMenu+B/down hide/fresh tap restore,exact clean+visible composition,source command/glyph/manual100/disk preservation;no synthetic native contacts or physical HID claim"
    );
}

// Recompose at the actual fixed1080 extent, then scale exactly once as the
// window does. Rendering feedback directly into720p would be a different AA
// and sampling experiment. The source is Runtime's CLEAN cache, never an old
// visible feedback frame. This tests integration, not independent shader math.
fn source_pixels(
    gpu: &mut Graphics,
    runtime: &Runtime,
    extent: (u32, u32),
    feedback: bool,
) -> Vec<u8> {
    assert!(!runtime.shutdown.active());
    gpu.dynamic.recall();
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Runtime touch reference presentation"),
        size: wgpu::Extent3d {
            width: extent.0,
            height: extent.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let depth = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Runtime touch reference window stencil"),
        size: target.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::DEPTH_STENCIL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&Default::default());
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let mut context = gpu.context(&mut encoder);
    let composition = runtime.composition.as_ref().unwrap();
    let mut expected = feedback.then(|| {
        RenderTexture::new(
            context.device.clone(),
            composition.resize.handle(),
            "Clean source plus touch reference".into(),
        )
    });
    if let Some(expected) = &mut expected {
        let mut pass = context.begin_pass(
            expected.as_texture_target(),
            Some(context.depth_stencil),
            "Reference global touch composition",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
        Runtime::render_texture(&composition.color, &mut pass);
        runtime
            .touch_effect
            .render_ripples(&mut pass, composition.color.as_texture_source());
        runtime.touch_effect.render(
            &mut pass,
            &runtime.globals().unwrap().unwrap().common.system,
        );
    }
    {
        let mut pass = context.begin_pass(
            TextureTarget {
                view: &view,
                kind: TextureTargetKind::Screen,
            },
            Some(DepthStencilTarget { view: &depth_view }),
            "Reference final cached copy",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
        Runtime::render_texture(expected.as_ref().unwrap_or(&composition.color), &mut pass);
    }
    crate::layer::user::readback_test_texture(&mut context, &target)
}

fn exact_presentation(fixture: &mut Fixture, different: bool) {
    let frames = fixture.runtime.frames;
    let clock = fixture.runtime.globals().unwrap().unwrap().clock.ticks();
    let presence = (
        fixture.runtime.touch_effect.has_stars(),
        fixture.runtime.touch_effect.has_ripples(),
    );
    for extent in [(1280, 720), (1920, 1080)] {
        let clean = source_pixels(&mut fixture.gpu, &fixture.runtime, extent, false);
        let expected = source_pixels(&mut fixture.gpu, &fixture.runtime, extent, true);
        let visible = fixture.gpu.pixels(&fixture.runtime, extent, false);
        let capture = fixture.gpu.pixels(&fixture.runtime, extent, true);
        let repeated = fixture.gpu.pixels(&fixture.runtime, extent, false);
        assert_eq!(
            visible, expected,
            "exact clean-source+feedback composition{extent:?}"
        );
        assert_eq!(
            visible, capture,
            "visible capture must be a pure copy{extent:?}"
        );
        assert_eq!(
            visible, repeated,
            "repeated rendering cannot age/compound feedback{extent:?}"
        );
        assert_eq!(
            clean,
            source_pixels(&mut fixture.gpu, &fixture.runtime, extent, false),
            "feedback must not mutate factory/save source{extent:?}"
        );
        if different {
            assert_ne!(
                visible, clean,
                "real feedback must visibly affect the real scene at{extent:?}"
            );
        } else {
            assert_eq!(visible, clean, "no visible feedback at{extent:?}");
        }
    }
    assert_eq!(fixture.runtime.frames, frames);
    assert_eq!(
        fixture.runtime.globals().unwrap().unwrap().clock.ticks(),
        clock
    );
    assert_eq!(
        (
            fixture.runtime.touch_effect.has_stars(),
            fixture.runtime.touch_effect.has_ripples()
        ),
        presence
    );
    fixture.unchanged_rows_and_disk();
}

fn config(runtime: &Runtime) -> Option<(u32, usize, bool, u8)> {
    runtime.slot()?.title.as_ref()?.reference_config()
}
fn config_input_ready(runtime: &Runtime) -> bool {
    runtime
        .slot()
        .and_then(|slot| slot.title.as_ref())
        .is_some_and(|title| title.reference_config_input_ready())
}
fn reset_question(runtime: &Runtime) -> Option<(i32, i32)> {
    runtime.slot()?.title.as_ref()?.reference_config_question()
}

#[test]
#[ignore = "actual normal Runtime touch feedback/config/reset/cancel,exact720p1080p clean/presentation/capture comparison;real assets/offscreen GPU/exact Loopback Audio;temporary progress only"]
fn higurashi_runtime_touch_feedback_preserves_clean_sources_and_live_config_binding() {
    let mut f = Fixture::new("feedback");
    let small = viewport((1280, 720));
    let large = viewport((1920, 1080));
    let deadline = Instant::now() + Duration::from_secs(90);
    f.boot(small, deadline);
    f.runtime.touch_effect = crate::app::touch_effect::TouchEffect::with_seed(5489);
    assert!(f.runtime.globals().unwrap().unwrap().platform_flag);
    exact_presentation(&mut f, false);

    // Same authored point through both real physical viewport sizes. A release
    // starts at age0; the next actual callback makes the ripple visible.
    for v in [small, large] {
        // Admit the viewport itself on a neutral frame before a fresh Down;
        // a resize while Down is already held is deliberately quarantined.
        f.step(&RawInputState::new(), v);
        f.pointer(v, Vec2::new(80., 540.), true);
        f.pointer(v, Vec2::new(80., 540.), false);
        f.pointer(v, Vec2::new(80., 540.), false);
        assert!(f.runtime.touch_effect.has_ripples());
        exact_presentation(&mut f, true);
        // Consecutive actual updates must use the new CLEAN source, rather
        // than the previous already-distorted presentation.
        f.pointer(v, Vec2::new(80., 540.), false);
        exact_presentation(&mut f, true);
        f.expire(v);
        f.pointer(v, Vec2::new(80., 540.), true);
        f.pointer(v, Vec2::new(80., 720.), true);
        assert!(f.runtime.touch_effect.has_stars());
        exact_presentation(&mut f, true);
        f.pointer(v, Vec2::new(80., 720.), false);
        assert!(
            !f.runtime.touch_effect.has_ripples(),
            "drag release is not a tap"
        );
        f.expire(v);
    }

    // Neither focus cancellation nor a viewport change can turn the inherited
    // held contact into a fresh release ripple after the visible resize.
    f.pointer(large, Vec2::new(80., 540.), true);
    f.runtime.focus_lost().unwrap();
    f.runtime.set_focused(true);
    f.pointer(large, Vec2::new(80., 540.), true);
    f.pointer(large, Vec2::new(80., 540.), false);
    f.pointer(large, Vec2::new(80., 540.), false);
    exact_presentation(&mut f, false);
    f.pointer(large, Vec2::new(80., 540.), true);
    f.pointer(small, Vec2::new(80., 540.), true);
    f.pointer(small, Vec2::new(80., 540.), false);
    f.pointer(small, Vec2::new(80., 540.), false);
    exact_presentation(&mut f, false);

    // A contact beginning in a letterbox remains quarantined after entering
    // the authored canvas; the real shared pointer producer owns that history.
    f.step(&mouse(small.offset + Vec2::new(-10., 360.), true), small);
    f.pointer(small, Vec2::new(80., 540.), true);
    f.pointer(small, Vec2::new(80., 540.), false);
    f.pointer(small, Vec2::new(80., 540.), false);
    exact_presentation(&mut f, false);
    f.pointer(small, Vec2::new(80., 540.), true);
    f.pointer(small, Vec2::new(80., 540.), false);
    f.pointer(small, Vec2::new(80., 540.), false);
    exact_presentation(&mut f, true);
    f.expire(small);

    // Denied input still ages effects, but its real Down cannot be recreated
    // as a fresh Down when permission returns while the mouse remains held.
    assert!(matches!(
        f.gpu.update_with_permission(
            &mut f.runtime,
            &raw_pointer(small, Vec2::new(80., 540.), true),
            small,
            Duration::from_millis(50),
            false,
        ),
        Frame::Advanced(_)
    ));
    f.pointer(small, Vec2::new(80., 540.), true);
    f.pointer(small, Vec2::new(80., 540.), false);
    f.pointer(small, Vec2::new(80., 540.), false);
    exact_presentation(&mut f, false);
    f.pointer(small, Vec2::new(80., 540.), true);
    f.pointer(small, Vec2::new(80., 540.), false);
    f.pointer(small, Vec2::new(80., 540.), false);
    exact_presentation(&mut f, true);
    f.expire(small);

    f.tap(KeyCode::ArrowDown, small);
    assert_eq!(
        f.runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .reference_top_selection(),
        Some(2)
    );
    f.tap(KeyCode::Enter, small);
    f.until(small, deadline, "actual Config Input", |r| {
        config(r).is_some_and(|(flags, row, ready, _)| flags == 0 && row == 0 && ready)
            && config_input_ready(r)
    });
    for selected in 1..=12 {
        f.until(
            small,
            deadline,
            "Config ready before next Down",
            config_input_ready,
        );
        f.tap(KeyCode::ArrowDown, small);
        f.until(
            small,
            deadline,
            "Config scroll completed after Down",
            config_input_ready,
        );
        assert_eq!(config(&f.runtime).unwrap().1, selected);
    }
    // Higurashi row12 is common17/scene-title, not the touch-effect control.
    f.until(
        small,
        deadline,
        "Config ready before scene-title edit",
        config_input_ready,
    );
    f.tap(KeyCode::ArrowLeft, small);
    assert!(
        !f.runtime
            .globals()
            .unwrap()
            .unwrap()
            .data
            .settings
            .common
            .v17_showscenetitle
    );
    assert!(f.runtime.globals().unwrap().unwrap().platform_flag);
    f.pointer(small, Vec2::new(80., 540.), true);
    f.pointer(small, Vec2::new(80., 540.), false);
    f.pointer(small, Vec2::new(80., 540.), false);
    exact_presentation(&mut f, true);
    f.expire(small);
    f.until(
        small,
        deadline,
        "Config ready before touch row Down",
        config_input_ready,
    );
    f.tap(KeyCode::ArrowDown, small);
    f.until(
        small,
        deadline,
        "Config touch row scroll completed",
        config_input_ready,
    );
    assert_eq!(config(&f.runtime).unwrap().1, 13);
    f.tap(KeyCode::ArrowLeft, small);
    assert!(!f.runtime.globals().unwrap().unwrap().platform_flag);
    assert!(
        !f.runtime
            .globals()
            .unwrap()
            .unwrap()
            .data
            .settings
            .common
            .v16_showtoucheffect
    );
    f.pointer(small, Vec2::new(80., 540.), true);
    f.pointer(small, Vec2::new(80., 540.), false);
    f.pointer(small, Vec2::new(80., 540.), false);
    assert!(!f.runtime.touch_effect.has_ripples() && !f.runtime.touch_effect.has_stars());
    exact_presentation(&mut f, false);

    // Use the real default-No Question, then native stage0 Yes resets both
    // settings. No direct flag assignment or Model.question_callback occurs.
    f.until(
        small,
        deadline,
        "Config ready before reset",
        config_input_ready,
    );
    f.tap(KeyCode::KeyM, small);
    f.until(small, deadline, "Config reset Question", |r| {
        reset_question(r) == Some((2, 1))
    });
    f.tap(KeyCode::ArrowLeft, small);
    assert_eq!(reset_question(&f.runtime), Some((2, 0)));
    f.tap(KeyCode::Enter, small);
    f.until(
        small,
        deadline,
        "reset published through actual Question",
        |r| r.globals().unwrap().unwrap().platform_flag,
    );
    assert!(
        f.runtime
            .globals()
            .unwrap()
            .unwrap()
            .data
            .settings
            .common
            .v16_showtoucheffect
    );
    assert!(
        f.runtime
            .globals()
            .unwrap()
            .unwrap()
            .data
            .settings
            .common
            .v17_showscenetitle
    );
    f.until(small, deadline, "reset Question retirement", |r| {
        reset_question(r).is_none()
    });
    f.until(
        small,
        deadline,
        "Config Input after reset/prune",
        config_input_ready,
    );
    f.pointer(small, Vec2::new(80., 540.), true);
    f.pointer(small, Vec2::new(80., 540.), false);
    f.pointer(small, Vec2::new(80., 540.), false);
    exact_presentation(&mut f, true);
    f.expire(small);
    f.until(
        small,
        deadline,
        "Config ready before B return",
        config_input_ready,
    );
    f.tap(KeyCode::Escape, small);
    f.until(small, deadline, "unchanged Config return to Top2", |r| {
        config(r).is_none() && ready_menu(r, "top")
    });
    assert_eq!(
        f.runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .reference_top_selection(),
        Some(2)
    );
    exact_presentation(&mut f, false);
    f.unchanged_rows_and_disk();
    eprintln!(
        "Runtime touch:real boot/tap/drag at720p1080p,exact clean-source feedback and pure visible capture,no recursive accumulation,focus/resize cancellation,actual common17 contrast/common16 disable/default-No reset-Yes/unchanged Config return,100 rich manual rows and disk preserved;native shader fidelity reused separately"
    );
}
