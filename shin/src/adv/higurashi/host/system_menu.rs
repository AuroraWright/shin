//! SystemMenu/confirmation ownership in the older host's borrowed mode stack.
//! Child UI constructors are retained requests until their concrete owners exist.
use super::{ConcealOwners, HigurashiHost, HostMode, ModalOwners, ModeServices};
use crate::{
    adv::higurashi::{
        mode_stack::ModePush,
        system_menu::{Confirmation, Cue, MenuServices, SystemMenu, view::View},
    },
    asset::system::AssetServer,
    audio::SystemSoundPlayer,
};
use anyhow::{Context, Result, bail, ensure};
use parking_lot::Mutex;
use shin_core::{time::Ticks, vm::command::types::Volume};
use std::sync::Arc;
pub(super) mod returns;
pub(super) mod tips;
pub(crate) use returns::{Destination as MenuReturn, fragment_selected};

/// Application-owned global player. It can survive host/ADV replacement.
pub type GlobalSounds = Arc<Mutex<SystemSoundPlayer>>;

/// Startup and First retain these resources for the concrete gameplay owner.
pub struct Resources {
    pub atlas: Arc<crate::adv::characters::view::CharacterTextures>,
    pub font: Arc<crate::asset::font::GpuFontLazy>,
    pub sounds: Arc<shin_core::format::sysse::SysSe>,
}

#[derive(Clone, Copy, Default)]
pub(super) struct MenuFrame {
    setting: u8,
    play_ticks: f64,
}

