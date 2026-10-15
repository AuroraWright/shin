//! Retained ADV/audio host frame, with native platform elapsed-time clamp.
//! Input/state/platform services remain explicit caller responsibilities.
use super::{
    audio::{AudioFramePolicy, HigurashiAudio},
    hidden_window::{ConcealServices, ConcealState, HiddenWindow},
    input::{self, Buttons, Context, InputState, Request, Services},
    mode_stack::{Mode, ModePush, ModeStack},
    read_state::{ReadPolicy, ReadServices},
    reconstruction_mode::Phase,
    runner::{HigurashiRunner, Policy, Status},
};
use crate::adv::input_area::PointerFrame;
use anyhow::{Context as _, Result, ensure};
use shin_core::time::Ticks;

pub mod browser;
pub mod navigation;
pub mod scene;
pub mod system_menu;

pub struct HigurashiHost {
    pub runner: HigurashiRunner,
    pub audio: HigurashiAudio,
    pub ui: super::ui_state::HigurashiUiState,
    graphics: Option<super::frame_renderer::FrameRenderer>,
    modes: ModeStack<HostMode>,
    mode_input_permission: bool,
    root_touch: super::touch::RootTouch,
    touch_overlay: Option<Box<dyn super::touch::Overlay>>,
    conceal: ConcealState,
    overlay_visibility: Vec<bool>,
    modal: super::modal::ModalState,
    modal_backdrop: Option<shin_render::render_texture::RenderTexture>,
    menu_frame: system_menu::MenuFrame,
    system_sounds: Option<system_menu::GlobalSounds>,
    menu_requests: Vec<system_menu::ChildRequest>,
    next_menu_child: u64,
    browser_frame: browser::BrowserFrame,
    browser_notifications: Vec<browser::Notification>,
    next_browser: u64,
    browser_audio: Option<(system_menu::GlobalSounds, u8)>,
    navigation_disabled: bool,
    chapter_requests: Vec<super::navigation::Chapter>,
    save_notifications: Vec<u64>,
    menu_tips_return: bool,
    menu_return: Option<system_menu::MenuReturn>,
}

enum HostMode {
    Reconstruction(ReconstructionStackMode),
    Hidden(HiddenWindow),
    Menu(system_menu::MenuOwner),
    MenuConfirmation(system_menu::ConfirmationMode),
    MenuChild(system_menu::ChildMode),
    LayerReload(system_menu::LayerReloadMode),
    Browser(browser::BrowserMode),
    ChapterQuestion(navigation::QuestionMode),
    Save(super::save_game::SaveGame),
    TipsQuestion(system_menu::tips::QuestionMode),
    MenuReturnQuestion(system_menu::returns::QuestionMode),
}

