//! Updated14744 factory/preflight and1803c early-return scope in the retained
//! ADV mode stack. Child asset jobs belong to the browser; transport requests
//! retain concrete payloads for the application instead of fabricating effects.
use super::{ConcealOwners, HigurashiHost, HostMode, ModalOwners, ModeServices};
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::{
            browser_controls::{self, Event},
            browser_history::BrowserHistory,
            browser_lifecycle::{
                self, LoadedNavigationView, LoadedQuestion, NavigationViewRequest, Question,
            },
            browser_worker::{Browser, Request},
            choice::ChoiceHistory,
            input::InputState,
            progress::ProgressMemory,
            read_history::MessageReadHistory,
        },
    },
    asset::system::AssetServer,
};
use anyhow::{Result, bail, ensure};
use shin_core::time::Ticks;
use std::sync::Arc;
mod audio;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrowserId(u64);
#[cfg(test)]
impl BrowserId { pub(crate) fn reference_id(value: u64) -> Self { Self(value) } }
#[derive(Clone, Copy)]
pub struct BrowserFrame {
    pub duck_music: bool,
    pub outline_mode: u8,
}
#[derive(Clone, Debug)]
pub struct VoiceFile {
    pub path: String,
    pub volume: f32,
}
#[derive(Clone, Debug)]
pub enum Notice {
    /// Native14744 chooses the enabled/disabled global cue by preflight result.
    Availability(bool),
    ControlCue(crate::adv::higurashi::system_menu::Cue),
    QuestionCue(crate::adv::higurashi::question::Cue),
    Voice {
        row: usize,
        files: Vec<VoiceFile>,
    },
    StopVoice,
    VoiceTick,
    MusicMultiplier(f32),
    /// Original275b0 queues a copied GameData closure; it does not synchronously
    /// reconstruct, release the modal scope or complete the browser.
    Reconstruct {
        source: ProgressMemory,
        mode: u32,
    },
}
#[derive(Clone, Debug)]
pub struct Notification {
    pub browser: BrowserId,
    pub notice: Notice,
}

