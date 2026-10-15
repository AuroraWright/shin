//! Concrete58b44/59ccc/59090 startup and5a1fc deferred title request.
use super::{
    first_boot::{CommonAssets, FirstBoot},
    load_game::{LoadGame, QuestionKind, QuestionMode, Storage},
};
use crate::{
    adv::{
        higurashi::{
            input::Buttons,
            mode_stack::{Mode, ModePush, ModeStack},
        },
        input_area::PointerFrame,
    },
    asset::system::AssetServer,
    audio::SystemSoundPlayer,
    render::PreRenderContext,
};
use anyhow::Result;
use parking_lot::Mutex;
use shin_core::{format::save::higurashi::SaveData, time::Ticks};
use std::sync::Arc;

pub(super) struct Ready {
    pub common: Arc<CommonAssets>,
    pub data: SaveData,
    /// Native59ccc converts saved seconds to the global double tick clock.
    pub play_ticks: f64,
    /// Saved byte+f8, applied by27540/e7aa4 to the platform owner+70.
    pub platform_flag: bool,
}
/// The owned e71a0 closure payload. The application retires startup and
/// constructs the concrete opening TitleDemo on its next dispatch, as2714c does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TitleDemoRequest {
    pub mode: u32,
}
#[derive(Default)]
struct Transition {
    play_ticks: f64,
    platform_flag: bool,
    pending: Option<TitleDemoRequest>,
}
impl Transition {
    fn loaded(&mut self, data: &SaveData) {
        self.play_ticks = f64::from(data.play_seconds) * 60.0;
        self.platform_flag = data.settings.common.v16_showtoucheffect;
    }
    fn callback(&mut self, event: crate::adv::higurashi::question::Callback) {
        if event.stage == 1 {
            self.pending = Some(TitleDemoRequest { mode: 0 });
        }
    }
}
pub(super) struct Startup {
    boot: FirstBoot,
    children: ModeStack<Child>,
    common: Option<Arc<CommonAssets>>,
    loaded: Option<SaveData>,
    storage: Arc<dyn Storage>,
    server: Arc<AssetServer>,
    sounds: Arc<Mutex<SystemSoundPlayer>>,
    transition: Transition,
}
enum Child {
    Load(Box<LoadGame>),
    Notice(QuestionMode),
}
struct Services<'a, 'b> {
    buttons: Buttons,
    pointer: &'a PointerFrame<'b>,
    server: &'a Arc<AssetServer>,
    sounds: &'a Arc<Mutex<SystemSoundPlayer>>,
    loaded: &'a mut Option<SaveData>,
    transition: &'a mut Transition,
}
impl<'a, 'b> Mode<Services<'a, 'b>> for Child {
    fn completion(&mut self) -> u32 {
        match self {
            Self::Load(child) => child.complete() as u32,
            Self::Notice(child) => child.complete() as u32,
        }
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        services: &mut Services<'a, 'b>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        match self {
            Self::Load(child) => {
                let result = child.poll(delta, permission, services.buttons, services.pointer)?;
                if let Some(data) = child.take_loaded() {
                    services.transition.loaded(&data);
                    services
                        .sounds
                        .lock()
                        .set_settings_volume(data.settings.common.v3_sysvol);
                    *services.loaded = Some(data);
                    // Native59ccc->59090 pushes into the parent while its
                    // snapshot is being polled. The notice awaits the next poll.
                    pushes.push(Self::Notice(QuestionMode::new(
                        services.server.clone(),
                        QuestionKind::AutoSleep,
                    )));
                }
                Ok(result)
            }
            Self::Notice(child) => {
                for event in child.poll_frame(
                    delta,
                    permission,
                    services.buttons,
                    services.pointer,
                    services.sounds,
                    services.server,
                )? {
                    services.transition.callback(event);
                }
                Ok(0)
            }
        }
    }
}
impl Child {
    fn prepare(&mut self, context: &mut PreRenderContext) {
        match self {
            Self::Load(child) => child.prepare(context),
            Self::Notice(child) => child.prepare(context),
        }
    }
    fn render(&mut self, pass: &mut shin_render::render_pass::RenderPass) {
        match self {
            Self::Load(child) => child.render(pass),
            Self::Notice(child) => child.render(pass),
        }
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    fn reference_question(&self) -> Option<(QuestionKind, i32, i32)> {
        match self {
            Self::Load(child) => child.reference_question(),
            Self::Notice(child) => child.reference_state(),
        }
    }
    #[cfg(test)]
    fn reference_retirements(&self) -> Vec<shin_tasks::LoaderRetirement> {
        match self {
            Self::Load(child) => child.reference_retirements(),
            Self::Notice(child) => vec![child.reference_retirement()],
        }
    }
}
impl Startup {
    /// Report only a notice present in the last native borrowed draw snapshot.
    pub(super) fn touch_diagnostic_phase(&self) -> Option<&'static str> {
        let mut ready = false;
        self.children.draw_ref(|child| {
            if let Child::Notice(notice) = child {
                ready |= notice.touch_diagnostic_notice_ready();
            }
        });
        ready.then_some("notice")
    }
    pub fn new(
        server: Arc<AssetServer>,
        manager: Arc<shin_audio::AudioManager>,
        sounds: Arc<Mutex<SystemSoundPlayer>>,
        storage: Arc<dyn Storage>,
    ) -> Self {
        Self {
            boot: FirstBoot::new(server.clone(), manager, sounds.clone()),
            children: ModeStack::default(),
            common: None,
            loaded: None,
            storage,
            server,
            sounds,
            transition: Transition::default(),
        }
    }
    #[cfg(test)]
    pub fn poll(
        &mut self,
        delta: Ticks,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<Option<Ready>> {
        self.poll_with_permission(delta, true, buttons, pointer)
    }
    pub fn poll_with_permission(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<Option<Ready>> {
        // Original FirstBoot always polls its retained children first. A newly
        // constructed LoadGame waits for the next snapshot before its first poll.
        // Its internal native permission is1. Compose the desktop outer input
        // gate here while continuing child jobs, clocks and accepted closures.
        self.children.poll(
            delta,
            u32::from(permission),
            &mut Services {
                buttons,
                pointer,
                server: &self.server,
                sounds: &self.sounds,
                loaded: &mut self.loaded,
                transition: &mut self.transition,
            },
        )?;
        if let Some(common) = self.boot.poll(delta, self.children.is_empty())? {
            self.common = Some(common);
            self.children.push(Child::Load(Box::new(LoadGame::new(
                self.storage.clone(),
                self.server.clone(),
                self.sounds.clone(),
            ))));
        }
        Ok(self.loaded.take().map(|data| Ready {
            common: self
                .common
                .as_ref()
                .expect("Common assets precede LoadGame")
                .clone(),
            play_ticks: self.transition.play_ticks,
            platform_flag: self.transition.platform_flag,
            data,
        }))
    }
    pub fn take_title_demo_request(&mut self) -> Option<TitleDemoRequest> {
        self.transition.pending.take()
    }
    pub fn cancel_input(&mut self) {
        for child in self.children.iter_mut() {
            match child {
                Child::Load(child) => child.cancel_input(),
                Child::Notice(child) => child.cancel_input(),
            }
        }
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub(super) fn reference_questions(&self) -> Vec<(QuestionKind, i32, i32)> {
        self.children
            .iter()
            .filter_map(Child::reference_question)
            .collect()
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub(super) fn reference_notice_center(&self) -> Option<glam::Vec2> {
        self.children.iter().find_map(|child| match child {
            Child::Notice(child) => child.reference_button_center(0),
            _ => None,
        })
    }
    #[cfg(test)]
    pub(super) fn reference_retirements(&self) -> Vec<shin_tasks::LoaderRetirement> {
        let mut handles = self.boot.reference_retirements();
        handles.extend(self.children.iter().flat_map(Child::reference_retirements));
        handles
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) {
        self.children.draw(|child| child.prepare(context));
    }
    pub fn render(&mut self, pass: &mut shin_render::render_pass::RenderPass) {
        self.boot
            .render(pass, |pass| self.children.draw(|child| child.render(pass)));
    }
}

#[cfg(test)]
mod tests;
