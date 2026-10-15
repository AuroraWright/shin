//! Original275b0 keeps the latest copied constructor closure. Platform2714c
//! destroys the old ADV before185d4 creates the new one through11cd8.
use super::{
    HigurashiHost, HostMode, ReconstructionStackMode,
    browser::{BrowserId, Notice, Notification},
};
use crate::adv::higurashi::{
    audio::HigurashiAudio, checkpoints::CheckpointMemory, progress::ProgressMemory,
    read_history::MessageReadHistory, reconstruction_mode::Phase, replay::ReplaySession,
    runner::HigurashiRunner,
};
use anyhow::{Result, ensure};
use shin_core::format::scenario::higurashi::HigurashiScenario;
use shin_render::render_texture::RenderTexture;
use std::sync::Arc;

struct Globals {
    assets: Arc<crate::asset::system::AssetServer>,
    scenario: Arc<HigurashiScenario>,
    audio: Option<HigurashiAudio>,
    menu_frame: super::system_menu::MenuFrame,
    browser_frame: super::browser::BrowserFrame,
    browser_audio: Option<(super::system_menu::GlobalSounds, u8)>,
    system_sounds: Option<super::system_menu::GlobalSounds>,
    layer_setting: u8,
    next_browser: u64,
    next_menu_child: u64,
    snapshot: RenderTexture,
}
struct Jump {
    browser: BrowserId,
    source: ProgressMemory,
    mode: u32,
    globals: Option<Globals>,
}
/// Original275b0 copies the browser's GameData and constructor flags into one
/// latest-writer scene slot. Both host-only and full gameplay adapters use this
/// payload; validating a notification never completes its browser.
#[derive(Clone)]
pub struct BrowserJump {
    pub browser: BrowserId,
    pub source: ProgressMemory,
    pub flags: u32,
}
impl BrowserJump {
    pub fn from_notification(notification: Notification) -> Result<Self> {
        let Notice::Reconstruct { source, mode } = notification.notice else {
            anyhow::bail!("Scene slot requires a reconstruction notification");
        };
        Ok(Self {
            browser: notification.browser,
            source,
            flags: mode,
        })
    }
}
impl Jump {
    fn from_notification(notification: Notification) -> Result<Self> {
        let request = BrowserJump::from_notification(notification)?;
        Ok(Self {
            browser: request.browser,
            source: request.source,
            mode: request.flags,
            globals: None,
        })
    }
}
/// Application scene slot. Queuing does not run reconstruction or release the
/// browser; dispatch happens at the next platform boundary with a composed frame.
pub struct SceneSlot {
    active: Option<HigurashiHost>,
    pending: Option<Jump>,
}
impl SceneSlot {
    pub fn new(active: HigurashiHost) -> Self {
        Self {
            active: Some(active),
            pending: None,
        }
    }
    pub fn active(&self) -> Option<&HigurashiHost> {
        self.active.as_ref()
    }
    pub fn active_mut(&mut self) -> Option<&mut HigurashiHost> {
        self.active.as_mut()
    }
    pub fn pending_browser(&self) -> Option<BrowserId> {
        self.pending.as_ref().map(|jump| jump.browser)
    }
    pub fn queue_browser_jump(&mut self, notification: Notification) -> Result<()> {
        ensure!(
            self.active.is_some(),
            "Browser jump requires an active scene"
        );
        // Native275b0 swaps its one global constructor slot and destroys the
        // previous closure, so a newer request replaces the older owned payload.
        self.pending = Some(Jump::from_notification(notification)?);
        Ok(())
    }
    /// Capture the caller's already composed frame, including browser/modal/UI
    /// output. Runner.capture_frame would omit those platform owners.
    /// Preflight errors leave the active scene untouched. After native source
    /// retirement, a constructor error retains globals/payload/snapshot for retry.
    pub async fn dispatch_browser_jump(
        &mut self,
        context: &mut crate::render::PreRenderContext<'_, '_, '_, '_>,
        composed_frame: &RenderTexture,
        history: MessageReadHistory,
        mut seeds: impl FnMut() -> [u32; 4] + Send + 'static,
    ) -> Result<bool> {
        let Some(jump) = &mut self.pending else {
            return Ok(false);
        };
        if jump.globals.is_none() {
            let old = self.active.as_mut().unwrap();
            // Validate the empty-archive fork before retiring the source.
            new_replay(old.runner.scenario().clone(), &jump.source)?;
            let snapshot = composed_frame.copy_snapshot(&mut context.render_clone_ctx());
            let globals = Globals {
                assets: old.runner.assets().clone(),
                scenario: old.runner.scenario().clone(),
                audio: Some(old.audio.take_for_adv_replacement()),
                menu_frame: old.menu_frame,
                browser_frame: old.browser_frame,
                browser_audio: old.browser_audio.clone(),
                system_sounds: old.system_sounds.clone(),
                layer_setting: old.runner.layer_setting(),
                next_browser: old.next_browser,
                next_menu_child: old.next_menu_child,
                snapshot,
            };
            jump.globals = Some(globals);
            drop(self.active.take()); // Joins browser/children before new ADV construction.
            jump.globals
                .as_mut()
                .unwrap()
                .audio
                .as_mut()
                .unwrap()
                .reset_adv_for_replacement();
        }
        let globals = jump.globals.as_mut().unwrap();
        let entry = jump.source.progress().active_entry();
        let mut runner = HigurashiRunner::load(
            globals.assets.clone(),
            globals.scenario.clone(),
            entry.scenario_id,
            entry.random_seed,
            &mut seeds,
        )
        .await?;
        runner.set_layer_setting(globals.layer_setting);
        let replay = new_replay(globals.scenario.clone(), &jump.source)?;
        let snapshot = globals
            .snapshot
            .copy_snapshot(&mut context.render_clone_ctx());
        runner.begin_fresh_reconstruction_mode(replay, jump.mode, snapshot, history, seeds)?;
        let mut replacement = HigurashiHost::new(runner, globals.audio.take().unwrap());
        replacement.bind_graphics(context.device, context.queue)?;
        replacement.menu_frame = globals.menu_frame;
        replacement.browser_frame = globals.browser_frame;
        replacement.browser_audio = globals.browser_audio.take();
        replacement.system_sounds = globals.system_sounds.take();
        replacement.next_browser = globals.next_browser;
        replacement.next_menu_child = globals.next_menu_child;
        replacement.modal.animations_enabled = false;
        replacement.mode_input_permission = false;
        replacement
            .modes
            .push(HostMode::Reconstruction(ReconstructionStackMode {
                phase: Phase::Preparing,
                completed_owner: None,
            }));
        self.active = Some(replacement);
        self.pending = None;
        Ok(true)
    }
}
fn new_replay(scenario: Arc<HigurashiScenario>, source: &ProgressMemory) -> Result<ReplaySession> {
    let entry = source.progress().active_entry();
    let fork = CheckpointMemory::new(Vec::new()).fork_for_restore(
        entry.save_position,
        entry.replay_target,
        source,
    )?;
    ReplaySession::new(scenario, fork)
}

#[cfg(test)]
mod tests;
