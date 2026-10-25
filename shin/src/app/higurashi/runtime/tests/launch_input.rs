//! Normal Runtime permission and missing-file startup boundaries. The existing
//! component/native tests supply the Question/TitleDemo model and pixel oracles;
//! these checks exercise their actual outer Runtime wiring and temporary Paths.
use super::*;
use std::path::PathBuf;

struct TemporaryProgress(PathBuf);
impl TemporaryProgress {
    fn new(label: &str) -> Self {
        Self(std::env::temp_dir().join(format!(
            "higurashi-launch-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )))
    }
    fn paths(&self) -> Paths {
        Paths {
            current: Some(self.0.join("progress.dat")),
            trial: None,
        }
    }
}
impl Drop for TemporaryProgress {
    fn drop(&mut self) {
        if self.0.exists() {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

/// Real atomic Paths reads/writes, with observation outside the implementation.
/// No gate, injected success, user paths or system-default audio device is used.
struct ObservedPaths {
    paths: Paths,
    reads: StdMutex<Vec<Source>>,
    writes: StdMutex<Vec<Vec<u8>>>,
}
impl Storage for ObservedPaths {
    fn trial_token(&self) -> bool {
        self.paths.trial_token()
    }
    fn read(&self, source: Source) -> Result<Option<Vec<u8>>> {
        self.reads.lock().unwrap().push(source);
        self.paths.read(source)
    }
    fn trial_exists(&self) -> Result<bool> {
        self.paths.trial_exists()
    }
    fn write(&self, bytes: &[u8]) -> Result<()> {
        self.paths.write(bytes)?;
        self.writes.lock().unwrap().push(bytes.to_vec());
        Ok(())
    }
}
struct Launch {
    // Drop the Runtime's movie/loader/writer scopes before their temporary path.
    runtime: Runtime,
    gpu: Graphics,
    storage: Arc<ObservedPaths>,
    _temporary: TemporaryProgress,
}
impl Launch {
    fn new(label: &str, initial: Option<&SaveData>) -> Self {
        let temporary = TemporaryProgress::new(label);
        let paths = temporary.paths();
        if let Some(initial) = initial {
            paths.write(&initial.encode().unwrap()).unwrap();
        } else {
            assert!(paths.read(Source::Current).unwrap().is_none());
            assert!(!paths.current.as_ref().unwrap().exists());
        }
        let storage = Arc::new(ObservedPaths {
            paths,
            reads: StdMutex::new(Vec::new()),
            writes: StdMutex::new(Vec::new()),
        });
        let (device, queue, server) = crate::adv::higurashi::reference_gpu_assets();
        let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
        let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
        let mut runtime = Runtime::new(Arc::new(server), manager, sounds, storage.clone());
        runtime.set_focused(true);
        Self {
            runtime,
            gpu: Graphics::new(device, queue),
            storage,
            _temporary: temporary,
        }
    }
    fn step(&mut self, raw: &RawInputState, viewport: InputViewport, permission: bool) -> Frame {
        self.gpu.update_with_permission(
            &mut self.runtime,
            raw,
            viewport,
            Duration::from_millis(50),
            permission,
        )
    }
    fn until(
        &mut self,
        viewport: InputViewport,
        permission: bool,
        deadline: Instant,
        description: &str,
        ready: impl Fn(&Runtime) -> bool,
    ) {
        let blank = RawInputState::new();
        while !ready(&self.runtime) {
            assert!(Instant::now() < deadline, "{description}");
            assert!(matches!(
                self.step(&blank, viewport, permission),
                Frame::Advanced(_)
            ));
        }
    }
    fn no_writes(&self, expected_file: Option<&[u8]>) {
        assert!(self.storage.writes.lock().unwrap().is_empty());
        let actual = self.storage.paths.read(Source::Current).unwrap();
        assert_eq!(actual.as_deref(), expected_file);
    }
    fn demo_motion(&self) -> Option<(f32, bool)> {
        self.runtime.bootstrap.as_ref()?.reference_demo_motion()
    }
}

#[test]
#[ignore = "real Runtime Startup/notice/TitleDemo permission propagation; offscreen Metal and the default audio output; temporary Paths only"]
fn higurashi_runtime_denied_input_keeps_boot_and_accepted_transitions_advancing() {
    let mut saved = SaveData::default();
    saved.play_seconds = 123;
    saved.settings.common.v3_sysvol = 37;
    let initial = saved.encode().unwrap();
    let mut launch = Launch::new("denied-input", Some(&saved));
    let deadline = Instant::now() + Duration::from_secs(90);
    let viewport = viewport((1920, 1080));
    let blank = RawInputState::new();
    let enter = key(KeyCode::Enter);
    const A: u32 = 1 << 24;

    // A denied outer permission is separate from focus. Actual jobs must still
    // publish the loaded file, common assets, system sounds and ready notice.
    launch.until(
        viewport,
        false,
        deadline,
        "denied startup did not finish loading",
        |r| notice(r).is_some_and(|(phase, _)| phase == 2),
    );
    assert_eq!(*launch.storage.reads.lock().unwrap(), [Source::Current]);
    let common = launch.runtime.globals().unwrap().unwrap().common.clone();
    assert_eq!(
        launch
            .runtime
            .globals()
            .unwrap()
            .unwrap()
            .data
            .settings
            .common
            .v3_sysvol,
        37
    );
    assert_eq!(
        launch
            .runtime
            .globals()
            .unwrap()
            .unwrap()
            .sounds
            .lock()
            .volume_percent,
        37
    );
    launch.no_writes(Some(&initial));

    launch.step(&blank, viewport, false);
    launch.step(&enter, viewport, false);
    assert_ne!(
        launch.runtime.input.buttons().pressed & A,
        0,
        "denied fresh A must reach the focused Runtime input bridge"
    );
    assert_eq!(
        notice(&launch.runtime).unwrap().0,
        2,
        "outer permission must reach the actual Startup notice"
    );
    launch.step(&enter, viewport, true);
    assert_eq!(launch.runtime.input.buttons().pressed & A, 0);
    assert_eq!(
        notice(&launch.runtime).unwrap().0,
        2,
        "a held denied A is not replayed when permission returns"
    );
    launch.step(&blank, viewport, false);

    // Runtime applies the letterbox offset and physical scaling once. The
    // notice's own InputArea then consumes its usual canonical coordinates.
    let center = notice(&launch.runtime).unwrap().1;
    let physical = viewport.offset + center / Vec2::new(1920., 1080.) * viewport.size;
    launch.step(&mouse(physical, true), viewport, false);
    launch.step(&mouse(physical, false), viewport, false);
    assert_eq!(
        notice(&launch.runtime).unwrap().0,
        2,
        "denied down/up cannot answer the ready notice"
    );
    launch.step(&blank, viewport, true);
    launch.step(&enter, viewport, true);
    assert_ne!(launch.runtime.input.buttons().pressed & A, 0);
    assert_eq!(
        notice(&launch.runtime).unwrap().0,
        3,
        "a fresh permitted A accepts the same retained notice"
    );

    let accepted_clock = launch.runtime.globals().unwrap().unwrap().clock.ticks();
    launch.until(
        viewport,
        false,
        deadline,
        "accepted notice closing or denied demo preparation stalled",
        |r| {
            r.bootstrap
                .as_ref()
                .is_some_and(SceneHost::reference_demo_ready)
        },
    );
    assert!(launch.runtime.globals().unwrap().unwrap().clock.ticks() > accepted_clock);
    assert_eq!(launch.demo_motion(), Some((1., false)));
    assert!(launch.runtime.slot().is_none());
    launch.no_writes(Some(&initial));

    launch.step(&blank, viewport, false);
    let before = launch.runtime.globals().unwrap().unwrap().clock.ticks();
    launch.step(&enter, viewport, false);
    assert_ne!(launch.runtime.input.buttons().pressed & A, 0);
    assert_eq!(
        launch.demo_motion(),
        Some((1., false)),
        "denied fresh A must not start the live demo's fade"
    );
    assert!(launch.runtime.globals().unwrap().unwrap().clock.ticks() > before);
    launch.step(&enter, viewport, true);
    assert_eq!(launch.runtime.input.buttons().pressed & A, 0);
    assert_eq!(launch.demo_motion(), Some((1., false)));
    launch.step(&blank, viewport, false);
    launch.step(
        &mouse(viewport.offset + viewport.size * 0.5, true),
        viewport,
        false,
    );
    launch.step(
        &mouse(viewport.offset + viewport.size * 0.5, false),
        viewport,
        false,
    );
    assert_eq!(
        launch.demo_motion(),
        Some((1., false)),
        "denied contact must not start the demo's fade"
    );
    launch.step(&blank, viewport, true);
    launch.step(&enter, viewport, true);
    let (opacity, fading) = launch.demo_motion().unwrap();
    assert!(
        fading && opacity < 1.,
        "permitted A must start the actual fade"
    );

    let mut last_opacity = opacity;
    while launch.runtime.slot().is_none() {
        assert!(
            Instant::now() < deadline,
            "denied accepted fade/title dispatch stalled"
        );
        launch.step(&blank, viewport, false);
        if let Some((opacity, _)) = launch.demo_motion() {
            assert!(opacity <= last_opacity);
            last_opacity = opacity;
        }
    }
    assert_eq!(
        last_opacity, 0.,
        "accepted demo fade completes while input is denied"
    );
    assert!(launch.runtime.bootstrap.is_none());
    assert_eq!(
        launch.runtime.slot().unwrap().active_scene().unwrap(),
        ActiveScene::Title
    );
    launch.until(
        viewport,
        false,
        deadline,
        "denied title opening stalled",
        |r| ready_menu(r, "top"),
    );
    assert!(Arc::ptr_eq(
        launch.runtime.globals().unwrap().unwrap().common,
        &common
    ));
    launch.no_writes(Some(&initial));
}

#[test]
#[ignore = "pristine missing-file normal Runtime boot and successful deferred close; offscreen Metal and the default audio output; temporary Paths only"]
fn higurashi_runtime_pristine_boot_defers_its_first_progress_write_until_close() {
    let mut launch = Launch::new("pristine", None);
    let deadline = Instant::now() + Duration::from_secs(90);
    let viewport = viewport((1280, 720));
    let blank = RawInputState::new();
    let enter = key(KeyCode::Enter);
    launch.until(
        viewport,
        true,
        deadline,
        "pristine mandatory notice not ready",
        |r| notice(r).is_some_and(|(phase, _)| phase == 2),
    );
    assert_eq!(*launch.storage.reads.lock().unwrap(), [Source::Current]);
    let common = launch.runtime.globals().unwrap().unwrap().common.clone();
    {
        let globals = launch.runtime.globals().unwrap().unwrap();
        let mut initial = globals.data.clone();
        initial.play_seconds = 0; // The actual global clock has already advanced.
        assert_eq!(
            initial.encode_raw().unwrap(),
            SaveData::default().encode_raw().unwrap()
        );
        let slots = globals.runtime_saves.unwrap();
        assert!(!slots.auto.valid);
        assert!(slots.manual.iter().all(|slot| !slot.valid));
    }
    launch.no_writes(None);
    launch.step(&blank, viewport, true);
    launch.step(&enter, viewport, true);
    assert_eq!(notice(&launch.runtime).unwrap().0, 3);
    launch.until(
        viewport,
        true,
        deadline,
        "pristine opening movie not ready",
        |r| {
            r.bootstrap
                .as_ref()
                .is_some_and(SceneHost::reference_demo_ready)
        },
    );
    launch.no_writes(None);
    launch.step(&blank, viewport, true);
    launch.step(&enter, viewport, true);
    launch.until(viewport, true, deadline, "pristine title not ready", |r| {
        ready_menu(r, "top")
    });
    assert_eq!(
        launch.runtime.slot().unwrap().active_scene().unwrap(),
        ActiveScene::Title
    );
    assert!(launch.runtime.bootstrap.is_none());
    assert!(Arc::ptr_eq(
        launch.runtime.globals().unwrap().unwrap().common,
        &common
    ));
    launch.no_writes(None);

    // Freeze the complete current settings plus all 101 runtime rows at the
    // actual close boundary. Comparing raw codec bytes avoids assigning any
    // significance to the obfuscator's transport wrapper.
    let frozen = launch.runtime.freeze_progress().unwrap().unwrap();
    let expected = frozen
        .slots
        .as_ref()
        .unwrap()
        .disk(&frozen.data)
        .encode_raw()
        .unwrap();
    let clock = launch.runtime.globals().unwrap().unwrap().clock.ticks();
    launch.runtime.request_close().unwrap();
    assert!(launch.runtime.close_active());
    loop {
        assert!(
            Instant::now() < deadline,
            "pristine atomic close writer did not finish"
        );
        let frame = launch.step(&blank, viewport, false);
        assert!(launch.runtime.failure().is_none());
        if matches!(frame, Frame::Exit) {
            break;
        }
        assert!(matches!(frame, Frame::Closing));
        assert_eq!(
            launch.runtime.globals().unwrap().unwrap().clock.ticks(),
            clock
        );
        assert!(launch.runtime.freezes_scene());
    }
    assert!(launch.runtime.exited);
    assert!(launch.runtime.bootstrap.is_none() && launch.runtime.slot().is_none());
    assert!(launch.runtime.globals().unwrap().is_none());
    let writes = launch.storage.writes.lock().unwrap();
    assert_eq!(writes.len(), 1, "one real successful desktop-close write");
    assert_eq!(
        std::fs::read(launch.storage.paths.current.as_ref().unwrap()).unwrap(),
        writes[0]
    );
    let decoded = SaveData::decode(&writes[0]).unwrap();
    assert_eq!(decoded.encode_raw().unwrap(), expected);
    assert!(decoded.auto_save_slot.is_none());
    assert!(decoded.manual_save_slots.iter().all(Option::is_none));
}