pub(super) struct MenuOwner(Arc<Mutex<OwnedMenu>>);
struct OwnedMenu {
    model: SystemMenu,
    view: View,
    assets: Arc<AssetServer>,
}
impl MenuOwner {
    pub(super) fn cancel_input(&mut self) {
        self.0.lock().model.cancel_input(&mut ReturnOnly);
    }
}
pub(super) struct ConfirmationMode {
    parent: Arc<Mutex<OwnedMenu>>,
    callback: Confirmation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildKind {
    /// Original1c804's selected atlas item; the concrete constructor is pending.
    Item(u32),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildId(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildRequest {
    pub id: ChildId,
    pub kind: ChildKind,
}
/// Application-owned children execute inside the same native host snapshot.
/// A true poll result means the child's final return callback, not permission.
pub(crate) trait ChildServices {
    fn layer_settings(&mut self) -> Option<&mut shin_core::format::save::higurashi::SaveData> {
        None
    }
    fn construct(&mut self, request: ChildRequest) -> Result<()>;
    fn construct_config(&mut self, request: ChildRequest, _flags: u32) -> Result<()> {
        self.construct(request)
    }
    fn poll(&mut self, request: ChildRequest, delta: Ticks, permission: bool) -> Result<bool>;
    fn retired(&mut self, request: ChildRequest);
}
pub(super) struct ChildMode {
    parent: Arc<Mutex<OwnedMenu>>,
    request: ChildRequest,
    completed: bool,
}
impl ChildMode {
    pub(super) fn request(&self) -> ChildRequest {
        self.request
    }
}

/// Original1d924 retains an asynchronous worker and a callback to Menu+98.
pub(super) struct LayerReloadMode {
    parent: Arc<Mutex<OwnedMenu>>,
    worker:
        Option<shin_tasks::AsyncTask<Result<crate::adv::higurashi::layers::reload::LoadedReload>>>,
    ready: Option<crate::adv::higurashi::layers::reload::LoadedReload>,
    retirement: Option<shin_tasks::LoaderRetirement>,
    fault: Option<String>,
    completed: bool,
}
impl Drop for LayerReloadMode {
    fn drop(&mut self) {
        // Native1dff4 sets cancellation, joins the task, then retires factories.
        // Destruction does not release the menu's modal scope or invoke callback.
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(worker) = self.worker.take() {
                worker.cancel_and_retire();
            }
            if let Some(retirement) = self.retirement.take() {
                retirement.retire(self.ready.take());
            }
        }
    }
}
impl LayerReloadMode {
    fn poll(&mut self, services: &mut ModeServices<'_>) -> Result<u32> {
        if let Some(fault) = &self.fault {
            bail!("Higurashi layer reload failed: {fault}");
        }
        if let Some(worker) = &mut self.worker {
            if let Some(result) = worker.poll_naive() {
                self.worker = None;
                match result {
                    Ok(loaded) => self.ready = Some(loaded),
                    Err(error) => {
                        self.fault = Some(format!("{error:#}"));
                        return Err(error);
                    }
                }
            }
        }
        if let Some(loaded) = &mut self.ready {
            if let Err(error) = services.runner.publish_layer_reload(loaded) {
                self.fault = Some(format!("{error:#}"));
                return Err(error);
            }
            self.ready = None;
            services.modal.animations_enabled = true;
            self.parent.lock().model.layer_reload_finished(); // Original20320.
            self.completed = true; // phase0 ->1 ->-1 in the same poll.
        }
        Ok(0) // Completion is pruned on the next d11c8 poll, never consumes input.
    }
}

impl HigurashiHost {
    /// Native settings+100 and global play clock are independent of paused ADV.
    pub fn set_menu_frame(&mut self, setting: u8, play_ticks: f64) {
        self.runner.set_layer_setting(setting);
        self.menu_frame = MenuFrame {
            setting,
            play_ticks,
        };
    }

    /// Prepare all fallible resources and the cached-screen capture before
    /// acquiring native15038's counted owners. The title comes from the chapter-captured SAVEINFO1,
    /// already decoded by the scenario reader, and is snapshotted on opening.
    /// Scenario ID is the active GameData context's ID, not the CLI game enum.
    pub async fn open_system_menu(
        &mut self,
        scenario_id: i32,
        sounds: GlobalSounds,
        context: &mut crate::render::PreRenderContext<'_, '_, '_, '_>,
        screens: &crate::adv::higurashi::scene_textures::HigurashiSceneTextures,
    ) -> Result<()> {
        let assets = self.runner.assets().clone();
        let resources = Resources {
            atlas: assets.load("/adv.txa").await?,
            font: assets.load("/newrodin.fnt").await?,
            sounds: assets.load("/sysse.bin").await?,
        };
        self.open_system_menu_from_resources(scenario_id, sounds, resources, context, screens)
    }

    /// Attach the menu without starting another asset worker. All resources
    /// are supplied by the retained gameplay/startup owners; capture remains
    /// fallible and precedes counted pause/conceal/audio mutations.
    pub fn open_system_menu_from_resources(
        &mut self,
        scenario_id: i32,
        sounds: GlobalSounds,
        resources: Resources,
        context: &mut crate::render::PreRenderContext<'_, '_, '_, '_>,
        screens: &crate::adv::higurashi::scene_textures::HigurashiSceneTextures,
    ) -> Result<()> {
        ensure!(!self.has_modes(), "SystemMenu requires an idle mode stack");
        ensure!(
            self.runner.reconstruction_phase().is_none(),
            "SystemMenu cannot open during reconstruction"
        );
        if let Some(previous) = &self.system_sounds {
            ensure!(
                Arc::ptr_eq(previous, &sounds),
                "SystemMenu must retain the application's global sound owner"
            );
        }
        let title = self
            .runner
            .native_save_metadata()
            .context("SystemMenu requires native SAVEINFO metadata")?
            .saved()[1]
            .clone();
        let assets = self.runner.assets().clone();
        let view = View::from_loaded(
            &assets,
            resources.atlas,
            resources.font,
            &title,
            self.menu_frame.play_ticks,
        );
        let bank = resources.sounds;
        for name in ["move", "cancel", "decide"] {
            ensure!(
                bank.sounds.contains_key(name),
                "Missing Higurashi SystemMenu cue {name}"
            );
        }
        let snapshot = screens.capture_modal(context)?;
        sounds.lock().bank = Some(bank);
        self.system_sounds = Some(sounds);
        let setting = self.menu_frame.setting;
        let model = SystemMenu::new(
            scenario_id,
            setting,
            &mut Opening {
                host: self,
                snapshot: Some(snapshot),
            },
        );
        self.modes
            .push(HostMode::Menu(MenuOwner(Arc::new(Mutex::new(OwnedMenu {
                model,
                view,
                assets,
            })))));
        // d11c8 will publish the new borrowed draw snapshot on the next poll.
        self.mode_input_permission = false;
        Ok(())
    }

    /// Unimplemented child constructors stay owned and block lower input.
    /// Draining notifications does not complete their retained modes.
    pub fn take_menu_child_requests(&mut self) -> Vec<ChildRequest> {
        std::mem::take(&mut self.menu_requests)
    }
    pub(crate) fn take_manual_menu_requests(&mut self) -> Vec<ChildRequest> {
        let mut requests = Vec::new();
        self.menu_requests.retain(|request| {
            if matches!(request.kind, ChildKind::Item(0 | 1)) {
                requests.push(*request);
                false
            } else {
                true
            }
        });
        requests
    }
    pub(crate) fn take_config_menu_requests(&mut self) -> Vec<ChildRequest> {
        let mut requests = Vec::new();
        self.menu_requests.retain(|request| {
            if request.kind == ChildKind::Item(2) {
                requests.push(*request);
                false
            } else {
                true
            }
        });
        requests
    }
    pub(crate) fn take_help_menu_requests(&mut self) -> Vec<ChildRequest> {
        let mut requests = Vec::new();
        self.menu_requests.retain(|request| {
            if request.kind == ChildKind::Item(3) {
                requests.push(*request);
                false
            } else {
                true
            }
        });
        requests
    }
    pub(crate) fn take_chart_menu_requests(&mut self) -> Vec<ChildRequest> {
        let mut requests = Vec::new();
        self.menu_requests.retain(|request| {
            if request.kind == ChildKind::Item(4) {
                requests.push(*request);
                false
            } else {
                true
            }
        });
        requests
    }
    pub(crate) fn take_menu_tips_return(&mut self) -> bool {
        std::mem::take(&mut self.menu_tips_return)
    }
    pub(crate) fn take_menu_return(&mut self) -> Option<MenuReturn> {
        self.menu_return.take()
    }
    pub(crate) fn prepare_menu_questions(&mut self, context: &mut crate::render::PreRenderContext) {
        for mode in self.modes.iter_mut() {
            if let HostMode::TipsQuestion(question) = mode {
                question.prepare(context);
            }
            if let HostMode::MenuReturnQuestion(question) = mode {
                question.prepare(context);
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_menu_tips_question(&self) -> Option<(i32, i32)> {
        self.modes.iter().find_map(|mode| match mode {
            HostMode::TipsQuestion(question) => question.state(),
            _ => None,
        })
    }
    #[cfg(test)]
    pub(crate) fn reference_menu_return_question(&self) -> Option<(MenuReturn, i32, i32)> {
        self.modes.iter().find_map(|mode| match mode {
            HostMode::MenuReturnQuestion(question) => question.state(),
            _ => None,
        })
    }
    pub(crate) fn touch_diagnostic_menu_ready(&self) -> bool {
        self.modes.iter().any(|mode| match mode {
            HostMode::Menu(owner) => owner.0.lock().model.input_ready(),
            _ => false,
        })
    }
    #[cfg(test)]
    pub(crate) fn reference_system_menu_rows(&self) -> Option<(i32, Vec<(u32, f32)>)> {
        self.modes.iter().find_map(|mode| match mode {
            HostMode::Menu(owner) => {
                let owner = owner.0.lock();
                Some((
                    owner.model.phase(),
                    owner
                        .model
                        .rows()
                        .iter()
                        .map(|row| (row.item, row.slide.value()))
                        .collect(),
                ))
            }
            _ => None,
        })
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub(crate) fn reference_menu_control(&self) -> Option<(u32, bool)> {
        self.modes.iter().find_map(|mode| match mode {
            HostMode::Menu(owner) => {
                let owner = owner.0.lock();
                Some((
                    owner.model.rows()[owner.model.selected()].item,
                    owner.model.reference_input_ready(),
                ))
            }
            _ => None,
        })
    }

    /// Deliver the child's return-to-menu callback (1ba34).
    /// Accepted route/load/save callbacks need their real services
    /// and are deliberately not acknowledged by this operation.
    pub fn return_from_menu_child(&mut self, id: ChildId) -> Result<()> {
        let mut found = false;
        for mode in self.modes.iter_mut() {
            if let HostMode::MenuChild(child) = mode {
                if child.request.id != id {
                    continue;
                }
                if child.completed {
                    break;
                }
                child.parent.lock().model.child_returned(&mut ReturnOnly);
                child.completed = true;
                found = true;
                break;
            }
        }
        ensure!(found, "Unknown or already completed SystemMenu child");
        self.menu_requests.retain(|request| request.id != id);
        Ok(())
    }

    /// Originald147c draws only the last poll's snapshot, in forward order.
    /// Call after render_adv and the optional ADV UI/indicator owners.
    pub fn render_modes(
        &mut self,
        pass: &mut shin_render::render_pass::RenderPass<'_, '_, '_, '_, '_>,
    ) {
        self.render_modes_with_children(pass, |_, _| {});
    }
    pub(crate) fn render_modes_with_children(
        &mut self,
        pass: &mut shin_render::render_pass::RenderPass<'_, '_, '_, '_, '_>,
        mut child: impl FnMut(
            ChildRequest,
            &mut shin_render::render_pass::RenderPass<'_, '_, '_, '_, '_>,
        ),
    ) {
        self.modes.draw(|mode| match mode {
            HostMode::Menu(owner) => {
                let owner = owner.0.lock();
                owner.view.render(&owner.model, pass);
            }
            HostMode::Browser(owner) => owner.render(pass),
            HostMode::ChapterQuestion(owner) => owner.render(pass),
            HostMode::Save(owner) => owner.render(pass),
            HostMode::TipsQuestion(owner) => owner.render(pass),
            HostMode::MenuReturnQuestion(owner) => owner.render(pass),
            HostMode::MenuChild(owner) => child(owner.request, pass),
            _ => {}
        });
    }

    /// Global cues have no ADV modal pause/focus gate. The application calls
    /// this once per frame, including frames where its ADV scene is paused.
    pub fn update_system_sounds(&mut self, volume_percent: u8) {
        if let Some(sounds) = &self.system_sounds {
            let mut sounds = sounds.lock();
            sounds.set_settings_volume(volume_percent);
            sounds.update();
        }
    }
}

struct Opening<'a> {
    host: &'a mut HigurashiHost,
    snapshot: Option<shin_render::render_texture::RenderTexture>,
}
impl MenuServices for Opening<'_> {
    fn enter_modal(&mut self) {
        let snapshot = self.snapshot.take().expect("Constructor enters once");
        self.host
            .enter_modal(|_| Ok(snapshot))
            .expect("Prepared capture cannot fail");
    }
    fn exit_modal(&mut self) {
        unreachable!("Constructor does not exit")
    }
    fn cue(&mut self, _: Cue) {
        unreachable!("Constructor does not play a cue")
    }
    fn push_confirmation(&mut self, _: Confirmation) {
        unreachable!("Constructor does not confirm")
    }
    fn push_layer_reloader(&mut self) {
        unreachable!("Constructor does not reload layers")
    }
    fn dispatch_item(&mut self, _: u32) {
        unreachable!("Constructor does not dispatch")
    }
}
struct ReturnOnly;
impl MenuServices for ReturnOnly {
    fn enter_modal(&mut self) {
        unreachable!("1ba34 does not enter")
    }
    fn exit_modal(&mut self) {
        unreachable!("1ba34 does not exit")
    }
    fn cue(&mut self, _: Cue) {
        unreachable!("1ba34 does not play a cue")
    }
    fn push_confirmation(&mut self, _: Confirmation) {
        unreachable!("1ba34 does not confirm")
    }
    fn push_layer_reloader(&mut self) {
        unreachable!("1ba34 does not reload layers")
    }
    fn dispatch_item(&mut self, _: u32) {
        unreachable!("1ba34 does not dispatch")
    }
}

struct Polling<'a, 'b, 'c> {
    parent: Arc<Mutex<OwnedMenu>>,
    assets: Arc<AssetServer>,
    services: &'a mut ModeServices<'b>,
    pushes: &'a mut ModePush<'c, HostMode>,
    dispatched: bool,
    fault: Option<anyhow::Error>,
    config_flags: u32,
}
impl Polling<'_, '_, '_> {
    fn push_child(&mut self, kind: ChildKind) {
        let id = ChildId(*self.services.next_menu_child);
        *self.services.next_menu_child = self
            .services
            .next_menu_child
            .checked_add(1)
            .expect("SystemMenu child ID exhausted");
        let request = ChildRequest { id, kind };
        self.services.menu_requests.push(request);
        self.pushes.push(HostMode::MenuChild(ChildMode {
            parent: self.parent.clone(),
            request,
            completed: false,
        }));
        if let Some(callbacks) = self.services.menu_children.as_mut() {
            let result = if kind == ChildKind::Item(2) {
                callbacks.construct_config(request, self.config_flags)
            } else {
                callbacks.construct(request)
            };
            if let Err(error) = result {
                self.fault = Some(error);
            }
        }
    }
}
impl MenuServices for Polling<'_, '_, '_> {
    fn enter_modal(&mut self) {
        unreachable!("Polling does not enter a modal")
    }
    fn exit_modal(&mut self) {
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
    }
    fn cue(&mut self, cue: Cue) {
        let name = match cue {
            Cue::Move => "move",
            Cue::Cancel => "cancel",
            Cue::Confirm => "decide",
        };
        let mut sounds = self
            .services
            .system_sounds
            .expect("Menu prepared its global sound owner")
            .lock();
        sounds.update();
        sounds.play(&self.assets, name, Volume(1.0));
    }
    fn push_confirmation(&mut self, callback: Confirmation) {
        self.pushes
            .push(HostMode::MenuConfirmation(ConfirmationMode {
                parent: self.parent.clone(),
                callback,
            }));
    }
    fn push_layer_reloader(&mut self) {
        self.services.modal.animations_enabled = false;
        let worker = self
            .services
            .runner
            .begin_layer_reload(self.services.menu_frame.setting);
        let retirement = worker.retirement_handle();
        self.pushes.push(HostMode::LayerReload(LayerReloadMode {
            parent: self.parent.clone(),
            worker: Some(worker),
            ready: None,
            retirement,
            fault: None,
            completed: false,
        }));
    }
    fn dispatch_item(&mut self, item: u32) {
        self.dispatched = true;
        if matches!(item, 5 | 7) {
            self.pushes
                .push(HostMode::MenuReturnQuestion(returns::QuestionMode::new(
                    if item == 5 {
                        MenuReturn::Title
                    } else {
                        MenuReturn::Kakera
                    },
                    self.parent.clone(),
                    self.assets.clone(),
                )));
        } else if item == 6 {
            self.pushes
                .push(HostMode::TipsQuestion(tips::QuestionMode::new(
                    self.parent.clone(),
                    self.assets.clone(),
                )));
        } else {
            self.push_child(ChildKind::Item(item));
        }
    }
}

pub(super) fn completion(mode: &mut HostMode) -> u32 {
    match mode {
        HostMode::Menu(owner) => owner.0.lock().model.completion_word(),
        HostMode::MenuConfirmation(task) => task.callback.completion_word(),
        HostMode::MenuChild(child) => u32::from(child.completed),
        HostMode::LayerReload(task) => u32::from(task.completed),
        _ => unreachable!(),
    }
}
pub(super) fn poll(
    mode: &mut HostMode,
    delta: Ticks,
    permission: bool,
    services: &mut ModeServices<'_>,
    pushes: &mut ModePush<'_, HostMode>,
) -> Result<u32> {
    let parent = match mode {
        HostMode::Menu(owner) => owner.0.clone(),
        HostMode::MenuConfirmation(task) => task.parent.clone(),
        HostMode::MenuChild(child) => {
            if let Some(callbacks) = services.menu_children.as_mut() {
                if callbacks.poll(child.request, delta, permission)? {
                    child.parent.lock().model.child_returned(&mut ReturnOnly);
                    child.completed = true;
                }
            }
            return Ok(0);
        }
        HostMode::LayerReload(task) => return task.poll(services),
        _ => bail!("Invalid SystemMenu mode"),
    };
    let frame = services.menu_frame;
    let buttons = services.buttons;
    let mouse_release = services.mouse_release;
    let empty = crate::adv::input_area::PointerFrame {
        number: 0,
        time_seconds: 0.0,
        events: &[],
    };
    let pointer = services.pointer.unwrap_or(&empty);
    let mut owner = parent.lock();
    let mut callbacks = Polling {
        parent: parent.clone(),
        assets: owner.assets.clone(),
        services,
        pushes,
        dispatched: false,
        fault: None,
        config_flags: owner.model.config_flags(),
    };
    let result = match mode {
        HostMode::Menu(_) => {
            let result = owner.model.poll_with_mouse_release(
                delta,
                u32::from(permission),
                buttons.pressed,
                buttons.repeat,
                pointer,
                frame.setting,
                frame.play_ticks,
                &mut callbacks,
                mouse_release,
            );
            let OwnedMenu { view, assets, .. } = &mut *owner;
            view.update_time(assets, frame.play_ticks);
            result
        }
        HostMode::MenuConfirmation(task) => task.callback.poll(&mut owner.model, &mut callbacks),
        _ => unreachable!(),
    };
    if let Some(error) = callbacks.fault.take() {
        return Err(error);
    }
    if callbacks.dispatched {
        owner.model.child_started();
    }
    Ok(result)
}

#[cfg(test)]
pub(in crate::adv::higurashi) mod tests;
