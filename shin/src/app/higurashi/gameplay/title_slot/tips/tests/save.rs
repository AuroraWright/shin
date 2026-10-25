use super::*;
use crate::app::higurashi::load_game::{Paths, Source, Storage};
use std::sync::atomic::AtomicBool;

struct Store {
    path: Paths,
    entered: AtomicBool,
    writes: AtomicUsize,
    gate: (std::sync::Mutex<bool>, std::sync::Condvar),
    bytes: std::sync::Mutex<Vec<u8>>,
}
struct Release(Arc<Store>);
impl Drop for Release {
    fn drop(&mut self) {
        *self.0.gate.0.lock().unwrap() = true;
        self.0.gate.1.notify_all();
    }
}
impl Storage for Store {
    fn trial_token(&self) -> bool {
        false
    }
    fn read(&self, source: Source) -> Result<Option<Vec<u8>>> {
        self.path.read(source)
    }
    fn trial_exists(&self) -> Result<bool> {
        Ok(false)
    }
    fn write(&self, bytes: &[u8]) -> Result<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        *self.bytes.lock().unwrap() = bytes.to_vec();
        self.entered.store(true, Ordering::SeqCst);
        let mut open = self.gate.0.lock().unwrap();
        while !*open {
            open = self.gate.1.wait(open).unwrap();
        }
        drop(open);
        self.path.write(bytes)
    }
}

