//! MessageBrowser composition and visible-row glyph cache.
use std::{collections::BTreeMap, sync::Arc};

use anyhow::{Result, anyhow};
use shin_core::time::{Ticks, Tween, Tweener};
use shin_render::render_pass::RenderPass;

use crate::adv::{
    assets::AdvFonts,
    backlog::Backlog,
    backlog::model::{BacklogModel, VIEW_HEIGHT, voice_paths},
    backlog::text::{BacklogText, display_text, layout_display_text, localize_item},
    backlog::view::{BacklogShell, BacklogTextures},
};
use crate::render::PreRenderContext;

pub(crate) struct BacklogScreen {
    pub model: BacklogModel,
    pub shell: BacklogShell,
    history: Backlog,
    fonts: AdvFonts,
    textures: Arc<BacklogTextures>,
    outline_mode: u8,
    system: Arc<crate::adv::characters::view::CharacterTextures>,
    cached: BTreeMap<usize, BacklogText>,
    voice_flashes: BTreeMap<usize, Tweener>,
    confirmation_feedback: super::feedback::RowConfirmationFeedback,
}

#[cfg(test)]
pub(crate) fn reference_saved_backlog_latency<'a>(
    server: &'a Arc<crate::asset::system::AssetServer>,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
) {
    use std::time::Instant;
    let path = std::env::var_os("SHIN_REFERENCE_BACKLOG_SAVE").unwrap();
    let globals = shin_core::format::save::SaveData::decode(&std::fs::read(path).unwrap()).unwrap();
    let source = globals
        .auto_save_slot
        .as_ref()
        .or_else(|| globals.manual_save_slots.iter().flatten().next())
        .unwrap();
    let scenario: Arc<shin_core::format::scenario::Scenario> =
        server.load_sync("/main.snr").unwrap();
    let fonts = crate::adv::assets::AdvFonts {
        system_font: server.load_sync("/system.fnt").unwrap(),
        medium_font: server.load_sync("/newrodin-medium.fnt").unwrap(),
        bold_font: server.load_sync("/newrodin-bold.fnt").unwrap(),
    };
    eprintln!(
        "Backlog latency snapshot: scenario{} boundary{} unread-gate{}",
        source.entry.scenario_id,
        source.entry.save_position,
        globals.settings.v11_disallowskipunread
    );
    let mut expected = None;
    for pass in 0..3 {
        let begin = Instant::now();
        let collected = super::collection::BrowserHistory::collect(
            &scenario,
            &source.entry,
            &globals,
            1_000_000,
        )
        .unwrap();
        let collection = begin.elapsed();
        let rows = collected.history.entries.len();
        let current = collected.current_row.unwrap();
        if let Some(previous) = expected {
            assert_eq!(previous, (rows, current));
        }
        expected = Some((rows, current));
        let begin = Instant::now();
        let textures = server.load_sync::<BacklogTextures>("/logview.txa").unwrap();
        let atlas = begin.elapsed();
        let begin = Instant::now();
        let screen = BacklogScreen::new(
            collected.history,
            fonts.clone(),
            textures,
            server.load_sync("/systex.txa").unwrap(),
            current,
            globals.settings.v8,
        )
        .unwrap();
        let layout = begin.elapsed();
        assert_eq!(screen.history().entries.len(), rows);
        assert_eq!(screen.model.selected, Some(current));
        eprintln!(
            "Backlog latency pass{pass}: rows{rows} current{current} collection={collection:?} atlas={atlas:?} layout={layout:?}"
        );
    }
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    for pass in 0..3 {
        let begin = Instant::now();
        let mut modal = super::modal::BacklogModal::new(
            scenario.clone(),
            source.clone(),
            globals.clone(),
            server.clone(),
            audio.clone(),
            fonts.clone(),
        );
        let mut update = crate::update::UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: Default::default(),
            delta_ticks: Ticks::from_f32(1.0),
            asset_server: server,
            pre_render: context,
        };
        while modal.screen.is_none() {
            assert!(
                begin.elapsed() < std::time::Duration::from_secs(2),
                "backlog publication timed out"
            );
            assert!(
                !modal.update(&mut update),
                "backlog closed before publication"
            );
            if modal.screen.is_none() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        let publication = begin.elapsed();
        let screen = modal.screen.as_ref().unwrap();
        assert_eq!(
            (
                screen.history().entries.len(),
                screen.model.selected.unwrap()
            ),
            expected.unwrap()
        );
        assert_eq!(screen.shell.progress(), 0.1);
        assert!(!screen.shell.is_open());
        for _ in 0..8 {
            assert!(!modal.update(&mut update));
        }
        assert!(!modal.screen.as_ref().unwrap().shell.is_open());
        assert!(!modal.update(&mut update));
        assert!(modal.screen.as_ref().unwrap().shell.is_open());
        assert!(modal.screen.as_ref().unwrap().cached_row_count() > 0);
        eprintln!(
            "Backlog modal pass{pass}: publication={publication:?}; native ten-tick slide and visible glyph preparation pass"
        );
    }
}