struct ReconstructionStackMode {
    phase: Phase,
    completed_owner: Option<Box<super::runner::reconstruction_mode::Transition>>,
}
struct ModeServices<'a> {
    runner: &'a mut HigurashiRunner,
    audio: &'a mut HigurashiAudio,
    ui: &'a mut super::ui_state::HigurashiUiState,
    conceal: &'a mut ConcealState,
    overlay_visibility: &'a mut Vec<bool>,
    touch_overlay: &'a mut Option<Box<dyn super::touch::Overlay>>,
    buttons: Buttons,
    pointer: Option<&'a PointerFrame<'a>>,
    mouse_release: Option<crate::adv::input_area::MouseRelease>,
    modal: &'a mut super::modal::ModalState,
    menu_frame: system_menu::MenuFrame,
    system_sounds: Option<&'a system_menu::GlobalSounds>,
    menu_requests: &'a mut Vec<system_menu::ChildRequest>,
    next_menu_child: &'a mut u64,
    save_notifications: &'a mut Vec<u64>,
    browser_frame: browser::BrowserFrame,
    browser_notifications: &'a mut Vec<browser::Notification>,
    navigation_disabled: bool,
    chapter_requests: &'a mut Vec<super::navigation::Chapter>,
    menu_tips_return: &'a mut bool,
    menu_return: &'a mut Option<system_menu::MenuReturn>,
    menu_children: Option<&'a mut dyn system_menu::ChildServices>,
}
impl ReconstructionStackMode {
    fn poll(&mut self, services: &mut ModeServices<'_>) -> Result<u32> {
        // Explicit desktop cancellation has already drained the runner owner.
        // Its stack entry still retires in the next completion-pruning pass.
        if services.runner.reconstruction_phase().is_none() {
            self.phase = Phase::Complete;
            return Ok(0);
        }
        let preparing = self.phase == Phase::Preparing;
        let settings = services
            .menu_children
            .as_mut()
            .and_then(|children| children.layer_settings());
        self.phase = services
            .runner
            .poll_retained_reconstruction_mode_with_settings(
                services.audio,
                services.ui,
                settings,
            )?;
        // Optional child0's native c1cbc clone returnsNULL. Publication replaces
        // the source root after capture, and retires its command/UI together.
        if preparing && self.phase != Phase::Preparing {
            *services.touch_overlay = None;
        }
        if self.phase != Phase::Preparing {
            services.modal.animations_enabled = true;
        }
        if self.phase == Phase::Complete {
            self.completed_owner = services.runner.take_completed_reconstruction_mode();
        }
        // Native19578 returns0 even in the VM-installing completion frame.
        Ok(0)
    }
}
impl HostMode {
    fn reconstruction_phase(&self) -> Option<Phase> {
        match self {
            Self::Reconstruction(mode) => Some(mode.phase),
            _ => None,
        }
    }
}
struct ConcealOwners<'a> {
    runner: &'a mut HigurashiRunner,
    ui: &'a mut super::ui_state::HigurashiUiState,
    overlay_visibility: &'a mut Vec<bool>,
    overlay: &'a mut Option<Box<dyn super::touch::Overlay>>,
}
impl ConcealServices for ConcealOwners<'_> {
    fn hide_message(&mut self, hidden: bool) {
        self.runner.set_message_hidden(hidden);
    }
    fn hide_overlay(&mut self, hidden: bool) {
        self.overlay_visibility.push(hidden);
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_hidden(hidden);
        }
    }
    fn hide_indicator(&mut self, hidden: bool) {
        self.ui.indicator.hidden = hidden;
    }
    fn hide_menu(&mut self, hidden: bool) {
        self.ui.menu.indicator.hidden = hidden;
    }
}
struct ModalOwners<'a> {
    conceal: &'a mut ConcealState,
    owners: ConcealOwners<'a>,
    audio: &'a mut HigurashiAudio,
}
impl super::modal::ModalServices for ModalOwners<'_> {
    fn conceal(&mut self, raw: u32) {
        self.conceal.control(raw, &mut self.owners);
    }
    fn pause_voice(&mut self, paused: bool) {
        self.audio.set_modal_voice_paused(paused);
    }
    fn pause_effects(&mut self, paused: bool) {
        self.audio.set_modal_effects_paused(paused);
    }
}
impl<'a> Mode<ModeServices<'a>> for HostMode {
    fn completion(&mut self) -> u32 {
        match self {
            Self::Reconstruction(mode) => u32::from(mode.phase == Phase::Complete),
            Self::Hidden(mode) => mode.completion_word(),
            Self::Browser(mode) => mode.completion(),
            Self::ChapterQuestion(mode) => mode.completion(),
            Self::Save(mode) => u32::from(mode.complete()),
            Self::TipsQuestion(mode) => mode.completion(),
            Self::MenuReturnQuestion(mode) => mode.completion(),
            _ => system_menu::completion(self),
        }
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        services: &mut ModeServices<'a>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        match self {
            Self::Reconstruction(mode) => mode.poll(services),
            Self::Browser(mode) => mode.poll(delta, permission, services),
            Self::ChapterQuestion(mode) => mode.poll(delta, permission, services),
            Self::TipsQuestion(mode) => mode.poll(delta, permission, services),
            Self::MenuReturnQuestion(mode) => mode.poll(delta, permission, services),
            Self::Save(mode) => {
                let result = mode.poll(delta, permission, services.buttons, services.pointer)?;
                if mode.take_notification() {
                    services.audio.browser_music_multiplier(1.0);
                    services.save_notifications.push(mode.id);
                }
                Ok(result)
            }
            Self::Hidden(mode) => {
                let empty = PointerFrame {
                    number: 0,
                    time_seconds: 0.0,
                    events: &[],
                };
                Ok(mode.poll(
                    u32::from(permission),
                    services.buttons.pressed,
                    services.pointer.unwrap_or(&empty),
                ))
            }
            _ => system_menu::poll(self, delta, permission, services, pushes),
        }
    }
    fn retire(&mut self, services: &mut ModeServices<'a>) {
        if let Self::MenuChild(child) = self {
            if let Some(callbacks) = services.menu_children.as_mut() {
                callbacks.retired(child.request());
            }
        }
        if matches!(self, Self::Hidden(_)) {
            services.conceal.control(
                0,
                &mut ConcealOwners {
                    runner: services.runner,
                    ui: services.ui,
                    overlay_visibility: services.overlay_visibility,
                    overlay: services.touch_overlay,
                },
            );
        }
    }
}
impl Drop for HigurashiHost {
    fn drop(&mut self) {
        self.modes.clear_with(&mut ModeServices {
            runner: &mut self.runner,
            audio: &mut self.audio,
            ui: &mut self.ui,
            conceal: &mut self.conceal,
            overlay_visibility: &mut self.overlay_visibility,
            touch_overlay: &mut self.touch_overlay,
            buttons: Buttons::default(),
            pointer: None,
            mouse_release: None,
            modal: &mut self.modal,
            menu_frame: self.menu_frame,
            system_sounds: self.system_sounds.as_ref(),
            menu_requests: &mut self.menu_requests,
            next_menu_child: &mut self.next_menu_child,
            save_notifications: &mut self.save_notifications,
            menu_tips_return: &mut self.menu_tips_return,
            menu_return: &mut self.menu_return,
            menu_children: None,
            browser_frame: self.browser_frame,
            browser_notifications: &mut self.browser_notifications,
            navigation_disabled: self.navigation_disabled,
            chapter_requests: &mut self.chapter_requests,
        });
    }
}

/// Requests require concrete platform/modal services before they can be
/// acknowledged. Processing input alone does not complete external commands.
pub struct InputOutcome {
    pub overlay_input_enabled: bool,
    pub requests: Vec<Request>,
    /// Native Overlay virtual68 conceal edges, for the concrete interactive
    /// overlay owner when present. Message/indicator/menu edges are applied here.
    pub overlay_visibility: Vec<bool>,
}