enum Child {
    Navigation(LoadedNavigationView),
    Question(LoadedQuestion),
}
#[derive(Clone, Copy)]
enum ChildId {
    Navigation(u64),
    Question(u64),
}
struct Job {
    id: ChildId,
    task: Option<shin_tasks::AsyncTask<Result<Child>>>,
    retirement: Option<shin_tasks::LoaderRetirement>,
}
impl Job {
    fn start(
        id: ChildId,
        future: impl std::future::Future<Output = Result<Child>> + Send + 'static,
    ) -> Self {
        let task = shin_tasks::async_io::spawn_loader(future);
        let retirement = task.retirement_handle().unwrap();
        Self {
            id,
            task: Some(task),
            retirement: Some(retirement),
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(task) = self.task.take() {
                task.cancel_and_retire();
            }
            if let Some(retirement) = self.retirement.take() {
                retirement.retire(());
            }
        }
    }
}
pub(super) struct BrowserMode {
    // Drop joins core history/atlas jobs before any retained child job/view dies.
    pub browser: Browser,
    assets: Arc<AssetServer>,
    system: Arc<CharacterTextures>,
    id: BrowserId,
    jobs: Vec<Job>,
    scope_active: bool,
    voice_playing: bool,
    audio: Option<audio::BrowserAudio>,
    fault: Option<String>,
}
impl BrowserMode {
    pub(super) fn cancel_input(&mut self) {
        if let Some(lifecycle) = self.browser.lifecycle_mut() { lifecycle.cancel_input(); }
    }
    fn publish(&mut self) -> Result<()> {
        for job in &mut self.jobs {
            let Some(result) = job.task.as_mut().and_then(|t| t.poll_naive()) else {
                continue;
            };
            job.task = None;
            let child = result?;
            let Some(lifecycle) = self.browser.lifecycle_mut() else {
                bail!("Browser child completed without lifecycle")
            };
            let wanted = match job.id {
                ChildId::Navigation(id) => lifecycle.wants_navigation_view(id),
                ChildId::Question(id) => lifecycle.wants_question_view(id),
            };
            if wanted {
                match child {
                    Child::Navigation(view) => lifecycle.publish_navigation_view(view)?,
                    Child::Question(view) => lifecycle.publish_question(view)?,
                }
            }
            // Expired loaded values retire here; completed job scopes remain
            // owned until browser drop, including tasks whose result was consumed.
        }
        Ok(())
    }
    pub(super) fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        services: &mut ModeServices<'_>,
    ) -> Result<u32> {
        if let Some(fault) = &self.fault {
            bail!("Higurashi browser failed: {fault}");
        }
        if let Err(error) = self.publish() {
            self.fault = Some(format!("{error:#}"));
            return Err(error);
        }
        let empty = crate::adv::input_area::PointerFrame {
            events: &[],
            number: 0,
            time_seconds: 0.0,
        };
        let pointer = services.pointer.unwrap_or(&empty);
        let buttons = services.buttons;
        let mouse_release = services.mouse_release;
        let mut events = Vec::new();
        let rows = self.browser.history_rows();
        let mut callbacks = Callbacks {
            id: self.id,
            assets: self.assets.clone(),
            jobs: &mut self.jobs,
            scope_active: &mut self.scope_active,
            voice_playing: self.voice_playing,
            audio: self.audio.as_mut(),
            rows,
            events: &mut events,
            services,
        };
        let result = self.browser.poll_with_mouse_release(
            delta,
            u32::from(permission),
            buttons.pressed,
            buttons.repeat,
            pointer,
            callbacks.services.browser_frame.duck_music,
            &mut callbacks,
            mouse_release,
        );
        if let Err(error) = result {
            self.fault = Some(format!("{error:#}"));
            return Err(error);
        }
        for event in events {
            if self.audio.is_some() {
                continue; // Audio events have already executed, in native order.
            }
            let notice = match event {
                Event::Cue(cue) => Some(Notice::ControlCue(cue)),
                Event::Voice(row) => Some(Notice::Voice {
                    row,
                    files: self
                        .browser
                        .history_rows()
                        .and_then(|rows| rows.get(row).cloned())
                        .and_then(|row| match row.item {
                            crate::adv::higurashi::browser_history::Item::Message {
                                text, ..
                            } => Some(audio::files(&text)),
                            _ => None,
                        })
                        .unwrap_or_default(),
                }),
                Event::StopVoice => Some(Notice::StopVoice),
                Event::VoiceTick => Some(Notice::VoiceTick),
                Event::MusicMultiplier(value) => Some(Notice::MusicMultiplier(value)),
                Event::Close | Event::Jump(_) | Event::Navigation => None, // Lifecycle consumed these actions.
            };
            if let Some(notice) = notice {
                services.browser_notifications.push(Notification {
                    browser: self.id,
                    notice,
                });
            }
        }
        self.browser.prepare(services.browser_frame.outline_mode)?;
        Ok(0) // Both opening and completion frame consume lower ADV permission.
    }
    pub(super) fn completion(&self) -> u32 {
        self.browser.lifecycle().map_or(0, |b| b.completion_word())
    }
    pub(super) fn prepare(&mut self, context: &mut crate::render::PreRenderContext) {
        self.browser.prepare_questions(context);
    }
    pub(super) fn render(&mut self, pass: &mut shin_render::render_pass::RenderPass) {
        self.browser.render(pass, &self.system);
    }
}
struct Callbacks<'a, 'b> {
    id: BrowserId,
    assets: Arc<AssetServer>,
    jobs: &'a mut Vec<Job>,
    scope_active: &'a mut bool,
    voice_playing: bool,
    audio: Option<&'a mut audio::BrowserAudio>,
    rows: Option<Arc<[crate::adv::higurashi::browser_history::Row]>>,
    events: &'a mut Vec<Event>,
    services: &'a mut ModeServices<'b>,
}
impl browser_controls::Services for Callbacks<'_, '_> {
    fn event(&mut self, event: Event) {
        if let Some(audio) = &mut self.audio {
            match event {
                Event::Cue(cue) => audio.cue(match cue {
                    crate::adv::higurashi::system_menu::Cue::Move => "move",
                    crate::adv::higurashi::system_menu::Cue::Cancel => "cancel",
                    crate::adv::higurashi::system_menu::Cue::Confirm => "decide",
                }),
                Event::Voice(row) => {
                    if let Some(crate::adv::higurashi::browser_history::Row {
                        item: crate::adv::higurashi::browser_history::Item::Message { text, .. },
                        ..
                    }) = self.rows.as_deref().and_then(|rows| rows.get(row))
                    {
                        audio.prepare(audio::files(text));
                    }
                }
                Event::StopVoice => audio.stop(),
                Event::VoiceTick => audio.update(),
                Event::MusicMultiplier(value) => {
                    self.services.audio.browser_music_multiplier(value)
                }
                Event::Close | Event::Jump(_) | Event::Navigation => {}
            }
        }
        self.events.push(event);
    }
    fn voice_is_playing(&self) -> bool {
        self.audio
            .as_ref()
            .map_or(self.voice_playing, |audio| audio.playing())
    }
}
impl browser_lifecycle::Services for Callbacks<'_, '_> {
    fn returned(&mut self, complete: bool) {
        if !complete && *self.scope_active {
            self.services.modal.exit(&mut ModalOwners {
                conceal: self.services.conceal,
                owners: ConcealOwners {
                    overlay: self.services.touch_overlay,
                    runner: self.services.runner,
                    ui: self.services.ui,
                    overlay_visibility: self.services.overlay_visibility,
                },
                audio: self.services.audio,
            });
            *self.scope_active = false;
        }
    }
    fn question(&mut self, request: Question) {
        let assets = self.assets.clone();
        self.jobs
            .push(Job::start(ChildId::Question(request.id), async move {
                request.load(&assets).await.map(Child::Question)
            }));
    }
    fn question_cue(&mut self, cue: crate::adv::higurashi::question::Cue) {
        if let Some(audio) = &mut self.audio {
            audio.cue(match cue {
                crate::adv::higurashi::question::Cue::Move => "move",
                crate::adv::higurashi::question::Cue::Cancel => "cancel",
                crate::adv::higurashi::question::Cue::Confirm => "decide",
                crate::adv::higurashi::question::Cue::NoSelection => "error",
            });
            return;
        }
        self.services.browser_notifications.push(Notification {
            browser: self.id,
            notice: Notice::QuestionCue(cue),
        });
    }
    fn navigation_view(&mut self, request: NavigationViewRequest) {
        let assets = self.assets.clone();
        self.jobs
            .push(Job::start(ChildId::Navigation(request.id), async move {
                request.load(&assets).await.map(Child::Navigation)
            }));
    }
    fn reconstruct(&mut self, source: &ProgressMemory, mode: u32) -> Result<()> {
        self.services.browser_notifications.push(Notification {
            browser: self.id,
            notice: Notice::Reconstruct {
                source: source.clone(),
                mode,
            },
        });
        Ok(())
    }
}
impl HigurashiHost {
    /// Bind application-owned global cues and the browser's independent voice
    /// bus before opening. An unbound diagnostic host retains audio requests.
    pub fn configure_browser_audio(
        &mut self,
        sounds: super::system_menu::GlobalSounds,
        voice_volume: u8,
    ) -> Result<()> {
        ensure!(!self.has_modes(), "Configure browser audio before opening");
        let bank: Arc<shin_core::format::sysse::SysSe> =
            self.runner.assets().load_sync("/sysse.bin")?;
        self.configure_browser_audio_from_bank(sounds, voice_volume, bank)
    }
    /// The concrete gameplay owner already retains the startup sound bank.
    pub fn configure_browser_audio_from_bank(
        &mut self,
        sounds: super::system_menu::GlobalSounds,
        voice_volume: u8,
        bank: Arc<shin_core::format::sysse::SysSe>,
    ) -> Result<()> {
        ensure!(!self.has_modes(), "Configure browser audio before opening");
        for name in ["page", "error", "move", "cancel", "decide"] {
            ensure!(
                bank.sounds.contains_key(name),
                "Missing browser sound {name}"
            );
        }
        sounds.lock().bank = Some(bank);
        self.browser_audio = Some((sounds, voice_volume));
        Ok(())
    }
    pub fn set_browser_frame(&mut self, frame: BrowserFrame) -> Result<()> {
        ensure!(frame.outline_mode <= 3, "Unsupported browser outline");
        self.browser_frame = frame;
        Ok(())
    }
    /// The live GameData/settings snapshots are application-owned, as for the
    /// runner's progress/checkpoint services. The scenario comes from this ADV.
    /// False preflight does not capture or mutate input/pause/conceal state.
    pub async fn open_message_browser(
        &mut self,
        input: &mut InputState,
        source: &ProgressMemory,
        history: &MessageReadHistory,
        choices: &ChoiceHistory,
        read_only: bool,
        command_limit: usize,
        context: &mut crate::render::PreRenderContext<'_, '_, '_, '_>,
        screens: &crate::adv::higurashi::scene_textures::HigurashiSceneTextures,
    ) -> Result<Option<BrowserId>> {
        if !self.browser_available(source, history, choices, read_only, command_limit)? {
            return Ok(None);
        }
        let system = self.runner.assets().load("/systex.txa").await?;
        self.attach_message_browser(
            input, source, history, choices, read_only, command_limit, system, context, screens,
        ).map(Some)
    }
    /// Reuse Startup's system atlas, while retaining the browser's original
    /// history/layout workers and false-preflight input/modal behavior.
    pub fn open_message_browser_from_resources(
        &mut self,
        input: &mut InputState,
        source: &ProgressMemory,
        history: &MessageReadHistory,
        choices: &ChoiceHistory,
        read_only: bool,
        command_limit: usize,
        system: Arc<CharacterTextures>,
        context: &mut crate::render::PreRenderContext<'_, '_, '_, '_>,
        screens: &crate::adv::higurashi::scene_textures::HigurashiSceneTextures,
    ) -> Result<Option<BrowserId>> {
        if !self.browser_available(source, history, choices, read_only, command_limit)? {
            return Ok(None);
        }
        self.attach_message_browser(
            input, source, history, choices, read_only, command_limit, system, context, screens,
        ).map(Some)
    }
    fn browser_available(
        &mut self,
        source: &ProgressMemory,
        history: &MessageReadHistory,
        choices: &ChoiceHistory,
        read_only: bool,
        command_limit: usize,
    ) -> Result<bool> {
        ensure!(
            !self.has_modes(),
            "MessageBrowser requires an idle mode stack"
        );
        ensure!(
            self.runner.reconstruction_phase().is_none(),
            "MessageBrowser cannot open during reconstruction"
        );
        let scenario = self.runner.scenario().clone();
        if !BrowserHistory::available(
            &scenario,
            source,
            history,
            choices,
            read_only,
            command_limit,
        )? {
            if let Some((sounds, _)) = &self.browser_audio {
                let mut sounds = sounds.lock();
                sounds.update();
                sounds.play(
                    self.runner.assets(),
                    "error",
                    shin_core::vm::command::types::Volume(1.0),
                );
            }
            self.browser_notifications.push(Notification {
                browser: BrowserId(self.next_browser),
                notice: Notice::Availability(false),
            });
            return Ok(false);
        }
        Ok(true)
    }
    fn attach_message_browser(
        &mut self,
        input: &mut InputState,
        source: &ProgressMemory,
        history: &MessageReadHistory,
        choices: &ChoiceHistory,
        read_only: bool,
        command_limit: usize,
        system: Arc<CharacterTextures>,
        context: &mut crate::render::PreRenderContext<'_, '_, '_, '_>,
        screens: &crate::adv::higurashi::scene_textures::HigurashiSceneTextures,
    ) -> Result<BrowserId> {
        let scenario = self.runner.scenario().clone();
        let id = BrowserId(self.next_browser);
        let next = self
            .next_browser
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Browser IDs exhausted"))?;
        let assets = self.runner.assets().clone();
        ensure!(system.contains("scroll"), "System atlas has no scroll");
        let browser = Browser::start(
            assets.clone(),
            Request {
                scenario,
                source: source.clone(),
                read_history: history.clone(),
                choices: choices.clone(),
                read_only,
                command_limit,
            },
        );
        ensure!(
            screens.modal_capture_ready(),
            "Higurashi screen cache has not been prepared"
        );
        let audio = self.browser_audio.as_ref().map(|(sounds, volume)| {
            audio::BrowserAudio::new(
                self.audio.browser_voice_player(),
                sounds.clone(),
                assets.clone(),
                *volume,
            )
        });
        if let Some(audio) = &audio {
            audio
                .sounds
                .lock()
                .play(&assets, "page", shin_core::vm::command::types::Volume(1.0));
        }
        let snapshot = screens.capture_modal(context)?;
        self.enter_modal(|_| Ok(snapshot))?;
        input.modes = 0;
        input.temporary_indicator = 0;
        self.next_browser = next;
        self.browser_notifications.push(Notification {
            browser: id,
            notice: Notice::Availability(true),
        });
        self.modes.push(HostMode::Browser(BrowserMode {
            browser,
            assets,
            system,
            id,
            jobs: Vec::new(),
            scope_active: true,
            voice_playing: false,
            audio,
            fault: None,
        }));
        self.mode_input_permission = false;
        Ok(id)
    }
    /// Configured audio executes during polling and is not emitted again.
    /// Availability is diagnostic; reconstruction retains its owned request.
    /// Draining never completes a browser or acknowledges reconstruction.
    pub fn take_browser_notifications(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.browser_notifications)
    }
    /// Transfer only scene-constructor requests to the owning application.
    /// Native275b0 keeps the latest request. Diagnostic availability/audio
    /// notices stay retained, and this operation does not release the browser.
    pub fn take_browser_reconstruction_request(&mut self) -> Option<Notification> {
        let mut latest = None;
        self.browser_notifications.retain(|notification| {
            if matches!(notification.notice, Notice::Reconstruct { .. }) {
                latest = Some(notification.clone());
                false
            } else {
                true
            }
        });
        latest
    }
    pub fn set_browser_voice_playing(&mut self, id: BrowserId, playing: bool) -> Result<()> {
        let owner = self
            .modes
            .iter_mut()
            .find_map(|m| match m {
                HostMode::Browser(owner) if owner.id == id => Some(owner),
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("Expired browser voice status"))?;
        ensure!(
            owner.audio.is_none(),
            "Configured browser reads its actual voice status"
        );
        owner.voice_playing = playing;
        Ok(())
    }
    pub fn prepare_mode_views(&mut self, context: &mut crate::render::PreRenderContext) {
        self.modes.draw(|m| {
            if let HostMode::Browser(b) = m {
                b.prepare(context);
            }
            if let HostMode::ChapterQuestion(question) = m { question.prepare(context); }
        });
    }
    pub(crate) fn touch_diagnostic_browser_ready(&self) -> bool {
        self.modes.iter().any(|mode| match mode {
            HostMode::Browser(owner) => owner
                .browser
                .lifecycle()
                .is_some_and(|life| life.input_ready()),
            _ => false,
        })
    }
    #[cfg(test)]
    pub(crate) fn reference_browser_ready(&self) -> bool {
        self.modes.iter().any(|mode| match mode {
            HostMode::Browser(owner) => owner.browser.lifecycle().is_some_and(|life| life.reference_input_ready()),
            _ => false,
        })
    }
    #[cfg(test)]
    pub(crate) fn reference_browser_lifetime(&self) -> Option<(Arc<std::sync::atomic::AtomicBool>, Vec<shin_tasks::LoaderRetirement>)> {
        self.modes.iter().find_map(|mode| match mode {
            HostMode::Browser(owner) => Some(owner.browser.reference_worker_lifetime()),
            _ => None,
        })
    }
    #[cfg(test)]
    pub(crate) fn reference_browser_selected(&self) -> Option<usize> {
        self.modes.iter().find_map(|mode| match mode {
            HostMode::Browser(owner) => owner.browser.lifecycle().map(|life| life.controls().selected()),
            _ => None,
        })
    }
    #[cfg(test)]
    pub(crate) fn reference_browser_question_ready(&mut self) -> bool {
        self.modes.iter_mut().any(|mode| match mode {
            HostMode::Browser(owner) => owner.browser.lifecycle_mut().is_some_and(|life| {
                let mut ids = Vec::new();
                life.draw_questions(|id| ids.push(id));
                !ids.is_empty() && ids.iter().all(|id| !life.wants_question_view(*id))
            }),
            _ => false,
        })
    }
}
impl Default for BrowserFrame {
    fn default() -> Self {
        Self {
            duck_music: false,
            outline_mode: 0,
        }
    }
}

#[cfg(test)]
mod tests;
