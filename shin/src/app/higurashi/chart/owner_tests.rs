//! Unregistered concrete Chart-owner acceptance candidates. Parent alone runs
//! these with real assets, offscreen GPU and the default audio output. The draws
//! below check resource/owner transport, not independent native shader math.
use super::{
    catalog::Catalog,
    owner::{Owner, ReferenceGate, Request},
};
use crate::{
    adv::{
        higurashi::{
            autosave::{ProgressEntry, SaveProgress},
            input::Buttons,
            question::Question,
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
use glam::{IVec2, Vec2};
use parking_lot::Mutex;
use shin_core::{
    format::{save::higurashi::SaveDate, text::read_sjis_string},
    primitives::color::UnormColor,
    time::Ticks,
};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const EXTENTS: [(u16, u16); 2] = [(1280, 720), (1920, 1080)];
const SEED: u32 = 0x93a5_b7c9;
fn seeds() -> (Arc<dyn Fn() -> u32 + Send + Sync>, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    (
        Arc::new(move || SEED.wrapping_add(calls.fetch_add(1, Ordering::SeqCst) as u32)),
        count,
    )
}
struct Fixture {
    device: wgpu::Device,
    queue: wgpu::Queue,
    server: Arc<AssetServer>,
    common: Arc<CommonAssets>,
    sounds: Arc<Mutex<SystemSoundPlayer>>,
    // Retain the exact-device manager through every owner and sound lifetime.
    _manager: Arc<shin_audio::AudioManager>,
    catalog: Arc<Catalog>,
    unlocks: Vec<u32>,
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
        let catalog = Arc::new(Catalog::from_scenario(&common.scenario).unwrap());
        assert_eq!(
            catalog.records.len(),
            1449,
            "Use the updated authored catalog"
        );
        let max_id = catalog
            .records
            .iter()
            .filter(|r| r.kind == 0)
            .map(|r| usize::from(r.value))
            .max()
            .unwrap();
        let unlocks = vec![u32::MAX; max_id / 32 + 1];
        let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
        let mut sounds = SystemSoundPlayer::new(manager.clone());
        sounds.bank = Some(common.sysse.clone());
        Self {
            device,
            queue,
            server,
            common,
            sounds: Arc::new(Mutex::new(sounds)),
            _manager: manager,
            catalog,
            unlocks,
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
    fn owner(
        &self,
        group: i32,
        extent: (u16, u16),
        next_seed: Arc<dyn Fn() -> u32 + Send + Sync>,
    ) -> Owner {
        Owner::new(
            self.environment(),
            self.catalog.clone(),
            group,
            30,
            &self.unlocks,
            &[],
            extent,
            next_seed,
        )
        .unwrap()
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
                    "Concrete older Chart owner",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                owner.render(&mut pass).unwrap();
            },
        )
    }
    fn question_point(&self, owner: &Owner, index: i32, answer: usize) -> Vec2 {
        // Only choose a fixture contact from the same real font/layout. This
        // auxiliary pure layout is not the tested child or a pixel oracle.
        let n = &owner.model().nodes[index as usize];
        let label = self.catalog.records[n.record].label.as_ref().unwrap();
        let text = read_sjis_string(&mut &label.c_string_bytes()[..], None).unwrap();
        let prompt = format!("『{text}』へのショートカットを実行します。@rよろしいですか？");
        let q = Question::from_text(
            self.common.font.as_ref(),
            &prompt,
            &["はい", "いいえ"],
            1,
            true,
        )
        .unwrap();
        let r = q.buttons()[answer].rect;
        q.origin() + Vec2::new(30. + r.x + r.z * 0.5, 46. + r.y + r.w * 0.5)
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
        delta: f32,
        permission: bool,
        buttons: Buttons,
        axes: [i8; 4],
        events: &[PointerEvent],
    ) {
        self.number = self.number.wrapping_add(1);
        owner
            .poll(
                Ticks::from_f32(delta),
                permission,
                super::model::Input {
                    buttons,
                    axes,
                    ..Default::default()
                },
                &PointerFrame {
                    number: self.number,
                    time_seconds: self.number as f64 / 60.,
                    events,
                },
            )
            .unwrap();
    }
    fn idle(&mut self, owner: &mut Owner, delta: f32) {
        self.step(owner, delta, true, Buttons::default(), [0; 4], &[]);
    }
    fn button(&mut self, owner: &mut Owner, pressed: u32, repeat: u32) {
        self.step(
            owner,
            0.,
            true,
            Buttons {
                pressed,
                repeat,
                ..Default::default()
            },
            [0; 4],
            &[],
        );
    }
    fn until(&mut self, owner: &mut Owner, delta: f32, predicate: impl Fn(&Owner) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !predicate(owner) {
            self.idle(owner, delta);
            assert!(
                Instant::now() < deadline,
                "Chart did not reach expected state: {:?}, Question {:?}",
                owner.reference_state(),
                owner.reference_question()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn open(&mut self, owner: &mut Owner, count: &AtomicUsize) {
        self.until(owner, 0., |v| v.reference_state().1);
        assert_eq!(owner.model().progress.to_bits(), 0f32.to_bits());
        self.idle(owner, 6.);
        self.idle(owner, 0.); // Open's newly pushed Input starts on the next snapshot.
        assert!(owner.reference_state().2);
        assert_eq!(owner.model().progress.to_bits(), 1f32.to_bits());
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }
    fn contact(&mut self, owner: &mut Owner, kind: PointerKind, logical: Vec2, extent: (u16, u16)) {
        self.step(
            owner,
            0.,
            true,
            Buttons::default(),
            [0; 4],
            &[pointer(kind, logical, extent)],
        );
    }
    fn question(&mut self, owner: &mut Owner, extent: (u16, u16)) -> i32 {
        // Select an authored visible label via the real InputArea. Follow its
        // first selection with A if needed; never replace it with direct writes.
        let (index, point) = visible_label(owner);
        self.contact(owner, PointerKind::Down, point, extent);
        self.contact(owner, PointerKind::Up, point, extent);
        if owner.reference_question().is_none() {
            assert_eq!(owner.reference_state().5, index);
            self.button(owner, 1 << 24, 0);
        }
        self.until(owner, 3., |v| {
            v.reference_question()
                .is_some_and(|(_, phase, _)| phase == 2)
        });
        assert_eq!(owner.reference_question(), Some((index, 2, 1)));
        index
    }
    fn stage0(&mut self, owner: &mut Owner, index: i32, answer: i32) {
        assert_eq!(owner.reference_question(), Some((index, 3, answer)));
        if answer >= 0 {
            self.idle(owner, 8.); // Input already returned; finish its feedback clock.
        }
        self.idle(owner, 0.); // Original next callback emits stage0 and closes Question.
        assert_eq!(owner.reference_question(), Some((index, 4, answer)));
        assert_eq!(
            owner.model().choice,
            -1,
            "Stage0 must NOT select a Chart node"
        );
        assert!(!owner.model().closing);
        assert!(owner.pending_request().is_none());
    }
    fn stage1(&mut self, owner: &mut Owner, index: i32, answer: i32) {
        self.idle(owner, 12.);
        assert_eq!(owner.reference_question(), Some((index, 4, answer)));
        self.idle(owner, 5.);
        assert_eq!(owner.reference_question(), Some((index, -1, answer)));
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
fn visible_label(owner: &Owner) -> (i32, Vec2) {
    let m = owner.model();
    let g = &m.groups[m.current_group as usize];
    m.nodes
        .iter()
        .filter(|n| n.group == m.current_group)
        .find_map(|n| {
            let r = &m.catalog.records[n.record];
            let p = Vec2::new(
                (g.origin_x + n.x * 36 + 18) as f32,
                (g.origin_y + n.y * 56 + 28 + 165) as f32 - g.scroll.value(),
            );
            (r.kind == 1
                && (r.value as i16) >= 0
                && p.x > 100.
                && p.x < 1720.
                && p.y > 200.
                && p.y < 950.)
                .then_some((n.index, p))
        })
        .expect("Real updated Chart has an unlocked, visible nonnegative shortcut")
}
fn assert_visible(image: &[u8], extent: (u16, u16)) {
    assert_eq!(
        image.len(),
        usize::from(extent.0) * usize::from(extent.1) * 4
    );
    let mut colors = Vec::<[u8; 4]>::new();
    for pixel in image.chunks_exact(4) {
        let pixel: [u8; 4] = pixel.try_into().unwrap();
        if !colors.contains(&pixel) {
            colors.push(pixel);
            if colors.len() == 3 {
                break;
            }
        }
    }
    assert_eq!(
        colors.len(),
        3,
        "Actual decoded Chart must have spatial content"
    );
}
fn expected_adv(group: i32, scenario: i32) -> RuntimeGameData {
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
            group as u32,
            [
                ProgressEntry {
                    scenario_id: scenario,
                    random_seed: SEED,
                    ..empty.clone()
                },
                empty,
            ],
        )
        .unwrap(),
        // Original leaves Date/dormant scalar bytes unwritten; explicit zeros
        // here test the documented desktop initialization boundary, not native bytes.
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
fn assert_adv(request: &Request, expected: &RuntimeGameData) {
    match request {
        Request::StartAdv { data, flags } => {
            assert_eq!(
                data, expected,
                "Both records and all four vectors must match"
            );
            assert_eq!(*flags, 0);
        }
        other => panic!("Expected selected full ADV payload, got {other:?}"),
    }
}

#[test]
#[ignore = "real Chart atlas worker publication/cancel/join/prune;offscreen GPU/the default audio output"]
fn higurashi_chart_owner_real_atlas_publication_cancel_and_first_prune() {
    let fixture = Fixture::new();
    for extent in EXTENTS {
        for cancel in [true, false] {
            let (next_seed, count) = seeds();
            let (publish, reached) = mpsc::sync_channel(1);
            let (release, hold) = futures::channel::oneshot::channel::<()>();
            let gate: ReferenceGate = Box::new(move |atlas| {
                Box::pin(async move {
                    publish.send(atlas).unwrap();
                    let _ = hold.await;
                })
            });
            let mut owner = Owner::new_with_reference_gate(
                fixture.environment(),
                fixture.catalog.clone(),
                -1,
                30,
                &fixture.unlocks,
                &[],
                extent,
                next_seed,
                gate,
            )
            .unwrap();
            let atlas = reached.recv_timeout(Duration::from_secs(20)).unwrap();
            assert!(atlas.upgrade().is_some());
            let mut frames = Frames::default();
            frames.idle(&mut owner, 0.);
            frames.idle(&mut owner, 17.);
            let state = owner.reference_state();
            assert_eq!(
                (state.0, state.1, state.2, state.3),
                (true, false, false, 0)
            );
            assert_eq!(
                owner.model().frame,
                2,
                "Parent tail runs while First is pending"
            );
            assert!(owner.reference_first_job().is_some_and(|(task, _)| task));
            assert_eq!(count.load(Ordering::SeqCst), 0);
            assert!(owner.pending_request().is_none());
            let image = fixture.draw(&owner, extent); // Actual published atlas, still progress0.
            assert_eq!(
                image.len(),
                usize::from(extent.0) * usize::from(extent.1) * 4
            );
            if cancel {
                drop(owner); // Join/cancel the SAME held loader; never release its gate first.
                assert!(atlas.upgrade().is_none());
                assert!(
                    release.send(()).is_err(),
                    "Actual loader future must have retired"
                );
            } else {
                release.send(()).unwrap();
                frames.until(&mut owner, 0., |v| v.reference_state().1);
                assert_eq!(owner.model().progress.to_bits(), 0f32.to_bits());
                assert!(owner.reference_first_job().is_some_and(|(task, _)| !task));
                assert!(atlas.upgrade().is_some());
                frames.idle(&mut owner, 0.); // Native done First is now pruned, before Open advances.
                assert_eq!(owner.reference_first_job(), None);
                assert!(
                    atlas.upgrade().is_some(),
                    "Atlas is a separate retained parent owner"
                );
                frames.open(&mut owner, &count);
                assert_visible(&fixture.draw(&owner, extent), extent);
                assert!(!owner.has_save_task());
                drop(owner);
                assert!(atlas.upgrade().is_none());
            }
            assert_eq!(count.load(Ordering::SeqCst), 0);
        }
    }
    eprintln!(
        "Chart owner:4 actual published-worker flows;2 held cancellations/2 completed First prune lifetimes;zero seeds;6 retained resource-transport renders"
    );
}

#[test]
#[ignore = "real Chart preflight and actual native close-flash/source callback stages;offscreen GPU/the default audio output"]
fn higurashi_chart_owner_preflight_and_two_source_return_stages() {
    let fixture = Fixture::new();
    let (next_seed, count) = seeds();
    for (group, extent) in [
        (5, EXTENTS[0]),
        (i32::MAX, EXTENTS[1]),
        (-1, (0, 720)),
        (0, (1280, 0)),
    ] {
        let (called, reached) = mpsc::sync_channel(1);
        let gate: ReferenceGate =
            Box::new(move |_| Box::pin(async move { called.send(()).unwrap() }));
        assert!(
            Owner::new_with_reference_gate(
                fixture.environment(),
                fixture.catalog.clone(),
                group,
                30,
                &fixture.unlocks,
                &[],
                extent,
                next_seed.clone(),
                gate
            )
            .is_err()
        );
        assert!(matches!(
            reached.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }
    for extent in EXTENTS {
        for group in [-1, 2] {
            let (next_seed, count) = seeds();
            let mut owner = fixture.owner(group, extent, next_seed);
            let mut frames = Frames::default();
            frames.open(&mut owner, &count);
            assert_visible(&fixture.draw(&owner, extent), extent);
            if group >= 0 {
                frames.button(&mut owner, (1 << 10) | (1 << 11), 0);
                assert_eq!(
                    owner.model().current_group,
                    group,
                    "ADV Chart has no title group switching"
                );
            }
            frames.button(&mut owner, 1 << 25, 0);
            assert!(!owner.model().closing);
            assert!(owner.pending_request().is_none());
            frames.idle(&mut owner, 8.);
            assert!(
                owner.pending_request().is_none(),
                "CloseWait observes before clock advance"
            );
            frames.idle(&mut owner, 0.);
            assert!(owner.model().closing);
            assert!(!owner.complete());
            assert!(matches!(
                owner.take_request(),
                Some(Request::ReturnSource(0))
            ));
            assert!(owner.pending_request().is_none());
            frames.idle(&mut owner, 4.);
            assert!(!owner.complete());
            assert!(owner.pending_request().is_none());
            frames.idle(&mut owner, 2.);
            assert!(owner.complete());
            assert!(matches!(
                owner.take_request(),
                Some(Request::ReturnSource(1))
            ));
            frames.idle(&mut owner, 20.);
            assert!(owner.pending_request().is_none());
            assert_eq!(count.load(Ordering::SeqCst), 0);
        }
    }
    eprintln!(
        "Chart owner:4 preflight failures before First/entropy;4 actual title/ADV close flows deliver exactly ReturnSource0 then1 without selected payload/seed"
    );
}

#[test]
#[ignore = "real authored Chart labels/actual Question No/cancel/Yes stage1/full ADV payload;offscreen GPU/the default audio output"]
fn higurashi_chart_owner_real_question_stage1_and_full_selected_payload_once() {
    let fixture = Fixture::new();
    for initial_extent in EXTENTS {
        for group in [-1, 2] {
            let mut extent = initial_extent;
            let (next_seed, count) = seeds();
            let mut owner = fixture.owner(group, extent, next_seed);
            let mut frames = Frames::default();
            frames.open(&mut owner, &count);
            let index = frames.question(&mut owner, extent);
            assert_visible(&fixture.draw(&owner, extent), extent);
            if group < 0 {
                // The real child owns this held Yes contact. Both explicit
                // cancellation and changed-size resize must suppress its release.
                let yes = fixture.question_point(&owner, index, 0);
                for resize in [false, true] {
                    frames.contact(&mut owner, PointerKind::Down, yes, extent);
                    let state = owner.reference_question();
                    let frame = owner.model().frame;
                    let cues = fixture.sounds.lock().sounds.len();
                    if resize {
                        extent = if extent == EXTENTS[0] {
                            EXTENTS[1]
                        } else {
                            EXTENTS[0]
                        };
                        owner.resize(extent).unwrap();
                    } else {
                        owner.cancel_input();
                    }
                    assert_eq!(owner.reference_question(), state);
                    assert_eq!(
                        owner.model().frame,
                        frame,
                        "Cancellation must not poll clocks"
                    );
                    assert_eq!(fixture.sounds.lock().sounds.len(), cues);
                    frames.contact(&mut owner, PointerKind::Up, yes, extent);
                    assert_eq!(owner.reference_question(), Some((index, 2, 1)));
                }
                let no = fixture.question_point(&owner, index, 1);
                frames.contact(&mut owner, PointerKind::Down, no, extent);
                owner.resize(extent).unwrap(); // No-op must preserve the actual child capture.
                frames.contact(&mut owner, PointerKind::Up, no, extent);
            } else {
                frames.button(&mut owner, 1 << 24, 0); // Actual default-No controller path.
            }
            frames.stage0(&mut owner, index, 1);
            assert_eq!(count.load(Ordering::SeqCst), 0);
            frames.stage1(&mut owner, index, 1);
            assert_eq!(owner.model().choice, -1);
            assert!(!owner.model().closing);
            frames.until(&mut owner, 0., |v| v.reference_question().is_none());

            let index = frames.question(&mut owner, extent);
            frames.button(&mut owner, 1 << 25, 0);
            frames.stage0(&mut owner, index, -1);
            frames.stage1(&mut owner, index, -1);
            assert_eq!(owner.model().choice, -1);
            assert!(!owner.model().closing);
            assert!(owner.pending_request().is_none());
            assert_eq!(count.load(Ordering::SeqCst), 0);
            frames.until(&mut owner, 0., |v| v.reference_question().is_none());

            let index = frames.question(&mut owner, extent);
            frames.button(&mut owner, 0, 1 << 7); // Left selects Yes in both native layouts.
            assert_eq!(owner.reference_question(), Some((index, 2, 0)));
            frames.button(&mut owner, 1 << 24, 0);
            frames.stage0(&mut owner, index, 0);
            assert_eq!(count.load(Ordering::SeqCst), 0);
            assert_visible(&fixture.draw(&owner, extent), extent);
            frames.stage1(&mut owner, index, 0);
            assert_eq!(owner.model().choice, index);
            assert!(owner.model().closing);
            assert!(!owner.complete());
            assert!(
                owner.pending_request().is_none(),
                "New Exit must await its next snapshot"
            );
            assert_eq!(count.load(Ordering::SeqCst), 0);
            let model = owner.model();
            let selected_group = model.current_group;
            let scenario =
                i32::from(model.catalog.records[model.nodes[index as usize].record].value as i16);
            let expected = expected_adv(selected_group, scenario);
            frames.idle(&mut owner, 6.);
            assert_eq!(count.load(Ordering::SeqCst), 1);
            assert!(
                !owner.complete(),
                "A selected scene request is not ReturnSource completion"
            );
            assert_adv(owner.pending_request().unwrap(), &expected);
            let frozen = owner.pending_request().unwrap().clone();
            frames.idle(&mut owner, 20.); // A fallible caller can retain/retry the same owned request.
            assert_adv(owner.pending_request().unwrap(), &expected);
            assert_adv(&frozen, &expected);
            assert_adv(&owner.take_request().unwrap(), &expected);
            frames.idle(&mut owner, 20.);
            assert!(owner.pending_request().is_none());
            assert_eq!(count.load(Ordering::SeqCst), 1);
        }
    }
    eprintln!(
        "Chart owner:4 title/ADV selection flows,12 real Questions, stage0 inert/stage1 Yes only;2 child-contact cancel/resize flows;4 complete two-record/four-vector payloads with exactly one retained seed each;8 resource-transport renders"
    );
}

#[test]
#[ignore = "real Chart signed sticks/held D-pad/contact/permission/resize;offscreen GPU/the default audio output"]
fn higurashi_chart_owner_signed_axes_pointer_priority_and_resize_boundaries() {
    let fixture = Fixture::new();
    for mut extent in EXTENTS {
        let (next_seed, count) = seeds();
        let mut owner = fixture.owner(-1, extent, next_seed);
        let mut frames = Frames::default();
        frames.open(&mut owner, &count);
        frames.button(&mut owner, (1 << 10) | (1 << 11), 0);
        assert_eq!(
            owner.model().current_group,
            4,
            "L wins over R and wraps title groups"
        );
        frames.button(&mut owner, 1 << 11, 0);
        assert_eq!(owner.model().current_group, 0);

        // Choose an unlabelled interior point so this real tap moves the cursor
        // without dispatching a shortcut Question. The catalog remains authored.
        let point = (0..16)
            .map(|i| Vec2::new(600. + i as f32 * 40., 565.))
            .find(|p| {
                let m = owner.model();
                let g = &m.groups[m.current_group as usize];
                let x = p.x - g.origin_x as f32;
                let y = p.y - g.origin_y as f32 + g.scroll.value() - 165.;
                !m.nodes.iter().any(|n| {
                    n.group == m.current_group
                        && m.catalog.records[n.record].kind == 1
                        && x >= (n.x * 36 - 45) as f32
                        && x < (n.x * 36 + 81) as f32
                        && y >= (n.y * 56 - 7) as f32
                        && y < (n.y * 56 + 63) as f32
                })
            })
            .expect("Authored chart has a graph interior outside labels");
        frames.contact(&mut owner, PointerKind::Down, point, extent);
        frames.contact(&mut owner, PointerKind::Up, point, extent);
        assert!(owner.reference_question().is_none());
        assert_eq!(owner.reference_state().5, -1);
        let group = owner.model().current_group as usize;
        let g = &owner.model().groups[group];
        let (x, y) = (g.x.value(), g.y.value());
        assert!(x > (g.min_x * 36 + 20) as f32 && x < (g.max_x * 36 - 20) as f32);
        assert!(y > (g.min_y * 56 + 20) as f32 && y < (g.max_y * 56 - 20) as f32);
        frames.step(
            &mut owner,
            0.,
            true,
            Buttons {
                held: (1 << 5) | (1 << 4),
                ..Default::default()
            },
            [-127, 127, 88, 0],
            &[],
        );
        let g = &owner.model().groups[group];
        assert_eq!(
            (g.x.value(), g.y.value()),
            (x - 12., y + 12.),
            "Signed stick /10 overrides held D-pad"
        );
        for axes in [[9, -9, -128, 15], [-9, 9, 127, -15]] {
            frames.step(
                &mut owner,
                0.,
                true,
                Buttons {
                    held: (1 << 5) | (1 << 6),
                    ..Default::default()
                },
                axes,
                &[],
            );
            let g = &owner.model().groups[group];
            assert_eq!(
                (g.x.value(), g.y.value()),
                (x - 12., y + 12.),
                "Nonzero /10 truncates toward zero; second X ignored and Y deadzone includes ±15"
            );
        }
        for axis in [127, -128] {
            let g = &owner.model().groups[group];
            let (x, y, old) = (g.x.value(), g.y.value(), g.scroll.value());
            let end = (((g.max_y - g.min_y) * 56) as f32 - 819.).max(0.);
            let expected = (old + ((i32::from(axis) / 16) * 4) as f32).clamp(0., end);
            frames.step(
                &mut owner,
                0.,
                true,
                Buttons::default(),
                [-128, 127, 127, axis],
                &[],
            );
            let g = &owner.model().groups[group];
            assert_eq!(g.scroll.value(), expected);
            assert_eq!(
                (g.x.value(), g.y.value()),
                (x - 12., y + expected - old + 12.),
                "Native right-Y scroll is followed by signed first-stick pan"
            );
        }
        let g = &owner.model().groups[group];
        let (x, y) = (g.x.value(), g.y.value());
        frames.step(
            &mut owner,
            0.,
            true,
            Buttons {
                held: (1 << 7) | (1 << 4),
                ..Default::default()
            },
            [0; 4],
            &[],
        );
        let g = &owner.model().groups[group];
        assert_eq!((g.x.value(), g.y.value()), (x - 10., y - 10.));
        frames.button(&mut owner, 0, (1 << 5) | (1 << 6));
        let g = &owner.model().groups[group];
        assert_eq!(
            (g.x.value(), g.y.value()),
            (x - 10., y - 10.),
            "Chart digital movement reads held, not repeat"
        );
        let denied = (g.x.value(), g.y.value(), g.scroll.value());
        let frame = owner.model().frame;
        frames.step(
            &mut owner,
            7.,
            false,
            Buttons {
                pressed: (1 << 24) | (1 << 11),
                held: u32::MAX,
                ..Default::default()
            },
            [127, -128, 127, -128],
            &[],
        );
        let g = &owner.model().groups[group];
        assert_eq!((g.x.value(), g.y.value(), g.scroll.value()), denied);
        assert_eq!(owner.model().frame, frame.wrapping_add(1));
        assert_eq!(owner.model().current_group as usize, group);
        assert!(owner.reference_question().is_none());

        let close = Vec2::new(1800., 70.);
        for explicit in [true, false] {
            frames.contact(&mut owner, PointerKind::Down, close, extent);
            assert_eq!(owner.model().target, 1);
            if explicit {
                let frame = owner.model().frame;
                let cues = fixture.sounds.lock().sounds.len();
                owner.cancel_input();
                assert_eq!(owner.model().frame, frame);
                assert_eq!(fixture.sounds.lock().sounds.len(), cues);
            } else {
                frames.step(&mut owner, 0., false, Buttons::default(), [0; 4], &[]);
            }
            assert_eq!(owner.model().target, -1);
            frames.contact(&mut owner, PointerKind::Up, close, extent);
            assert!(owner.pending_request().is_none());
            assert!(owner.model().flashes[0].is_idle());
        }
        frames.contact(&mut owner, PointerKind::Down, close, extent);
        let frame = owner.model().frame;
        extent = if extent == EXTENTS[0] {
            EXTENTS[1]
        } else {
            EXTENTS[0]
        };
        owner.resize(extent).unwrap();
        assert_eq!(owner.model().target, -1);
        assert_eq!(owner.model().frame, frame);
        frames.contact(&mut owner, PointerKind::Up, close, extent);
        assert!(owner.model().flashes[0].is_idle());
        assert!(owner.pending_request().is_none());
        assert_visible(&fixture.draw(&owner, extent), extent);

        frames.contact(&mut owner, PointerKind::Down, close, extent);
        owner.resize(extent).unwrap();
        assert_eq!(
            owner.model().target,
            1,
            "Same-size resize must retain contact"
        );
        frames.step(
            &mut owner,
            0.,
            true,
            Buttons {
                pressed: 1 << 11,
                ..Default::default()
            },
            [0; 4],
            &[pointer(PointerKind::Up, close, extent)],
        );
        assert_eq!(
            owner.model().current_group as usize,
            group,
            "Actual pointer release consumes the shoulder input"
        );
        assert!(!owner.model().flashes[0].is_idle());
        frames.idle(&mut owner, 8.);
        frames.idle(&mut owner, 0.);
        assert!(matches!(
            owner.take_request(),
            Some(Request::ReturnSource(0))
        ));
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }
    eprintln!(
        "Chart owner:2 real signed four-byte stick/held digital/deadzone priority flows;permission leaves tail clocks alive;pointer cancellation/changed resize/no-op resize+release precedence;zero seeds;2 real resource-transport renders"
    );
}
