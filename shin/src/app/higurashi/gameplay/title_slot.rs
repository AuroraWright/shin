//! Deferred title-to-ADV transfer;capture and retire the old title before ctor.
use super::*;
use crate::adv::higurashi::{checkpoints::CheckpointMemory, host::scene::BrowserJump};
use crate::app::higurashi::{
    main_title::{CategorySceneRequest, Opening},
    scene_host::TitleTransition,
};
use anyhow::Context;
struct Replacement {
    request: BrowserJump,
    globals: Option<Globals>,
    bindings: Option<Bindings>,
    snapshot: Option<RenderTexture>,
    seeds: Option<Seeds>,
    clock: Option<Box<dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate> + Send>>,
    mode_ids: Option<(u64, u64)>,
}
struct StoryTransfer {
    flags: u32,
    globals: Option<Globals>,
    bindings: Option<Bindings>,
    snapshot: Option<RenderTexture>,
    seeds: Seeds,
}
struct TipsTransfer {
    request: super::super::tips_menu::factory::Request,
    globals: Option<Globals>,
    snapshot: Option<RenderTexture>,
    extent: (u16, u16),
}
mod active;
mod cg_gallery;
mod movie_gallery;
mod music_gallery;
pub(in crate::app::higurashi) use active::{
    ActiveGlobals, ActiveGlobalsMut, ActiveScene, ExitRequest, TitleDemoTransfer,
};
#[cfg(test)]
mod bonus_tests;
mod kakera;
mod manual_load;
#[cfg(test)]
mod menu_returns_tests;
mod quiz;
mod scenario_select;
mod tips;
use kakera::{Continuation, Return};
#[cfg(test)]
mod active_fault_tests;
#[cfg(test)]
mod chart_tests;
#[cfg(test)]
mod save_tests;
enum Pending {
    Movie,
    PreparedMovie(movie_gallery::Transfer),
    MovieTitle(super::super::title_demo::TitleRequest),
    Music,
    PreparedMusic(music_gallery::Transfer),
    MusicTitle(super::super::title_demo::TitleRequest),
    Cg,
    PreparedCg(cg_gallery::Transfer),
    CgTitle(super::super::title_demo::TitleRequest),
    Story(GameData, u32),
    PreparedStory(StoryTransfer),
    Browser(Replacement),
    Kakera(super::kakera::SceneRequest),
    Gameplay(Return),
    Title(super::super::title_demo::TitleRequest),
    Tips(super::super::tips_menu::factory::Request),
    PreparedTips(TipsTransfer),
    TipsReturn(super::super::tips_menu::SceneRequest),
    Quiz(super::quiz::SceneRequest),
    ScenarioSelect(super::scenario_select::SceneRequest),
    Origin(i32),
    PreparedScenarioSelect(scenario_select::Transfer),
    PickerTitle(super::super::title_demo::TitleRequest),
    ManualLoad(manual_load::Transfer),
    ChartAdv(manual_load::Transfer),
    BonusAdv(manual_load::Transfer),
}
pub(in crate::app::higurashi) struct TitleSlot {
    pub title: Option<Opening>,
    pub game: Option<Gameplay>,
    pub kakera: Option<crate::app::higurashi::kakera_menu::opening::Opening>,
    pub tips: Option<crate::app::higurashi::tips_menu::opening::Opening>,
    pub quiz: Option<crate::app::higurashi::quiz_scene::opening::Opening>,
    pub scenario_select: Option<crate::app::higurashi::scenario_select::opening::Opening>,
    pub cg_gallery: Option<crate::app::higurashi::cg_gallery::opening::Opening>,
    pub music_gallery: Option<crate::app::higurashi::music_gallery::opening::Opening>,
    pub movie_gallery: Option<crate::app::higurashi::movie_gallery::opening::Opening>,
    pending: Option<Pending>,
    other: std::collections::VecDeque<CategorySceneRequest>,
    title_destinations:
        std::collections::VecDeque<crate::app::higurashi::main_title::TitleDestination>,
    title_demo_request: Option<crate::app::higurashi::startup::TitleDemoRequest>,
    continuation: Option<Continuation>,
    #[cfg(test)]
    reference_scene_events: Vec<u32>,
    #[cfg(test)]
    reference_factory_fault: bool,
}
impl TitleSlot {
    pub fn set_desktop_mouse_release(
        &mut self,
        release: Option<crate::adv::input_area::MouseRelease>,
    ) {
        if let Some(game) = &mut self.game {
            game.set_desktop_mouse_release(release);
        }
    }
    pub fn set_analog_input(&mut self, input: crate::app::higurashi::input_bridge::Analog) {
        if let Some(title) = &mut self.title {
            title.set_analog_input(input);
        }
        if let Some(game) = &mut self.game {
            game.set_analog_input(input);
        }
        if let Some(cg) = &mut self.cg_gallery {
            cg.set_analog_input(input);
        }
    }
    #[cfg(test)]
    fn reference_fail_next_constructor(&mut self) {
        self.reference_factory_fault = true;
    }
    #[cfg(test)]
    fn reference_check_factory_fault(&mut self) -> Result<()> {
        ensure!(
            !std::mem::take(&mut self.reference_factory_fault),
            "Injected retained scene constructor fault"
        );
        Ok(())
    }
    pub fn from_game(game: Gameplay) -> Self {
        Self {
            title: None,
            game: Some(game),
            kakera: None,
            tips: None,
            quiz: None,
            scenario_select: None,
            cg_gallery: None,
            music_gallery: None,
            movie_gallery: None,
            pending: None,
            other: Default::default(),
            title_destinations: Default::default(),
            title_demo_request: None,
            continuation: None,
            #[cfg(test)]
            reference_scene_events: Vec::new(),
            #[cfg(test)]
            reference_factory_fault: false,
        }
    }
    pub fn new(transition: TitleTransition, capture: Option<RenderTexture>) -> Self {
        Self {
            title: Some(Opening::new(transition, capture)),
            game: None,
            kakera: None,
            tips: None,
            quiz: None,
            scenario_select: None,
            cg_gallery: None,
            music_gallery: None,
            movie_gallery: None,
            pending: None,
            other: Default::default(),
            title_destinations: Default::default(),
            title_demo_request: None,
            continuation: None,
            #[cfg(test)]
            reference_scene_events: Vec::new(),
            #[cfg(test)]
            reference_factory_fault: false,
        }
    }
    pub fn update_title(
        &mut self,
        elapsed: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        ensure!(
            self.pending.is_none(),
            "Dispatch the pending story before the next title frame"
        );
        let delta = crate::adv::higurashi::host::frame_delta(elapsed)?;
        let title = self
            .title
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Title was already transferred"))?;
        title.update_with_input(delta, permission, buttons, pointer)?;
        if let Some((data, flags)) = title.take_manual_load() {
            self.pending = Some(Pending::ManualLoad(manual_load::Transfer::new(data, flags)));
        }
        if let Some((data, flags)) = title.take_chart_adv() {
            self.pending = Some(Pending::ChartAdv(manual_load::Transfer::new(data, flags)));
        }
        if let Some(request) = title.take_category_scene_request() {
            match request {
                CategorySceneRequest::Cg => self.pending = Some(Pending::Cg),
                CategorySceneRequest::Music => self.pending = Some(Pending::Music),
                CategorySceneRequest::Movie => self.pending = Some(Pending::Movie),
                CategorySceneRequest::Story { data, flags } => {
                    self.pending = Some(Pending::Story(data, flags))
                }
            }
        }
        if let Some(crate::app::higurashi::main_title::TitleDestination::Tips {
            selected,
            alternate,
        }) = title.take_tips_destination()
        {
            self.pending = Some(Pending::Tips(super::super::tips_menu::factory::Request {
                data: None,
                selected,
                alternate: u32::from(alternate),
            }));
        }
        // Unsupported destinations stay owned until a concrete runtime accepts
        // them. IdleDemo is the fade-start marker; its later factory is distinct.
        while let Some(destination) = title.take_destination() {
            if let crate::app::higurashi::main_title::TitleDestination::Origin { selected } =
                destination
            {
                self.pending = Some(Pending::Origin(selected));
            } else {
                self.title_destinations.push_back(destination);
            }
        }
        if let Some(request) = title.take_title_demo_request() {
            self.title_demo_request = Some(request);
        }
        title.globals.clock.advance(elapsed.as_f32());
        title.globals.data.play_seconds = title.globals.clock.saved_seconds();
        Ok(())
    }
    pub fn pending_story(&self) -> bool {
        matches!(
            self.pending,
            Some(Pending::Story(..) | Pending::PreparedStory(_))
        )
    }
    /// Poll the concrete ADV and transfer its latest copied browser request.
    /// Dispatch at the next platform boundary before another gameplay update.
    pub async fn update_game(
        &mut self,
        elapsed: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        policy: Policy,
        audio_policy: AudioFramePolicy,
    ) -> Result<Status> {
        ensure!(
            self.pending.is_none(),
            "Dispatch the pending gameplay replacement before updating"
        );
        let game = self
            .game
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No active Higurashi gameplay"))?;
        #[cfg(test)]
        self.reference_scene_events.push(3);
        let status = game
            .update(elapsed, permission, buttons, pointer, policy, audio_policy)
            .await?;
        if let Some(notification) = game
            .host_mut()
            .unwrap()
            .take_browser_reconstruction_request()
        {
            let request = BrowserJump::from_notification(notification)?;
            self.pending = Some(Pending::Browser(Replacement {
                request,
                globals: None,
                bindings: None,
                snapshot: None,
                seeds: None,
                clock: None,
                mode_ids: None,
            }));
        }
        if let Some((data, flags)) = game.take_manual_load() {
            self.pending = Some(Pending::ManualLoad(manual_load::Transfer::new(data, flags)));
        }
        if let Some((data, flags)) = game.take_chart_adv() {
            self.pending = Some(Pending::ChartAdv(manual_load::Transfer::new(data, flags)));
        }
        // The event command queues after mode callbacks. As275b0 does, a
        // later owned factory replaces the earlier pending factory copy.
        if let Some(request) = game.take_kakera_scene_request() {
            self.pending = Some(Pending::Kakera(request));
        }
        if let Some(request) = game.take_tips_scene_request() {
            self.pending = Some(Pending::Tips(request));
        }
        if let Some(request) = game.take_quiz_scene_request() {
            self.pending = Some(Pending::Quiz(request));
        }
        if let Some(request) = game.take_scenario_select_scene_request() {
            self.pending = Some(Pending::ScenarioSelect(request));
        }
        if let Some(request) = game.take_title_scene_request() {
            self.pending = Some(Pending::Title(request));
        }
        if let Some(request) = game.take_bonus_scene_request() {
            self.pending = Some(Pending::BonusAdv(manual_load::Transfer::new(
                request.data,
                request.flags,
            )));
        }
        Ok(status)
    }
    pub fn pending_gameplay_replacement(&self) -> bool {
        matches!(self.pending, Some(Pending::Browser(_)))
    }
    pub fn pending_quiz(&self) -> Option<&super::quiz::SceneRequest> {
        match &self.pending {
            Some(Pending::Quiz(request)) => Some(request),
            _ => None,
        }
    }
    /// Copy the already composed platform frame, retire the whole old Gameplay
    /// and its browser/asset workers, then use the same First resource gate as
    /// story startup. All fallible preflight precedes source retirement; later
    /// constructor errors retain globals/source/snapshot for explicit retry.
    pub fn dispatch_gameplay_replacement(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
    ) -> Result<bool> {
        let Some(Pending::Browser(replacement)) = &mut self.pending else {
            return Ok(false);
        };
        if replacement.globals.is_none() {
            let old = self.game.as_ref().unwrap();
            let mut bindings = old
                .bindings
                .replacement(&old.globals.data, &replacement.request.source)?;
            let entry = bindings.progress.progress().active_entry();
            let fork = CheckpointMemory::new(Vec::new()).fork_for_restore(
                entry.save_position,
                entry.replay_target,
                &bindings.progress,
            )?;
            // Validate the fresh VM/route before removing the playable source.
            ReplaySession::new(old.globals.common.scenario.clone(), fork)?;
            let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
            let mut old = self.game.take().unwrap();
            replacement.mode_ids = Some(old.host().unwrap().next_mode_ids());
            replacement.seeds = Some(old.seeds.clone());
            replacement.clock = Some(std::mem::replace(
                &mut old.clock_source,
                Box::new(|| Ok(shin_core::format::save::higurashi::SaveDate::now())),
            ));
            replacement.snapshot = Some(snapshot);
            let (globals, history, choices, autosave) = old.into_globals_and_histories();
            bindings.history = history;
            bindings.choices = choices;
            bindings.autosave = autosave;
            replacement.bindings = Some(bindings);
            replacement.globals = Some(globals);
        }
        let seeds = replacement.seeds.as_ref().unwrap();
        let runner = Gameplay::prepare_runner(replacement.globals.as_ref().unwrap(), seeds)?;
        let snapshot = replacement
            .snapshot
            .as_ref()
            .unwrap()
            .copy_snapshot(&mut context.render_clone_ctx());
        let mut game = Gameplay::from_prepared(
            replacement.globals.take().unwrap(),
            replacement.bindings.take().unwrap(),
            runner,
            replacement.request.flags,
            snapshot,
            seeds.clone(),
        );
        game.clock_source = replacement.clock.take().unwrap();
        game.host_mut()
            .unwrap()
            .set_next_mode_ids(replacement.mode_ids.take().unwrap());
        self.game = Some(game);
        self.pending = None;
        Ok(true)
    }
    pub fn take_other_scene_request(&mut self) -> Option<CategorySceneRequest> {
        self.other.pop_front()
    }
    /// Consume at the next scene-dispatch boundary; source Gameplay remains
    /// owned until the concrete Kakera factory accepts its global owners.
    pub fn take_kakera_scene_request(&mut self) -> Option<super::kakera::SceneRequest> {
        if matches!(self.pending, Some(Pending::Kakera(_))) {
            let Some(Pending::Kakera(request)) = self.pending.take() else {
                unreachable!()
            };
            Some(request)
        } else {
            self.game.as_mut()?.take_kakera_scene_request()
        }
    }
    /// Call at the next platform boundary,using its last composed frame.
    pub fn dispatch_story(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
        seeds: impl FnMut() -> [u32; 4] + Send + 'static,
    ) -> Result<bool> {
        if !self.pending_story() {
            return Ok(false);
        }
        if let Some(Pending::Story(data, flags)) = &self.pending {
            let source = self
                .title
                .as_ref()
                .context("Story transfer lost its source title")?;
            ensure!(
                source.globals.common.scenario.variant
                    == shin_core::format::scenario::higurashi::HigurashiVariant::Updated,
                "Fresh ADV requires updated Higurashi"
            );
            // Pure validation leaves the old Title playable. Seed consumption
            // and the concrete runner constructor still follow its retirement.
            let mut bindings = Bindings::new(&source.globals.data, data)?;
            if let Some(slots) = &source.globals.runtime_saves {
                bindings.autosave = slots.autosave();
            }
            let flags = *flags;
            let seeds = self
                .continuation
                .as_ref()
                .map(|c| c.seeds.clone())
                .unwrap_or_else(|| Arc::new(Mutex::new(Box::new(seeds))));
            let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
            let globals = self.title.take().unwrap().into_globals();
            self.pending = Some(Pending::PreparedStory(StoryTransfer {
                flags,
                globals: Some(globals),
                bindings: Some(bindings),
                snapshot: Some(snapshot),
                seeds,
            }));
            #[cfg(test)]
            self.reference_scene_events.push(1);
        }
        #[cfg(test)]
        self.reference_check_factory_fault()?;
        let Some(Pending::PreparedStory(transfer)) = &mut self.pending else {
            unreachable!()
        };
        let runner = Gameplay::prepare_runner(transfer.globals.as_ref().unwrap(), &transfer.seeds)?;
        let mut bindings = transfer.bindings.take().unwrap();
        let globals = transfer.globals.take().unwrap();
        let snapshot = transfer.snapshot.take().unwrap();
        let seeds = transfer.seeds.clone();
        let flags = transfer.flags;
        self.game = Some(if let Some(continuation) = self.continuation.take() {
            bindings.history = continuation.history;
            bindings.choices = continuation.choices;
            bindings.autosave = continuation.autosave;
            let mut game =
                Gameplay::from_prepared(globals, bindings, runner, flags, snapshot, seeds);
            game.clock_source = continuation.clock;
            game.host_mut()
                .unwrap()
                .set_next_mode_ids(continuation.mode_ids);
            game
        } else {
            Gameplay::from_prepared(globals, bindings, runner, flags, snapshot, seeds)
        });
        self.pending = None;
        #[cfg(test)]
        self.reference_scene_events.push(2);
        Ok(true)
    }
    pub fn pending_title_return(&self) -> bool {
        matches!(self.pending, Some(Pending::Title(_)))
    }
    pub fn dispatch_title_return(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
    ) -> Result<bool> {
        if !self.pending_title_return() {
            return Ok(false);
        }
        ensure!(
            self.continuation.is_none(),
            "Previous scene continuation was not consumed"
        );
        let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
        let Some(Pending::Title(request)) = self.pending.take() else {
            unreachable!()
        };
        let mut old = self
            .game
            .take()
            .context("Title return lost its source Gameplay")?;
        let seeds = old.seeds.clone();
        let clock = std::mem::replace(
            &mut old.clock_source,
            Box::new(|| Ok(shin_core::format::save::higurashi::SaveDate::now())),
        );
        let mode_ids = old.host().unwrap().next_mode_ids();
        let (globals, history, choices, autosave) = old.into_globals_and_histories();
        #[cfg(test)]
        self.reference_scene_events.push(1);
        self.continuation = Some(Continuation {
            seeds,
            clock,
            history,
            choices,
            autosave,
            mode_ids,
        });
        self.title = Some(Opening::new(
            TitleTransition { request, globals },
            Some(snapshot),
        ));
        #[cfg(test)]
        self.reference_scene_events.push(2);
        Ok(true)
    }
}
