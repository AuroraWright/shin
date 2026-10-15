//! Native SaveMenu child lifetime, serial visual work and external storage handoff.
use super::{
    character_view::CharacterTextures,
    input_area::{InputArea, PointerFrame},
    save_menu::{SLOT_COUNT, SaveMenuInput, SaveMenuModel, SaveMenuPhase, SaveMenuRequest},
    save_menu_dialog::SaveMenuDialog,
    save_menu_preview::{SaveMenuPreview, SaveMenuPreviewScene},
    save_menu_scene::{SaveMenuSceneCache, SavedSlotScene},
    save_menu_text::SaveSlotText,
    save_menu_view,
};
use crate::{
    app::AppAction,
    asset::{font::GpuFontLazy, system::AssetServer},
    render::PreRenderContext,
};
use anyhow::Result;
use enum_map::EnumMap;
use glam::Vec2;
use shin_audio::AudioManager;
use shin_core::{
    format::{
        save::{GameData, GameDataEntry, SaveData},
        scenario::Scenario,
    },
    time::Ticks,
};
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

type Atlases = (
    Arc<CharacterTextures>,
    Arc<CharacterTextures>,
    Arc<GpuFontLazy>,
);
struct PreviewJob {
    slot: usize,
    source: Arc<SavedSlotScene>,
    task: shin_tasks::AsyncTask<Result<SaveMenuPreviewScene>>,
}
pub(crate) struct SaveMenuScreen {
    loading: Option<shin_tasks::AsyncTask<Result<Atlases>>>,
    atlases: Option<Atlases>,
    model: SaveMenuModel,
    scenes: SaveMenuSceneCache,
    texts: [Option<SaveSlotText>; SLOT_COUNT],
    // Retain visible preview owners: the asset caches are weak. Dropping their
    // scene after capture forced each following row to decode shared PIC/BUP
    // resources again. Offscreen/write invalidation retires both owners together.
    previews: [Option<(Arc<SavedSlotScene>, SaveMenuPreview, SaveMenuPreviewScene)>; SLOT_COUNT],
    preview_job: Option<PreviewJob>,
    ready_preview: Option<(usize, Arc<SavedSlotScene>, SaveMenuPreviewScene)>,
    preview_error: Option<anyhow::Error>,
    dialog: Option<SaveMenuDialog>,
    area: InputArea,
    extent: (u16, u16),
    canvas: (u32, u32),
    pointer_held: bool,
    requests: Vec<SaveMenuRequest>,
    writing_slot: Option<usize>,
    write_published: bool,
    sounds: Vec<&'static str>,
    server: Arc<AssetServer>,
    scenario: Arc<Scenario>,
    audio: Arc<AudioManager>,
}
impl SaveMenuScreen {
    pub(crate) fn suspend_pointer(&mut self, held: bool) {
        self.pointer_held = held;
        self.set_pointer_extent(self.extent);
    }
    pub fn new(
        server: Arc<AssetServer>,
        audio: Arc<AudioManager>,
        scenario: Arc<Scenario>,
        data: SaveData,
        entry: Option<GameData>,
        canvas: (u32, u32),
    ) -> Self {
        let assets = server.clone();
        let model = SaveMenuModel::new(&data, entry);
        let mut area = InputArea::canvas();
        area.set_long_press_enabled(true);
        Self {
            loading: Some(shin_tasks::async_io::spawn(async move {
                futures::try_join!(
                    assets.load("/saveload.txa"),
                    assets.load("/systex.txa"),
                    assets.load("/system.fnt")
                )
            })),
            atlases: None,
            model,
            scenes: SaveMenuSceneCache::new(scenario.clone(), data),
            texts: std::array::from_fn(|_| None),
            previews: std::array::from_fn(|_| None),
            preview_job: None,
            ready_preview: None,
            preview_error: None,
            dialog: None,
            area,
            extent: (1920, 1080),
            canvas,
            pointer_held: false,
            requests: Vec::new(),
            writing_slot: None,
            write_published: false,
            sounds: Vec::new(),
            server,
            scenario,
            audio,
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_dialog_is_open(&self) -> bool {
        self.dialog
            .as_ref()
            .is_some_and(SaveMenuDialog::reference_is_open)
    }
    #[cfg(test)]
    pub(crate) fn reference_preview_ready(&self, slot: usize) -> bool {
        self.previews[slot].is_some()
    }
    #[cfg(test)]
    pub(crate) fn reference_preview_dimensions(&self, slot: usize) -> Option<(u32, u32)> {
        self.previews[slot]
            .as_ref()
            .map(|(_, preview, _)| preview.dimensions())
    }
    #[cfg(test)]
    pub(crate) fn reference_dialog_button_center(&self, index: usize) -> Vec2 {
        self.dialog.as_ref().unwrap().reference_button_center(index)
    }
    pub fn model(&self) -> &SaveMenuModel {
        &self.model
    }
    pub fn set_pointer_extent(&mut self, extent: (u16, u16)) {
        self.area.disable(&mut self.model);
        self.area = InputArea::new(extent.0, extent.1);
        self.area.set_long_press_enabled(true);
        self.extent = extent;
        if let Some(dialog) = &mut self.dialog {
            dialog.set_pointer_extent(extent, self.pointer_held);
        }
    }
    pub fn pointer_state(&mut self, position: Vec2, held: bool, elapsed: f64) {
        self.pointer_held = held;
        if let Some(dialog) = &mut self.dialog {
            dialog.pointer_state(position, held, elapsed);
        }
    }
    pub fn input_frame(
        &mut self,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        axis: f32,
    ) {
        if self.atlases.is_none() {
            return;
        }
        if let Some(dialog) = &mut self.dialog {
            self.area.disable(&mut self.model);
            dialog.keyboard(state);
        } else if self.model.phase() != SaveMenuPhase::Active {
            self.area.disable(&mut self.model);
        } else if !self.area.update(&mut self.model, frame) {
            self.model.input(SaveMenuInput {
                up: state[AppAction::Up].is_clicked_or_repeated,
                down: state[AppAction::AnyDown].is_clicked_or_repeated,
                left: state[AppAction::Left].is_clicked_or_repeated,
                right: state[AppAction::Right].is_clicked_or_repeated,
                previous_page: state[AppAction::CharacterPageUp].is_clicked_or_repeated,
                next_page: state[AppAction::CharacterPageDown].is_clicked_or_repeated,
                confirm: state[AppAction::Enter].is_clicked,
                delete: state[AppAction::DeleteSave].is_clicked,
                close: state[AppAction::Cancel].is_clicked,
                axis,
            });
        }
    }
    fn current_source(&self, slot: usize, source: &Arc<SavedSlotScene>) -> bool {
        self.scenes
            .scene(slot)
            .is_some_and(|current| Arc::ptr_eq(current, source))
            && self.model.visible_slots().any(|visible| visible == slot)
            && self.model.slots()[slot].is_some()
    }
    fn update_rows(&mut self) -> Result<()> {
        if let Some(error) = self.preview_error.take() {
            return Err(error);
        }
        let visible: Vec<_> = self.model.visible_slots().collect();
        for slot in 0..SLOT_COUNT {
            if !visible.contains(&slot) {
                self.texts[slot] = None;
                self.previews[slot] = None;
                continue;
            }
            let labels = self.scenes.labels(&self.model, slot);
            if self.texts[slot]
                .as_ref()
                .is_none_or(|text| text.labels != labels)
            {
                self.texts[slot] = Some(SaveSlotText::load(&self.server, labels)?);
            }
            if self.previews[slot]
                .as_ref()
                .is_some_and(|(source, _, _)| !self.current_source(slot, source))
            {
                self.previews[slot] = None;
            }
            if self.preview_job.is_none()
                && self.ready_preview.is_none()
                && self.previews[slot].is_none()
            {
                if let Some(source) = self.scenes.scene(slot).cloned() {
                    let (assets, audio, scenario, state) = (
                        self.server.clone(),
                        self.audio.clone(),
                        self.scenario.clone(),
                        source.clone(),
                    );
                    #[cfg(test)]
                    let serial = self.scenes.reference_disable_reuse;
                    self.preview_job = Some(PreviewJob {
                        slot,
                        source,
                        task: shin_tasks::async_io::spawn(async move {
                            #[cfg(test)]
                            if serial {
                                return SaveMenuPreviewScene::reference_load_serial(
                                    &assets,
                                    &audio,
                                    &scenario,
                                    &state.scene.state,
                                )
                                .await;
                            }
                            SaveMenuPreviewScene::load(
                                &assets,
                                &audio,
                                &scenario,
                                &state.scene.state,
                            )
                            .await
                        }),
                    });
                }
            }
        }
        self.poll_preview()?;
        // Native text/texture scheduling precedes replay completion publication.
        self.scenes.update(&self.model)?;
        Ok(())
    }
    fn poll_preview(&mut self) -> Result<()> {
        if let Some(result) = self
            .preview_job
            .as_mut()
            .and_then(|job| job.task.poll_naive())
        {
            let job = self.preview_job.take().unwrap();
            if self.current_source(job.slot, &job.source) {
                self.ready_preview = Some((job.slot, job.source, result?));
            }
        }
        Ok(())
    }
    pub fn update(&mut self, delta: Ticks) -> Result<Option<i32>> {
        if let Some(task) = &mut self.loading {
            if let Some(result) = task.poll_naive() {
                self.loading = None;
                let (menu, system, font) = result?;
                menu.require_names(
                    "/saveload.txa",
                    &[
                        "bg",
                        "obi",
                        "window",
                        "focus",
                        "line",
                        "num",
                        "bar",
                        "nodata",
                        "close",
                        "help_saveload",
                        "key",
                        if self.model.is_save() {
                            "caption_save"
                        } else {
                            "caption_load"
                        },
                    ],
                )?;
                system.require_names("/systex.txa", &["scroll"])?;
                self.atlases = Some((menu, system, font));
                self.model.assets_ready();
            }
            return Ok(None);
        }
        if let Some(dialog) = &mut self.dialog {
            let mut finished = false;
            for response in dialog.update(delta) {
                self.model
                    .dialog_response(response.button, i32::from(response.final_flag));
                finished |= response.final_flag;
            }
            if finished {
                self.sounds.extend(dialog.take_sounds());
                self.dialog = None;
            }
        }
        self.model.update(delta.as_f32());
        for request in self.model.take_requests() {
            match request {
                SaveMenuRequest::Confirmation {
                    action, overwrite, ..
                } => {
                    let mut dialog =
                        SaveMenuDialog::load(&self.server, action, overwrite, self.pointer_held)?;
                    dialog.set_pointer_extent(self.extent, self.pointer_held);
                    self.area.disable(&mut self.model);
                    self.dialog = Some(dialog);
                }
                request => {
                    if let SaveMenuRequest::Save { slot, .. } | SaveMenuRequest::Delete { slot } =
                        &request
                    {
                        self.writing_slot = Some(*slot);
                        self.write_published = false;
                    }
                    self.requests.push(request);
                }
            }
        }
        self.update_rows()?;
        Ok(self.model.take_responses().into_iter().next())
    }
    pub fn take_requests(&mut self) -> Vec<SaveMenuRequest> {
        std::mem::take(&mut self.requests)
    }
    pub fn write_started(&mut self, game: Option<GameData>) {
        let Some(slot) = self.writing_slot else {
            return;
        };
        if self.write_published {
            return;
        }
        self.model.write_started(game);
        self.write_published = true;
        self.scenes.invalidate(slot);
        self.texts[slot] = None;
        self.previews[slot] = None;
    }
    pub fn write_finished(&mut self, success: bool, game: Option<GameData>) {
        let Some(slot) = self.writing_slot.take() else {
            return;
        };
        self.model.write_finished(success, game);
        // Native dafb0 success clears only the pending flag: retain the new
        // preview already prepared since da9a0 published the slot. Failure
        // restores its old context and retires matching visual work.
        if !success || !self.write_published {
            self.scenes.invalidate(slot);
            self.texts[slot] = None;
            self.previews[slot] = None;
        }
        self.write_published = false;
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        let mut sounds = std::mem::take(&mut self.sounds);
        sounds.extend(self.model.take_sounds());
        if let Some(dialog) = &mut self.dialog {
            sounds.extend(dialog.take_sounds());
        }
        sounds
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) {
        // Resource workers can finish between update and preparation. Poll
        // again before rendering rather than adding a frame per visible row.
        // Preparation errors return through the ordinary update/child recovery
        // path on the next frame; stale slot results are still rejected.
        #[cfg(test)]
        let late_poll = !self.scenes.reference_disable_reuse;
        #[cfg(not(test))]
        let late_poll = true;
        if late_poll {
            if let Err(error) = self.poll_preview() {
                self.preview_error = Some(error);
            }
        }
        if let Some((slot, source, mut scene)) = self.ready_preview.take() {
            if self.current_source(slot, &source) {
                let preview = scene.capture(context, self.canvas);
                self.previews[slot] = Some((source, preview, scene));
            }
        }
        if let Some(dialog) = &mut self.dialog {
            dialog.prepare(context);
        }
    }
    pub fn render(&self, pass: &mut RenderPass) {
        if let Some((atlas, system, _)) = &self.atlases {
            save_menu_view::render(
                &self.model,
                atlas,
                system,
                pass,
                crate::render::top_left_projection_matrix(),
                |pass, slot, transform, builder| {
                    if let Some(text) = &self.texts[slot] {
                        text.render(pass, transform, builder);
                    }
                    if let Some((_, preview, _)) = &self.previews[slot] {
                        preview.render(pass, transform, builder);
                        true
                    } else {
                        false
                    }
                },
            );
            if let Some(dialog) = &self.dialog {
                dialog.render(pass);
            }
        }
    }
}

#[cfg(test)]
pub(super) fn assert_reference_thumbnail_reuse(
    context: &mut PreRenderContext,
    output: &wgpu::Texture,
    server: &Arc<AssetServer>,
) {
    use shin_core::primitives::color::UnormColor;
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    use std::time::{Duration, Instant};

    let path = std::path::PathBuf::from(std::env::var_os("SHIN_LOAD_FEEDBACK_SAVE").unwrap());
    assert!(
        path.starts_with("/private/tmp"),
        "Use a copied working save"
    );
    let bytes = std::fs::read(&path).unwrap();
    let mut data = SaveData::decode(&bytes).unwrap();
    let game = data
        .manual_save_slots
        .iter()
        .flatten()
        .next()
        .unwrap()
        .clone();
    // A real authored saved history displayed in five occupied rows. Repeated
    // rooms/characters are the normal cache-sharing case; no save is written.
    data.save_menu_position = 0;
    let distinct = std::env::var_os("SHIN_THUMBNAIL_DISTINCT_HISTORIES").is_some();
    if !distinct {
        data.manual_save_slots = std::array::from_fn(|index| (index < 5).then(|| game.clone()));
    }
    assert!(data.manual_save_slots[..5].iter().all(Option::is_some));
    let scenario: Arc<Scenario> = server.load_sync("/main.snr").unwrap();
    let audio = Arc::new(AudioManager::new_for_tests());
    let mut records = Vec::new();
    let mut baseline_pixels = None;
    for retain in [false, true] {
        let mut screen = SaveMenuScreen::new(
            server.clone(),
            audio.clone(),
            scenario.clone(),
            data.clone(),
            None,
            (1920, 1080),
        );
        screen.scenes.reference_disable_reuse = !retain;
        let deadline = Instant::now() + Duration::from_secs(45);
        while screen.atlases.is_none() {
            screen.update(Ticks::ZERO).unwrap();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        screen.update(Ticks::from_f32(10.0)).unwrap();
        let start = Instant::now();
        let requests_before = server.reference_uncached_paths().len();
        let mut frames = 0;
        while screen.previews[..5].iter().any(Option::is_none) {
            screen.update(Ticks::ZERO).unwrap();
            screen.prepare(context);
            if !retain {
                for (_, _, scene) in screen.previews.iter_mut().flatten() {
                    // Reproduce the former capture-and-drop policy, with the
                    // same current replay/render code and the same authored save.
                    scene.reference_release_resources();
                }
            }
            frames += 1;
            assert!(Instant::now() < deadline, "Thumbnail batch stalled");
            if screen.previews[..5].iter().any(Option::is_none) {
                std::thread::sleep(Duration::from_secs_f32(1.0 / 60.0));
            }
        }
        let seconds = start.elapsed().as_secs_f64();
        let uncached = server.reference_uncached_paths()[requests_before..].to_vec();
        let visual_reads: Vec<_> = uncached
            .into_iter()
            .filter(|p| p.ends_with(".pic") || p.ends_with(".bup"))
            .collect();
        let picture_owners = server
            .context()
            .picture_cache
            .reference_ownership_counts()
            .0;
        let bustup_owners = server.context().bustup_cache.reference_ownership_counts().0;
        if retain {
            assert!(
                picture_owners + bustup_owners > 0,
                "Visible previews must retain actual resources"
            );
        } else {
            assert_eq!(
                picture_owners + bustup_owners,
                0,
                "Former policy drops visual owners"
            );
        }
        {
            let view = output.create_view(&Default::default());
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Measured SaveMenu thumbnails",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            screen.render(&mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(context, output);
        if let Some(baseline) = &baseline_pixels {
            assert_eq!(
                &pixels, baseline,
                "Resource retention must preserve every UI/thumbnail pixel"
            );
        } else {
            baseline_pixels = Some(pixels.clone());
        }
        if retain {
            std::fs::write(
                std::path::Path::new(&std::env::var_os("SHIN_LOAD_FEEDBACK_OUTPUT").unwrap())
                    .join("load-window-thumbnails.rgba"),
                &pixels,
            )
            .unwrap();
        }
        records.push(serde_json::json!({
            "retain_visible_resources": retain, "seconds": seconds, "frames": frames,
            "visual_uncached_loads": visual_reads, "picture_owners": picture_owners,
            "bustup_block_owners": bustup_owners, "scenario": game.entry.scenario_id,
            "saved_boundary": game.entry.save_position,
            "replay_starts": screen.scenes.reference_replay_starts,
            "distinct_histories": distinct,
        }));
        let source = screen.scenes.scene(0).unwrap().clone();
        if retain {
            screen.previews[0] = None;
            screen.preview_job = Some(PreviewJob {
                slot: 0,
                source: source.clone(),
                task: shin_tasks::async_io::spawn(async { anyhow::bail!("late preview failure") }),
            });
            while screen.preview_job.is_some() {
                screen.prepare(context);
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert!(screen.preview_error.is_some());
            assert!(
                screen
                    .update(Ticks::ZERO)
                    .unwrap_err()
                    .to_string()
                    .contains("late preview failure")
            );
        }
        screen.model.input(SaveMenuInput {
            next_page: true,
            ..Default::default()
        });
        screen.update(Ticks::from_f32(1000.0)).unwrap();
        screen.update(Ticks::ZERO).unwrap();
        assert!(
            screen.previews[..5].iter().all(Option::is_none),
            "Offscreen previews retire"
        );
        if retain {
            screen.preview_job = Some(PreviewJob {
                slot: 0,
                source,
                task: shin_tasks::async_io::spawn(async { anyhow::bail!("stale preview failure") }),
            });
            while screen.preview_job.is_some() {
                screen.prepare(context);
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert!(
                screen.preview_error.is_none(),
                "Retired row cannot report a stale failure"
            );
            screen.update(Ticks::ZERO).unwrap();
        }
        drop(screen);
        assert_eq!(
            server
                .context()
                .picture_cache
                .reference_ownership_counts()
                .0,
            0
        );
        assert_eq!(
            server.context().bustup_cache.reference_ownership_counts().0,
            0
        );
    }
    let baseline_reads = records[0]["visual_uncached_loads"]
        .as_array()
        .unwrap()
        .len();
    let retained_reads = records[1]["visual_uncached_loads"]
        .as_array()
        .unwrap()
        .len();
    assert!(
        retained_reads > 0
            && retained_reads <= baseline_reads
            && (distinct || retained_reads < baseline_reads),
        "Visible shared assets must avoid redundant loads: {baseline_reads} -> {retained_reads}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let out = std::path::PathBuf::from(std::env::var_os("SHIN_LOAD_FEEDBACK_OUTPUT").unwrap());
    std::fs::write(
        out.join("thumbnail-measurements.json"),
        serde_json::to_vec_pretty(&records).unwrap(),
    )
    .unwrap();
    eprintln!(
        "Five authored thumbnails: redundant visual loads {baseline_reads} -> {retained_reads}; identical pixels and offscreen retirement pass"
    );
}

#[cfg(test)]
pub(super) fn assert_reference_screen(
    context: &mut PreRenderContext,
    output: &wgpu::Texture,
    server: &Arc<AssetServer>,
) {
    use shin_core::primitives::color::UnormColor;
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    fn send(screen: &mut SaveMenuScreen, frame: &mut u32, action: AppAction) {
        *frame += 1;
        let mut state = EnumMap::default();
        state[action] = ActionState {
            is_clicked: true,
            is_clicked_or_repeated: true,
            ..Default::default()
        };
        screen.input_frame(
            &PointerFrame {
                number: *frame,
                time_seconds: *frame as f64 / 60.0,
                events: &[],
            },
            &state,
            0.0,
        );
    }
    fn assets(screen: &mut SaveMenuScreen) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while screen.atlases.is_none() {
            assert_eq!(screen.update(Ticks::from_f32(1000.0)).unwrap(), None);
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(screen.model.progress(), 0.0);
        screen.update(Ticks::from_f32(10.0)).unwrap();
    }
    fn open_dialog(screen: &mut SaveMenuScreen, frame: &mut u32, action: AppAction) {
        send(screen, frame, action);
        screen.update(Ticks::from_f32(7.0)).unwrap();
        screen.update(Ticks::ZERO).unwrap();
        assert_eq!(screen.model.phase(), SaveMenuPhase::Confirmation);
        screen.update(Ticks::from_f32(12.0)).unwrap();
        screen.update(Ticks::ZERO).unwrap();
        assert!(screen.dialog.as_ref().unwrap().reference_is_open());
    }
    fn choose(screen: &mut SaveMenuScreen, frame: &mut u32, yes: bool) {
        if yes {
            send(screen, frame, AppAction::Left);
        }
        send(screen, frame, AppAction::Enter);
        screen.update(Ticks::from_f32(7.0)).unwrap();
        screen.update(Ticks::ZERO).unwrap();
    }
    fn finish_dialog(screen: &mut SaveMenuScreen) {
        screen.update(Ticks::from_f32(12.0)).unwrap();
        screen.update(Ticks::from_f32(5.0)).unwrap();
        assert!(screen.dialog.is_none());
    }
    fn warm_preview(screen: &mut SaveMenuScreen, context: &mut PreRenderContext) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while screen.previews[screen.model.selected()].is_none() {
            screen.update(Ticks::ZERO).unwrap();
            screen.prepare(context);
            assert!(
                std::time::Instant::now() < deadline,
                "SaveMenu serial scene/preview timed out"
            );
            std::thread::yield_now();
        }
    }
    let scenario: Arc<Scenario> = server.load_sync("/main.snr").unwrap();
    let audio = Arc::new(AudioManager::new_for_tests());
    let entry = super::VmState::for_scenario(7, 42).snapshot_entry();
    let mut frame = 0;
    let mut screen = SaveMenuScreen::new(
        server.clone(),
        audio.clone(),
        scenario.clone(),
        SaveData::default(),
        Some(GameData::from_entry(entry.clone())),
        (1920, 1080),
    );
    screen.set_pointer_extent((960, 540));
    send(&mut screen, &mut frame, AppAction::AnyDown);
    assert_eq!(screen.model.selected(), 0, "loading owns input");
    assets(&mut screen);
    open_dialog(&mut screen, &mut frame, AppAction::Enter);
    screen.prepare(context);
    let view = output.create_view(&Default::default());
    {
        let mut pass = context.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::Screen,
                view: &view,
            },
            Some(context.depth_stencil),
            "SaveMenu live confirmation owner",
        );
        pass.clear(Some(UnormColor::BLACK), Some(128), Some(1.0));
        screen.render(&mut pass);
    }
    std::fs::write(
        "/private/tmp/shin-save-menu-screen-dialog.rgba",
        crate::layer::user::readback_test_texture(context, output),
    )
    .unwrap();
    choose(&mut screen, &mut frame, false);
    assert!(
        screen.take_requests().is_empty(),
        "default No must not write"
    );
    finish_dialog(&mut screen);
    assert_eq!(screen.model.phase(), SaveMenuPhase::Active);
    open_dialog(&mut screen, &mut frame, AppAction::Enter);
    choose(&mut screen, &mut frame, true);
    assert_eq!(screen.model.phase(), SaveMenuPhase::Writing);
    assert!(
        matches!(screen.take_requests().as_slice(), [SaveMenuRequest::Save { slot:0, game }] if game.entry.random_seed == 42)
    );
    assert!(screen.take_requests().is_empty());
    screen.write_finished(false, None);
    assert!(screen.model.slots()[0].is_none());
    assert_eq!(screen.model.phase(), SaveMenuPhase::Writing);
    finish_dialog(&mut screen);
    assert_eq!(screen.model.phase(), SaveMenuPhase::Active);
    open_dialog(&mut screen, &mut frame, AppAction::Enter);
    choose(&mut screen, &mut frame, true);
    let requests = screen.take_requests();
    let [SaveMenuRequest::Save { game: saved, .. }] = requests.as_slice() else {
        panic!("Save request missing");
    };
    let game = saved.clone();
    screen.write_finished(true, Some(game.clone()));
    screen.write_finished(false, None); // Duplicate completion cannot roll back.
    finish_dialog(&mut screen);
    assert_eq!(
        screen.model.slots()[0].as_ref().unwrap().entry.random_seed,
        42
    );
    warm_preview(&mut screen, context);
    let former = screen.scenes.scene(0).unwrap().clone();
    assert!(screen.current_source(0, &former));
    // A committed overwrite changes identity and invalidates pending old visual work.
    open_dialog(&mut screen, &mut frame, AppAction::Enter);
    choose(&mut screen, &mut frame, true);
    assert!(matches!(
        screen.take_requests().as_slice(),
        [SaveMenuRequest::Save { slot: 0, .. }]
    ));
    screen.write_finished(true, Some(game.clone()));
    assert!(!screen.current_source(0, &former));
    assert!(screen.previews[0].is_none());
    finish_dialog(&mut screen);
    warm_preview(&mut screen, context);
    assert!(!Arc::ptr_eq(screen.scenes.scene(0).unwrap(), &former));
    open_dialog(&mut screen, &mut frame, AppAction::DeleteSave);
    choose(&mut screen, &mut frame, true);
    assert!(matches!(
        screen.take_requests().as_slice(),
        [SaveMenuRequest::Delete { slot: 0 }]
    ));
    let old_preview = screen.scenes.scene(0).unwrap().clone();
    screen.write_started(None);
    assert!(screen.model.slots()[0].is_none() && screen.previews[0].is_none());
    screen.write_finished(false, None);
    assert!(screen.model.slots()[0].is_some() && screen.previews[0].is_none());
    assert!(
        !screen.current_source(0, &old_preview),
        "Native failed deletion restores context and retires its preview/jobs"
    );
    finish_dialog(&mut screen);
    warm_preview(&mut screen, context);
    assert!(screen.previews[0].is_some());
    assert!(!Arc::ptr_eq(screen.scenes.scene(0).unwrap(), &old_preview));
    // Actual resized InputArea must keep optional long press enabled.
    use super::input_area::{PointerEvent, PointerKind};
    frame += 1;
    screen.pointer_state(Vec2::new(240.0, 110.0), true, 0.0);
    screen.input_frame(
        &PointerFrame {
            number: frame,
            time_seconds: 100.0,
            events: &[PointerEvent {
                id: 1,
                kind: PointerKind::Down,
                position: glam::ivec2(240, 110),
            }],
        },
        &EnumMap::default(),
        0.0,
    );
    frame += 1;
    screen.input_frame(
        &PointerFrame {
            number: frame,
            time_seconds: 101.16,
            events: &[],
        },
        &EnumMap::default(),
        0.0,
    );
    assert_eq!(screen.model.phase(), SaveMenuPhase::RowFlash);
    screen.update(Ticks::from_f32(7.0)).unwrap();
    screen.update(Ticks::ZERO).unwrap();
    screen.update(Ticks::from_f32(12.0)).unwrap();
    screen.update(Ticks::ZERO).unwrap();
    screen.pointer_state(Vec2::new(240.0, 110.0), false, 0.0);
    frame += 1;
    screen.input_frame(
        &PointerFrame {
            number: frame,
            time_seconds: 101.17,
            events: &[],
        },
        &EnumMap::default(),
        0.0,
    );
    choose(&mut screen, &mut frame, true);
    assert!(matches!(
        screen.take_requests().as_slice(),
        [SaveMenuRequest::Delete { slot: 0 }]
    ));
    screen.write_finished(true, None);
    finish_dialog(&mut screen);
    assert!(screen.model.slots()[0].is_none() && screen.previews[0].is_none());
    send(&mut screen, &mut frame, AppAction::Cancel);
    screen.update(Ticks::from_f32(7.0)).unwrap();
    assert_eq!(screen.update(Ticks::ZERO).unwrap(), Some(0));
    assert_eq!(screen.update(Ticks::from_f32(10.0)).unwrap(), Some(1));
    assert!(screen.take_requests().is_empty());
    let mut data = SaveData::default();
    data.manual_save_slots[0] = Some(game);
    let mut load = SaveMenuScreen::new(server.clone(), audio, scenario, data, None, (1920, 1080));
    assets(&mut load);
    open_dialog(&mut load, &mut frame, AppAction::Enter);
    choose(&mut load, &mut frame, true);
    assert!(
        load.take_requests().is_empty(),
        "Load waits for final dialog callback"
    );
    finish_dialog(&mut load);
    assert_eq!(load.update(Ticks::from_f32(10.0)).unwrap(), None);
    assert!(
        matches!(load.take_requests().as_slice(), [SaveMenuRequest::Load { slot:0, game }] if game.entry.random_seed == 42)
    );
    assert!(load.take_requests().is_empty());
    assert_eq!(load.model.phase(), SaveMenuPhase::Closed);
    eprintln!(
        "SaveMenu screen: async resources/replay/preview, default No, final0 Save/Delete, failure/duplicate completion, overwrite invalidation, ordinary close0/1 and final1 Load handoff passed"
    );
}

#[cfg(test)]
pub(crate) fn reference_authored_thumbnail_entry(scenario: &Scenario) -> GameDataEntry {
    use super::StartableCommand;
    use shin_core::vm::{
        Scripter,
        command::{CommandResult, RuntimeCommand},
    };
    let mut script = Scripter::new(scenario, 0, 42);
    let mut state = super::VmState::for_scenario(0, 42);
    let mut result = CommandResult::None;
    for _ in 0..10000 {
        let command = script.run(result).unwrap();
        if let RuntimeCommand::SGET(get) = command {
            let value = get.apply_state(&mut state);
            result = get.token.finish(value);
        } else {
            super::command::apply_command_state_ref(&command, &mut state).unwrap();
            if matches!(command, RuntimeCommand::MSGSET(_))
                && state.save_position > 10
                && state
                    .layers
                    .layerbanks
                    .iter()
                    .filter(|(_, layer)| layer.layer_type.is_some())
                    .count()
                    > 1
            {
                return state.snapshot_entry();
            }
            result = command.execute_dummy().unwrap();
        }
    }
    panic!("Authored thumbnail boundary not reached");
}
