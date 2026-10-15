//! Scene owner for native ScenarioSelect dc970 and its input/task lifecycle.
use super::{
    scenario_select::ScenarioSelectSetup,
    scenario_select_details::ScenarioDetails,
    scenario_select_input::ScenarioSelectInput,
    scenario_select_opening::{OpeningEvent, ScenarioSelectOpening},
    scenario_select_selection::{ScenarioSelection, SelectionEvent},
    scenario_select_view as view,
};
use crate::{
    adv::{SaveMenuDialog, input_area::PointerFrame},
    app::AppAction,
    asset::system::AssetServer,
    audio::{BgmPlayer, SystemSoundPlayer},
    render::{PreRenderContext, top_left_projection_matrix},
};
use enum_map::EnumMap;
use shin_core::{
    format::{
        save::{SaveData, Settings},
        scenario::instruction_elements::FromNumber,
        scenario::{Scenario, info::BgmId},
    },
    time::{Ticks, Tween},
    vm::command::types::Volume,
};
use shin_input::ActionState;
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};
use std::sync::Arc;

pub(super) const SCENARIO_UNLOCK_PROMPT: &str = "本編シナリオの全てと、追加シナリオの一部をアンロックします。よろしいですか？@r@z75.一度アンロックしたら元の状態に戻すことはできません。ご注意ください。";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScenarioDestination {
    Title,
    Episode(u8),
    UnlockScenarios,
}
pub(crate) struct ScenarioSelectScene {
    opening: ScenarioSelectOpening,
    selection: ScenarioSelection,
    input: ScenarioSelectInput,
    details: ScenarioDetails,
    previous_screen: RenderTexture,
    dialog: Option<SaveMenuDialog>,
    unlock_dialog: bool,
    unlock_available: bool,
    back_pending: bool,
    destination: Option<ScenarioDestination>,
    sounds: Vec<&'static str>,
    music: Vec<OpeningEvent>,
    launch_music_stop: bool,
    extent: (u16, u16),
}
impl ScenarioSelectScene {
    /// The host supplies the retained complete previous screen, before retiring
    /// the old scene. Global players and save data stay owned by the host.
    pub fn new(
        server: Arc<AssetServer>,
        save: &SaveData,
        previous_screen: RenderTexture,
        ctx: &PreRenderContext,
        extent: (u16, u16),
    ) -> Self {
        let setup = ScenarioSelectSetup::from_save(save);
        let mut scene = Self {
            opening: ScenarioSelectOpening::new_for_setup(server, &setup),
            selection: ScenarioSelection::new_active(setup),
            input: ScenarioSelectInput::new(),
            details: ScenarioDetails::new(
                ctx,
                winit::dpi::PhysicalSize::new(extent.0 as u32, extent.1 as u32),
            ),
            previous_screen,
            dialog: None,
            unlock_dialog: false,
            unlock_available: save.persist_data.get(25) == 0,
            back_pending: false,
            destination: None,
            sounds: Vec::new(),
            music: Vec::new(),
            launch_music_stop: false,
            extent,
        };
        scene.input.resize(&mut scene.selection, extent);
        scene
    }
    pub fn is_loaded(&self) -> bool {
        self.opening.is_loaded()
    }
    pub fn accepts_input(&self) -> bool {
        self.opening.clock.accepts_input()
            && self.selection.accepts_input()
            && self.dialog.is_none()
    }
    #[cfg(test)]
    pub(crate) fn reference_dialog_button_center(&self, index: usize) -> glam::Vec2 {
        self.dialog.as_ref().unwrap().reference_button_center(index)
    }
    #[cfg(test)]
    pub(crate) fn reference_dialog_origin(&self) -> glam::Vec2 {
        self.dialog.as_ref().unwrap().reference_origin()
    }
    #[cfg(test)]
    pub(crate) fn reference_has_dialog(&self) -> bool {
        self.dialog.is_some()
    }
    pub fn resize(&mut self, extent: (u16, u16), held: bool) {
        self.input.resize(&mut self.selection, extent);
        self.extent = extent;
        if let Some(dialog) = &mut self.dialog {
            dialog.set_pointer_extent(extent, held);
        }
    }
    pub fn update_frame(
        &mut self,
        delta: Ticks,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        canvas_pointer: glam::Vec2,
        elapsed_seconds: f64,
        server: &AssetServer,
        ctx: &mut PreRenderContext,
    ) -> anyhow::Result<()> {
        if let Err(error) = self.opening.update_frame(delta) {
            tracing::warn!(%error, "Returning to title after Start menu load failed");
            self.destination = Some(ScenarioDestination::Title);
            return Ok(());
        }
        // The updated picker atlas has five 48px help rows; the original has
        // four. Keep original artwork/input behavior when using the base assets.
        let updated = self
            .opening
            .atlas()
            .is_some_and(|atlas| atlas.get("help_snrsel").size().height == 240);
        self.selection
            .set_unlock_available(updated && self.unlock_available);
        self.music.extend(self.opening.clock.take_events());
        // Native back task checks idle before any clocks advance this frame.
        if self.back_pending && self.selection.close_flash_idle() {
            self.back_pending = false;
            self.destination = Some(ScenarioDestination::Title);
        }
        let had_dialog = self.dialog.is_some();
        if let Some(dialog) = &mut self.dialog {
            dialog.pointer_state(
                canvas_pointer,
                state[AppAction::PointerClick].is_held,
                elapsed_seconds,
            );
            dialog.keyboard(state);
            let mut done = false;
            for response in dialog.update(delta) {
                if response.final_flag {
                    if self.unlock_dialog {
                        self.selection.answer_unlock_dialog(response.button == 0);
                    } else {
                        self.selection.answer_dialog(response.button == 0);
                    }
                    done = true;
                }
            }
            self.sounds.extend(dialog.take_sounds());
            if done {
                self.dialog = None;
                self.unlock_dialog = false;
            }
        }
        if self.opening.clock.accepts_input() && !had_dialog {
            self.input
                .update_frame(&mut self.selection, delta, frame, state);
        } else {
            self.selection.advance_clocks(delta);
        }
        for event in self.selection.take_events() {
            match event {
                SelectionEvent::CaptureDetails => self.details.capture_previous(ctx),
                SelectionEvent::MoveSound => self.sounds.push("move"),
                SelectionEvent::ConfirmSound => self.sounds.push("decide"),
                SelectionEvent::DeniedSound => self.sounds.push("error"),
                SelectionEvent::BackSound => self.sounds.push("cancel"),
                SelectionEvent::BeginBack => self.back_pending = true,
                SelectionEvent::ConfirmDialog => {
                    let dialog = SaveMenuDialog::load_prompt(
                        server,
                        "このシナリオを開始します。@rよろしいですか？",
                        state[AppAction::PointerClick].is_held,
                    );
                    match dialog {
                        Ok(mut dialog) => {
                            dialog.set_pointer_extent(
                                self.extent,
                                state[AppAction::PointerClick].is_held,
                            );
                            self.dialog = Some(dialog);
                        }
                        Err(error) => {
                            tracing::warn!(%error, "Keeping Start menu after scenario confirmation load failed");
                            self.selection.answer_dialog(false);
                            self.input.resize(&mut self.selection, self.extent);
                        }
                    }
                }
                SelectionEvent::UnlockDialog => {
                    let dialog = SaveMenuDialog::load_prompt(
                        server,
                        SCENARIO_UNLOCK_PROMPT,
                        state[AppAction::PointerClick].is_held,
                    );
                    match dialog {
                        Ok(mut dialog) => {
                            dialog.set_pointer_extent(
                                self.extent,
                                state[AppAction::PointerClick].is_held,
                            );
                            self.unlock_dialog = true;
                            self.dialog = Some(dialog);
                        }
                        Err(error) => {
                            tracing::warn!(%error, "Keeping Start menu after unlock confirmation load failed");
                            self.selection.answer_unlock_dialog(false);
                            self.input.resize(&mut self.selection, self.extent);
                        }
                    }
                }
                SelectionEvent::UnlockScenarios => {
                    self.destination = Some(ScenarioDestination::UnlockScenarios);
                }
                SelectionEvent::LaunchScene(id) => {
                    self.sounds.push("dooropen");
                    self.launch_music_stop = true;
                    self.destination = Some(ScenarioDestination::Episode(id));
                }
                SelectionEvent::InvalidNativeTabIndex(index) => anyhow::bail!(
                    "Native ScenarioSelect tab ordinal {index} exceeds {} visible groups",
                    self.selection.setup().groups.len()
                ),
            }
        }
        if let (Some(atlas), Some(system)) = (self.opening.atlas(), self.opening.system()) {
            let (group, entry) = self.selection.selected();
            self.details.pre_render(
                atlas,
                &self.selection.setup().groups[group].entries[entry],
                self.selection.details_fade(),
                ctx,
            );
        }
        if let Some(dialog) = &mut self.dialog {
            dialog.prepare(ctx);
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn reference_groups_and_selection(&self) -> (Vec<u8>, (usize, usize)) {
        (
            self.selection.setup().groups.iter().map(|g| g.id).collect(),
            self.selection.selected(),
        )
    }
    pub fn take_destination(&mut self) -> Option<ScenarioDestination> {
        self.destination.take()
    }
    pub fn update_audio(
        &mut self,
        server: &Arc<AssetServer>,
        scenario: &Scenario,
        settings: &Settings,
        bgm: &mut BgmPlayer,
        system: &mut SystemSoundPlayer,
        delta: Ticks,
    ) {
        bgm.set_settings_volume(settings.v0_bgmvol);
        system.set_settings_volume(settings.v3_sysvol);
        for event in self.music.drain(..) {
            match event {
                OpeningEvent::StopMusic => bgm.stop(Tween::linear(Ticks::from_f32(60.0))),
                OpeningEvent::StartMusic => {
                    let id = BgmId::from_number(5);
                    // Native33800 ->33cd0 passes loop count0 (unlimited),
                    // while w3 zero is the gain fade duration.
                    bgm.request(
                        server.clone(),
                        scenario.info_tables().bgm_info(id).path(),
                        id,
                        true,
                        Volume::default(),
                        Tween::IMMEDIATE,
                    );
                }
            }
        }
        if std::mem::take(&mut self.launch_music_stop) {
            bgm.stop(Tween::linear(Ticks::from_f32(120.0)));
        }
        for sound in self.sounds.drain(..) {
            system.play(server, sound, Volume::default());
        }
        bgm.update_voice_focus(delta, false, false);
        bgm.update();
        system.update();
    }
    pub fn render(&self, pass: &mut RenderPass) {
        let transform = top_left_projection_matrix();
        if let (Some(atlas), Some(system)) = (self.opening.atlas(), self.opening.system()) {
            let (group, entry) = self.selection.selected();
            let setup = self.selection.setup();
            view::render_background(atlas, pass, transform);
            view::render_footer(
                atlas,
                setup.groups.len(),
                setup.groups[group].entries[entry].state,
                self.unlock_available,
                pass,
                transform,
            );
            view::render_close(atlas, self.selection.close_flash(), pass, transform);
            self.details.render(pass, transform);
            view::render_chapters(
                &setup.groups[group],
                self.selection.scroll(),
                &self.selection.chapter_feedback(),
                atlas,
                pass,
                transform,
            );
            view::render_chapter_edges(self.details.source(), pass, transform);
            view::render_tabs(
                setup,
                &self.selection.tab_focus(),
                &self.selection.tab_flash(),
                atlas,
                pass,
                transform,
            );
            view::render_scrollbar(
                &setup.groups[group],
                self.selection.scroll(),
                system,
                pass,
                transform,
            );
        }
        self.opening
            .render_curtain(self.previous_screen.as_texture_source(), pass);
        if let Some(dialog) = &self.dialog {
            dialog.render(pass);
        }
    }
}

#[cfg(test)]
#[path = "scenario_select_scene_reference.rs"]
mod reference;
