//! Active retained scenes and resources for the normal application adapter.
//! Scene updates own native clamped delta/full PlayClock; these getters do not
//! update, prepare ADV children, retire factories, or acknowledge modal requests.
use super::*;
use crate::app::higurashi::main_title::TitleDestination;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app::higurashi) enum ActiveScene {
    Title,
    Gameplay,
    Kakera,
    Tips,
    Quiz,
    ScenarioSelect,
    CgGallery,
    MusicGallery,
    MovieGallery,
    /// A fallible constructor retired its source and still retains its payload.
    RetiredFactory,
}
pub(in crate::app::higurashi) struct ActiveGlobals<'a> {
    pub common: &'a Arc<CommonAssets>,
    pub data: &'a SaveData,
    pub clock: &'a PlayClock,
    pub platform_flag: bool,
    #[cfg(test)]
    pub last_title_mode: i32,
    pub storage: Option<&'a Arc<dyn crate::app::higurashi::load_game::Storage>>,
    pub runtime_saves: Option<&'a crate::app::higurashi::runtime_saves::Slots>,
    pub server: &'a Arc<AssetServer>,
    #[cfg(test)]
    pub manager: &'a Arc<shin_audio::AudioManager>,
    #[cfg(test)]
    pub sounds: &'a Arc<Mutex<SystemSoundPlayer>>,
}
#[cfg(test)]
#[allow(dead_code, reason = "Mutable native scene-state projection for direct save/restoration fixture access.")]
pub(in crate::app::higurashi) struct ActiveGlobalsMut<'a> {
    pub data: &'a mut SaveData,
    pub clock: &'a mut PlayClock,
    pub platform_flag: &'a mut bool,
    pub last_title_mode: &'a mut i32,
    pub storage: &'a mut Option<Arc<dyn crate::app::higurashi::load_game::Storage>>,
    pub runtime_saves: &'a mut Option<crate::app::higurashi::runtime_saves::Slots>,
}
impl<'a> From<&'a Globals> for ActiveGlobals<'a> {
    fn from(g: &'a Globals) -> Self {
        Self {
            common: &g.common,
            data: &g.data,
            clock: &g.clock,
            platform_flag: g.platform_flag,
            #[cfg(test)]
            last_title_mode: g.last_title_mode,
            storage: g.storage.as_ref(),
            runtime_saves: g.runtime_saves.as_ref(),
            server: &g.server,
            #[cfg(test)]
            manager: &g.manager,
            #[cfg(test)]
            sounds: &g.sounds,
        }
    }
}
impl<'a> From<&'a Metadata> for ActiveGlobals<'a> {
    fn from(g: &'a Metadata) -> Self {
        Self {
            common: &g.common,
            data: &g.data,
            clock: &g.clock,
            platform_flag: g.platform_flag,
            #[cfg(test)]
            last_title_mode: g.last_title_mode,
            storage: g.storage.as_ref(),
            runtime_saves: g.runtime_saves.as_ref(),
            server: &g.server,
            #[cfg(test)]
            manager: &g.manager,
            #[cfg(test)]
            sounds: &g.sounds,
        }
    }
}
#[cfg(test)]
impl<'a> From<&'a mut Globals> for ActiveGlobalsMut<'a> {
    fn from(g: &'a mut Globals) -> Self {
        Self {
            data: &mut g.data,
            clock: &mut g.clock,
            platform_flag: &mut g.platform_flag,
            #[cfg(test)]
            last_title_mode: &mut g.last_title_mode,
            storage: &mut g.storage,
            runtime_saves: &mut g.runtime_saves,
        }
    }
}
#[cfg(test)]
impl<'a> From<&'a mut Metadata> for ActiveGlobalsMut<'a> {
    fn from(g: &'a mut Metadata) -> Self {
        Self {
            data: &mut g.data,
            clock: &mut g.clock,
            platform_flag: &mut g.platform_flag,
            #[cfg(test)]
            last_title_mode: &mut g.last_title_mode,
            storage: &mut g.storage,
            runtime_saves: &mut g.runtime_saves,
        }
    }
}
pub(in crate::app::higurashi) struct TitleDemoTransfer {
    pub request: crate::app::higurashi::startup::TitleDemoRequest,
    pub globals: Globals,
    pub capture: RenderTexture,
}
impl TitleSlot {
    pub fn active_scene(&self) -> Result<ActiveScene> {
        let owners = [
            (self.title.is_some(), ActiveScene::Title),
            (self.game.is_some(), ActiveScene::Gameplay),
            (self.kakera.is_some(), ActiveScene::Kakera),
            (self.tips.is_some(), ActiveScene::Tips),
            (self.quiz.is_some(), ActiveScene::Quiz),
            (self.scenario_select.is_some(), ActiveScene::ScenarioSelect),
            (self.cg_gallery.is_some(), ActiveScene::CgGallery),
            (self.music_gallery.is_some(), ActiveScene::MusicGallery),
            (self.movie_gallery.is_some(), ActiveScene::MovieGallery),
        ];
        let count = owners.iter().filter(|(live, _)| *live).count();
        ensure!(
            count <= 1,
            "Higurashi active scene has multiple concrete owners"
        );
        if let Some((_, kind)) = owners.into_iter().find(|(live, _)| *live) {
            return Ok(kind);
        }
        ensure!(
            self.pending.is_some(),
            "Higurashi active scene has no retained owner"
        );
        Ok(ActiveScene::RetiredFactory)
    }
    pub(super) fn retained_globals(&self) -> Option<&Globals> {
        self.title
            .as_ref()
            .map(|o| &o.globals)
            .or_else(|| self.kakera.as_ref().map(|o| &o.globals))
            .or_else(|| self.tips.as_ref().map(|o| &o.globals))
            .or_else(|| self.quiz.as_ref().map(|o| &o.globals))
            .or_else(|| self.scenario_select.as_ref().map(|o| &o.globals))
            .or_else(|| self.cg_gallery.as_ref().map(|o| &o.globals))
            .or_else(|| self.music_gallery.as_ref().map(|o| &o.globals))
            .or_else(|| self.movie_gallery.as_ref().map(|o| &o.globals))
            .or_else(|| match &self.pending {
                Some(Pending::Browser(r)) => r.globals.as_ref(),
                Some(Pending::Gameplay(r)) => r.globals.as_ref(),
                Some(Pending::ManualLoad(r) | Pending::ChartAdv(r) | Pending::BonusAdv(r)) => {
                    r.globals.as_ref()
                }
                Some(Pending::PreparedStory(r)) => r.globals.as_ref(),
                Some(Pending::PreparedTips(r)) => r.globals.as_ref(),
                Some(Pending::PreparedScenarioSelect(r)) => r.globals.as_ref(),
                Some(Pending::PreparedCg(r)) => r.globals.as_ref(),
                Some(Pending::PreparedMusic(r)) => r.globals.as_ref(),
                Some(Pending::PreparedMovie(r)) => r.globals.as_ref(),
                _ => None,
            })
    }
    #[cfg(test)]
    pub(super) fn retained_globals_mut(&mut self) -> Option<&mut Globals> {
        if let Some(o) = &mut self.title {
            return Some(&mut o.globals);
        }
        if let Some(o) = &mut self.kakera {
            return Some(&mut o.globals);
        }
        if let Some(o) = &mut self.tips {
            return Some(&mut o.globals);
        }
        if let Some(o) = &mut self.quiz {
            return Some(&mut o.globals);
        }
        if let Some(o) = &mut self.scenario_select {
            return Some(&mut o.globals);
        }
        if let Some(o) = &mut self.cg_gallery {
            return Some(&mut o.globals);
        }
        if let Some(o) = &mut self.music_gallery {
            return Some(&mut o.globals);
        }
        if let Some(o) = &mut self.movie_gallery {
            return Some(&mut o.globals);
        }
        match &mut self.pending {
            Some(Pending::Browser(r)) => r.globals.as_mut(),
            Some(Pending::Gameplay(r)) => r.globals.as_mut(),
            Some(Pending::ManualLoad(r) | Pending::ChartAdv(r) | Pending::BonusAdv(r)) => {
                r.globals.as_mut()
            }
            Some(Pending::PreparedStory(r)) => r.globals.as_mut(),
            Some(Pending::PreparedTips(r)) => r.globals.as_mut(),
            Some(Pending::PreparedScenarioSelect(r)) => r.globals.as_mut(),
            Some(Pending::PreparedCg(r)) => r.globals.as_mut(),
            Some(Pending::PreparedMusic(r)) => r.globals.as_mut(),
            Some(Pending::PreparedMovie(r)) => r.globals.as_mut(),
            _ => None,
        }
    }
    pub fn globals(&self) -> Result<ActiveGlobals<'_>> {
        if let Some(game) = &self.game {
            return Ok((&game.globals).into());
        }
        self.retained_globals()
            .map(ActiveGlobals::from)
            .context("Scene factory has no retained global owner")
    }
    #[cfg(test)]
    pub fn globals_mut(&mut self) -> Result<ActiveGlobalsMut<'_>> {
        if self.game.is_some() {
            return Ok((&mut self.game.as_mut().unwrap().globals).into());
        }
        self.retained_globals_mut()
            .map(ActiveGlobalsMut::from)
            .context("Scene factory has no retained global owner")
    }
    pub fn active_globals(&self) -> Result<ActiveGlobals<'_>> {
        self.globals()
    }
    #[cfg(test)]
    pub fn active_globals_mut(&mut self) -> Result<ActiveGlobalsMut<'_>> {
        self.globals_mut()
    }
    /// Immutable writer snapshot only. Preserve uncommitted VM queues; merge
    /// already committed globals/full runtime rows without retiring the source.
    /// The caller owns readiness/quiescence of live SaveGame/preview writers.
    #[cfg(test)]
    pub fn save_snapshot(&self) -> Result<SaveData> {
        let globals = self.globals()?;
        let mut data = globals.data.clone();
        data.play_seconds = globals.clock.saved_seconds();
        let mut slots = globals.runtime_saves.cloned();
        if let Some(game) = &self.game {
            game.bindings.publish_globals(&mut data);
            slots
                .get_or_insert_with(|| crate::app::higurashi::runtime_saves::Slots::loaded(&data))
                .update_auto(&game.bindings.autosave);
        }
        Ok(match slots {
            Some(slots) => slots.disk(&data),
            None => data,
        })
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) -> Result<()> {
        match self.active_scene()? {
            ActiveScene::Title => self.title.as_mut().unwrap().prepare(context),
            ActiveScene::Gameplay => self.game.as_mut().unwrap().prepare(context),
            ActiveScene::Kakera => self.kakera.as_mut().unwrap().prepare(context),
            ActiveScene::Tips => self.tips.as_mut().unwrap().prepare(context),
            ActiveScene::Quiz => self.quiz.as_mut().unwrap().prepare(context),
            ActiveScene::ScenarioSelect => self.scenario_select.as_mut().unwrap().prepare(context),
            ActiveScene::CgGallery => self.cg_gallery.as_mut().unwrap().prepare(context),
            ActiveScene::MusicGallery => self.music_gallery.as_mut().unwrap().prepare(context),
            ActiveScene::MovieGallery => self.movie_gallery.as_mut().unwrap().prepare(context),
            ActiveScene::RetiredFactory => {
                bail!("Retry the retained factory before preparing its retired source")
            }
        }
    }
    /// Entire active platform scene, including menus/Question/First and55548.
    /// Gameplay::render samples its eager retained ADV cache; it never redraws
    /// child1/child2 merely because this output getter is called.
    pub fn render(&mut self, pass: &mut RenderPass) -> Result<()> {
        let active = self.active_scene()?;
        if matches!(
            active,
            ActiveScene::Kakera
                | ActiveScene::Tips
                | ActiveScene::Quiz
                | ActiveScene::ScenarioSelect
                | ActiveScene::CgGallery
                | ActiveScene::MusicGallery
                | ActiveScene::MovieGallery
        ) {
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.),
            );
        }
        match active {
            ActiveScene::Title => {
                self.title.as_ref().unwrap().render(pass);
                Ok(())
            }
            ActiveScene::Gameplay => self.game.as_mut().unwrap().render(pass),
            ActiveScene::Kakera => {
                self.kakera.as_ref().unwrap().render(pass);
                Ok(())
            }
            ActiveScene::Tips => {
                self.tips.as_ref().unwrap().render(pass);
                Ok(())
            }
            ActiveScene::Quiz => self.quiz.as_ref().unwrap().render(pass),
            ActiveScene::ScenarioSelect => self.scenario_select.as_ref().unwrap().render(pass),
            ActiveScene::CgGallery => self.cg_gallery.as_ref().unwrap().render(pass),
            ActiveScene::MusicGallery => self.music_gallery.as_ref().unwrap().render(pass),
            ActiveScene::MovieGallery => self.movie_gallery.as_ref().unwrap().render(pass),
            ActiveScene::RetiredFactory => {
                bail!("Render the last composed capture while its constructor is faulted")
            }
        }
    }
    /// Copy a runtime-owned whole-scene composition. No implicit preparation,
    /// cached-ADV update, UI draw, clock advancement or publication happens here.
    pub fn capture_composed(
        &self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
    ) -> RenderTexture {
        composed.copy_snapshot(&mut context.render_clone_ctx())
    }
    pub fn cancel_input(&mut self) -> Result<()> {
        match self.active_scene()? {
            ActiveScene::Title => self.title.as_mut().unwrap().cancel_input(),
            ActiveScene::Gameplay => self.game.as_mut().unwrap().cancel_input(),
            ActiveScene::Kakera => self.kakera.as_mut().unwrap().cancel_input(),
            ActiveScene::Tips => self.tips.as_mut().unwrap().cancel_input(),
            ActiveScene::Quiz => self.quiz.as_mut().unwrap().cancel_input(),
            ActiveScene::ScenarioSelect => self.scenario_select.as_mut().unwrap().cancel_input(),
            ActiveScene::CgGallery => self.cg_gallery.as_mut().unwrap().cancel_input(),
            ActiveScene::MusicGallery => self.music_gallery.as_mut().unwrap().cancel_input(),
            ActiveScene::MovieGallery => self.movie_gallery.as_mut().unwrap().cancel_input(),
            ActiveScene::RetiredFactory => Ok(()),
        }
    }
    pub fn take_input_flush(&mut self) -> bool {
        self.game.as_mut().is_some_and(Gameplay::take_input_flush)
    }
    pub fn has_save_task(&self) -> bool {
        self.game.as_ref().is_some_and(Gameplay::has_save_task)
            || self.title.as_ref().is_some_and(Opening::has_save_task)
            || self.scenario_select.as_ref().is_some_and(
                crate::app::higurashi::scenario_select::opening::Opening::has_save_task,
            )
    }
    pub fn pending_scene_factory(&self) -> bool {
        self.pending.is_some() || self.title_demo_request.is_some()
    }
    #[cfg(test)]
    pub fn title_destination(&self) -> Option<&TitleDestination> {
        self.title_destinations.front()
    }
    #[cfg(test)]
    pub fn title_demo_request(&self) -> Option<&crate::app::higurashi::startup::TitleDemoRequest> {
        self.title_demo_request.as_ref()
    }
    #[cfg(test)]
    pub fn pending_title_demo(&self) -> Option<&crate::app::higurashi::startup::TitleDemoRequest> {
        self.title_demo_request()
    }
    /// TitleDemo accepts only its later owned factory, not the earlier idle
    /// fade marker. Leave this vacant Slot alive if its continuation is needed
    /// when Demo returns; install_title reuses that owner without resetting it.
    pub fn take_title_demo_transition(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
    ) -> Result<Option<TitleDemoTransfer>> {
        if self.title_demo_request.is_none() {
            return Ok(None);
        }
        ensure!(
            self.pending.is_none(),
            "TitleDemo overlaps an owned scene factory"
        );
        ensure!(
            self.active_scene()? == ActiveScene::Title,
            "TitleDemo lost its title source"
        );
        let capture = self.capture_composed(context, composed);
        let request = self.title_demo_request.take().unwrap();
        let globals = self.title.take().unwrap().into_globals();
        self.title_destinations
            .retain(|d| *d != TitleDestination::IdleDemo);
        Ok(Some(TitleDemoTransfer {
            request,
            globals,
            capture,
        }))
    }
    pub fn install_title(
        &mut self,
        transition: TitleTransition,
        capture: Option<RenderTexture>,
    ) -> Result<()> {
        ensure!(
            self.title.is_none()
                && self.game.is_none()
                && self.kakera.is_none()
                && self.tips.is_none()
                && self.quiz.is_none()
                && self.scenario_select.is_none()
                && self.cg_gallery.is_none()
                && self.music_gallery.is_none()
                && self.movie_gallery.is_none()
                && self.pending.is_none(),
            "Title installation requires a retired source Slot"
        );
        ensure!(
            self.title_demo_request.is_none(),
            "Previous TitleDemo factory was not accepted"
        );
        self.title = Some(Opening::new(transition, capture));
        Ok(())
    }
}
