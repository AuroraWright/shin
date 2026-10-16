use super::*;

pub(super) struct Temporary(pub(super) std::path::PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub(super) struct Driver<'a> {
    pub(super) gpu: &'a Offscreen,
    pub(super) app: Gerokasu2App,
    pub(super) frames: Frames,
}
impl Driver<'_> {
    pub(super) fn step(&mut self, mut input: EnumMap<AppAction, ActionState>) {
        // Production pointer samplers accumulate elapsed time internally.
        let seconds = 1. / 60.;
        self.app.frame.advance();
        self.app.play_clock.advance(1.);
        self.app.save_store.as_mut().unwrap().data.play_seconds =
            self.app.play_clock.saved_seconds();
        self.app.poll_saves(Ticks::from_f32(1.));
        if self.app.write_wait.is_some() {
            input = EnumMap::default();
        }
        self.app.poll_system_menu();
        self.app.poll_title_destination();
        let completing_exit = self.app.exit_save_ready;
        let retaining_exit = self.app.host.is_some() && self.app.loading_title.is_some();
        if !completing_exit && !retaining_exit {
            self.app.poll_game_destination();
            self.app.poll_child();
            if self.app.title.is_none() {
                self.prepare_controls(&mut input);
            }
            let status = if let Some(title) = self.app.title.as_mut() {
                assert!(
                    title
                        .update(1., &input, Vec2::ZERO, viewport(), seconds)
                        .is_none()
                );
                RunnerStatus::Waiting
            } else {
                self.app
                    .update_adv_scene(Ticks::from_f32(1.), &input, viewport(), seconds)
                    .unwrap()
            };
            if status == RunnerStatus::Exited {
                self.app.begin_exit_write().unwrap();
            }
            if self.app.modal.advance(1.) {
                self.app.backdrop = None;
            }
            self.app.collect_scene_progress();
        }
        self.encode_frame(completing_exit, !retaining_exit && !completing_exit);
        self.app.advance_global_audio(Ticks::from_f32(1.));
        assert!(self.app.error.is_none(), "{:?}", self.app.error);
    }
    fn prepare_controls(&mut self, input: &mut EnumMap<AppAction, ActionState>) {
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let mut ctx = PreRenderContext {
            device: &self.gpu.device,
            queue: &self.gpu.queue,
            breakup_texture: None,
            resize_source: &self.frames.resize,
            sampler_store: &self.frames.samplers,
            depth_stencil: DepthStencilTarget {
                view: &self.frames.depth,
            },
            pipeline_storage: &mut self.frames.pipelines,
            dynamic_buffer: &mut self.frames.dynamic,
            encoder: &mut encoder,
        };
        self.app
            .prepare_adv_menus(input, &mut ctx, viewport(), 1. / 60.)
            .unwrap();
        let mut uploads = self.gpu.device.create_command_encoder(&Default::default());
        self.frames.dynamic.finish(&mut uploads);
        self.gpu.queue.submit([uploads.finish(), encoder.finish()]);
        self.frames.dynamic.recall();
    }
    fn encode_frame(&mut self, completing_exit: bool, compose: bool) {
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let mut ctx = PreRenderContext {
            device: &self.gpu.device,
            queue: &self.gpu.queue,
            breakup_texture: None,
            resize_source: &self.frames.resize,
            sampler_store: &self.frames.samplers,
            depth_stencil: DepthStencilTarget {
                view: &self.frames.depth,
            },
            pipeline_storage: &mut self.frames.pipelines,
            dynamic_buffer: &mut self.frames.dynamic,
            encoder: &mut encoder,
        };
        if completing_exit {
            assert!(self.app.complete_exit_save(&mut ctx).unwrap());
        }
        if compose {
            self.app.compose_adv_frame(&mut ctx).unwrap();
        }
        self.app.prepare_write_wait(&mut ctx).unwrap();
        let mut upload = self.gpu.device.create_command_encoder(&Default::default());
        self.frames.dynamic.finish(&mut upload);
        self.gpu.queue.submit([upload.finish(), encoder.finish()]);
        self.frames.dynamic.recall();
        self.gpu.device.poll(wgpu::PollType::Poll).unwrap();
    }
    pub(super) fn settle(&mut self, n: usize) {
        for _ in 0..n {
            self.step(EnumMap::default());
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    pub(super) fn press(&mut self, action: AppAction) {
        self.step(held(action));
        self.step(EnumMap::default());
    }
    pub(super) fn ready(&mut self) {
        // A row decision dispatches its factory only after the native flash.
        // Let that owner run before deciding that there is no worker to await.
        self.settle(35);
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.app.loading_system.is_some() || self.app.loading_child.is_some() {
            self.settle(1);
            assert!(Instant::now() < deadline, "menu worker did not publish");
        }
        self.settle(35);
    }
    pub(super) fn written(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.app.write_wait.is_some()
            || self.app.save_store.as_ref().unwrap().is_flush_pending()
        {
            self.settle(1);
            assert!(Instant::now() < deadline, "manual write did not finish");
        }
        self.settle(25);
    }
    pub(super) fn draw(&mut self) {
        self.capture();
    }
    pub(super) fn capture(&mut self) -> Vec<u8> {
        let pixels = self.capture_any();
        assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
        pixels
    }
    /// The composed frame, which may be entirely black.
    pub(super) fn capture_any(&mut self) -> Vec<u8> {
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let mut ctx = PreRenderContext {
            device: &self.gpu.device,
            queue: &self.gpu.queue,
            breakup_texture: None,
            resize_source: &self.frames.resize,
            sampler_store: &self.frames.samplers,
            depth_stencil: DepthStencilTarget {
                view: &self.frames.depth,
            },
            pipeline_storage: &mut self.frames.pipelines,
            dynamic_buffer: &mut self.frames.dynamic,
            encoder: &mut encoder,
        };
        self.app.prepare_write_wait(&mut ctx).unwrap();
        {
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &self.frames.view,
                },
                Some(ctx.depth_stencil),
                "Ger app journey",
            );
            self.app.present_frame(&mut pass).unwrap();
        }
        self.frames.frames += 1;
        crate::layer::user::readback_test_texture(&mut ctx, &self.frames.target)
    }
    pub(super) fn open(&mut self) {
        assert!(self.app.host.as_ref().unwrap().scene.system_menu_allowed());
        let mut input = held(AppAction::SystemMenu);
        assert!(self.app.system_menu_requested(&mut input));
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let mut ctx = PreRenderContext {
            device: &self.gpu.device,
            queue: &self.gpu.queue,
            breakup_texture: None,
            resize_source: &self.frames.resize,
            sampler_store: &self.frames.samplers,
            depth_stencil: DepthStencilTarget {
                view: &self.frames.depth,
            },
            pipeline_storage: &mut self.frames.pipelines,
            dynamic_buffer: &mut self.frames.dynamic,
            encoder: &mut encoder,
        };
        self.app.open_system_menu(&mut ctx).unwrap();
        self.gpu.queue.submit([encoder.finish()]);
        self.ready();
        assert_eq!(
            (self.app.modal.pause_depth, self.app.modal.conceal_depth),
            (1, 1)
        );
        assert!(
            self.app
                .menu_bgm
                .lock()
                .shared_audio()
                .lock()
                .se_player
                .reference_is_paused()
        );
        self.draw();
    }
}