struct InputServices<'a, 'b> {
    runner: &'a mut HigurashiRunner,
    audio: &'a mut HigurashiAudio,
    ui: &'a mut super::ui_state::HigurashiUiState,
    modes: &'a mut ModeStack<HostMode>,
    conceal: &'a mut ConcealState,
    overlay_visibility: &'a mut Vec<bool>,
    requests: Vec<Request>,
    jump_probe: Option<&'b mut dyn FnMut(&HigurashiRunner) -> Result<bool>>,
    fault: Option<anyhow::Error>,
    overlay: &'a mut Option<Box<dyn super::touch::Overlay>>,
}
impl super::touch::TouchServices for InputServices<'_, '_> {
    fn current_context(&self, mut context: Context) -> Context {
        context.disabled |= self.runner.exit_closing();
        context.continuation_blocked |= self.conceal.depth != 0;
        context.hint_busy = self.ui.hint.is_waiting();
        if let Some(state) = self.runner.native_read_state() {
            context.vm = Some(state);
        }
        if let Some(overlay) = &self.overlay {
            context.overlay = Some(overlay.input_permissions());
        }
        context
    }
    fn overlay_probe(&mut self, position: glam::Vec2) -> bool {
        self.overlay
            .as_mut()
            .is_some_and(|overlay| overlay.probe(position))
    }
    fn overlay_press(&mut self, position: glam::Vec2) {
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.press(position);
        }
    }
    fn overlay_release(&mut self, position: glam::Vec2) {
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.release(position);
        }
    }
    fn overlay_cancel(&mut self) {
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.cancel();
        }
    }
}
impl Services for InputServices<'_, '_> {
    fn scene_jump_available(&mut self, resolved: bool) -> bool {
        let Some(probe) = &mut self.jump_probe else {
            return resolved;
        };
        match probe(self.runner) {
            Ok(available) => available,
            Err(error) => {
                self.fault = Some(error);
                false
            }
        }
    }
    fn request(&mut self, request: Request) {
        if request == Request::HintPulse {
            self.ui.pulse_hint();
        } else if request == Request::HideWindow {
            let window = HiddenWindow::default();
            self.conceal.control(
                1,
                &mut ConcealOwners {
                    runner: self.runner,
                    ui: self.ui,
                    overlay_visibility: self.overlay_visibility,
                    overlay: self.overlay,
                },
            );
            self.modes.push(HostMode::Hidden(window));
        } else {
            self.requests.push(request);
        }
    }
    fn click_message(&mut self) -> bool {
        self.runner.click_message(self.audio)
    }
    fn message_interested(&self) -> bool {
        self.runner.frame().message.playback.interested_in_input()
    }
    fn interrupt_wait(&mut self) {
        self.runner.interrupt_wait();
    }
}

pub(crate) fn frame_delta(elapsed: Ticks) -> Result<Ticks> {
    let elapsed = elapsed.as_f32();
    ensure!(
        elapsed.is_finite() && elapsed >= 0.0,
        "Invalid Higurashi host elapsed ticks"
    );
    // Actual platform271d4: fmin s8,s8,3. Real host duration is nonnegative.
    Ok(Ticks::from_f32(elapsed.min(3.0)))
}

