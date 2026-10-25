//! Outer Runtime routing, not a new native shader oracle. The unchanged native
//! Top/TIPS/TitleDemo component proofs supply their model and render contracts.
use super::*;
use crate::app::higurashi::{
    first_boot::CommonAssets, main_title::TitleDestination, runtime_game_data::RuntimeGameData,
};
use shin_core::format::save::higurashi::{GameData, GameDataEntry, SaveDate};

fn saved_rows() -> SaveData {
    let row = |index: u32| GameData {
        date: SaveDate {
            year: 2025,
            month: 10,
            day: 1 + (index % 28) as u8,
            hour: (index % 24) as u8,
            minute: (index % 60) as u8,
            second: ((index * 7) % 60) as u8,
        },
        extra_word: (index % 8) as u8,
        primary_entry: Some(GameDataEntry {
            scenario_id: index as i32,
            random_seed: 0x1234_0000 + index,
            save_position: index * 3,
            replay_target: index * 3,
            response_data: vec![index as u8, 0xf1, 0x55],
        }),
        entry: GameDataEntry {
            scenario_id: 1000 + index as i32,
            random_seed: 0x8765_0000 + index,
            save_position: index * 5,
            replay_target: index * 5,
            response_data: vec![0x23, index as u8, 0xee, 0x80],
        },
        trailing_values: vec![1023, index as u16, 0x123],
        byte_variables: vec![9, index as u8, 255],
    };
    let mut data = SaveData::default();
    data.play_seconds = 17;
    data.settings.common.v3_sysvol = 37;
    data.auto_save_slot = Some(row(0));
    data.manual_save_slots = std::array::from_fn(|index| Some(row(index as u32 + 1)));
    // Initial temporary progress only. No scene/count/worker state is seeded.
    // These physical banks prevent normal title/demo/BGM acquisitions from
    // obscuring whether the complete unrelated settings/rows survive transfer.
    data.save_vectors.unlocked_movies = vec![u32::MAX; 256];
    data.save_vectors.unlocked_bgms = vec![u32::MAX; 256];
    data.save_vectors.unlocked_cgs = vec![u32::MAX; 256];
    data.save_vectors.extra_words_0 = vec![u32::MAX; 20];
    data.save_vectors.seen_messages = vec![0xa5a5_a5a5, 0, 0x1234_5678];
    data.save_vectors.select_seen_choices = vec![0x4321, 0];
    data.save_vectors.select_last_choices = vec![1, 2, 15, 0];
    data
}

fn complete_rows(runtime: &Runtime) -> Vec<(bool, RuntimeGameData)> {
    let globals = runtime.globals().unwrap().unwrap();
    let slots = globals.runtime_saves.unwrap();
    std::iter::once(&slots.auto)
        .chain(slots.manual.iter())
        .map(|slot| (slot.valid, slot.data.clone()))
        .collect()
}

fn stable_progress(runtime: &Runtime) -> Vec<u8> {
    let mut data = runtime.globals().unwrap().unwrap().data.clone();
    data.play_seconds = 0; // The actual full elapsed global clock keeps running.
    data.encode_raw().unwrap()
}

struct Retained {
    common: Arc<CommonAssets>,
    server: Arc<AssetServer>,
    manager: Arc<shin_audio::AudioManager>,
    sounds: Arc<Mutex<SystemSoundPlayer>>,
    storage: Arc<dyn Storage>,
    platform_flag: bool,
    rows: Vec<(bool, RuntimeGameData)>,
    progress: Vec<u8>,
    clock: f64,
}
impl Retained {
    fn new(runtime: &Runtime) -> Self {
        let globals = runtime.globals().unwrap().unwrap();
        Self {
            common: globals.common.clone(),
            server: globals.server.clone(),
            manager: globals.manager.clone(),
            sounds: globals.sounds.clone(),
            storage: globals.storage.unwrap().clone(),
            platform_flag: globals.platform_flag,
            rows: complete_rows(runtime),
            progress: stable_progress(runtime),
            clock: globals.clock.ticks(),
        }
    }
    fn check(&self, runtime: &Runtime) {
        let globals = runtime.globals().unwrap().unwrap();
        assert!(Arc::ptr_eq(globals.common, &self.common));
        assert!(Arc::ptr_eq(globals.server, &self.server));
        assert!(Arc::ptr_eq(globals.manager, &self.manager));
        assert!(Arc::ptr_eq(globals.sounds, &self.sounds));
        assert!(Arc::ptr_eq(globals.storage.unwrap(), &self.storage));
        assert_eq!(globals.platform_flag, self.platform_flag);
        assert!(globals.clock.ticks() >= self.clock);
        assert_eq!(globals.data.play_seconds, globals.clock.saved_seconds());
        let rows = complete_rows(runtime);
        assert_eq!(rows.len(), 101);
        for (index, (actual, expected)) in rows.iter().zip(&self.rows).enumerate() {
            assert_eq!(actual, expected, "complete retained runtime row{index}");
        }
        assert_eq!(stable_progress(runtime), self.progress);
        assert!(runtime.failure().is_none());
    }
}

