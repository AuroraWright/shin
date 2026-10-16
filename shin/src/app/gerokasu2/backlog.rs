//! Retained MessageBrowser, private children and asynchronous ADV handoff.
mod chapter;
mod contact;
pub(super) mod measurement;
mod parent;
mod resources;
mod text;
mod voice;
use super::{AppAction, Viewport};
use crate::{
    adv::{
        gerokasu2::{
            backlog::{self as model, lifecycle, view},
            config::dialog::GerDialog,
        },
        input_area::{InputArea, PointerSampler},
    },
    asset::system::AssetServer,
    audio::SystemSoundPlayer,
    render::PreRenderContext,
};
use anyhow::{Context, Result};
use enum_map::EnumMap;
use glam::Vec2;
pub(super) use resources::{Options, Resources};
use shin_audio::AudioManager;
use shin_core::{
    format::{
        save::gerokasu2::{GameData, SaveData},
        scenario::gerokasu2::Gerokasu2Scenario,
    },
    time::Ticks,
    vm::command::types::Volume,
};
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct Preparation {
    task: Option<shin_tasks::AsyncTask<Result<Option<Host>>>>,
    cancelled: Option<Arc<AtomicBool>>,
}
impl Drop for Preparation {
    fn drop(&mut self) {
        if let Some(cancelled) = self.cancelled.as_ref() {
            cancelled.store(true, Ordering::Release);
        }
        if let Some(task) = self.task.take() {
            #[cfg(not(target_arch = "wasm32"))]
            task.cancel_and_retire();
            #[cfg(target_arch = "wasm32")]
            drop(task);
        }
    }
}
pub(super) struct Slot {
    preparation: Option<Preparation>,
    host: Option<Host>,
    retire: bool,
}
impl Slot {
    #[cfg(test)]
    pub(super) fn reference(&self) -> Option<Reference> {
        let h = self.host.as_ref()?;
        Some(Reference {
            phase: h.lifecycle.phase(),
            selected: h.controls.selected(),
            scroll: h.controls.scroll(),
            scroll_busy: h.controls.scroll_busy(),
            measured: h.resources.measurement_reference(),
            allowed: h.controls.allowed(),
            rows: h.resources.rows(&[]),
            frozen: h.resources.history.jump_source.clone(),
            chapter: h.chapter.as_ref().map(|c| c.controller().phase()),
            question: h.dialog.is_some(),
            voice: h.voice.playing(),
            voice_micros: h.voice.reference_position(),
            jump: h.jump.clone(),
        })
    }
    fn pending(
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        audio: Arc<AudioManager>,
        source: GameData,
        data: SaveData,
        options: Options,
    ) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let token = cancelled.clone();
        Self {
            preparation: Some(Preparation {
                task: Some(shin_tasks::async_io::spawn_loader(Host::load(
                    assets, scenario, audio, source, data, token, options,
                ))),
                cancelled: Some(cancelled),
            }),
            host: None,
            retire: false,
        }
    }
    fn live(host: Host) -> Self {
        Self {
            preparation: None,
            host: Some(host),
            retire: false,
        }
    }
    fn poll(&mut self) -> Option<Result<Option<Host>>> {
        let preparation = self.preparation.as_mut()?;
        let task = preparation.task.as_mut()?;
        let retirement = task.retirement_handle();
        let mut result = task.poll_naive()?;
        preparation.task = None;
        if let Ok(Some(host)) = result.as_mut() {
            // The live resource owner now owns cancellation. Dropping this
            // completed preparation must not stop measurement at row100.
            preparation.cancelled = None;
            host.retirement = retirement;
        } else {
            // Errors/empty history are caller-side retirement boundaries too.
            if let Some(cancelled) = preparation.cancelled.as_ref() {
                cancelled.store(true, Ordering::Release);
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(retirement) = retirement {
                retirement.retire(());
            }
        }
        self.preparation = None;
        Some(result)
    }
    pub fn cancel_input(&mut self) {
        if let Some(host) = self.host.as_mut() {
            host.cancel_input();
        }
    }
    pub fn set_scroll_axis(&mut self, value: f32) {
        if let Some(host) = self.host.as_mut() {
            host.axis = if value.is_finite() { value } else { 0. };
        }
    }
    pub fn prepare(&mut self, pre: &mut PreRenderContext) -> Result<()> {
        if let Some(host) = self.host.as_mut() {
            host.prepare(pre)?;
        }
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        if let Some(host) = self.host.as_ref() {
            host.render(pass)?;
        }
        Ok(())
    }
    pub fn voice_focus(&self) -> Option<bool> {
        self.host.as_ref().map(|h| h.voice.playing())
    }
    pub fn recover_failed_jump(&mut self) {
        if let Some(host) = self.host.as_mut() {
            host.lifecycle.recover_failed_jump();
            host.resources.resume_measurement();
            host.jump = None;
            host.dialog = None;
            host.chapter = None;
            host.cancel_input();
        }
        self.retire = false;
    }
}
#[cfg(test)]
pub(super) struct Reference {
    pub phase: lifecycle::Phase,
    pub selected: usize,
    pub scroll: f32,
    pub scroll_busy: bool,
    pub measured: (usize, bool, bool),
    pub allowed: std::ops::Range<usize>,
    pub rows: Vec<view::Row>,
    pub frozen: GameData,
    pub chapter: Option<model::chapter::Phase>,
    pub question: bool,
    pub voice: bool,
    pub voice_micros: u64,
    pub jump: Option<GameData>,
}
enum Event {
    LeaveModal,
    Finished,
    Jump(GameData),
}
struct Host {
    resources: Resources,
    controls: model::Controller,
    lifecycle: lifecycle::Controller,
    assets: Arc<AssetServer>,
    voice: voice::Replay,
    sounds: SystemSoundPlayer,
    chapter: Option<chapter::ChapterHost>,
    dialog: Option<GerDialog>,
    jump: Option<GameData>,
    sampler: PointerSampler,
    area: InputArea,
    pointer: view::pointer::Pointer,
    armed: bool,
    axis: f32,
    retirement: Option<shin_tasks::LoaderRetirement>,
}
impl Drop for Host {
    fn drop(&mut self) {
        if let Some(retirement) = self.retirement.take() {
            // Assigned only by the caller-side publication poll. A pending
            // future instead leaves draining to Preparation's loader scope.
            self.resources.retire_measurement();
            #[cfg(not(target_arch = "wasm32"))]
            retirement.retire(());
        }
    }
}
impl Host {
    async fn load(
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        audio: Arc<AudioManager>,
        source: GameData,
        data: SaveData,
        cancelled: Arc<AtomicBool>,
        options: Options,
    ) -> Result<Option<Self>> {
        let settings = data.settings.clone();
        let Some(mut resources) = Resources::load_with_options(
            assets.clone(),
            scenario.clone(),
            source,
            data,
            cancelled,
            options,
        )
        .await?
        else {
            return Ok(None);
        };
        let controls = resources.controller()?;
        resources.prepare(controls.scroll())?;
        let mut lifecycle = lifecycle::Controller::new(resources.history.entries.len());
        lifecycle.sync_cached(resources.cached_rows());
        let voice = voice::Replay::new(audio.clone(), assets.clone(), &scenario, &settings);
        let mut sounds = SystemSoundPlayer::new(audio);
        sounds.set_settings_volume(settings.v3_sysvol);
        Ok(Some(Self {
            resources,
            controls,
            lifecycle,
            assets,
            voice,
            sounds,
            chapter: None,
            dialog: None,
            jump: None,
            sampler: Default::default(),
            area: InputArea::canvas(),
            pointer: Default::default(),
            armed: false,
            axis: 0.,
            retirement: None,
        }))
    }
    fn cue(&mut self, name: &str) {
        self.sounds.play(&self.assets, name, Volume(1.));
    }
    fn cancel_input(&mut self) {
        self.sampler.cancel_contact(true);
        self.armed = false;
        let mut events = vec![];
        self.area.disable(&mut contact::Contact::new(
            &mut self.pointer,
            &self.controls,
            &self.resources,
            &mut events,
        ));
        self.pointer.cancel();
        self.pointer.stop_movement();
        for event in events {
            self.pointer_event(event);
        }
        if let Some(child) = self.chapter.as_mut() {
            let events = child.cancel_input();
            self.chapter_events(events);
        }
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.cancel_input();
        }
    }
    fn lifecycle_events(
        &mut self,
        events: Vec<lifecycle::Event>,
        held: bool,
        out: &mut Vec<Event>,
    ) -> Result<()> {
        for event in events {
            match event {
                lifecycle::Event::ShowJumpQuestion { .. } => {
                    match GerDialog::load(
                        &self.assets,
                        model::JUMP_PROMPT,
                        &["はい", "いいえ"],
                        1,
                        true,
                        held,
                    ) {
                        Ok(dialog) => {
                            self.dialog = Some(dialog);
                            self.cancel_input();
                        }
                        Err(error) => {
                            tracing::error!(%error,"Ger backlog question failed; retaining browser");
                            self.lifecycle.answer(true, 1);
                            self.cue("error");
                            self.cancel_input();
                        }
                    }
                }
                lifecycle::Event::JumpAccepted { row } => {
                    self.jump = Some(self.resources.jump_data(row)?);
                }
                lifecycle::Event::ParentReturn { reason: 0 } => out.push(Event::LeaveModal),
                lifecycle::Event::PublishJump { .. } => out.push(Event::Jump(
                    self.jump
                        .take()
                        .context("Ger backlog jump lost its frozen memory payload")?,
                )),
                lifecycle::Event::Done => out.push(Event::Finished),
                lifecycle::Event::CancelHistory => self.resources.cancel_measurement(),
                lifecycle::Event::MainInstalled
                | lifecycle::Event::ParentReturn { .. }
                | lifecycle::Event::Retire => {}
            }
        }
        Ok(())
    }
    fn replay_voice(&mut self, row: usize) {
        if !self.controls.voice_available(row) {
            return;
        }
        self.voice
            .start(self.resources.history.entries[row].voices());
        self.lifecycle.pulse_portrait(row);
    }
    fn quick_load(&mut self, row: usize) {
        self.cue("decide");
        self.lifecycle
            .request_quick_load(row, self.resources.cached_rows().contains(&row));
    }
    fn open_chapter(&mut self) {
        self.cue("decide");
        match chapter::ChapterHost::load(
            self.assets.clone(),
            &self.resources.history,
            self.controls.selected(),
            self.resources.geometry.selected,
        ) {
            Ok(child) => {
                self.chapter = Some(child);
                self.cancel_input();
            }
            Err(error) => {
                tracing::error!(%error,"Ger ChapterSelector preparation failed");
                self.cue("error");
            }
        }
    }
    fn pointer_event(&mut self, event: view::pointer::Event) {
        use view::pointer::Event as E;
        match event {
            E::Select {
                row,
                scroll,
                immediate,
            } => {
                self.controls.select_external(row, scroll, immediate);
            }
            E::Close => {
                self.cue("decide");
                self.lifecycle.request_close();
            }
            E::ReplayVoice { row } => self.replay_voice(row),
            E::QuickLoad { row } => self.quick_load(row),
            E::ChapterSelector => self.open_chapter(),
            E::CloseFlash { value } => self.lifecycle.set_close_flash(value),
            E::VoiceFlash { row, value } => self.lifecycle.set_portrait_flash(row, value),
        }
    }
    fn chapter_events(&mut self, events: Vec<model::chapter::Event>) {
        use model::chapter::Event as E;
        for event in events {
            match event {
                E::ParentSelect { row } => {
                    self.controls.select_and_reveal(row);
                }
                E::Move => self.cue("move"),
                E::Back => self.cue("cancel"),
                E::Decide => self.cue("decide"),
                E::ReplayVoice { row } => self.replay_voice(row),
                E::QuickLoad { row } => self.quick_load(row),
                E::CloseBrowser => self.lifecycle.request_close(),
                E::CloseFlash { held } => {
                    self.lifecycle.set_close_flash(if held { 0.7 } else { 0. })
                }
                E::Jump { .. } | E::DisableInput => {}
            }
        }
    }
    fn update(
        &mut self,
        delta: f32,
        input: &EnumMap<AppAction, ActionState>,
        point: Vec2,
        viewport: Viewport,
        seconds: f64,
    ) -> Result<Vec<Event>> {
        self.resources.refresh_geometry(&mut self.controls)?;
        let mut out = vec![];
        let held = input[AppAction::PointerClick].is_held;
        let events = self.lifecycle.begin_frame(delta);
        let question_created = events
            .iter()
            .any(|e| matches!(e, lifecycle::Event::ShowJumpQuestion { .. }));
        self.lifecycle_events(events, held, &mut out)?;
        //108 sweeps a finished child before building this frame's snapshot.
        // Its done-setting frame still owns the footer/render boundary; Main
        // regains permission on this next sweep without another release gate.
        if self
            .chapter
            .as_ref()
            .is_some_and(|c| c.controller().phase() == model::chapter::Phase::Finished)
        {
            self.chapter = None;
        }
        self.sounds.update();
        if [
            AppAction::Enter,
            AppAction::Cancel,
            AppAction::Up,
            AppAction::AnyDown,
            AppAction::Left,
            AppAction::Right,
            AppAction::PageUp,
            AppAction::PageDown,
            AppAction::Backlog,
            AppAction::SystemMenu,
            AppAction::PointerClick,
        ]
        .into_iter()
        .all(|a| !input[a].is_held)
        {
            self.armed = true;
        }
        let point = viewport.map(point).unwrap_or(Vec2::splat(-1.));
        self.sampler.sample(point, held, seconds);
        let dialog_owned = self.dialog.is_some();
        if let Some(dialog) = self.dialog.as_mut()
            && !question_created
        {
            if self.armed {
                dialog.pointer_state(point, held, seconds);
                dialog.keyboard(input);
            }
            let responses = dialog.update(Ticks::from_f32(delta));
            for cue in dialog.take_sounds() {
                self.sounds.play(&self.assets, cue, Volume(1.));
            }
            let done = responses.iter().any(|r| r.final_flag);
            for response in responses {
                if let Some(child) = self.chapter.as_mut() {
                    child
                        .controller_mut()
                        .jump_response(response.final_flag, response.button);
                }
                let events = self.lifecycle.answer(response.final_flag, response.button);
                self.lifecycle_events(events, held, &mut out)?;
            }
            if done {
                self.dialog = None;
                self.cancel_input();
            }
        }
        if self.chapter.is_some() {
            let (pressed, repeated) = buttons(input);
            let child = self.chapter.as_mut().unwrap();
            let events = child.update(
                delta,
                model::chapter::Input {
                    permission: self.armed && self.lifecycle.accepts_main() && !dialog_owned,
                    captured: false,
                    pressed,
                    repeated,
                    axis: if dialog_owned { 0. } else { self.axis },
                    first_row: self.controls.allowed().start,
                    last_row: self.controls.allowed().end,
                    parent_close_busy: self.lifecycle.close_busy(),
                },
                point,
                held,
                seconds,
            );
            self.chapter_events(events);
        } else if !dialog_owned && self.armed && self.lifecycle.accepts_main() {
            //82C38 dispatches contacts before82C40 checks the scroll motion.
            // A held row starts a reveal without surrendering its release.
            // Controller::input applies the busy gate to keyboard input only.
            let mut events = vec![];
            let captured = self.area.update_with_mouse_release(
                &mut contact::Contact::new(
                    &mut self.pointer,
                    &self.controls,
                    &self.resources,
                    &mut events,
                ),
                &self.sampler.frame(),
                self.sampler.mouse_release(),
            );
            for event in events {
                self.pointer_event(event);
            }
            let (pressed, repeated) = buttons(input);
            let events = self.controls.input(model::Input {
                permission: self.lifecycle.accepts_main() && self.chapter.is_none(),
                captured,
                pressed,
                repeated,
                axis: self.axis,
            });
            for event in events {
                match event {
                    model::Event::Move => {
                        self.pointer.stop_movement();
                        self.cue("move");
                    }
                    model::Event::Close => {
                        self.pointer.stop_movement();
                        self.cue("cancel");
                        self.voice.stop();
                        self.lifecycle.request_close();
                    }
                    model::Event::ChapterSelector => {
                        self.pointer.stop_movement();
                        self.open_chapter();
                    }
                    model::Event::ReplayVoice { row } => {
                        self.pointer.stop_movement();
                        self.replay_voice(row);
                    }
                    model::Event::QuickLoad { row } => {
                        self.pointer.stop_movement();
                        self.quick_load(row);
                    }
                }
            }
        } else {
            let mut events = vec![];
            self.area.disable(&mut contact::Contact::new(
                &mut self.pointer,
                &self.controls,
                &self.resources,
                &mut events,
            ));
            for event in events {
                self.pointer_event(event);
            }
        }
        let rows = self.resources.rows(&[]);
        let events = self.pointer.advance(
            delta,
            &view::pointer::Context {
                rows: &rows,
                selected: self.controls.selected(),
                scroll: self.controls.scroll(),
                minimum: self.resources.geometry.minimum,
                bottom: self.resources.geometry.bottom,
            },
        );
        for event in events {
            self.pointer_event(event);
        }
        self.controls.advance_scroll();
        self.voice.update();
        self.lifecycle.end_frame(delta);
        Ok(out)
    }
    fn prepare(&mut self, pre: &mut PreRenderContext) -> Result<()> {
        self.resources.refresh_geometry(&mut self.controls)?;
        self.resources.prepare(self.controls.scroll())?;
        self.lifecycle.sync_cached(self.resources.cached_rows());
        if let Some(child) = self.chapter.as_mut() {
            child.set_parent_bounds(self.controls.allowed().start, self.controls.allowed().end);
            child.prepare()?;
        }
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.prepare(pre)?;
        }
        Ok(())
    }
    fn render(&self, pass: &mut RenderPass) -> Result<()> {
        let view = view::Presentation {
            progress: self.lifecycle.progress(),
            scroll: self.controls.scroll(),
            selected: self.controls.selected(),
            chapter_open: self.chapter.is_some(),
            swapped: false,
            close_flash: self.lifecycle.close_flash(),
            game: self.resources.game(),
        };
        let flashes = (0..self.resources.history.entries.len())
            .map(|i| self.lifecycle.portrait_flash(i))
            .collect::<Vec<_>>();
        self.resources.render_rows(pass, &view, &flashes)?;
        if let Some(child) = self.chapter.as_ref() {
            child.render(pass)?;
        }
        self.resources.render_chrome(pass, &view);
        if let Some(dialog) = self.dialog.as_ref() {
            dialog.render(pass)?;
        }
        Ok(())
    }
}
fn buttons(input: &EnumMap<AppAction, ActionState>) -> (u32, u32) {
    let mut pressed = 0;
    let mut repeated = 0;
    for (action, bit) in [
        (AppAction::Up, 24),
        (AppAction::AnyDown, 25),
        (AppAction::Left, 26),
        (AppAction::Right, 27),
        (AppAction::PageUp, 10),
        (AppAction::PageDown, 11),
        (AppAction::Cancel, 29),
        (AppAction::Enter, 28),
        (AppAction::Backlog, 6),
        (AppAction::SystemMenu, 7),
    ] {
        if input[action].is_clicked {
            pressed |= 1 << bit;
        }
        if input[action].is_clicked_or_repeated {
            repeated |= 1 << bit;
        }
    }
    (pressed, repeated)
}
