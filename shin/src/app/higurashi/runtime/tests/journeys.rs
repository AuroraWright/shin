//! Full Runtime journeys complement the existing native/component contracts.
//! Input enters through RawInputState; asset, scene and storage work uses the
//! actual asynchronous owners, offscreen Metal and exact Loopback Audio.
use super::*;
use std::path::PathBuf;

mod kakera;
mod menus;
mod quiz;
mod save_failure;
mod save_load;
mod sustained;

struct TemporaryProgress(PathBuf);
impl Drop for TemporaryProgress {
    fn drop(&mut self) {
        if self.0.exists() {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
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
struct Journey {
    // Retire scene/audio/IO scopes before deleting their unique temporary path.
    runtime: Runtime,
    gpu: Graphics,
    storage: Arc<ObservedPaths>,
    _temporary: TemporaryProgress,
}
impl Journey {
    fn new(label: &str, initial: &SaveData) -> Self {
        let temporary = TemporaryProgress(std::env::temp_dir().join(format!(
            "higurashi-runtime-journeys-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        )));
        let paths = Paths {
            current: Some(temporary.0.join("progress.dat")),
            trial: None,
        };
        paths.write(&initial.encode().unwrap()).unwrap();
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
    fn step(&mut self, raw: &RawInputState, viewport: InputViewport) -> Frame {
        self.gpu
            .update(&mut self.runtime, raw, viewport, Duration::from_millis(50))
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
            assert!(matches!(
                self.step(&RawInputState::new(), viewport),
                Frame::Advanced(_)
            ));
        }
    }
    fn boot_title(&mut self, viewport: InputViewport, deadline: Instant) {
        self.until(
            viewport,
            deadline,
            "normal notice did not become ready",
            |r| notice(r).is_some_and(|(phase, _)| phase == 2),
        );
        self.tap(KeyCode::Enter, viewport);
        self.until(
            viewport,
            deadline,
            "opening demo did not become ready",
            |r| {
                r.bootstrap
                    .as_ref()
                    .is_some_and(SceneHost::reference_demo_ready)
            },
        );
        self.tap(KeyCode::Enter, viewport);
        self.until(
            viewport,
            deadline,
            "normal top menu did not become ready",
            |r| ready_menu(r, "top"),
        );
        assert!(self.runtime.bootstrap.is_none());
        assert_eq!(
            self.runtime.slot().unwrap().active_scene().unwrap(),
            ActiveScene::Title
        );
        assert_eq!(*self.storage.reads.lock().unwrap(), [Source::Current]);
    }
    fn start_story(&mut self, viewport: InputViewport, deadline: Instant) {
        assert!(ready_menu(&self.runtime, "top"));
        self.tap(KeyCode::Enter, viewport);
        self.until(
            viewport,
            deadline,
            "normal category did not become ready",
            |r| ready_menu(r, "cat"),
        );
        self.tap(KeyCode::Enter, viewport);
        self.until(
            viewport,
            deadline,
            "authored first message did not become ready",
            |r| {
                use shin_core::vm::higurashi::command::HigurashiRuntimeCommand as Cmd;
                r.slot()
                    .and_then(|s| s.game.as_ref())
                    .and_then(|g| g.host())
                    .is_some_and(|h| {
                        h.runner.completed_commands() >= 10
                            && matches!(
                                h.runner.pending_command(),
                                Some(Cmd::MSGWAIT { .. } | Cmd::MSGSET { auto_wait: 1, .. })
                            )
                    })
            },
        );
        for _ in 0..8 {
            self.step(&RawInputState::new(), viewport);
        }
    }
    fn no_writes(&self, expected_file: Option<&[u8]>) {
        assert!(self.storage.writes.lock().unwrap().is_empty());
        assert_eq!(
            self.storage.paths.read(Source::Current).unwrap().as_deref(),
            expected_file
        );
    }
}