impl BacklogScreen {
    pub fn history(&self) -> &Backlog {
        &self.history
    }
    pub fn textures(&self) -> &BacklogTextures {
        &self.textures
    }
    #[cfg(test)]
    pub fn voice_flash(&self, index: usize) -> Option<f32> {
        self.voice_flashes.get(&index).map(Tweener::value)
    }
    #[cfg(test)]
    pub fn has_cached_row(&self, index: usize) -> bool {
        self.cached.contains_key(&index)
    }
    #[cfg(test)]
    pub fn cached_row_count(&self) -> usize {
        self.cached.len()
    }
    pub fn new(
        history: Backlog,
        fonts: AdvFonts,
        textures: Arc<BacklogTextures>,
        system: Arc<crate::adv::characters::view::CharacterTextures>,
        current: usize,
        outline_mode: u8,
    ) -> Result<Self> {
        let mut sizes = Vec::with_capacity(history.entries.len());
        for entry in &history.entries {
            let (text, color) = display_text(&entry.item).ok_or_else(|| {
                anyhow!(
                    "Unresolved native QUIZ backlog display at saved position {}",
                    entry.save_position
                )
            })?;
            let (_, _, size) = layout_display_text(
                &text,
                color,
                fonts.medium_font.as_ref(),
                fonts.bold_font.as_ref(),
                localize_item(&entry.item),
            );
            sizes.push((size.y, !voice_paths(&entry.item).is_empty()));
        }
        Ok(Self {
            model: BacklogModel::new(sizes, current),
            shell: BacklogShell::default(),
            history,
            fonts,
            textures,
            outline_mode,
            system,
            cached: BTreeMap::new(),
            voice_flashes: BTreeMap::new(),
            confirmation_feedback: Default::default(),
        })
    }

    pub fn update(&mut self, delta: Ticks) {
        self.shell.update(delta);
        self.model.update(delta);
        self.confirmation_feedback.update(delta);
        for flash in self.voice_flashes.values_mut() {
            flash.update(delta);
        }
    }

    pub fn navigate(&mut self, delta: i32) -> bool {
        self.shell.is_open() && self.model.navigate(delta)
    }

    pub fn navigate_checkpoint(&mut self, forward: bool) -> bool {
        if !self.shell.is_open() || !self.model.accepts_keyboard() {
            return false;
        }
        let Some(selected) = self.model.selected else {
            return false;
        };
        let Some(target) =
            super::model::checkpoint_target(&self.history, selected, forward)
        else {
            return false;
        };
        self.model.select(target);
        true
    }

    /// Stage the boundary before changing feedback. The live modal retains
    /// browser/pause ownership while waiting for this row's cached flash.
    pub fn prepare_rollback(
        &mut self,
        source: &shin_core::format::save::GameData,
    ) -> Result<(usize, super::rollback::BacklogRollback)> {
        anyhow::ensure!(self.shell.is_open(), "Backlog is not open");
        let index = self
            .model
            .selected
            .ok_or_else(|| anyhow!("No backlog row selected"))?;
        let rollback =
            super::rollback::BacklogRollback::from_game_data(&self.history, index, source)?;
        self.confirmation_feedback.request(index);
        Ok((index, rollback))
    }
    pub fn confirmation_feedback_finished(&self, index: usize) -> bool {
        self.confirmation_feedback.is_idle(index)
    }

    pub fn current_confirmation_feedback_finished(&self) -> bool {
        self.confirmation_feedback
            .selected_is_idle(self.model.selected)
    }

    /// Native a6e80 ignores nonmessage/voiceless rows and retains full @v paths.
    /// Returns paths to the eventual replay owner; feedback does not play audio.
    pub fn request_voice(&mut self) -> Option<Vec<String>> {
        if !self.shell.is_open() {
            return None;
        }
        let index = self.model.selected?;
        let paths = voice_paths(&self.history.entries[index].item);
        if paths.is_empty() {
            return None;
        }
        if let Some(flash) = self.voice_flashes.get_mut(&index) {
            let current = flash.value();
            flash.fast_forward_to(current);
            flash.enqueue(
                0.7,
                Tween::linear(Ticks::from_f32((0.7 - current).abs() / 0.2)),
            );
            flash.enqueue(0.0, Tween::linear(Ticks::from_f32(0.7 / 0.2)));
        }
        Some(paths)
    }