impl HigurashiHost {
    #[cfg(feature = "higurashi-window-probe")]
    pub(crate) fn reference_input_diagnostics(&self) -> String {
        format!(
            "permission={},read={:?},interested={},message_busy={},pause={},conceal={},fade_busy={},fade={},reconstruction={},notification_busy={},overlay={:?}",
            self.mode_input_permission,
            self.runner.native_read_state(),
            self.runner.frame().message.playback.interested_in_input(),
            self.runner.frame().message.playback.busy(),
            self.modal.pause_depth,
            self.conceal.depth,
            self.modal.fade.is_waiting(),
            self.modal.fade.value(),
            self.runner.reconstruction_phase().is_some(),
            self.runner
                .reconstruction_overlay()
                .is_some_and(|overlay| overlay.clock.busy()),
            self.touch_overlay
                .as_ref()
                .map(|overlay| overlay.input_permissions())
        )
    }
    pub(crate) fn cancel_input(&mut self) {
        self.root_touch.cancel_input(&mut self.touch_overlay);
        for mode in self.modes.iter_mut() {
            match mode {
                HostMode::Hidden(mode) => mode.cancel_input(),
                HostMode::Menu(mode) => mode.cancel_input(),
                HostMode::Browser(mode) => mode.cancel_input(),
                HostMode::ChapterQuestion(mode) => mode.cancel_input(),
                HostMode::TipsQuestion(mode) => mode.cancel_input(),
                HostMode::MenuReturnQuestion(mode) => mode.cancel_input(),
                HostMode::Save(mode) => mode.cancel_input(),
                _ => {}
            }
        }
    }
    pub(crate) fn has_save_task(&self) -> bool {
        self.modes
            .iter()
            .any(|mode| matches!(mode, HostMode::Save(save) if save.blocks_close()))
    }
    pub(crate) fn pending_menu_child_requests(&self) -> &[system_menu::ChildRequest] {
        &self.menu_requests
    }
    pub(crate) fn next_mode_ids(&self) -> (u64, u64) {
        (self.next_browser, self.next_menu_child)
    }
    pub(crate) fn set_next_mode_ids(&mut self, ids: (u64, u64)) {
        self.next_browser = ids.0;
        self.next_menu_child = ids.1;
    }
    pub fn new(runner: HigurashiRunner, audio: HigurashiAudio) -> Self {
        Self {
            runner,
            audio,
            ui: Default::default(),
            graphics: None,
            modes: Default::default(),
            mode_input_permission: true,
            root_touch: Default::default(),
            touch_overlay: None,
            conceal: Default::default(),
            overlay_visibility: Vec::new(),
            modal: Default::default(),
            modal_backdrop: None,
            menu_frame: Default::default(),
            system_sounds: None,
            menu_requests: Vec::new(),
            next_menu_child: 0,
            browser_frame: Default::default(),
            browser_notifications: Vec::new(),
            next_browser: 0,
            browser_audio: None,
            navigation_disabled: false,
            chapter_requests: Vec::new(),
            save_notifications: Vec::new(),
            menu_tips_return: false,
            menu_return: None,
        }
    }
    /// Bind before normal runtime callbacks. CPU/component fixtures can supply
    /// their own explicit preparation boundary; normal Gameplay binds eagerly.
    pub(crate) fn bind_graphics(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<()> {
        if self.graphics.is_none() {
            self.graphics = Some(super::frame_renderer::FrameRenderer::new(
                device,
                queue,
                self.runner.frame(),
            )?);
        }
        Ok(())
    }
    fn prepare_admitted_frame(&mut self, before: u64) -> Result<()> {
        if before != self.runner.frame().visual_epoch() {
            if let Some(graphics) = &mut self.graphics {
                graphics.prepare(self.runner.frame(), self.touch_overlay.as_deref())?;
            }
        }
        Ok(())
    }
    pub fn set_touch_overlay(&mut self, overlay: Option<Box<dyn super::touch::Overlay>>) {
        self.touch_overlay = overlay;
    }
    pub fn clear_touch_overlay(&mut self, identity: usize) {
        if identity != 0
            && self
                .touch_overlay
                .as_ref()
                .is_some_and(|o| o.identity() == identity)
        {
            self.touch_overlay = None;
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_touch_overlay_identity(&self) -> Option<usize> {
        self.touch_overlay.as_ref().map(|o| o.identity())
    }
    pub fn take_overlay_cues(&mut self) -> Vec<&'static str> {
        self.touch_overlay
            .as_mut()
            .map_or_else(Vec::new, |o| o.take_cues())
    }
    pub fn touch_state(&self) -> super::touch::State {
        self.root_touch.state
    }
    pub fn ui_elapsed_seconds(&self) -> f32 {
        self.root_touch.elapsed_seconds()
    }
    pub fn advance_touch(&mut self, indicator: u8, delta: Ticks) {
        self.root_touch.advance(indicator, delta);
    }

    /// Native First18950 transfers a fresh VM into mode18b78 with mode0.
    pub(crate) fn begin_fresh_scene(
        &mut self,
        replay: super::replay::ReplaySession,
        snapshot: shin_render::render_texture::RenderTexture,
        history: super::read_history::MessageReadHistory,
        seeds: impl FnMut() -> [u32; 4] + Send + 'static,
    ) -> Result<()> {
        self.runner
            .begin_fresh_reconstruction_mode(replay, 0, snapshot, history, seeds)?;
        self.modal.animations_enabled = false;
        self.mode_input_permission = false;
        self.modes
            .push(HostMode::Reconstruction(ReconstructionStackMode {
                phase: Phase::Preparing,
                completed_owner: None,
            }));
        Ok(())
    }

    ///15038 captures ScreenLayer's cached texture and older blur before any
    /// pause/conceal change. The renderer supplies that prepared snapshot.
    /// `enter_modal_from_screen` supplies the older capture/blur implementation.
    pub fn enter_modal(
        &mut self,
        capture: impl FnOnce(&HigurashiRunner) -> Result<shin_render::render_texture::RenderTexture>,
    ) -> Result<()> {
        let snapshot = capture(&self.runner)?;
        let backdrop = &mut self.modal_backdrop;
        self.modal.enter(
            || {
                *backdrop = Some(snapshot);
                Ok(())
            },
            &mut ModalOwners {
                conceal: &mut self.conceal,
                owners: ConcealOwners {
                    runner: &mut self.runner,
                    ui: &mut self.ui,
                    overlay_visibility: &mut self.overlay_visibility,
                    overlay: &mut self.touch_overlay,
                },
                audio: &mut self.audio,
            },
        )
    }
    /// Capture the prepared ScreenLayer cache through original150b8's older
    /// filter. Snapshot preparation still precedes every modal owner mutation.
    pub fn enter_modal_from_screen(
        &mut self,
        context: &mut crate::render::PreRenderContext,
        screens: &super::scene_textures::HigurashiSceneTextures,
    ) -> Result<()> {
        self.enter_modal(|_| screens.capture_modal(context))
    }
    ///158e0 restores counted owners when the concrete modal finishes closing,
    /// before its mode is retired by the next poll. Its destructor does not exit.
    pub fn exit_modal(&mut self) {
        self.modal.exit(&mut ModalOwners {
            conceal: &mut self.conceal,
            owners: ConcealOwners {
                runner: &mut self.runner,
                ui: &mut self.ui,
                overlay_visibility: &mut self.overlay_visibility,
                overlay: &mut self.touch_overlay,
            },
            audio: &mut self.audio,
        });
    }
    pub fn modal_snapshot(&self) -> Option<&shin_render::render_texture::RenderTexture> {
        self.modal_backdrop.as_ref()
    }
    pub fn modal_progress(&self) -> f32 {
        self.modal.fade.value()
    }
    pub fn modal_pause_depth(&self) -> u32 {
        self.modal.pause_depth
    }
    pub(crate) fn touch_diagnostic_modal_idle(&self) -> bool {
        self.modal.pause_depth == 0
            && !self.modal.fade.is_waiting()
            && self.modal.fade.value() == 0.0
    }
    fn modal_policy(&self, mut policy: Policy) -> Policy {
        policy.paused |= self.modal.pause_depth != 0;
        policy.animations_enabled &= self.modal.animations_enabled;
        policy
    }
    fn modal_audio_policy(&self, mut policy: AudioFramePolicy) -> AudioFramePolicy {
        policy.focus_mode_active &= !self.has_modes(); // Originald11b8.
        policy.effects_update_enabled &= !self.audio.modal_effects_paused();
        policy
    }
    fn advance_modal(&mut self, delta: Ticks) {
        if self.modal.advance(delta) {
            self.modal_backdrop = None;
        }
    }

    /// Original1382c draws live ADV only below fade1, then the quantized modal
    /// snapshot and reconstruction overlay. UI/mode views follow this pass.
    pub fn render_adv(
        &self,
        pass: &mut shin_render::render_pass::RenderPass,
        screens: &super::scene_textures::HigurashiSceneTextures,
    ) -> Result<()> {
        let plan =
            super::modal::backdrop::plan(self.modal_backdrop.is_some(), self.modal_progress());
        if plan.render_root {
            pass.clear(None, Some(0), Some(1.0));
            screens.render_cached_frame(pass, self.runner.frame(), 1);
        }
        if let Some(packet) = plan.backdrop {
            packet.render(
                pass,
                self.modal_backdrop.as_ref().unwrap().as_texture_source(),
            );
        }
        if let Some(overlay) = self.runner.reconstruction_overlay() {
            overlay.render(pass);
        }
        Ok(())
    }

    pub fn has_modes(&self) -> bool {
        !self.modes.is_empty()
    }
    pub fn take_save_notifications(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.save_notifications)
    }
    pub(crate) fn push_save_mode(&mut self, save: super::save_game::SaveGame) {
        self.modes.push(HostMode::Save(save));
    }
    pub fn save_phase(&self, id: u64) -> Option<i32> {
        self.modes.iter().find_map(|mode| match mode {
            HostMode::Save(save) if save.id == id => Some(save.phase()),
            _ => None,
        })
    }
    pub fn prepare_save_modes(&mut self, context: &mut crate::render::PreRenderContext) {
        for mode in self.modes.iter_mut() {
            if let HostMode::Save(save) = mode {
                save.prepare(context);
            }
        }
    }

    /// Call before logical input, as native12b9c does. Completion pruning
    /// happens before a new borrowed snapshot and reverse permission polling.
    /// Modal constructors and physical binding/repeat are still caller services.
    pub fn poll_modes(&mut self, elapsed: Ticks, initial_permission: u32) -> Result<bool> {
        self.poll_modes_core(
            elapsed,
            initial_permission,
            Buttons::default(),
            None,
            None,
            None,
        )
    }
    /// Pointer samples use the native1280x720 touch coordinate extent. Pass
    /// the same frame/button snapshot here and to subsequent logical input.
    pub fn poll_modes_with_input(
        &mut self,
        elapsed: Ticks,
        initial_permission: u32,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<bool> {
        self.poll_modes_core(
            elapsed,
            initial_permission,
            buttons,
            Some(pointer),
            None,
            None,
        )
    }
    pub(crate) fn poll_modes_with_menu_children<'a>(
        &'a mut self,
        elapsed: Ticks,
        initial_permission: u32,
        buttons: Buttons,
        pointer: &'a PointerFrame<'a>,
        children: &'a mut (dyn system_menu::ChildServices + 'a),
    ) -> Result<bool> {
        self.poll_modes_with_menu_children_and_mouse_release(
            elapsed,
            initial_permission,
            buttons,
            pointer,
            children,
            None,
        )
    }
    pub(crate) fn poll_modes_with_mouse_release(
        &mut self,
        elapsed: Ticks,
        initial_permission: u32,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        mouse_release: Option<crate::adv::input_area::MouseRelease>,
    ) -> Result<bool> {
        self.poll_modes_core(
            elapsed,
            initial_permission,
            buttons,
            Some(pointer),
            None,
            mouse_release,
        )
    }
    pub(crate) fn poll_modes_with_menu_children_and_mouse_release<'a>(
        &'a mut self,
        elapsed: Ticks,
        initial_permission: u32,
        buttons: Buttons,
        pointer: &'a PointerFrame<'a>,
        children: &'a mut (dyn system_menu::ChildServices + 'a),
        mouse_release: Option<crate::adv::input_area::MouseRelease>,
    ) -> Result<bool> {
        self.poll_modes_core(
            elapsed,
            initial_permission,
            buttons,
            Some(pointer),
            Some(children),
            mouse_release,
        )
    }
    fn poll_modes_core<'a>(
        &'a mut self,
        elapsed: Ticks,
        initial_permission: u32,
        buttons: Buttons,
        pointer: Option<&'a PointerFrame<'a>>,
        menu_children: Option<&'a mut (dyn system_menu::ChildServices + 'a)>,
        mouse_release: Option<crate::adv::input_area::MouseRelease>,
    ) -> Result<bool> {
        let delta = frame_delta(elapsed)?;
        if let Some(phase) = self.runner.reconstruction_phase() {
            if !self.modes.iter().any(|mode| {
                mode.reconstruction_phase()
                    .is_some_and(|phase| phase != Phase::Complete)
            }) {
                self.modes
                    .push(HostMode::Reconstruction(ReconstructionStackMode {
                        phase,
                        completed_owner: None,
                    }));
            }
        }
        // An error is a desktop retry boundary; do not allow input into a
        // partially published mode before that retry succeeds.
        self.mode_input_permission = false;
        let permission = self.modes.poll(
            delta,
            initial_permission,
            &mut ModeServices {
                runner: &mut self.runner,
                audio: &mut self.audio,
                ui: &mut self.ui,
                conceal: &mut self.conceal,
                overlay_visibility: &mut self.overlay_visibility,
                touch_overlay: &mut self.touch_overlay,
                buttons,
                pointer,
                mouse_release,
                modal: &mut self.modal,
                menu_frame: self.menu_frame,
                system_sounds: self.system_sounds.as_ref(),
                menu_requests: &mut self.menu_requests,
                next_menu_child: &mut self.next_menu_child,
                save_notifications: &mut self.save_notifications,
                menu_tips_return: &mut self.menu_tips_return,
                menu_return: &mut self.menu_return,
                menu_children,
                browser_frame: self.browser_frame,
                browser_notifications: &mut self.browser_notifications,
                navigation_disabled: self.navigation_disabled,
                chapter_requests: &mut self.chapter_requests,
            },
        )?;
        self.mode_input_permission = permission;
        Ok(permission)
    }

    /// Compatibility entry for callers polling only reconstruction. The host
    /// uses its native mode stack and retains the completion-frame owner.
    pub fn poll_reconstruction_mode(&mut self) -> Result<super::reconstruction_mode::Phase> {
        self.poll_modes(Ticks::ZERO, 1)?;
        self.modes
            .iter()
            .filter_map(HostMode::reconstruction_phase)
            .last()
            .context("No Higurashi reconstruction mode")
    }

    /// Core frame using command-current VM read fields. Full post-command
    /// group/UI/SE skip effects and platform snapshot storage remain services.
    pub async fn update_native(
        &mut self,
        elapsed: Ticks,
        policy: Policy,
        read_policy: ReadPolicy,
        read_services: &mut impl ReadServices,
        audio_policy: AudioFramePolicy,
        max_commands: usize,
        max_instructions: usize,
        seeds: impl FnMut() -> [u32; 4],
    ) -> Result<Status> {
        self.update_native_with_external(
            elapsed,
            policy,
            read_policy,
            read_services,
            audio_policy,
            max_commands,
            max_instructions,
            seeds,
            |_, _, _| Ok(false),
        )
        .await
    }

    /// Resolve owned command services before the frame's WAIT/visual/audio tick.
    /// Unhandled requests remain installed for their concrete UI/platform owner.
    pub async fn update_native_with_external<S: ReadServices>(
        &mut self,
        elapsed: Ticks,
        policy: Policy,
        read_policy: ReadPolicy,
        read_services: &mut S,
        audio_policy: AudioFramePolicy,
        max_commands: usize,
        max_instructions: usize,
        seeds: impl FnMut() -> [u32; 4],
        external: impl FnMut(
            &mut HigurashiRunner,
            &mut S,
            &mut super::ui_state::HigurashiUiState,
        ) -> Result<bool>,
    ) -> Result<Status> {
        self.update_native_with_frame_services(
            elapsed,
            policy,
            read_policy,
            read_services,
            audio_policy,
            max_commands,
            max_instructions,
            seeds,
            external,
            |_, _, _, _| Ok(()),
        )
        .await
    }
    /// Original12c44 post-poll read effects precede WAIT and visual clocks.
    pub async fn update_native_with_frame_services<S: ReadServices>(
        &mut self,
        elapsed: Ticks,
        policy: Policy,
        read_policy: ReadPolicy,
        read_services: &mut S,
        audio_policy: AudioFramePolicy,
        max_commands: usize,
        max_instructions: usize,
        mut seeds: impl FnMut() -> [u32; 4],
        mut external: impl FnMut(
            &mut HigurashiRunner,
            &mut S,
            &mut super::ui_state::HigurashiUiState,
        ) -> Result<bool>,
        post_poll: impl FnOnce(
            &mut HigurashiRunner,
            &mut HigurashiAudio,
            &mut super::ui_state::HigurashiUiState,
            &mut S,
        ) -> Result<()>,
    ) -> Result<Status> {
        self.update_native_with_overlay_services(
            elapsed,
            policy,
            read_policy,
            read_services,
            audio_policy,
            max_commands,
            max_instructions,
            seeds,
            move |runner, services, ui, _| external(runner, services, ui),
            post_poll,
        )
        .await
    }
    pub async fn update_native_with_overlay_services<S: ReadServices>(
        &mut self,
        elapsed: Ticks,
        policy: Policy,
        read_policy: ReadPolicy,
        read_services: &mut S,
        audio_policy: AudioFramePolicy,
        max_commands: usize,
        max_instructions: usize,
        mut seeds: impl FnMut() -> [u32; 4],
        mut external: impl FnMut(
            &mut HigurashiRunner,
            &mut S,
            &mut super::ui_state::HigurashiUiState,
            &mut Option<Box<dyn super::touch::Overlay>>,
        ) -> Result<bool>,
        post_poll: impl FnOnce(
            &mut HigurashiRunner,
            &mut HigurashiAudio,
            &mut super::ui_state::HigurashiUiState,
            &mut S,
        ) -> Result<()>,
    ) -> Result<Status> {
        self.update_native_with_overlay_audio_services(
            elapsed,
            policy,
            read_policy,
            read_services,
            audio_policy,
            max_commands,
            max_instructions,
            seeds,
            move |runner, services, ui, overlay, _, _, _| external(runner, services, ui, overlay),
            post_poll,
        )
        .await
    }
    /// EXIT's close services run during command polling,before any frame tick.
    pub async fn update_native_with_overlay_audio_services<S: ReadServices>(
        &mut self,
        elapsed: Ticks,
        policy: Policy,
        read_policy: ReadPolicy,
        read_services: &mut S,
        audio_policy: AudioFramePolicy,
        max_commands: usize,
        max_instructions: usize,
        mut seeds: impl FnMut() -> [u32; 4],
        mut external: impl FnMut(
            &mut HigurashiRunner,
            &mut S,
            &mut super::ui_state::HigurashiUiState,
            &mut Option<Box<dyn super::touch::Overlay>>,
            &mut HigurashiAudio,
            bool,
            &mut Vec<super::save_game::SaveGame>,
        ) -> Result<bool>,
        post_poll: impl FnOnce(
            &mut HigurashiRunner,
            &mut HigurashiAudio,
            &mut super::ui_state::HigurashiUiState,
            &mut S,
        ) -> Result<()>,
    ) -> Result<Status> {
        let delta = frame_delta(elapsed)?;
        let policy = self.modal_policy(policy);
        let audio_policy = self.modal_audio_policy(audio_policy);
        ensure!(
            audio_policy.music_rate.is_finite() && audio_policy.music_rate >= 0.0,
            "Invalid Higurashi global music rate"
        );
        ensure!(
            max_commands > 0 && max_instructions > 0,
            "Higurashi runner budgets must be positive"
        );
        let before = self.runner.completed_commands();
        let mut saves = Vec::new();
        let status = loop {
            let used = self.runner.completed_commands().saturating_sub(before) as usize;
            if used >= max_commands {
                break Status::CommandBudget;
            }
            let status = self
                .runner
                .pump_native(
                    policy,
                    read_policy,
                    read_services,
                    max_commands - used,
                    max_instructions,
                    &mut seeds,
                    &mut self.audio,
                )
                .await?;
            if status != Status::External {
                break status;
            }
            let completed = external(
                &mut self.runner,
                read_services,
                &mut self.ui,
                &mut self.touch_overlay,
                &mut self.audio,
                self.modes.is_empty(),
                &mut saves,
            )?;
            for save in saves.drain(..) {
                self.modes.push(HostMode::Save(save));
            }
            if !completed {
                break status;
            }
        };
        if !policy.paused && self.runner.native_vm_attached() {
            post_poll(
                &mut self.runner,
                &mut self.audio,
                &mut self.ui,
                read_services,
            )?;
            if input::message_requests(
                self.runner.native_read_state(),
                read_policy.modes,
                read_policy.forced,
                read_policy.read_only,
            )
            .fast_forward
            {
                if let Some(overlay) = self.touch_overlay.as_mut() {
                    overlay.fast_forward();
                }
            }
        }
        if self.runner.visual_updates_admitted(policy) {
            if let Some(overlay) = self.touch_overlay.as_mut() {
                overlay.advance_with_voice(delta, &mut self.audio);
            }
        }
        let before_visual = self.runner.frame().visual_epoch();
        self.runner.advance(delta, policy, &mut self.audio, None);
        self.prepare_admitted_frame(before_visual)?;
        self.runner.flush_message_completions(read_services)?;
        self.advance_modal(delta);
        let ui = &mut self.ui;
        let mut audio_policy = audio_policy;
        audio_policy.focus_mode_active &= self.modes.is_empty();
        self.audio
            .update_frame_with_ui(delta, audio_policy, || ui.advance(delta));
        Ok(status)
    }

    /// Native input publishes final-message completion immediately, before the
    /// same frame's pump can construct a revisited message.
    pub fn process_input_native(
        &mut self,
        state: &mut InputState,
        context: Context,
        buttons: Buttons,
        read_services: &mut impl ReadServices,
    ) -> Result<InputOutcome> {
        ensure!(
            self.runner.native_read_state().is_some()
                || self.runner.reconstruction_phase().is_some(),
            "Native input requires a native VM"
        );
        self.runner.flush_message_completions(read_services)?;
        let outcome = self.process_input(state, context, buttons);
        self.runner.flush_message_completions(read_services)?;
        Ok(outcome)
    }

    /// Called before update/pump. Native mode, transition and read state remain
    /// explicit; message and WAIT decisions use the actual retained owners.
    pub fn process_input(
        &mut self,
        state: &mut InputState,
        context: Context,
        buttons: Buttons,
    ) -> InputOutcome {
        self.process_input_resolved(state, context, buttons, None, None, None)
            .0
    }
    pub fn process_input_with_scene_jump(
        &mut self,
        state: &mut InputState,
        context: Context,
        buttons: Buttons,
        probe: &mut dyn FnMut(&HigurashiRunner) -> Result<bool>,
    ) -> Result<InputOutcome> {
        let (outcome, fault) =
            self.process_input_resolved(state, context, buttons, None, Some(probe), None);
        if let Some(fault) = fault {
            return Err(fault);
        }
        Ok(outcome)
    }
    pub fn process_input_with_pointer_and_scene_jump(
        &mut self,
        state: &mut InputState,
        context: Context,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        probe: &mut dyn FnMut(&HigurashiRunner) -> Result<bool>,
    ) -> Result<InputOutcome> {
        let (outcome, fault) =
            self.process_input_resolved(state, context, buttons, Some(pointer), Some(probe), None);
        if let Some(fault) = fault {
            return Err(fault);
        }
        Ok(outcome)
    }
    pub(crate) fn process_desktop_input_with_pointer_and_scene_jump(
        &mut self,
        state: &mut InputState,
        context: Context,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        probe: &mut dyn FnMut(&HigurashiRunner) -> Result<bool>,
        mouse_release: Option<crate::adv::input_area::MouseRelease>,
    ) -> Result<InputOutcome> {
        let (outcome, fault) = self.process_input_resolved(
            state,
            context,
            buttons,
            Some(pointer),
            Some(probe),
            mouse_release,
        );
        if let Some(fault) = fault {
            return Err(fault);
        }
        Ok(outcome)
    }
    fn process_input_resolved(
        &mut self,
        state: &mut InputState,
        mut context: Context,
        buttons: Buttons,
        pointer: Option<&PointerFrame<'_>>,
        jump_probe: Option<&mut dyn FnMut(&HigurashiRunner) -> Result<bool>>,
        mouse_release: Option<crate::adv::input_area::MouseRelease>,
    ) -> (InputOutcome, Option<anyhow::Error>) {
        context.permission &= self.mode_input_permission;
        context.disabled |= self.runner.exit_closing();
        context.paused |= self.modal.pause_depth != 0;
        context.animations_enabled &= self.modal.animations_enabled;
        context.continuation_blocked |= self.conceal.depth != 0;
        if self.runner.reconstruction_phase().is_some() {
            //19578 always returns0 to d11c8. This gate precedes even the
            // auto/skip toggles in13140, before its detached-VM/busy tests.
            context.permission = false;
            context.overlay_transition_busy = true;
            context.vm = None;
        }
        context.wait_allow_interrupt = self.runner.wait_allow_interrupt();
        context.overlay_transition_busy |= self.modal.fade.is_waiting();
        if self.modal.fade.is_waiting() || self.modal.fade.value() > 0.0 {
            context.overlay_transition_value = self.modal.fade.value();
        }
        context.hint_busy = self.ui.hint.is_waiting();
        if let Some(overlay) = self.runner.reconstruction_overlay() {
            context.notification_busy = overlay.clock.busy();
        }
        if let Some(state) = self.runner.native_read_state() {
            context.vm = Some(state);
        }
        if let Some(overlay) = &self.touch_overlay {
            context.overlay = Some(overlay.input_permissions());
        }
        // Native13140 denies only its own ADV+30 InputArea, including callers
        //without a pointer snapshot. Do not cancel the shared pointer producer
        //or contacts retained by a higher mode such as Manual/Config.
        if !context.permission {
            self.root_touch.cancel_input(&mut self.touch_overlay);
        }
        let mut services = InputServices {
            runner: &mut self.runner,
            audio: &mut self.audio,
            ui: &mut self.ui,
            modes: &mut self.modes,
            conceal: &mut self.conceal,
            overlay_visibility: &mut self.overlay_visibility,
            requests: Vec::new(),
            jump_probe,
            fault: None,
            overlay: &mut self.touch_overlay,
        };
        if let Some(pointer) = pointer {
            context.captured |= self.root_touch.process_with_mouse_release(
                state,
                context,
                pointer,
                &mut services,
                mouse_release,
            );
        }
        if buttons.pressed & (1 << 15) != 0 {
            tracing::debug!(
                ?context,
                pressed = buttons.pressed,
                modes = state.modes,
                interested = services.message_interested(),
                "HIGURASHI_Y_RESOLVED"
            );
        }
        let overlay_input_enabled = input::process(state, context, buttons, &mut services);
        let InputServices {
            requests, fault, ..
        } = services;
        if self.runner.native_vm_attached() && !context.paused {
            if let Some(overlay) = self.touch_overlay.as_mut() {
                overlay.input(overlay_input_enabled, buttons);
            }
        }
        (
            InputOutcome {
                overlay_input_enabled,
                requests,
                overlay_visibility: std::mem::take(&mut self.overlay_visibility),
            },
            fault,
        )
    }

    pub async fn update(
        &mut self,
        elapsed: Ticks,
        policy: Policy,
        audio_policy: AudioFramePolicy,
        max_commands: usize,
        max_instructions: usize,
        seeds: impl FnMut() -> [u32; 4],
    ) -> Result<Status> {
        let delta = frame_delta(elapsed)?;
        let policy = self.modal_policy(policy);
        let audio_policy = self.modal_audio_policy(audio_policy);
        ensure!(
            audio_policy.music_rate.is_finite() && audio_policy.music_rate >= 0.0,
            "Invalid Higurashi global music rate"
        );
        // Asset commands schedule/poll tasks; pump never awaits PIC/BUP I/O.
        let status = self
            .runner
            .pump(
                policy,
                max_commands,
                max_instructions,
                seeds,
                &mut self.audio,
            )
            .await?;
        if self.runner.visual_updates_admitted(policy) {
            if let Some(overlay) = self.touch_overlay.as_mut() {
                overlay.advance(delta);
            }
        }
        let before_visual = self.runner.frame().visual_epoch();
        self.runner.advance(delta, policy, &mut self.audio, None);
        self.prepare_admitted_frame(before_visual)?;
        self.advance_modal(delta);
        let ui = &mut self.ui;
        self.audio
            .update_frame_with_ui(delta, audio_policy, || ui.advance(delta));
        Ok(status)
    }
}

#[cfg(test)]
mod tests;
