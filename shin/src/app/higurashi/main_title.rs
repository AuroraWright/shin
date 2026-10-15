//! Updated main Title/First readiness and retained opening gates.
//! Owns the older parent backdrop/particles and retained Top/Cat menus.
use super::scene_host::{Globals, TitleTransition};
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::{
            audio::MusicEvent, music_commands::MusicCommands, reconstruction_mode::OverlayClock,
            runner::reconstruction_mode::Overlay, voice_commands::VoiceCommands,
        },
    },
    asset::{mask::MaskTexture, system::AssetServer},
    layer::higurashi_properties::HigurashiTweener,
};
use anyhow::{Result, bail, ensure};
use shin_core::time::Ticks;
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};
use std::sync::{Arc, mpsc};

mod cat_menu;
mod chart;
mod particles;
mod quiz_menu;
mod top_menu;
mod view;
pub(super) use cat_menu::SceneRequest as CategorySceneRequest;
pub(super) use top_menu::Destination as TitleDestination;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MenuRequest {
    Top { selected: i32 },
    Extra { variant: u32, selected: i32 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    First,
    BackgroundGate,
    LogoGate,
    Menu,
}
#[derive(Clone, Debug)]
pub(super) struct Frame {
    pub background_offset: f32,
    #[allow(dead_code, reason = "Preserve the native caption tween in frame inspection data.")]
    pub caption_alpha: f32,
    pub logo_alpha: f32,
    pub logo_offset: f32,
    pub black_alpha: f32,
}
#[derive(Default)]
struct Actions {
    bgm: Option<i32>,
    voice: bool,
}
struct Motion {
    selected: i32,
    mode: u32,
    phase: Phase,
    first_phase: i32,
    overlay: OverlayClock,
    clocks: [HigurashiTweener; 5],
    pending: Option<MenuRequest>,
}
fn resolve_mode(argument: u32, last: &mut i32) -> u32 {
    let value = argument as i32;
    let raw = if value < 0 {
        *last
    } else {
        *last = value;
        value
    };
    raw as u32 & 1
}
impl Motion {
    fn new(selected: i32, mode: u32, captured: bool) -> Self {
        Self {
            selected,
            mode,
            phase: Phase::First,
            first_phase: 0,
            overlay: OverlayClock::captured(captured),
            clocks: std::array::from_fn(|_| HigurashiTweener::new(0.0)),
            pending: None,
        }
    }
    fn advance(&mut self, status: u32, delta: Ticks) -> Result<Actions> {
        let mut actions = Actions::default();
        match self.phase {
            Phase::First => {
                if self.first_phase == 0 && status == 2 {
                    actions.bgm = Some(if self.mode == 0 { 158 } else { 74 });
                    // def7c resets the queues as well as their scalar values.
                    for (index, value) in [
                        (0, if self.selected < 0 { -200.0 } else { 0.0 }),
                        (2, if self.selected < 0 { 0.0 } else { 1.0 }),
                        (3, if self.selected < 0 { -30.0 } else { 0.0 }),
                    ] {
                        self.clocks[index].fast_forward_to(value);
                    }
                    if self.selected < 0 {
                        self.clocks[0].control(0.0, Ticks::from_f32(90.0), 2)?;
                        self.clocks[0].control(0.0, Ticks::from_f32(10.0), 0x80)?;
                        self.overlay.clear(60);
                    } else {
                        self.overlay.masked(30, f32::from_bits(0x3e99999a), 0, true);
                    }
                    self.first_phase = 1;
                }
                if self.first_phase == 1 && !self.overlay.busy() {
                    self.first_phase = -1;
                    self.phase = Phase::BackgroundGate;
                }
                // e4d44 evaluates readiness/completion before55524 advances.
                self.overlay.advance(delta);
            }
            Phase::BackgroundGate => {
                if self.clocks[0].is_idle() {
                    if self.selected < 0 {
                        self.clocks[2].control(1.0, Ticks::from_f32(30.0), 0)?;
                        self.clocks[3].control(0.0, Ticks::from_f32(30.0), 2)?;
                        self.clocks[3].control(0.0, Ticks::from_f32(10.0), 0x80)?;
                        actions.voice = true;
                    }
                    self.phase = Phase::LogoGate;
                }
            }
            Phase::LogoGate => {
                if self.clocks[3].is_idle() {
                    self.pending = Some(if self.selected >= 10 {
                        MenuRequest::Extra {
                            variant: 1,
                            selected: self.selected - 10,
                        }
                    } else {
                        MenuRequest::Top {
                            selected: self.selected,
                        }
                    });
                    self.phase = Phase::Menu;
                }
            }
            Phase::Menu => {}
        }
        for clock in &mut self.clocks[..4] {
            clock.update(delta);
        }
        // Parent de938 advances its blackout clock by1 even at zero scene delta.
        self.clocks[4].update(Ticks::from_f32(1.0));
        Ok(actions)
    }
    fn frame(&self) -> Frame {
        Frame {
            background_offset: self.clocks[0].value(),
            caption_alpha: self.clocks[1].value(),
            logo_alpha: self.clocks[2].value(),
            logo_offset: self.clocks[3].value(),
            black_alpha: self.clocks[4].value(),
        }
    }
}
struct Job {
    task: Option<shin_tasks::AsyncTask<Result<Arc<MaskTexture>>>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl Drop for Job {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(task) = self.task.take() {
                task.cancel_and_retire();
            }
            self.retirement.clone().retire(());
        }
    }
}
pub(super) struct Opening {
    pub globals: Globals,
    pub atlas: Option<Arc<CharacterTextures>>,
    job: Option<Job>,
    received: mpsc::Receiver<Arc<CharacterTextures>>,
    mask: Option<Arc<MaskTexture>>,
    overlay: Option<Overlay>,
    motion: Motion,
    ready: bool,
    fault: Option<String>,
    particles: particles::Particles,
    plus: bool,
    top: Option<top_menu::TopMenu>,
    top_visible: bool,
    cat: Option<cat_menu::CatMenu>,
    cat_visible: bool,
    quiz: Option<quiz_menu::QuizMenu>,
    quiz_visible: bool,
    save: Option<(i32, super::save_menu::Owner)>,
    save_visible: bool,
    config: Option<(i32, super::config::owner::Owner)>,
    config_visible: bool,
    manual: Option<(i32, super::manual::owner::Owner)>,
    manual_visible: bool,
    chart: Option<(i32, super::chart::owner::Owner)>,
    chart_visible: bool,
    chart_input: Option<super::input_bridge::Analog>,
    chart_seed: Arc<dyn Fn() -> u32 + Send + Sync>,
    chart_adv: Option<(super::runtime_game_data::RuntimeGameData, u32)>,
    manual_load: Option<(super::runtime_game_data::RuntimeGameData, u32)>,
    menu_extent: (u16, u16),
    #[allow(dead_code, reason = "Retained native input-frame state for title input integration.")]
    input_frame: u32,
    destinations: std::collections::VecDeque<top_menu::Destination>,
    idle_gate: bool,
    title_demo_request: Option<super::startup::TitleDemoRequest>,
    story_gate: Option<shin_core::format::save::higurashi::GameData>,
    category_scenes: std::collections::VecDeque<CategorySceneRequest>,
}
impl Opening {
    /// Report actual retained owners used by the bounded normal-window journey.
    pub(super) fn touch_diagnostic_phase(&self) -> Option<&'static str> {
        if self.fault.is_some() || !self.ready || self.motion.phase != Phase::Menu {
            return None;
        }
        if self.save_visible {
            return self.save.as_ref()?.1.touch_diagnostic_phase();
        }
        if self.config_visible
            || self.manual_visible
            || self.chart_visible
            || self.cat_visible
            || self.quiz_visible
        {
            return None;
        }
        (self.top_visible
            && self
                .top
                .as_ref()
                .is_some_and(top_menu::TopMenu::touch_diagnostic_ready))
        .then_some("title")
    }
    /// Native ddcf8/e49d0 capture the previous composed frame at construction.
    /// Consume the real startup handoff;all loaded globals retain their owners.
    pub fn new(mut transition: TitleTransition, capture: Option<RenderTexture>) -> Self {
        let mode = resolve_mode(
            transition.request.mode,
            &mut transition.globals.last_title_mode,
        );
        let motion = Motion::new(transition.request.selected, mode, capture.is_some());
        let plus = transition.globals.server.contains("/append.txt");
        let particles = particles::Particles::warm(crate::engine_random::next_seed());
        let (sender, received) = mpsc::channel();
        let server: Arc<AssetServer> = transition.globals.server.clone();
        let task = shin_tasks::async_io::spawn_loader(async move {
            let atlas = server.load::<CharacterTextures, _>("/title.txa").await?;
            let _ = sender.send(atlas);
            server.load::<MaskTexture, _>("/default.msk").await
        });
        let retirement = task.retirement_handle().unwrap();
        let overlay = capture.map(|snapshot| Overlay {
            clock: motion.overlay.clone(),
            snapshot,
            mask: None,
        });
        Self {
            globals: transition.globals,
            atlas: None,
            job: Some(Job {
                task: Some(task),
                retirement,
            }),
            received,
            mask: None,
            overlay,
            motion,
            ready: false,
            fault: None,
            particles,
            plus,
            top: None,
            top_visible: false,
            cat: None,
            cat_visible: false,
            quiz: None,
            quiz_visible: false,
            save: None,
            save_visible: false,
            config: None,
            config_visible: false,
            manual: None,
            manual_visible: false,
            chart: None,
            chart_visible: false,
            chart_input: None,
            chart_seed: Arc::new(crate::engine_random::next_seed),
            chart_adv: None,
            manual_load: None,
            menu_extent: (1920, 1080),
            input_frame: 0,
            destinations: Default::default(),
            idle_gate: false,
            title_demo_request: None,
            story_gate: None,
            category_scenes: Default::default(),
        }
    }
    #[cfg(test)]
    pub fn update(&mut self, delta: Ticks) -> Result<()> {
        self.input_frame = self.input_frame.wrapping_add(1);
        let pointer = crate::adv::input_area::PointerFrame {
            events: &[],
            number: self.input_frame,
            time_seconds: f64::from(self.input_frame) / 60.0,
        };
        self.update_with_input(
            delta,
            true,
            crate::adv::higurashi::input::Buttons::default(),
            &pointer,
        )
    }
    pub fn update_with_input(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: crate::adv::higurashi::input::Buttons,
        pointer: &crate::adv::input_area::PointerFrame<'_>,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.0,
            "Invalid title opening delta"
        );
        if let Some(fault) = &self.fault {
            bail!("Higurashi title preparation failed:{fault}");
        }
        // The native completed First child retires at the next parent poll.
        if self.motion.phase != Phase::First {
            self.overlay.take();
            self.mask.take();
            self.job.take();
        }
        let result = (|| {
            if self.idle_gate && self.motion.clocks[4].is_idle() {
                self.title_demo_request = Some(super::startup::TitleDemoRequest {
                    mode: self.motion.mode + 1,
                });
                self.idle_gate = false;
            }
            if self.motion.clocks[4].is_idle() {
                if let Some(data) = self.story_gate.take() {
                    self.category_scenes
                        .push_back(CategorySceneRequest::Story { data, flags: 0 });
                }
            }
            if self.top.as_ref().is_some_and(top_menu::TopMenu::complete) {
                self.top.take();
            }
            if self.cat.as_ref().is_some_and(cat_menu::CatMenu::complete) {
                self.cat.take();
            }
            if self
                .quiz
                .as_ref()
                .is_some_and(quiz_menu::QuizMenu::complete)
            {
                self.quiz.take();
            }
            if self.save.as_ref().is_some_and(|(_, menu)| menu.complete()) {
                self.save.take().unwrap().1.retire(&mut self.globals.data);
            }
            if self
                .config
                .as_ref()
                .is_some_and(|(_, menu)| menu.complete())
            {
                self.config.take();
            }
            if self
                .manual
                .as_ref()
                .is_some_and(|(_, menu)| menu.complete())
            {
                self.manual.take();
            }
            self.top_visible = self.top.is_some();
            self.cat_visible = self.cat.is_some();
            self.quiz_visible = self.quiz.is_some();
            self.save_visible = self.save.is_some();
            self.config_visible = self.config.is_some();
            self.manual_visible = self.manual.is_some();
            if self.chart.as_ref().is_some_and(|(_, menu)| menu.complete()) {
                self.chart.take();
            }
            self.chart_visible = self.chart.is_some();
            let menu_permission = permission
                && self.save.is_none()
                && self.config.is_none()
                && self.manual.is_none()
                && self.chart.is_none();
            self.poll_chart(delta, permission, buttons, pointer)?;
            let manual_requests = if let Some((_, menu)) = &mut self.manual {
                menu.poll(delta, permission, buttons, pointer)?;
                menu.take_requests()
            } else {
                Vec::new()
            };
            for stage in manual_requests {
                if stage == 1 {
                    let selected = self.manual.as_ref().unwrap().0;
                    self.top = Some(top_menu::TopMenu::new(
                        self.plus,
                        self.globals.data.auto_save_slot.is_some(),
                        selected,
                        self.menu_extent,
                    ));
                    self.motion.clocks[4].control(
                        0.,
                        Ticks::from_f32(f32::from_bits(0x3dcccccd)),
                        0x40,
                    )?;
                }
            }
            let config_requests = if let Some((_, menu)) = &mut self.config {
                menu.poll(
                    &mut self.globals.data,
                    self.globals.runtime_saves.as_ref(),
                    &mut self.globals.platform_flag,
                    delta,
                    permission,
                    buttons,
                    pointer,
                )?;
                menu.take_requests()
            } else {
                Vec::new()
            };
            for stage in config_requests {
                if stage == 1 {
                    let selected = self.config.as_ref().unwrap().0;
                    self.top = Some(top_menu::TopMenu::new(
                        self.plus,
                        self.globals.data.auto_save_slot.is_some(),
                        selected,
                        self.menu_extent,
                    ));
                    self.motion.clocks[4].control(
                        0.,
                        Ticks::from_f32(f32::from_bits(0x3dcccccd)),
                        0x40,
                    )?;
                }
            }
            let save_requests = if let Some((_, menu)) = &mut self.save {
                menu.poll(
                    &mut self.globals.data,
                    self.globals
                        .runtime_saves
                        .as_mut()
                        .expect("SaveMenu owns loaded slots"),
                    delta,
                    permission,
                    buttons,
                    pointer,
                    None,
                )?;
                menu.take_requests()
            } else {
                Vec::new()
            };
            for request in save_requests {
                match request {
                    super::save_menu::Request::ReturnSource(1) => {
                        // deec0 appends the new Top first,then controls blackout.
                        let selected = self.save.as_ref().unwrap().0;
                        self.top = Some(top_menu::TopMenu::new(
                            self.plus,
                            self.globals.data.auto_save_slot.is_some(),
                            selected,
                            self.menu_extent,
                        ));
                        self.motion.clocks[4].control(
                            0.0,
                            Ticks::from_f32(f32::from_bits(0x3dcccccd)),
                            0x40,
                        )?;
                    }
                    super::save_menu::Request::ReturnSource(_) => {}
                    super::save_menu::Request::Load { data, flags } => {
                        self.manual_load = Some((data, flags));
                    }
                }
            }
            // Snapshot both actors before any constructor replaces a child.
            let quiz_actions = if let Some(quiz) = &mut self.quiz {
                quiz.poll(delta, menu_permission, buttons, pointer)?
            } else {
                Vec::new()
            };
            let cat_actions = if let Some(cat) = &mut self.cat {
                cat.poll(delta, menu_permission, buttons, pointer)?
            } else {
                Vec::new()
            };
            let actions = if self.top_visible {
                self.top
                    .as_mut()
                    .unwrap()
                    .poll(delta, menu_permission, buttons, pointer)?
            } else {
                Vec::new()
            };
            for action in actions {
                match action {
                    top_menu::Action::Logo(value, ticks) => {
                        self.motion.clocks[2].control(value, Ticks::from_f32(ticks), 0)?
                    }
                    top_menu::Action::Cue(cue) => {
                        self.globals.sounds.lock().play(
                            &self.globals.server,
                            match cue {
                                top_menu::Cue::Move => "move",
                                top_menu::Cue::Confirm => "decide",
                                top_menu::Cue::Error => "error",
                            },
                            shin_core::vm::command::types::Volume(1.0),
                        );
                    }
                    top_menu::Action::Destination(destination) => {
                        if let top_menu::Destination::Cat { variant, selected } = destination {
                            self.cat = Some(cat_menu::CatMenu::new(
                                variant,
                                self.globals.data.persist_data.get(0) as i16,
                                selected,
                                self.menu_extent,
                            ));
                            continue;
                        }
                        if destination == top_menu::Destination::Quiz {
                            self.quiz = Some(quiz_menu::QuizMenu::new(self.menu_extent));
                            continue;
                        }
                        if matches!(
                            destination,
                            top_menu::Destination::Save { .. }
                                | top_menu::Destination::Config { .. }
                                | top_menu::Destination::Chart { .. }
                                | top_menu::Destination::Manual { .. }
                        ) {
                            self.motion.clocks[4].control(
                                0.5,
                                Ticks::from_f32(f32::from_bits(0x3dcccccd)),
                                0x40,
                            )?;
                        }
                        if let top_menu::Destination::Save { return_id } = destination {
                            let slots = self.globals.runtime_saves.get_or_insert_with(|| {
                                super::runtime_saves::Slots::loaded(&self.globals.data)
                            });
                            let prepared = super::save_menu::Prepared::new(
                                None,
                                &self.globals.data,
                                slots,
                                self.plus,
                                self.menu_extent,
                            )?;
                            self.save = Some((
                                return_id,
                                super::save_menu::Owner::from_prepared(
                                    super::save_menu::Environment {
                                        common: self.globals.common.clone(),
                                        server: self.globals.server.clone(),
                                        sounds: self.globals.sounds.clone(),
                                        storage: self.globals.storage.clone(),
                                    },
                                    prepared,
                                )?,
                            ));
                            continue;
                        }
                        if let top_menu::Destination::Config { flags, return_id } = destination {
                            let menu = super::config::owner::Owner::new(
                                super::save_menu::Environment {
                                    common: self.globals.common.clone(),
                                    server: self.globals.server.clone(),
                                    sounds: self.globals.sounds.clone(),
                                    storage: self.globals.storage.clone(),
                                },
                                &self.globals.data,
                                flags,
                                self.plus,
                                self.menu_extent,
                            )?;
                            self.config = Some((return_id, menu));
                            continue;
                        }
                        if let top_menu::Destination::Manual { return_id } = destination {
                            self.manual = Some((
                                return_id,
                                super::manual::owner::Owner::new(
                                    super::save_menu::Environment {
                                        common: self.globals.common.clone(),
                                        server: self.globals.server.clone(),
                                        sounds: self.globals.sounds.clone(),
                                        storage: self.globals.storage.clone(),
                                    },
                                    self.menu_extent,
                                )?,
                            ));
                            continue;
                        }
                        if let top_menu::Destination::Chart {
                            selected,
                            return_id,
                        } = destination
                        {
                            self.construct_chart(selected, return_id)?;
                            continue;
                        }
                        if destination == top_menu::Destination::IdleDemo {
                            self.globals.audio.stop_bgm(60);
                            self.motion.clocks[4].control(1.0, Ticks::from_f32(60.0), 0)?;
                            self.idle_gate = true;
                        }
                        self.destinations.push_back(destination);
                    }
                }
            }
            for action in cat_actions {
                match action {
                    cat_menu::Action::Cue(name) => self.globals.sounds.lock().play(
                        &self.globals.server,
                        name,
                        shin_core::vm::command::types::Volume(1.0),
                    ),
                    cat_menu::Action::Destination(destination) => match destination {
                        cat_menu::Destination::Top(selected) => {
                            self.top = Some(top_menu::TopMenu::new(
                                self.plus,
                                self.globals.data.auto_save_slot.is_some(),
                                selected,
                                self.menu_extent,
                            ));
                        }
                        cat_menu::Destination::Story(scenario_id) => {
                            use shin_core::format::save::higurashi::{
                                GameData, GameDataEntry, SaveDate,
                            };
                            self.story_gate = Some(GameData {
                                date: SaveDate {
                                    year: 0,
                                    month: 0,
                                    day: 0,
                                    hour: 0,
                                    minute: 0,
                                    second: 0,
                                },
                                extra_word: scenario_id as u8,
                                entry: GameDataEntry {
                                    scenario_id,
                                    random_seed: crate::engine_random::next_seed(),
                                    save_position: 0,
                                    replay_target: 0,
                                    response_data: Vec::new(),
                                },
                                primary_entry: None,
                                trailing_values: Vec::new(),
                                byte_variables: Vec::new(),
                            });
                            self.globals.audio.stop_bgm(90);
                            self.motion.clocks[4].control(1.0, Ticks::from_f32(90.0), 0)?;
                        }
                        cat_menu::Destination::Cg => {
                            self.category_scenes.push_back(CategorySceneRequest::Cg)
                        }
                        cat_menu::Destination::Music => {
                            self.category_scenes.push_back(CategorySceneRequest::Music)
                        }
                        cat_menu::Destination::Movie => {
                            self.category_scenes.push_back(CategorySceneRequest::Movie)
                        }
                    },
                }
            }
            for action in quiz_actions {
                match action {
                    quiz_menu::Action::Cue(name) => self.globals.sounds.lock().play(
                        &self.globals.server,
                        name,
                        shin_core::vm::command::types::Volume(1.0),
                    ),
                    quiz_menu::Action::Destination(quiz_menu::Destination::Top) => {
                        self.top = Some(top_menu::TopMenu::new(
                            self.plus,
                            self.globals.data.auto_save_slot.is_some(),
                            7,
                            self.menu_extent,
                        ));
                    }
                    quiz_menu::Action::Destination(quiz_menu::Destination::Story(scenario_id)) => {
                        use shin_core::format::save::higurashi::{
                            GameData, GameDataEntry, SaveDate,
                        };
                        self.story_gate = Some(GameData {
                            date: SaveDate {
                                year: 0,
                                month: 0,
                                day: 0,
                                hour: 0,
                                minute: 0,
                                second: 0,
                            },
                            extra_word: 0,
                            entry: GameDataEntry {
                                scenario_id,
                                random_seed: crate::engine_random::next_seed(),
                                save_position: 0,
                                replay_target: 0,
                                response_data: Vec::new(),
                            },
                            primary_entry: None,
                            trailing_values: Vec::new(),
                            byte_variables: Vec::new(),
                        });
                        self.globals.audio.stop_bgm(90);
                        self.motion.clocks[4].control(1.0, Ticks::from_f32(90.0), 0)?;
                    }
                }
            }
            if let Ok(atlas) = self.received.try_recv() {
                self.atlas = Some(atlas);
            }
            if let Some(job) = &mut self.job {
                if let Some(result) = job.task.as_mut().and_then(|task| task.poll_naive()) {
                    job.task = None;
                    self.mask = Some(result?);
                    self.ready = true;
                    // A producer may publish atlas and completion between polls.
                    if let Ok(atlas) = self.received.try_recv() {
                        self.atlas = Some(atlas);
                    }
                }
            }
            let before = self.motion.phase;
            let actions = self.motion.advance(if self.ready { 2 } else { 0 }, delta)?;
            if before != Phase::Menu && self.motion.phase == Phase::Menu {
                match self.motion.pending {
                    Some(MenuRequest::Top { selected }) => {
                        self.top = Some(top_menu::TopMenu::new(
                            self.plus,
                            self.globals.data.auto_save_slot.is_some(),
                            selected,
                            self.menu_extent,
                        ))
                    }
                    Some(MenuRequest::Extra { variant, selected }) => {
                        self.cat = Some(cat_menu::CatMenu::new(
                            variant,
                            self.globals.data.persist_data.get(0) as i16,
                            selected,
                            self.menu_extent,
                        ))
                    }
                    None => {}
                }
            }
            if let Some(id) = actions.bgm {
                self.globals.audio.play_bgm(id, 60, 0, 1.0)?;
            }
            if actions.voice {
                self.globals.audio.play_command("S00/title_call", 1.0, 0);
            }
            while let Some(event) = self.globals.audio.pop_music_event() {
                if let MusicEvent::Started(info) = event {
                    // Original329c4 unlocks both BGM ID and nonnegative linked ID.
                    for id in [info.id, i32::from(info.linked_id)]
                        .into_iter()
                        .filter(|id| *id >= 0)
                    {
                        let words = &mut self.globals.data.save_vectors.unlocked_bgms;
                        let at = id as usize / 32;
                        if at >= words.len() {
                            words.resize((at + 1).next_multiple_of(256), 0);
                        }
                        words[at] |= 1 << (id as usize % 32);
                    }
                }
            }
            self.globals.audio.update(delta, false);
            self.globals.sounds.lock().update();
            self.particles.advance(delta);
            if let Some(overlay) = &mut self.overlay {
                overlay.clock = self.motion.overlay.clone();
                overlay.mask = if self.motion.overlay.mask_present {
                    self.mask.clone()
                } else {
                    None
                };
            }
            Ok(())
        })();
        if let Err(error) = &result {
            self.fault = Some(format!("{error:#}"));
        }
        result
    }
    pub fn frame(&self) -> Frame {
        self.motion.frame()
    }
    pub fn prepare(&mut self, context: &mut crate::render::PreRenderContext) -> Result<()> {
        if let Some((_, chart)) = &self.chart {
            chart.prepare(context);
        }
        if let Some((_, menu)) = &self.config {
            menu.prepare(context);
        }
        if let Some((_, menu)) = &mut self.save {
            menu.prepare(context)?;
        }
        Ok(())
    }
    /// Updated parent artwork,then the retained First capture. Menus draw after.
    pub fn render(&self, pass: &mut RenderPass) {
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(0),
            Some(1.0),
        );
        for packet in view::packets(
            self.atlas.is_some(),
            self.plus,
            &self.frame(),
            &self.particles,
        ) {
            packet.render(pass, self.atlas.as_deref());
        }
        self.render_transition(pass);
        if self.top_visible {
            if let (Some(top), Some(atlas)) = (&self.top, &self.atlas) {
                top.render(pass, atlas);
            }
        }
        if self.cat_visible {
            if let (Some(cat), Some(atlas)) = (&self.cat, &self.atlas) {
                cat.render(pass, atlas, self.plus);
            }
        }
        if self.quiz_visible {
            if let (Some(quiz), Some(atlas)) = (&self.quiz, &self.atlas) {
                quiz.render(pass, atlas, self.plus);
            }
        }
        if self.save_visible {
            if let Some((_, menu)) = &self.save {
                menu.render(pass);
            }
        }
        if self.config_visible {
            if let Some((_, menu)) = &self.config {
                menu.render(pass)
                    .expect("Config prepared its native atlas/resources");
            }
        }
        if self.manual_visible {
            if let Some((_, menu)) = &self.manual {
                menu.render(pass);
            }
        }
        if self.chart_visible {
            if let Some((_, chart)) = &self.chart {
                chart
                    .render(pass)
                    .expect("Chart has its prepared native resources");
            }
        }
    }
    /// Draw First after the actual parent title backdrop,matching e4eb0.
    pub fn render_transition(&self, pass: &mut RenderPass) {
        if let Some(overlay) = &self.overlay {
            overlay.render(pass);
        }
    }
    #[cfg(test)]
    pub fn take_menu_request(&mut self) -> Option<MenuRequest> {
        self.motion.pending.take()
    }
    pub fn take_tips_destination(&mut self) -> Option<TitleDestination> {
        let at = self
            .destinations
            .iter()
            .position(|d| matches!(d, TitleDestination::Tips { .. }))?;
        self.destinations.remove(at)
    }
    pub fn take_destination(&mut self) -> Option<TitleDestination> {
        self.destinations.pop_front()
    }
    pub fn take_title_demo_request(&mut self) -> Option<super::startup::TitleDemoRequest> {
        self.title_demo_request.take()
    }
    pub fn take_category_scene_request(&mut self) -> Option<CategorySceneRequest> {
        self.category_scenes.pop_front()
    }
    pub fn take_manual_load(&mut self) -> Option<(super::runtime_game_data::RuntimeGameData, u32)> {
        self.manual_load.take()
    }
    pub fn resize_menu_input(&mut self, extent: (u16, u16)) {
        if self.menu_extent == extent {
            return;
        }
        self.menu_extent = extent;
        if let Some(top) = &mut self.top {
            top.resize(extent);
        }
        if let Some(cat) = &mut self.cat {
            cat.resize(extent);
        }
        if let Some(quiz) = &mut self.quiz {
            quiz.resize(extent);
        }
        if let Some((_, menu)) = &mut self.save {
            if let Err(error) = menu.resize(extent) {
                self.fault = Some(format!("{error:#}"));
            }
        }
        if let Some((_, menu)) = &mut self.config {
            if let Err(error) = menu.resize(extent) {
                self.fault = Some(format!("{error:#}"));
            }
        }
        if let Some((_, menu)) = &mut self.manual {
            if let Err(error) = menu.resize(extent) {
                self.fault = Some(format!("{error:#}"));
            }
        }
        if let Some((_, menu)) = &mut self.chart {
            if let Err(error) = menu.resize(extent) {
                self.fault = Some(format!("{error:#}"));
            }
        }
    }
    pub fn cancel_input(&mut self) -> Result<()> {
        if let Some(top) = &mut self.top {
            top.cancel_input();
        }
        if let Some(cat) = &mut self.cat {
            cat.cancel_input();
        }
        if let Some(quiz) = &mut self.quiz {
            quiz.cancel_input();
        }
        if let Some((_, menu)) = &mut self.save {
            menu.cancel_input();
        }
        if let Some((_, menu)) = &mut self.config {
            menu.cancel_input();
        }
        if let Some((_, menu)) = &mut self.manual {
            menu.cancel_input();
        }
        if let Some((_, menu)) = &mut self.chart {
            menu.cancel_input();
        }
        Ok(())
    }
    pub fn has_save_task(&self) -> bool {
        self.save
            .as_ref()
            .is_some_and(|(_, menu)| menu.has_save_task())
            || self
                .config
                .as_ref()
                .is_some_and(|(_, menu)| menu.has_save_task())
    }
    pub fn quiesce_progress(&mut self) -> Result<()> {
        if let Some((_, menu)) = &mut self.save {
            menu.quiesce_progress(&mut self.globals.data)?;
        }
        if let Some((_, menu)) = &self.config {
            menu.publish_progress(&mut self.globals.data);
        }
        Ok(())
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub(super) fn reference_menu_state(&self) -> Option<(&'static str, i32)> {
        if self.chart_visible {
            return self
                .chart
                .as_ref()
                .map(|(_, menu)| ("chart", if menu.reference_state().2 { 2 } else { 0 }));
        }
        if self.manual_visible {
            return self
                .manual
                .as_ref()
                .map(|(_, menu)| ("manual", if menu.reference_state().2 { 2 } else { 0 }));
        }
        if self.config_visible {
            return self
                .config
                .as_ref()
                .map(|(_, menu)| ("config", if menu.reference_state().2 { 2 } else { 0 }));
        }
        if self.top_visible {
            return self
                .top
                .as_ref()
                .map(|menu| ("top", menu.reference_phase()));
        }
        if self.cat_visible {
            return self
                .cat
                .as_ref()
                .map(|menu| ("cat", menu.reference_phase()));
        }
        None
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub(super) fn reference_config(&self) -> Option<(u32, usize, bool, u8)> {
        self.config.as_ref().map(|(_, menu)| menu.reference_state())
    }
    #[cfg(test)]
    pub(super) fn reference_config_input_ready(&self) -> bool {
        self.config_visible
            && self
                .config
                .as_ref()
                .is_some_and(|(_, menu)| menu.reference_input_ready())
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub(super) fn reference_manual(&self) -> Option<(usize, i32, bool, u32, i32)> {
        self.manual.as_ref().map(|(_, menu)| menu.reference_state())
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub(super) fn reference_top_selection(&self) -> Option<i32> {
        self.top.as_ref().map(|top| top.reference_selection())
    }
    #[cfg(test)]
    pub(super) fn reference_top_idle(&self) -> Option<i32> {
        self.top.as_ref().map(|top| top.reference_idle())
    }
    #[cfg(test)]
    pub(super) fn reference_quiz_menu(&self) -> Option<(i32, usize)> {
        if self.quiz_visible {
            self.quiz.as_ref().map(quiz_menu::QuizMenu::reference_state)
        } else {
            None
        }
    }
    #[cfg(test)]
    pub(super) fn reference_save_menu(&self) -> Option<&super::save_menu::Owner> {
        self.save.as_ref().map(|(_, menu)| menu)
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub(super) fn reference_config_question(&self) -> Option<(i32, i32)> {
        self.config
            .as_ref()
            .and_then(|(_, menu)| menu.reference_question())
    }
    /// Retire this title's retained worker/capture before returning global audio.
    pub fn into_globals(mut self) -> Globals {
        if let Some((_, menu)) = self.save.take() {
            menu.retire(&mut self.globals.data);
        }
        self.globals
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub(super) fn reference_category(&self) -> Option<(u32, i32, i32)> {
        self.cat
            .as_ref()
            .map(cat_menu::CatMenu::reference_selection)
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub(super) fn reference_initial_selection(&self) -> (i32, u32) {
        (self.motion.selected, self.motion.mode)
    }
}

#[cfg(test)]
mod tests;