    pub fn hold_voice(&mut self) {
        if let Some(index) = self.model.selected {
            if let Some(flash) = self.voice_flashes.get_mut(&index) {
                let current = flash.value();
                flash.fast_forward_to(current);
                flash.enqueue(
                    0.7,
                    Tween::linear(Ticks::from_f32((0.7 - current).abs() / 0.2)),
                );
            }
        }
    }
    pub fn cancel_voice(&mut self, index: usize) {
        if let Some(flash) = self.voice_flashes.get_mut(&index) {
            let current = flash.value();
            flash.fast_forward_to(current);
            flash.enqueue(0.0, Tween::linear(Ticks::from_f32(current.abs() / 0.2)));
        }
    }

    /// Replay is owned by the browser's modal task, independently of story audio.
    pub fn replay_selected(
        &mut self,
        replay: &mut super::voice::BacklogVoiceReplay,
    ) -> bool {
        if self.request_voice().is_none() {
            return false;
        }
        let super::BacklogItem::Message { text, .. } =
            &self.history.entries[self.model.selected.unwrap()].item
        else {
            unreachable!()
        };
        replay.play_message(text);
        true
    }

    pub fn axis(&mut self, amount: f32) {
        if self.shell.is_open() {
            self.model.axis(amount);
        }
    }

    /// Native rendering searches the viewport's row range and caches one
    /// preceding row too. Glyph textures are allocated only for that range;
    /// discarded rows keep their inexpensive layout/route metadata.
    pub fn prepare(&mut self, context: &PreRenderContext) {
        let top = self.model.scroll();
        let bottom = top + VIEW_HEIGHT;
        let start = self
            .model
            .rows
            .partition_point(|row| row.top < top)
            .saturating_sub(1);
        let end = self.model.rows.partition_point(|row| row.top <= bottom);
        self.cached.retain(|&i, _| (start..end).contains(&i));
        self.confirmation_feedback.retain(start..end);
        self.voice_flashes.retain(|&i, _| (start..end).contains(&i));
        for index in start..end {
            self.confirmation_feedback.cache(index);
            if !voice_paths(&self.history.entries[index].item).is_empty() {
                self.voice_flashes
                    .entry(index)
                    .or_insert_with(|| Tweener::new(0.0));
            }
            self.cached.entry(index).or_insert_with(|| {
                BacklogText::load(
                    context.device,
                    context.queue,
                    &self.fonts,
                    &self.history.entries[index].item,
                    self.outline_mode,
                )
                .expect("Constructor validated authored display text")
            });
        }
    }

    pub fn render(&self, pass: &mut RenderPass) {
        self.render_with_menu(pass, false);
    }
    #[cfg(test)]
    pub fn render_without_scrollbar(&self, pass: &mut RenderPass) {
        self.render_components(pass, false, false);
    }
    pub fn render_with_menu(&self, pass: &mut RenderPass, menu_open: bool) {
        self.render_components(pass, menu_open, true);
    }
    fn render_components(&self, pass: &mut RenderPass, menu_open: bool, with_scrollbar: bool) {
        self.shell.render_background(&self.textures, pass);
        for (&index, text) in &self.cached {
            super::decoration::render_row(
                pass,
                &self.textures,
                self.model.rows[index],
                self.model.scroll(),
                self.shell.progress(),
                self.model.selected == Some(index),
            );
            text.render(
                pass,
                self.model.rows[index].top,
                self.model.scroll(),
                self.shell.progress(),
            );
            if let Some(flash) = self.voice_flashes.get(&index) {
                super::decoration::render_voice(
                    pass,
                    &self.textures,
                    self.model.rows[index],
                    self.model.scroll(),
                    self.shell.progress(),
                    flash.value(),
                );
            }
            super::decoration::render_confirmation_flash(
                pass,
                self.model.rows[index],
                self.model.scroll(),
                self.shell.progress(),
                self.confirmation_feedback.value(index),
            );
        }
        if with_scrollbar {
            crate::adv::scrollbar::render(
                &self.system,
                pass,
                crate::render::top_left_projection_matrix()
                    * glam::Mat4::from_translation(glam::vec3(
                        0.0,
                        1920.0 * (1.0 - self.shell.progress()),
                        0.0,
                    )),
                crate::adv::scrollbar::slices(
                    self.model.content_height(),
                    self.model.scroll(),
                    1788.0,
                    188.0,
                    VIEW_HEIGHT,
                ),
            );
        }
        self.shell.render_foreground(&self.textures, pass);
        let replay_enabled = self
            .model
            .selected
            .is_some_and(|i| !voice_paths(&self.history.entries[i].item).is_empty());
        super::help::render(
            pass,
            &self.textures,
            self.shell.progress(),
            replay_enabled,
            menu_open,
        );
    }
}