pub(super) fn driver(
    gpu: &Offscreen,
    audio_story: bool,
) -> (Temporary, std::path::PathBuf, Driver<'_>) {
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut code = bytes();
    code.truncate(original.header.code_offset as usize);
    code.push(0xc1);
    for value in [0, 1, 0] {
        num(&mut code, value);
    }
    code.push(0xff);
    for value in [0xffff, -960, -540, 1920, 1080, 0, 0, 0] {
        num(&mut code, value);
    }
    if audio_story {
        code.push(0x90);
        for value in [0, 0, 0, 1000] {
            num(&mut code, value);
        }
        code.push(0x95);
        for value in [0, 0, 0, 0, 1000, 0, 1000] {
            num(&mut code, value);
        }
    }
    msg(&mut code, 1, "@rシステムメニューを検証しています。@k");
    msg(&mut code, 2, "@rメニューから復帰しました。@k");
    code.extend([0, 0, 0]);
    let size = code.len() as u32;
    code[4..8].copy_from_slice(&size.to_le_bytes());
    let scenario = Arc::new(Gerokasu2Scenario::new(code.into()).unwrap());
    driver_with_scenario(gpu, scenario)
}
pub(super) fn driver_with_scenario(
    gpu: &Offscreen,
    scenario: Arc<Gerokasu2Scenario>,
) -> (Temporary, std::path::PathBuf, Driver<'_>) {
    let assets = assets(gpu);
    let audio = Arc::new(AudioManager::new_for_tests());
    let folder = std::env::temp_dir().join(format!(
        "shin-ger-system-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&folder).unwrap();
    let temporary = Temporary(folder);
    let path = temporary.0.join("progress.dat");
    let store = saves::Store::load(path.clone(), None).unwrap();
    let bgm = Arc::new(parking_lot::Mutex::new(menu_audio::MenuMusic::new(
        audio.clone(),
        &scenario,
    )));
    let mut host = shin_tasks::block_on(GerHost::load(
        assets.clone(),
        scenario.clone(),
        audio.clone(),
        42,
    ))
    .unwrap();
    host.scene.import_save_globals(&store.data);
    host.scene
        .publish_global_audio(bgm.lock().shared_audio())
        .unwrap();
    let app = Gerokasu2App {
        backlog: None,
        backlog_options: Default::default(),
        system: None,
        loading_system: None,
        modal: Default::default(),
        backdrop: None,
        frozen_manual: None,
        system_title_exit: false,
        host: Some(host),
        title: None,
        first_boot: None,
        title_demo: None,
        gallery: None,
        loading_gallery: None,
        loading_title: None,
        title_child: None,
        loading_child: None,
        write_wait: None,
        waiting_for_child_save: false,
        waiting_for_exit_save: false,
        exit_save_ready: false,
        adv_frame: None,
        loading_game: None,
        loading_cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        assets,
        scenario,
        suspended: false,
        audio,
        menu_bgm: bgm,
        frame: Default::default(),
        point: Vec2::ZERO,
        error: None,
        save_store: Some(store),
        play_clock: crate::adv::play_clock::PlayClock::from_saved_seconds(0),
        touch_effect: crate::app::touch_effect::TouchEffect::with_seed(5489),
        touch_background: None,
        system_textures: None,
    };
    let d = Driver {
        gpu,
        app,
        frames: Frames::new(gpu),
    };
    (temporary, path, d)
}

#[test]
#[ignore = "real native-art SystemMenu/Save/Delete/Config/Manual and ADV pause on Metal/exact Loopback Audio; temporary progress only"]
fn ger_app_system_menu_manual_writes_child_returns_and_closing_fade_gate() {
    let gpu = Offscreen::new();
    let (temporary, path, mut d) = driver(&gpu, false);
    d.settle(15);
    let boundary = d
        .app
        .host
        .as_ref()
        .unwrap()
        .scene
        .save_session()
        .timeline
        .boundary;
    let completed = d
        .app
        .host
        .as_ref()
        .unwrap()
        .scene
        .runner()
        .completed_commands();
    d.open();
    d.press(AppAction::Enter);
    d.ready();
    assert!(matches!(
        d.app.title_child,
        Some(title_child::TitleChild::Load(_))
    ));
    d.draw();
    // The default No answer never stages a record.
    d.press(AppAction::Enter);
    d.settle(20);
    d.draw();
    d.press(AppAction::Enter);
    d.settle(25);
    assert!(d.app.save_store.as_ref().unwrap().data.manual_save_slots[0].is_none());
    // A second question accepts Yes on its early callback and persists the
    // frozen current VM record, while retaining the menu and paused scene.
    d.press(AppAction::Enter);
    d.settle(20);
    d.press(AppAction::Left);
    d.press(AppAction::Enter);
    d.settle(10);
    assert!(
        d.app.write_wait.is_some(),
        "Save must retain its wait child"
    );
    assert!(!path.exists(), "opening wait panel must not publish a file");
    d.draw();
    let wait_started = Instant::now();
    d.press(AppAction::Cancel);
    d.press(AppAction::Enter);
    assert!(d.app.write_wait.is_some(), "wait has no controller answers");
    d.written();
    assert!(wait_started.elapsed() >= Duration::from_millis(950));
    let saved = saves::read(&path).unwrap();
    assert_eq!(
        saved.manual_save_slots[0]
            .as_ref()
            .unwrap()
            .progress
            .active_entry()
            .save_position,
        boundary
    );
    assert!(d.app.title_child.is_some());
    d.draw();
    // Desktop recovery is separate from native SDK abort: failed atomic IO
    // restores only the staged record and returns to the retained Save owner.
    let previous = d.app.save_store.as_ref().unwrap().data.manual_save_slots[0].clone();
    let blocker = temporary.0.join("not-a-directory");
    std::fs::write(&blocker, b"block publication").unwrap();
    d.app.save_store.as_mut().unwrap().path = blocker.join("progress.dat");
    d.press(AppAction::Enter);
    d.settle(20);
    d.press(AppAction::Left);
    d.press(AppAction::Enter);
    d.settle(10);
    d.written();
    assert_eq!(
        d.app.save_store.as_ref().unwrap().data.manual_save_slots[0],
        previous
    );
    assert_eq!(saves::read(&path).unwrap().manual_save_slots[0], previous);
    assert!(d.app.title_child.is_some() && d.app.system.is_some());
    d.app.save_store.as_mut().unwrap().path = path.clone();
    d.press(AppAction::DeleteSave);
    d.settle(20);
    d.press(AppAction::Left);
    d.press(AppAction::Enter);
    d.settle(10);
    d.written();
    assert!(saves::read(&path).unwrap().manual_save_slots[0].is_none());
    assert_eq!(
        d.app.save_store.as_ref().unwrap().data.save_menu_position,
        0
    );
    d.press(AppAction::Cancel);
    d.settle(45);
    assert!(d.app.title_child.is_none());
    assert_eq!(d.app.system.as_ref().unwrap().model.selected, 0);
    d.app
        .host
        .as_mut()
        .unwrap()
        .scene
        .playback_mut()
        .seen_messages
        .add(12345);
    let before = d.app.save_store.as_ref().unwrap().data.settings.v3_sysvol;
    d.press(AppAction::AnyDown);
    d.press(AppAction::AnyDown);
    d.press(AppAction::Enter);
    d.ready();
    assert!(matches!(
        d.app.title_child,
        Some(title_child::TitleChild::Config(_))
    ));
    d.press(AppAction::Left);
    assert_ne!(
        d.app.save_store.as_ref().unwrap().data.settings.v3_sysvol,
        before
    );
    assert!(
        d.app
            .host
            .as_ref()
            .unwrap()
            .scene
            .playback()
            .seen_messages
            .contains(12345)
    );
    d.draw();
    d.press(AppAction::Cancel);
    let deadline = Instant::now() + Duration::from_secs(5);
    while d.app.write_wait.is_none() {
        d.settle(1);
        assert!(
            Instant::now() < deadline,
            "Config did not create its write owner"
        );
    }
    assert_eq!(
        d.app.write_wait.as_ref().unwrap().purpose,
        write_wait::Purpose::Config
    );
    assert!(!d.app.save_store.as_ref().unwrap().is_flush_pending());
    let frozen_seconds = d.app.save_store.as_ref().unwrap().data.play_seconds;
    // The real Host::load constructor has already copied the complete SaveData.
    // Later live changes during panel opening cannot enter this write.
    d.app.play_clock = crate::adv::play_clock::PlayClock::from_saved_seconds(frozen_seconds + 222);
    d.settle(6);
    d.draw();
    d.written();
    assert_eq!(saves::read(&path).unwrap().play_seconds, frozen_seconds);
    assert!(d.app.save_store.as_ref().unwrap().data.play_seconds >= frozen_seconds + 222);
    d.settle(70);
    assert!(d.app.title_child.is_none());
    d.press(AppAction::AnyDown);
    d.press(AppAction::Enter);
    d.ready();
    assert!(matches!(
        d.app.title_child,
        Some(title_child::TitleChild::Manual(_))
    ));
    d.draw();
    d.press(AppAction::Cancel);
    d.settle(45);
    assert!(d.app.title_child.is_none());
    assert_eq!(
        d.app
            .host
            .as_ref()
            .unwrap()
            .scene
            .runner()
            .completed_commands(),
        completed
    );
    assert_eq!(
        d.app
            .host
            .as_ref()
            .unwrap()
            .scene
            .save_session()
            .timeline
            .boundary,
        boundary
    );
    d.press(AppAction::Cancel);
    let deadline = Instant::now() + Duration::from_secs(5);
    while d.app.system.is_some() {
        d.settle(1);
        assert!(Instant::now() < deadline);
    }
    assert_eq!(d.app.modal.pause_depth, 0);
    assert!(d.app.backdrop.is_some());
    assert!(
        !d.app
            .menu_bgm
            .lock()
            .shared_audio()
            .lock()
            .se_player
            .reference_is_paused()
    );
    // Confirm during the closing fade must not reach the old message/VM.
    for _ in 0..5 {
        d.press(AppAction::Enter);
        assert_eq!(
            d.app
                .host
                .as_ref()
                .unwrap()
                .scene
                .runner()
                .completed_commands(),
            completed
        );
    }
    d.settle(15);
    assert!(d.app.backdrop.is_none());
    assert_eq!(d.app.modal.conceal_depth, 0);
    d.draw();
    for _ in 0..3 {
        d.press(AppAction::Enter);
        d.settle(2);
        if d.app
            .host
            .as_ref()
            .unwrap()
            .scene
            .runner()
            .completed_commands()
            > completed
        {
            break;
        }
    }
    assert!(
        d.app
            .host
            .as_ref()
            .unwrap()
            .scene
            .runner()
            .completed_commands()
            > completed
    );
    println!(
        "GER_SYSTEM_MENU_APP Save_No_Yes_IO_Failure_Delete_Config_Manual_wait_owner_parent_returns_frozen_VM_closing_gate_resume=verified frames={} audio=Loopback Audio",
        d.frames.frames
    );
}