#[test]
#[ignore = "actual active0 TIPS EXIT,odd completion bit,frozen101 slots,held temporary atomic writer,no-answer notification,post-save dcf00 and next-frame scene retirement at720p/1080p;the default audio output,no window/user saves"]
fn higurashi_primary_tips_exit_saves_completion_before_source_free_browser() {
    primary_save(false);
}
#[test]
#[ignore = "actual SystemMenu item6 stage1 Yes15c64(unlock0)/active0 frozen settings and101 runtime rows/held temporary atomic writer/minimum one second/source-free browser at720p/1080p;the default audio output,no window/user saves"]
fn higurashi_system_menu_primary_tip_saves_without_marking_unfinished_completion() {
    primary_save(true);
}
fn primary_save(from_menu: bool) {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let (_, common) = crate::app::higurashi::gameplay::tests::character::common(
        &server,
        &[0x83, 1, 0x83, 0xe8, 0, 0, 0, 0x83, 0, 1],
    );
    let id = i32::from(common.scenario.tips_info[0].updated_field.unwrap());
    assert!((1000..2000).contains(&id));
    for size in [(1280, 720), (1920, 1080)] {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "higurashi-save-tips-{}-{unique}",
            std::process::id()
        ));
        let path = dir.join("progress.dat");
        let store = Arc::new(Store {
            path: Paths {
                current: Some(path.clone()),
                trial: None,
            },
            entered: AtomicBool::new(false),
            writes: AtomicUsize::new(0),
            gate: (std::sync::Mutex::new(false), std::sync::Condvar::new()),
            bytes: std::sync::Mutex::new(Vec::new()),
        });
        let mut globals = load_globals(&server, common.clone());
        globals.storage = Some(store.clone());
        globals.data.save_vectors.extra_words_0 = vec![0; 20];
        globals.data.save_vectors.extra_words_0[0] = 1;
        globals.data.save_vectors.extra_words_0[19] = 0xf00d;
        globals.data.extra_word = 0x1234;
        let full_auto = globals.runtime_saves.as_ref().unwrap().auto.data.clone();
        let full_manual = globals.runtime_saves.as_ref().unwrap().manual[2]
            .data
            .clone();
        let mut source = crate::app::higurashi::tips_menu::tests::source(id, 0);
        source.progress.entries[0].save_position = 0;
        source.progress.entries[0].replay_target = 0;
        let mut game = None;
        reference_render_to_texture(&device, &queue, size, |context, _| {
            game = Some(
                Gameplay::new(
                    globals,
                    source,
                    0,
                    context.new_render_texture("TIPS save source".into()),
                    || [1, 2, 3, 4],
                )
                .unwrap(),
            );
        });
        let mut slot = TitleSlot::from_game(game.unwrap());
        let _release = Release(store.clone());
        let mut frames = Frames {
            number: 0,
            size,
            composed: None,
        };
        frames.step(&device, &queue, &mut slot, 0.0, 0);
        frames.wait_game(&device, &queue, &mut slot);
        slot.game
            .as_mut()
            .unwrap()
            .bindings
            .history
            .record_completion(true, 73);
        let history = slot
            .game
            .as_ref()
            .unwrap()
            .bindings
            .history
            .bits()
            .words()
            .as_ptr();
        if from_menu {
            super::menu::open_question(&device, &queue, &mut frames, &mut slot);
            super::menu::accept(&device, &queue, &mut frames, &mut slot);
            assert!(!slot.pending_tips());
            assert!(
                !slot
                    .game
                    .as_ref()
                    .unwrap()
                    .host()
                    .unwrap()
                    .runner
                    .exit_closing()
            );
        } else {
            assert!(
                slot.game
                    .as_mut()
                    .unwrap()
                    .host_mut()
                    .unwrap()
                    .runner
                    .interrupt_wait()
            );
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while !store.entered.load(Ordering::SeqCst) {
            frames.step(&device, &queue, &mut slot, 3.0, 0);
            assert!(!slot.pending_tips());
            assert!(
                Instant::now() < deadline,
                "TIPS save producer did not start"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let began = Instant::now();
        assert_eq!(store.writes.load(Ordering::SeqCst), 1);
        assert!(!path.exists());
        let bytes = store.bytes.lock().unwrap().clone();
        let frozen = SaveData::decode(&bytes).unwrap();
        assert_eq!(
            frozen.save_vectors.extra_words_0[0],
            if from_menu { 1 } else { 3 }
        );
        assert_eq!(frozen.save_vectors.extra_words_0[19], 0xf00d);
        assert_ne!(frozen.save_vectors.seen_messages[2] & (1 << 9), 0);
        assert_eq!(frozen.auto_save_slot.as_ref().unwrap().extra_word, 7);
        assert_eq!(
            frozen.manual_save_slots[2]
                .as_ref()
                .unwrap()
                .trailing_values,
            vec![1023, 0x234]
        );
        slot.game.as_mut().unwrap().globals.data.extra_word = 0x4321;
        for _ in 0..10 {
            frames.step(&device, &queue, &mut slot, 3.0, (1 << 24) | (1 << 25));
            assert!(!slot.pending_tips());
        }
        assert_eq!(*store.bytes.lock().unwrap(), bytes);
        *store.gate.0.lock().unwrap() = true;
        store.gate.1.notify_all();
        let deadline = Instant::now() + Duration::from_secs(10);
        let before = loop {
            let image = frames.step(&device, &queue, &mut slot, 3.0, 0);
            if slot.pending_tips() {
                break image;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        };
        assert!(began.elapsed() >= Duration::from_millis(900));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let Some(Pending::Tips(request)) = &slot.pending else {
            unreachable!()
        };
        assert!(request.data.is_none());
        assert_eq!((request.selected, request.alternate), (0, 0));
        assert!(slot.game.as_ref().unwrap().host().unwrap().has_modes());
        slot.reference_scene_events.clear();
        frames.step(&device, &queue, &mut slot, 0.0, 0);
        assert_eq!(slot.reference_scene_events, [1, 2, 3]);
        assert!(slot.game.is_none() && slot.title.is_none());
        let menu = slot.tips.as_ref().unwrap();
        assert!(menu.motion.model.data.is_none());
        assert_eq!(
            captured(&device, &queue, size, menu.reference_capture().unwrap()),
            before
        );
        assert_eq!(menu.globals.data.extra_word, 0x4321);
        assert_eq!(
            menu.globals.runtime_saves.as_ref().unwrap().auto.data,
            full_auto
        );
        assert_eq!(
            menu.globals.runtime_saves.as_ref().unwrap().manual[2].data,
            full_manual
        );
        assert_eq!(
            slot.continuation
                .as_ref()
                .unwrap()
                .history
                .bits()
                .words()
                .as_ptr(),
            history
        );
        frames.ready_tips(&device, &queue, &mut slot);
        frames.step(&device, &queue, &mut slot, 0.0, 1 << 25);
        while slot.pending.is_none() {
            frames.step(&device, &queue, &mut slot, 1.0, 0);
        }
        frames.step(&device, &queue, &mut slot, 0.0, 0);
        assert!(slot.title.is_some() && slot.game.is_none() && slot.tips.is_none());
        assert_eq!(
            slot.title
                .as_ref()
                .unwrap()
                .globals
                .runtime_saves
                .as_ref()
                .unwrap()
                .manual[2]
                .data,
            full_manual
        );
        drop(slot);
        std::fs::remove_dir_all(dir).unwrap();
    }
    if from_menu {
        println!(
            "Menu TIPS SaveGame:2 real720p/1080p active0 owners/4 composed transfers;unfinished completion remains clear before frozen101 slots,No-default stage1 Yes,held single temporary atomic writer/minimum one second,source-free callback/original history/auto/manual owners and next-frame retirement pass"
        );
    } else {
        println!(
            "TIPS SaveGame:2 real720p/1080p active0 EXIT owners/4 composed scene transfers;odd completion before frozen settings/full101 runtime slots,held single atomic producer/minimum one second/no-answer input gate,source-free dcf00 callback and original history/autosave/manual owners pass;temporary files only"
        );
    }
}
