//! Concrete owner acceptance candidates. Root alone registers/runs these on
//! offscreen Metal + the default audio output. Rendering here checks retained asset
//! transport; independent shader math remains the frozen parent packet tests.
use super::{
    SceneRequest,
    owner::{Owner, ReferenceGate, ReferencePublication, ReferenceResources},
};
use crate::{
    adv::{
        higurashi::{
            audio::HigurashiAudio,
            autosave::{ProgressEntry, SaveProgress},
            input::Buttons,
            music_commands::MusicCommands,
            reference_gpu_assets, reference_render_to_texture,
        },
        input_area::{PointerEvent, PointerFrame, PointerKind},
    },
    app::higurashi::{
        first_boot::CommonAssets, runtime_game_data::RuntimeGameData, save_menu::Environment,
    },
    asset::system::AssetServer,
    audio::SystemSoundPlayer,
};
use anyhow::Result;
use glam::{IVec2, Vec2};
use parking_lot::Mutex;
use shin_core::{
    format::save::higurashi::{SaveData, SaveDate},
    primitives::color::UnormColor,
    time::Ticks,
};
use shin_render::{
    render_texture::RenderTexture,
    shaders::types::texture::{TextureTarget, TextureTargetKind},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const EXTENTS: [(u16, u16); 2] = [(1280, 720), (1920, 1080)];
const CAPTURE_COLOR: [u8; 4] = [71, 113, 166, 255];
fn seed(index: usize) -> u32 {
    0x2468_ace1u32.wrapping_add(index as u32 * 0x10203)
}
fn seeds() -> (Arc<dyn Fn() -> u32 + Send + Sync>, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    (
        Arc::new(move || seed(calls.fetch_add(1, Ordering::SeqCst))),
        count,
    )
}
fn source(active: usize) -> RuntimeGameData {
    RuntimeGameData::from_progress(
        SaveProgress::new(
            active,
            0xdead_beef,
            std::array::from_fn(|i| ProgressEntry {
                scenario_id: 1010 + i as i32,
                random_seed: 0x8765_4321 + i as u32,
                save_position: 91 + i as u32,
                replay_target: 117 + i as u32,
                response_data: vec![0xf1, 0x55, 0x23, 0xee, i as u8],
            }),
        )
        .unwrap(),
        SaveDate {
            year: 2025,
            month: 11,
            day: 23,
            hour: 13,
            minute: 14,
            second: 15,
        },
        vec![0, 0x1234, 0xffff, 0x8000],
        vec![0, 255, 128, 37, 14],
    )
}
struct Fixture {
    device: wgpu::Device,
    queue: wgpu::Queue,
    server: Arc<AssetServer>,
    common: Arc<CommonAssets>,
    manager: Arc<shin_audio::AudioManager>,
    sounds: Arc<Mutex<SystemSoundPlayer>>,
}
impl Fixture {
    fn new() -> Self {
        let (device, queue, server) = reference_gpu_assets();
        let server = Arc::new(server);
        let common = Arc::new(CommonAssets {
            scenario: server.load_sync("/main.snr").unwrap(),
            font: server.load_sync("/newrodin.fnt").unwrap(),
            system: server.load_sync("/systex.txa").unwrap(),
            message: server.load_sync("/msgtex.txa").unwrap(),
            effects: server.load_sync("/effects.txa").unwrap(),
            sysse: server.load_sync("/sysse.bin").unwrap(),
        });
        let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
        let mut sounds = SystemSoundPlayer::new(manager.clone());
        sounds.bank = Some(common.sysse.clone());
        Self {
            device,
            queue,
            server,
            common,
            manager,
            sounds: Arc::new(Mutex::new(sounds)),
        }
    }
    fn environment(&self) -> Environment {
        Environment {
            common: self.common.clone(),
            server: self.server.clone(),
            sounds: self.sounds.clone(),
            storage: None,
        }
    }
    fn music(&self) -> Music {
        Music {
            audio: HigurashiAudio::new(
                self.manager.clone(),
                self.server.clone(),
                self.common.scenario.clone(),
            ),
            events: vec![],
        }
    }
    fn capture(&self, extent: (u16, u16)) -> Option<RenderTexture> {
        let mut captured = None;
        reference_render_to_texture(
            &self.device,
            &self.queue,
            (extent.0.into(), extent.1.into()),
            |context, _| {
                let mut texture =
                    context.new_render_texture("ScenarioSelect prior composition".into());
                {
                    let mut pass = context.begin_pass(
                        texture.as_texture_target(),
                        Some(context.depth_stencil),
                        "ScenarioSelect prior composition",
                    );
                    pass.clear(
                        Some(UnormColor::from_rgba(
                            CAPTURE_COLOR[0],
                            CAPTURE_COLOR[1],
                            CAPTURE_COLOR[2],
                            CAPTURE_COLOR[3],
                        )),
                        Some(0),
                        Some(1.),
                    );
                }
                captured = Some(texture);
            },
        );
        captured
    }
    fn draw(&self, owner: &Owner, extent: (u16, u16)) -> Vec<u8> {
        reference_render_to_texture(
            &self.device,
            &self.queue,
            (extent.0.into(), extent.1.into()),
            |context, target| {
                owner.prepare(context);
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Concrete ScenarioSelect",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                owner.render(&mut pass).unwrap();
            },
        )
    }
}
#[derive(Debug, PartialEq, Eq)]
enum MusicEvent {
    Play(i32, i32, u32, u32),
    Stop(i32),
}
struct Music {
    audio: HigurashiAudio,
    events: Vec<MusicEvent>,
}
impl MusicCommands for Music {
    fn play_bgm(&mut self, id: i32, fade: i32, loops: u32, volume: f32) -> Result<()> {
        self.audio.play_bgm(id, fade, loops, volume)?;
        self.events
            .push(MusicEvent::Play(id, fade, loops, volume.to_bits()));
        Ok(())
    }
    fn stop_bgm(&mut self, fade: i32) {
        self.audio.stop_bgm(fade);
        self.events.push(MusicEvent::Stop(fade));
    }
    fn volume_bgm(&mut self, volume: f32, fade: i32) -> Result<()> {
        self.audio.volume_bgm(volume, fade)
    }
    fn bgm_status(&self) -> i32 {
        self.audio.bgm_status()
    }
    fn bgm_clock(&self) -> u64 {
        self.audio.bgm_clock()
    }
}
#[derive(Default)]
struct Frames {
    number: u32,
}
impl Frames {
    fn step(
        &mut self,
        owner: &mut Owner,
        music: &mut Music,
        delta: f32,
        permission: bool,
        buttons: Buttons,
        events: &[PointerEvent],
    ) {
        self.number = self.number.wrapping_add(1);
        owner
            .poll(
                Ticks::from_f32(delta),
                permission,
                buttons,
                &PointerFrame {
                    number: self.number,
                    time_seconds: self.number as f64 / 60.,
                    events,
                },
                music,
            )
            .unwrap();
    }
    fn idle(&mut self, owner: &mut Owner, music: &mut Music, delta: f32) {
        self.step(owner, music, delta, true, Buttons::default(), &[]);
    }
    fn button(
        &mut self,
        owner: &mut Owner,
        music: &mut Music,
        delta: f32,
        pressed: u32,
        repeat: u32,
    ) {
        self.step(
            owner,
            music,
            delta,
            true,
            Buttons {
                pressed,
                repeat,
                ..Default::default()
            },
            &[],
        );
    }
    fn until(
        &mut self,
        owner: &mut Owner,
        music: &mut Music,
        delta: f32,
        predicate: impl Fn(&Owner) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !predicate(owner) {
            self.idle(owner, music, delta);
            assert!(
                Instant::now() < deadline,
                "ScenarioSelect did not reach expected state: {:?}, Question {:?}",
                owner.reference_state(),
                owner.reference_question()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn open(&mut self, owner: &mut Owner, music: &mut Music, count: &AtomicUsize) {
        self.until(owner, music, 0., |v| v.reference_state().1);
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert_eq!(owner.reference_state().3, Some(1));
        self.idle(owner, music, 30.);
        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "Fade completion is checked BEFORE ticking"
        );
        self.idle(owner, music, 0.);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(owner.reference_state().3, Some(-1));
        self.idle(owner, music, 0.); // Native prune retires First on the NEXT poll.
        assert_eq!(owner.reference_state().3, None);
        self.idle(owner, music, 120.);
        self.idle(owner, music, 0.);
        assert!(owner.reference_state().2);
    }
    fn question(&mut self, owner: &mut Owner, music: &mut Music) {
        self.button(owner, music, 8., 1 << 25, 0);
        self.idle(owner, music, 0.);
        self.until(owner, music, 3., |v| v.reference_question() == Some((2, 1)));
    }
}
fn pointer(kind: PointerKind, logical: Vec2, extent: (u16, u16)) -> PointerEvent {
    PointerEvent {
        kind,
        id: 71,
        position: IVec2::new(
            (logical.x * f32::from(extent.0) / 1920.) as i32,
            (logical.y * f32::from(extent.1) / 1080.) as i32,
        ),
    }
}
fn finish_adv(
    frames: &mut Frames,
    owner: &mut Owner,
    music: &mut Music,
    count: &AtomicUsize,
    source: &RuntimeGameData,
    scenario_id: i32,
) {
    assert_eq!(count.load(Ordering::SeqCst), 2); // Entrance + deciding shuffle.
    assert!(owner.reference_model().closing);
    assert_eq!(owner.data(), source);
    frames.idle(owner, music, 120.);
    frames.idle(owner, music, 0.);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert!(owner.pending_request().is_none());
    frames.idle(owner, music, 60.);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    frames.idle(owner, music, 0.);
    assert_eq!(count.load(Ordering::SeqCst), 3);
    let mut expected = source.clone();
    expected.progress.set_active_index(0).unwrap();
    let record = &mut expected.progress.entries[0];
    record.scenario_id = scenario_id;
    record.random_seed = seed(2);
    record.save_position = 0;
    record.replay_target = 0;
    assert_eq!(
        owner.pending_request(),
        Some(&SceneRequest::Adv {
            data: expected.clone(),
            flags: 0
        })
    );
    assert_eq!(
        owner.take_request(),
        Some(SceneRequest::Adv {
            data: expected,
            flags: 0
        })
    );
    frames.idle(owner, music, 0.);
    assert!(owner.pending_request().is_none());
    assert_eq!(count.load(Ordering::SeqCst), 3);
}

#[test]
#[ignore = "real ScenarioSelect assets/publication/retirement and captured texture transport;offscreen Metal/the default audio output"]
fn higurashi_scenario_select_owner_real_publication_gates_and_first_retirement() {
    let fixture = Fixture::new();
    let settings = SaveData::default();
    for extent in EXTENTS {
        for publication in [ReferencePublication::Atlas, ReferencePublication::Mask] {
            for cancel in [true, false] {
                let original = source(1);
                let (next_seed, count) = seeds();
                let mut capture = fixture.capture(extent);
                let (publish, reached) = mpsc::sync_channel::<ReferenceResources>(1);
                let (release, hold) = futures::channel::oneshot::channel::<()>();
                let gate: ReferenceGate = Box::new(move |resources| {
                    Box::pin(async move {
                        publish.send(resources).unwrap();
                        let _ = hold.await;
                    })
                });
                let mut owner = Owner::new_with_reference_gate(
                    fixture.environment(),
                    0,
                    0,
                    false,
                    &original,
                    &settings,
                    extent,
                    next_seed,
                    &mut capture,
                    publication,
                    gate,
                )
                .unwrap();
                assert!(capture.is_none());
                let resources = reached.recv_timeout(Duration::from_secs(20)).unwrap();
                assert!(resources.atlas.upgrade().is_some());
                assert_eq!(
                    resources.mask.is_some(),
                    publication == ReferencePublication::Mask
                );
                let mut frames = Frames::default();
                let mut music = fixture.music();
                frames.idle(&mut owner, &mut music, 0.);
                frames.idle(&mut owner, &mut music, 17.);
                assert_eq!(owner.reference_state(), (true, false, false, Some(0), 0));
                assert_eq!(count.load(Ordering::SeqCst), 0);
                assert!(music.events.is_empty());
                assert_eq!(owner.data(), &original);
                let image = fixture.draw(&owner, extent);
                assert!(
                    image.chunks_exact(4).all(|pixel| pixel == CAPTURE_COLOR),
                    "Actual retained First capture must cover the ready parent while loading"
                );
                if cancel {
                    drop(owner); // Gate remains held: drop must cancel+join, never await its sender.
                    assert!(resources.atlas.upgrade().is_none());
                    if let Some(mask) = resources.mask {
                        assert!(mask.upgrade().is_none());
                    }
                    assert!(release.send(()).is_err(), "Loader future must have retired");
                    assert_eq!(count.load(Ordering::SeqCst), 0);
                    continue;
                }
                release.send(()).unwrap();
                frames.until(&mut owner, &mut music, 0., |v| v.reference_state().1);
                assert_eq!(music.events, [MusicEvent::Play(134, 60, 0, 1f32.to_bits())]);
                assert_eq!(count.load(Ordering::SeqCst), 0);
                // Obtain a Weak only; no test-owned Arc may delay First retirement.
                let mask = resources.mask.unwrap_or_else(|| {
                    let loaded: Arc<crate::asset::mask::MaskTexture> =
                        fixture.server.load_sync("/default.msk").unwrap();
                    Arc::downgrade(&loaded)
                });
                frames.idle(&mut owner, &mut music, 15.);
                assert_ne!(
                    fixture.draw(&owner, extent),
                    image,
                    "Real simple fade must expose current artwork"
                );
                frames.idle(&mut owner, &mut music, 15.);
                assert_eq!(count.load(Ordering::SeqCst), 0);
                frames.idle(&mut owner, &mut music, 0.);
                assert_eq!(owner.reference_state().3, Some(-1));
                assert!(
                    mask.upgrade().is_some(),
                    "Completed First stays through current snapshot"
                );
                frames.idle(&mut owner, &mut music, 0.);
                assert_eq!(owner.reference_state().3, None);
                assert!(
                    mask.upgrade().is_none(),
                    "Following native prune releases First mask"
                );
                assert!(
                    resources.atlas.upgrade().is_some(),
                    "Parent atlas outlives First"
                );
                drop(owner);
                assert!(resources.atlas.upgrade().is_none());
            }
        }
    }
    eprintln!(
        "ScenarioSelect owner:8 real atlas/mask publication-gate cases, held cancellation and completion/prune, capture transport at720p/1080p;shader independence belongs to frozen literal renderer tests"
    );
}

#[test]
#[ignore = "real ScenarioSelect constructor preservation and all six ADV payloads/entropy sites;offscreen Metal/the default audio output"]
fn higurashi_scenario_select_owner_preflight_and_six_kind_full_adv_payloads() {
    let fixture = Fixture::new();
    let settings = SaveData::default();
    let settings_before = serde_json::to_value(&settings).unwrap();
    let original = source(1);
    let retained = original.clone();
    let (next_seed, count) = seeds();
    let mut capture = fixture.capture(EXTENTS[0]);
    let view_before = capture.as_ref().unwrap().as_texture_source().view as *const _ as usize;
    for (kind, selected, append, extent) in [
        (6, 0, false, EXTENTS[0]),
        (u32::MAX, 0, true, EXTENTS[0]),
        (0, -1, false, EXTENTS[0]),
        (0, 9, false, EXTENTS[0]),
        (4, 4, false, EXTENTS[0]),
        (4, 5, true, EXTENTS[0]),
        (5, 10, true, EXTENTS[0]),
        (0, 0, false, (0, 720)),
        (0, 0, false, (1280, 0)),
    ] {
        assert!(
            Owner::new_retained(
                fixture.environment(),
                kind,
                selected,
                append,
                &original,
                &settings,
                extent,
                next_seed.clone(),
                &mut capture
            )
            .is_err()
        );
        assert_eq!(
            capture.as_ref().unwrap().as_texture_source().view as *const _ as usize,
            view_before
        );
        assert_eq!(capture.as_ref().unwrap().dimensions(), (1280, 720));
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert_eq!(original, retained);
        assert_eq!(serde_json::to_value(&settings).unwrap(), settings_before);
    }
    drop(capture);
    for (index, extent) in EXTENTS.into_iter().enumerate() {
        for kind in 0..6 {
            let original = source(index);
            let before = original.clone();
            let (next_seed, count) = seeds();
            let mut capture = fixture.capture(extent);
            let mut owner = Owner::new_retained(
                fixture.environment(),
                kind,
                0,
                index != 0,
                &original,
                &settings,
                extent,
                next_seed,
                &mut capture,
            )
            .unwrap();
            assert_eq!(count.load(Ordering::SeqCst), 0);
            assert_eq!(
                owner.reference_model().count,
                match kind {
                    0 | 3 => 9,
                    1 | 2 => 6,
                    4 => 4 + index,
                    5 => 10,
                    _ => unreachable!(),
                }
            );
            let mut music = fixture.music();
            let mut frames = Frames::default();
            frames.open(&mut owner, &mut music, &count);
            let expected_bgm = [134, 135, 132, 82, 131, 221][kind as usize];
            assert_eq!(
                music.events,
                [MusicEvent::Play(expected_bgm, 60, 0, 1f32.to_bits())]
            );
            frames.button(&mut owner, &mut music, 0., 1 << 24, 0);
            finish_adv(
                &mut frames,
                &mut owner,
                &mut music,
                &count,
                &original,
                [100, 200, 300, 400, 500, 10001][kind as usize],
            );
            assert_eq!(original, before);
            assert_eq!(
                music
                    .events
                    .iter()
                    .filter(|event| **event == MusicEvent::Stop(60))
                    .count(),
                1
            );
        }
    }
    eprintln!(
        "ScenarioSelect owner:9 pure preflight failures preserve capture/data/settings/entropy;12 real six-kind/two-record/four-vector ADV transfers with exact three seed boundaries and append-specific rowcounts"
    );
}

#[test]
#[ignore = "real ScenarioSelect controller/contact/capture denial/cancel/resize dispatch;offscreen Metal/the default audio output"]
fn higurashi_scenario_select_owner_input_priority_locked_rows_and_contact_boundaries() {
    let fixture = Fixture::new();
    for mut extent in EXTENTS {
        let original = source(1);
        let (next_seed, count) = seeds();
        let mut capture = None; // Native still performs the same thirty-tick clock.
        let mut owner = Owner::new_retained(
            fixture.environment(),
            0,
            0,
            false,
            &original,
            &SaveData::default(),
            extent,
            next_seed,
            &mut capture,
        )
        .unwrap();
        let mut music = fixture.music();
        let mut frames = Frames::default();
        frames.open(&mut owner, &mut music, &count);
        frames.button(
            &mut owner,
            &mut music,
            0.,
            0,
            (1 << 4) | (1 << 6) | (1 << 7),
        );
        assert_eq!(owner.reference_model().selected, 0); // Up priority, clamped.
        frames.button(&mut owner, &mut music, 0., 0, 1 << 7);
        assert_eq!(owner.reference_model().selected, 1);
        frames.button(&mut owner, &mut music, 0., 0, 1 << 5);
        assert_eq!(owner.reference_model().selected, 2);
        frames.button(&mut owner, &mut music, 0., 0, 1 << 7);
        assert_eq!(owner.reference_model().selected, 1);
        frames.button(&mut owner, &mut music, 0., 0, (1 << 6) | (1 << 7));
        assert_eq!(owner.reference_model().selected, 3);
        assert_eq!(owner.reference_model().mask & (1 << 3), 0);
        frames.button(&mut owner, &mut music, 0., 1 << 24, 0);
        assert!(!owner.reference_model().closing);
        assert!(!owner.reference_model().hot[3].is_idle());
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "Locked feedback never samples a shuffle seed"
        );
        frames.step(
            &mut owner,
            &mut music,
            0.,
            false,
            Buttons {
                pressed: 1 << 24,
                repeat: 1 << 7,
                ..Default::default()
            },
            &[],
        );
        assert_eq!(owner.reference_model().selected, 3);
        assert!(!owner.reference_model().closing);
        let point = Vec2::new(1200., owner.reference_model().base_y() + 30.);
        frames.step(
            &mut owner,
            &mut music,
            0.,
            true,
            Buttons::default(),
            &[pointer(PointerKind::Down, point, extent)],
        );
        extent = if extent == EXTENTS[0] {
            EXTENTS[1]
        } else {
            EXTENTS[0]
        };
        owner.resize(extent).unwrap();
        frames.step(
            &mut owner,
            &mut music,
            0.,
            true,
            Buttons::default(),
            &[pointer(PointerKind::Up, point, extent)],
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(!owner.reference_model().closing);
        for explicit_cancel in [false, true] {
            frames.step(
                &mut owner,
                &mut music,
                0.,
                true,
                Buttons::default(),
                &[pointer(PointerKind::Down, point, extent)],
            );
            if explicit_cancel {
                owner.cancel_input();
            } else {
                frames.step(&mut owner, &mut music, 0., false, Buttons::default(), &[]);
            }
            frames.step(
                &mut owner,
                &mut music,
                0.,
                true,
                Buttons::default(),
                &[pointer(PointerKind::Up, point, extent)],
            );
            assert_eq!(count.load(Ordering::SeqCst), 1);
            assert!(!owner.reference_model().closing);
        }
        frames.step(
            &mut owner,
            &mut music,
            0.,
            true,
            Buttons::default(),
            &[pointer(PointerKind::Down, point, extent)],
        );
        owner.resize(extent).unwrap(); // Same-size call must preserve the held capture.
        frames.step(
            &mut owner,
            &mut music,
            0.,
            true,
            Buttons {
                pressed: 1 << 24,
                repeat: 1 << 6,
                ..Default::default()
            },
            &[pointer(PointerKind::Up, point, extent)],
        );
        assert_eq!(
            owner.reference_model().selected,
            0,
            "Pointer release wins over controller movement"
        );
        finish_adv(&mut frames, &mut owner, &mut music, &count, &original, 100);
    }
    eprintln!(
        "ScenarioSelect owner:2 actual input flows, native direction priority/locked error/no seed, permission/cancel/resized capture retirement and unchanged-extent held release priority"
    );
}

#[test]
#[ignore = "real ScenarioSelect Question glyphs/defaultNo/cancel/Yes stage0 and stage1 retirement;offscreen Metal/the default audio output"]
fn higurashi_scenario_select_owner_real_question_back_preserves_full_source_and_stage_order() {
    let fixture = Fixture::new();
    for extent in EXTENTS {
        let original = source(1);
        let (next_seed, count) = seeds();
        let mut capture = fixture.capture(extent);
        let mut owner = Owner::new_retained(
            fixture.environment(),
            0,
            0,
            false,
            &original,
            &SaveData::default(),
            extent,
            next_seed,
            &mut capture,
        )
        .unwrap();
        let mut music = fixture.music();
        let mut frames = Frames::default();
        frames.open(&mut owner, &mut music, &count);
        frames.question(&mut owner, &mut music);
        assert_eq!(owner.reference_question(), Some((2, 1)));
        let pixels = fixture.draw(&owner, extent); // Real Question glyph prepare + retained mixed child draw.
        assert!(
            pixels
                .chunks_exact(4)
                .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
        );
        frames.button(&mut owner, &mut music, 0., 1 << 24, 0); // Actual default-No control path.
        assert_eq!(owner.reference_question(), Some((3, 1)));
        frames.until(&mut owner, &mut music, 3., |v| {
            v.reference_question().is_none()
        });
        assert!(!owner.reference_model().closing);
        assert!(owner.pending_request().is_none());
        assert_eq!(owner.data(), &original);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        frames.question(&mut owner, &mut music);
        frames.button(&mut owner, &mut music, 0., 1 << 25, 0); // Real cancellable Question callback-1.
        assert_eq!(owner.reference_question(), Some((3, -1)));
        frames.until(&mut owner, &mut music, 3., |v| {
            v.reference_question().is_none()
        });
        assert!(!owner.reference_model().closing);
        assert!(owner.pending_request().is_none());
        assert_eq!(count.load(Ordering::SeqCst), 1);
        frames.question(&mut owner, &mut music);
        frames.button(&mut owner, &mut music, 0., 0, 1 << 7);
        assert_eq!(owner.reference_question(), Some((2, 0)));
        frames.button(&mut owner, &mut music, 0., 1 << 24, 0);
        assert_eq!(owner.reference_question(), Some((3, 0)));
        assert!(
            !owner.reference_model().closing,
            "Answer is not acknowledged on the input poll"
        );
        frames.idle(&mut owner, &mut music, 8.);
        assert!(!owner.reference_model().closing);
        frames.idle(&mut owner, &mut music, 0.);
        assert_eq!(owner.reference_question(), Some((4, 0)));
        assert!(
            owner.reference_model().closing,
            "Real stage0 Yes closes before Question stage1"
        );
        assert!(owner.pending_request().is_none());
        frames.idle(&mut owner, &mut music, 0.); // New ExitOpacity poll stops BGM exactly once.
        assert_eq!(
            music
                .events
                .iter()
                .filter(|e| **e == MusicEvent::Stop(60))
                .count(),
            1
        );
        frames.idle(&mut owner, &mut music, 12.);
        frames.idle(&mut owner, &mut music, 5.); // Real stage1 now executes after opacity + cooldown.
        assert_eq!(owner.reference_question(), Some((-1, 0)));
        assert_eq!(
            music
                .events
                .iter()
                .filter(|e| **e == MusicEvent::Stop(60))
                .count(),
            1
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        frames.idle(&mut owner, &mut music, 0.);
        assert!(owner.reference_question().is_none());
        frames.idle(&mut owner, &mut music, 60.);
        frames.idle(&mut owner, &mut music, 0.);
        assert_eq!(
            owner.take_request(),
            Some(SceneRequest::Adv {
                data: original.clone(),
                flags: 0
            })
        );
        frames.idle(&mut owner, &mut music, 0.);
        assert!(owner.pending_request().is_none());
        assert_eq!(owner.data(), &original);
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "Back neither shuffles rows nor creates an ADV seed"
        );
        assert_eq!(
            music
                .events
                .iter()
                .filter(|e| **e == MusicEvent::Stop(60))
                .count(),
            1,
            "Question stage1 must not start a second return/fade"
        );
    }
    eprintln!(
        "ScenarioSelect owner:2 real Question flows/defaultNo,physical controller cancel,Yes stage0 before retained stage1,full source/date/vectors returned once without additional seeds;no direct question_callback substitution"
    );
}

#[test]
#[ignore = "real Origin shared owner Tips/Title factories and seed sites;offscreen Metal/the default audio output"]
fn higurashi_scenario_select_owner_origin_tips_and_title_requests() {
    let fixture = Fixture::new();
    for extent in EXTENTS {
        for tips in [true, false] {
            let original = source(0);
            let (next_seed, count) = seeds();
            let mut capture = fixture.capture(extent);
            let mut owner = Owner::new_retained(
                fixture.environment(),
                5,
                if tips { 9 } else { 0 },
                true,
                &original,
                &SaveData::default(),
                extent,
                next_seed,
                &mut capture,
            )
            .unwrap();
            let mut music = fixture.music();
            let mut frames = Frames::default();
            frames.open(&mut owner, &mut music, &count);
            if tips {
                frames.button(&mut owner, &mut music, 0., 1 << 24, 0);
                assert_eq!(count.load(Ordering::SeqCst), 2);
                frames.idle(&mut owner, &mut music, 120.);
                frames.idle(&mut owner, &mut music, 0.);
                assert_eq!(
                    owner.take_request(),
                    Some(SceneRequest::Tips {
                        selected: 0,
                        alternate: true
                    })
                );
                assert_eq!(count.load(Ordering::SeqCst), 2);
                assert!(
                    !music
                        .events
                        .iter()
                        .any(|e| matches!(e, MusicEvent::Stop(_)))
                );
            } else {
                frames.button(&mut owner, &mut music, 8., 1 << 25, 0);
                assert!(owner.pending_request().is_none());
                assert!(!owner.reference_model().closing);
                frames.idle(&mut owner, &mut music, 0.);
                assert_eq!(
                    owner.take_request(),
                    Some(SceneRequest::Title {
                        selected: 8,
                        mode: -1
                    })
                );
                assert_eq!(count.load(Ordering::SeqCst), 1);
                assert_eq!(music.events.last(), Some(&MusicEvent::Stop(30)));
                assert!(owner.reference_model().closing);
            }
            assert!(owner.reference_question().is_none());
            assert_eq!(owner.data(), &original);
            frames.idle(&mut owner, &mut music, 0.);
            assert!(owner.pending_request().is_none());
        }
    }
    eprintln!(
        "ScenarioSelect owner:4 real Origin requests/Tips0+alternate1/Title8+mode-1,flash boundary and precise one/two seed counts;source data retained"
    );
}