fn top_selected(runtime: &Runtime) -> Option<i32> {
    runtime.slot()?.title.as_ref()?.reference_top_selection()
}

fn top_idle(runtime: &Runtime) -> Option<i32> {
    runtime.slot()?.title.as_ref()?.reference_top_idle()
}

fn pure_composition(journey: &mut Journey) {
    pure_composition_with_visibility(journey, true);
}

fn pure_composition_with_visibility(journey: &mut Journey, require_visible: bool) {
    let frames = journey.runtime.frames;
    let clock = journey.runtime.globals().unwrap().unwrap().clock.ticks();
    for extent in [(1280, 720), (1920, 1080)] {
        let visible = journey.gpu.pixels(&journey.runtime, extent, false);
        let captured = journey.gpu.pixels(&journey.runtime, extent, true);
        let repeated = journey.gpu.pixels(&journey.runtime, extent, false);
        if require_visible {
            assert!(visible.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
        }
        assert_eq!(visible, captured, "pure whole-composed capture{extent:?}");
        assert_eq!(visible, repeated, "pure repeated presentation{extent:?}");
    }
    assert_eq!(journey.runtime.frames, frames);
    assert_eq!(
        journey.runtime.globals().unwrap().unwrap().clock.ticks(),
        clock
    );
}

#[test]
#[ignore = "actual normal Runtime/raw input Top6→TIPS→Title/full101 rows/cached720p+1080p captures;real assets/offscreen Metal/the default audio output;temporary progress only"]
fn higurashi_runtime_title_tips_returns_through_actual_deferred_factories() {
    let initial = saved_rows();
    let mut journey = Journey::new("title-tips", &initial);
    let file = journey
        .storage
        .paths
        .read(Source::Current)
        .unwrap()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let viewport = viewport((1920, 1080));
    let blank = RawInputState::new();
    journey.boot_title(viewport, deadline);
    let retained = Retained::new(&journey.runtime);
    assert_eq!(top_selected(&journey.runtime), Some(1)); // Actual valid autosave.
    for selected in [3, 6] {
        journey.tap(KeyCode::ArrowDown, viewport);
        assert_eq!(top_selected(&journey.runtime), Some(selected));
    }
    pure_composition(&mut journey);
    journey.tap(KeyCode::Enter, viewport);
    journey.until(
        viewport,
        deadline,
        "Top6 did not queue its actual TIPS factory",
        |r| r.slot().is_some_and(|slot| slot.pending_tips()),
    );
    assert!(journey.runtime.slot().unwrap().title.is_some());
    assert!(journey.runtime.slot().unwrap().tips.is_none());
    let source_atlas = Arc::downgrade(
        journey
            .runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .atlas
            .as_ref()
            .unwrap(),
    );
    retained.check(&journey.runtime);
    journey.no_writes(Some(&file));

    // A getter cannot accept the pending factory. The following real callback
    // captures and retires Title before TIPS starts its captured First owner.
    let pending_frames = journey.runtime.frames;
    pure_composition(&mut journey);
    assert_eq!(journey.runtime.frames, pending_frames);
    assert!(journey.runtime.slot().unwrap().pending_tips());
    journey.step(&blank, viewport);
    assert!(journey.runtime.slot().unwrap().title.is_none());
    assert_eq!(
        journey.runtime.slot().unwrap().active_scene().unwrap(),
        ActiveScene::Tips
    );
    assert!(
        source_atlas.upgrade().is_none(),
        "retired title atlas owner survived transfer"
    );
    {
        let tips = journey.runtime.slot().unwrap().tips.as_ref().unwrap();
        assert!(tips.reference_capture().is_some());
        assert!(tips.motion.model.data.is_none());
        assert!(!tips.motion.model.alternate);
    }
    journey.until(
        viewport,
        deadline,
        "real TIPS First/input did not become ready",
        |r| {
            r.slot()
                .and_then(|slot| slot.tips.as_ref())
                .is_some_and(|tips| tips.reference_input_ready())
        },
    );
    assert!(
        !journey
            .runtime
            .slot()
            .unwrap()
            .tips
            .as_ref()
            .unwrap()
            .motion
            .model
            .rows
            .is_empty()
    );
    let tips_atlas = Arc::downgrade(
        journey
            .runtime
            .slot()
            .unwrap()
            .tips
            .as_ref()
            .unwrap()
            .atlas
            .as_ref()
            .unwrap(),
    );
    retained.check(&journey.runtime);
    pure_composition(&mut journey);
    journey.tap(KeyCode::Escape, viewport);
    journey.until(
        viewport,
        deadline,
        "TIPS B did not queue its native title return",
        |r| r.slot().is_some_and(|slot| slot.pending_scene_factory()),
    );
    assert!(journey.runtime.slot().unwrap().tips.is_some());
    assert!(journey.runtime.slot().unwrap().title.is_none());
    journey.no_writes(Some(&file));
    journey.step(&blank, viewport);
    assert!(journey.runtime.slot().unwrap().tips.is_none());
    assert!(journey.runtime.slot().unwrap().title.is_some());
    assert!(
        tips_atlas.upgrade().is_none(),
        "TIPS atlas must retire before the new Title owner"
    );
    journey.until(
        viewport,
        deadline,
        "fresh Title after TIPS did not open Top",
        |r| ready_menu(r, "top"),
    );
    assert_eq!(top_selected(&journey.runtime), Some(6));
    assert_eq!(
        journey
            .runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .reference_initial_selection()
            .0,
        6
    );
    assert!(journey.runtime.globals().unwrap().unwrap().clock.ticks() > retained.clock);
    retained.check(&journey.runtime);
    pure_composition(&mut journey);
    journey.no_writes(Some(&file));
    eprintln!(
        "Runtime Top6→TIPS→freshTitle6:actual raw arrows/A/B,asynchronous First,next-callback factories/atlas retirement,full101 rows/global owners/no writes and pure720p+1080p composed captures PASS"
    );
}

#[test]
#[ignore = "full3600 admitted Runtime Top callbacks→real idle MP4→Title return/retained Slot/full101 rows/cached720p+1080p captures;offscreen Metal/the default audio output;temporary progress only"]
fn higurashi_runtime_full_idle_demo_cycle_preserves_the_vacant_slot_and_globals() {
    let initial = saved_rows();
    let mut journey = Journey::new("idle-demo", &initial);
    let file = journey
        .storage
        .paths
        .read(Source::Current)
        .unwrap()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(180);
    let viewport = viewport((1280, 720));
    let blank = RawInputState::new();
    journey.boot_title(viewport, deadline);
    let retained = Retained::new(&journey.runtime);
    let slot_address = (&**journey.runtime.slot.as_ref().unwrap()) as *const TitleSlot;
    let title_mode = journey
        .runtime
        .slot()
        .unwrap()
        .title
        .as_ref()
        .unwrap()
        .reference_initial_selection()
        .1;

    // Two real navigation events restore the initial selection and reset idle
    // through native controls. No callback count or scene state is assigned.
    assert_eq!(top_selected(&journey.runtime), Some(1));
    journey.tap(KeyCode::ArrowLeft, viewport);
    assert_eq!(top_selected(&journey.runtime), Some(0));
    journey.step(&key(KeyCode::ArrowRight), viewport);
    assert_eq!(top_selected(&journey.runtime), Some(1));
    assert_eq!(top_idle(&journey.runtime), Some(0));
    pure_composition(&mut journey);
    let ready_frames = journey.runtime.frames;
    for count in 1..=3599 {
        assert!(
            Instant::now() < deadline,
            "full native idle callback interval stalled"
        );
        assert!(matches!(journey.step(&blank, viewport), Frame::Advanced(_)));
        assert_eq!(top_idle(&journey.runtime), Some(count));
        let slot = journey.runtime.slot().unwrap();
        assert!(slot.pending_title_demo().is_none());
        assert!(slot.title_destination().is_none());
        assert!(journey.runtime.bootstrap.is_none());
    }
    journey.step(&blank, viewport);
    assert_eq!(journey.runtime.frames - ready_frames, 3600);
    assert_eq!(top_idle(&journey.runtime), Some(3600));
    assert!(
        journey
            .runtime
            .slot()
            .unwrap()
            .pending_title_demo()
            .is_none()
    );
    assert_eq!(
        journey
            .runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .reference_menu_state(),
        Some(("top", 3))
    );
    journey.until(
        viewport,
        deadline,
        "idle row closing did not publish its early marker",
        |r| {
            r.slot()
                .is_some_and(|slot| slot.title_destination() == Some(&TitleDestination::IdleDemo))
        },
    );
    assert!(journey.runtime.bootstrap.is_none());
    assert!(
        journey
            .runtime
            .slot()
            .unwrap()
            .pending_title_demo()
            .is_none()
    );
    let marker_frames = journey.runtime.frames;
    for _ in 0..59 {
        journey.step(&blank, viewport);
        assert!(journey.runtime.bootstrap.is_none());
        assert!(
            journey
                .runtime
                .slot()
                .unwrap()
                .pending_title_demo()
                .is_none()
        );
    }
    assert_eq!(
        journey
            .runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .frame()
            .black_alpha,
        1.
    );
    journey.step(&blank, viewport);
    assert_eq!(journey.runtime.frames - marker_frames, 60);
    assert_eq!(
        journey
            .runtime
            .slot()
            .unwrap()
            .pending_title_demo()
            .unwrap()
            .mode,
        title_mode + 1
    );
    assert!(journey.runtime.slot().unwrap().title.is_some());
    assert!(journey.runtime.bootstrap.is_none());
    retained.check(&journey.runtime);
    journey.no_writes(Some(&file));

    journey.step(&blank, viewport);
    assert!(journey.runtime.bootstrap.is_some());
    assert!(journey.runtime.slot().is_none()); // The published active owner is Demo.
    let vacant = journey.runtime.slot.as_ref().unwrap();
    assert_eq!((&**vacant) as *const TitleSlot, slot_address);
    assert!(vacant.title.is_none());
    assert!(
        vacant.title_destination().is_none(),
        "accepted IdleDemo marker must be removed"
    );
    assert!(vacant.pending_title_demo().is_none());
    retained.check(&journey.runtime);
    journey.until(
        viewport,
        deadline,
        "actual idle-demo MP4 did not retain a frame",
        |r| {
            r.bootstrap
                .as_ref()
                .is_some_and(SceneHost::reference_demo_ready)
        },
    );
    let producers = journey
        .runtime
        .bootstrap
        .as_ref()
        .unwrap()
        .reference_demo_retirements();
    assert!(!producers.is_empty());
    // The actual first decoded movie frame may be black. Its retained pixels
    // must still copy/present identically without advancing player or clocks.
    pure_composition_with_visibility(&mut journey, false);
    journey.no_writes(Some(&file));
    journey.tap(KeyCode::Enter, viewport);
    journey.until(
        viewport,
        deadline,
        "idle demo did not return through the retained Slot",
        |r| ready_menu(r, "top"),
    );
    assert!(journey.runtime.bootstrap.is_none());
    assert_eq!(
        (&**journey.runtime.slot.as_ref().unwrap()) as *const TitleSlot,
        slot_address
    );
    assert!(
        producers
            .iter()
            .all(shin_tasks::LoaderRetirement::is_finished)
    );
    assert_eq!(
        journey
            .runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .reference_initial_selection(),
        (-1, (title_mode + 1) & 1)
    );
    assert_eq!(top_selected(&journey.runtime), Some(1));
    assert!(
        journey
            .runtime
            .slot()
            .unwrap()
            .title_destination()
            .is_none()
    );
    assert!(journey.runtime.globals().unwrap().unwrap().clock.ticks() > retained.clock);
    retained.check(&journey.runtime);
    pure_composition(&mut journey);
    journey.no_writes(Some(&file));
    eprintln!(
        "Runtime idle:3600 actual ready callbacks,early marker plus60-frame factory gate,real MP4/A-skip,producer retirement/same vacant Slot/fresh alternating Title,full101 rows/global owners/no writes and pure720p+1080p composed captures PASS"
    );
}
