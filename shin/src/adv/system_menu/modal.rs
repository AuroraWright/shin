//! Native ADV task owner for SystemMenu and its child callbacks.
use std::sync::Arc;

use enum_map::EnumMap;
use shin_core::{format::scenario::Scenario, time::Ticks};
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;

use crate::adv::{
    characters::screen::CharacterScreen,
    characters::view::CharacterTextures,
    config::screen::ConfigScreen,
    input_area::{MouseRelease, PointerEvent, PointerFrame},
    manual::screen::ManualScreen,
    save_menu::screen::SaveMenuScreen,
    system_menu::{SystemMenuPhase, SystemMenuSnapshot},
    system_menu::screen::{SystemMenuRequest, SystemMenuScreen},
    tips::screen::TipsScreen,
    title_return::TitleReturnDialog,
};
use crate::{app::AppAction, asset::system::AssetServer};

#[cfg(all(test, target_os = "macos"))]
mod failure_reference;

enum Child {
    Manual(ManualScreen),
    Characters(CharacterScreen),
    Tips(TipsScreen),
    Config(ConfigScreen),
    SaveMenu(SaveMenuScreen),
    TitleReturn(TitleReturnDialog),
}
pub(crate) struct SystemMenuModal {
    pub screen: SystemMenuScreen,
    child: Option<Child>,
    pending_request: Option<SystemMenuRequest>,
    pending_title_setup: Option<crate::title::TitleSetup>,
    pointer_extent: (u16, u16),
    pointer_held: bool,
    pending_input: Option<(
        u32,
        f64,
        Vec<PointerEvent>,
        EnumMap<AppAction, ActionState>,
        f32,
        Option<MouseRelease>,
    )>,
}
impl SystemMenuModal {
    /// Failed child construction/loading returns ownership to the same parent.
    /// ADV keeps its modal scopes until the parent itself closes.
    pub(crate) fn cancel_failed_child(&mut self) {
        self.child = None;
        self.pending_request = None;
        self.pending_title_setup = None;
        self.screen.child_response(1);
        self.suspend_pointer(self.pointer_held);
    }
    pub(crate) fn suspend_pointer(&mut self, held: bool) {
        self.pending_input = None;
        self.pointer_held = held;
        self.screen.set_pointer_extent(self.pointer_extent);
        match &mut self.child {
            Some(Child::Config(screen)) => screen.suspend_pointer(held),
            Some(Child::SaveMenu(screen)) => screen.suspend_pointer(held),
            Some(Child::TitleReturn(dialog)) => {
                dialog.set_pointer_extent(self.pointer_extent, held)
            }
            _ => {}
        }
    }
    pub fn new(
        server: &Arc<AssetServer>,
        snapshot: SystemMenuSnapshot,
        textures: Arc<CharacterTextures>,
        play_ticks: f64,
        pointer_extent: (u16, u16),
    ) -> Self {
        let mut screen = SystemMenuScreen::new(server, snapshot, textures, play_ticks);
        screen.set_pointer_extent(pointer_extent);
        Self {
            screen,
            child: None,
            pending_request: None,
            pending_title_setup: None,
            pointer_extent,
            pointer_held: false,
            pending_input: None,
        }
    }
    pub fn set_pointer_extent(&mut self, extent: (u16, u16)) {
        if self.pointer_extent == extent {
            return;
        }
        self.pointer_extent = extent;
        self.screen.set_pointer_extent(extent);
        if let Some(Child::Manual(child)) = &mut self.child {
            child.set_pointer_extent(extent);
        }
        if let Some(Child::Tips(child)) = &mut self.child {
            child.set_pointer_extent(extent);
        }
        if let Some(Child::Config(child)) = &mut self.child {
            child.set_pointer_extent(extent);
        }
        if let Some(Child::SaveMenu(child)) = &mut self.child {
            child.set_pointer_extent(extent);
        }
        if let Some(Child::TitleReturn(child)) = &mut self.child {
            child.set_pointer_extent(extent, self.pointer_held);
        }
    }
    pub fn pending_request(&self) -> Option<&SystemMenuRequest> {
        self.pending_request.as_ref()
    }
    pub(crate) fn desktop_diagnostic_ready(&self) -> bool {
        self.child.is_none() && self.pending_request.is_none() && self.screen.model.accepts_input()
    }
    pub fn input(
        &mut self,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        axis: f32,
    ) {
        self.input_with_mouse_release(frame, state, axis, None);
    }
    pub fn input_with_mouse_release(
        &mut self,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        axis: f32,
        mouse_release: Option<MouseRelease>,
    ) {
        self.pending_input = Some((
            frame.number,
            frame.time_seconds,
            frame.events.to_vec(),
            state.clone(),
            axis,
            mouse_release,
        ));
    }
    pub fn pointer_state(&mut self, canvas: glam::Vec2, held: bool, elapsed_seconds: f64) {
        self.pointer_held = held;
        if let Some(Child::Characters(child)) = &mut self.child {
            child.pointer_state(canvas, held, elapsed_seconds);
        }
        if let Some(Child::Config(child)) = &mut self.child {
            child.pointer_state(canvas, held, elapsed_seconds);
        }
        if let Some(Child::SaveMenu(child)) = &mut self.child {
            child.pointer_state(canvas, held, elapsed_seconds);
        }
        if let Some(Child::TitleReturn(child)) = &mut self.child {
            child.pointer_state(canvas, held, elapsed_seconds);
        }
    }
    /// Parent rows keep retreating while the child owns input. Only response1
    /// reopens them; Manual response0 leaves ownership with the parent/child.
    pub fn update(
        &mut self,
        delta: Ticks,
        server: &Arc<AssetServer>,
        scenario: &Arc<Scenario>,
        play_ticks: f64,
        settings: &shin_core::format::save::Settings,
        saves: &shin_core::format::save::SaveData,
        audio: &Arc<shin_audio::AudioManager>,
        canvas: (u32, u32),
    ) -> anyhow::Result<bool> {
        if let Some((number, time_seconds, events, state, axis, mouse_release)) =
            self.pending_input.take()
        {
            let frame = PointerFrame {
                number,
                time_seconds,
                events: &events,
            };
            self.screen.update_frame_with_mouse_release(
                delta,
                server,
                play_ticks,
                &frame,
                &state,
                mouse_release,
            );
            match &mut self.child {
                Some(Child::Manual(child)) => child.input_frame(&frame, &state),
                Some(Child::Tips(child)) => child.input_frame(&frame, &state, axis),
                Some(Child::Config(child)) => child.input_frame(&frame, &state, axis),
                Some(Child::SaveMenu(child)) => child.input_frame(&frame, &state, axis),
                Some(Child::TitleReturn(child)) => child.keyboard(&state),
                Some(Child::Characters(child)) if !child.consumes_keyboard() => {
                    child.model.handle_controller_input(&state, axis)
                }
                _ => {}
            }
        } else {
            self.screen.update(delta, server, play_ticks);
        }
        let response = match &mut self.child {
            Some(Child::Manual(child)) => child.update(delta)?,
            Some(Child::Tips(child)) => child.update(delta)?,
            Some(Child::Config(child)) => child.update(delta)?,
            Some(Child::SaveMenu(child)) => child.update(delta)?,
            Some(Child::TitleReturn(child)) => {
                if child.update(delta) {
                    self.screen.child_response(1);
                }
                None
            }
            Some(Child::Characters(child)) => {
                child.update(delta, server);
                child.model.is_closed().then_some(1)
            }
            None => None,
        };
        if let Some(response) = response {
            self.screen.child_response(response);
            if response == 1 {
                self.child = None;
            }
        }
        if self.child.as_ref().is_some_and(|child| {
            matches!(child,
            Child::TitleReturn(dialog) if dialog.is_declined_and_done())
        }) {
            self.child = None;
        }
        if let Some(request) = self.screen.take_request() {
            match request {
                SystemMenuRequest::Save | SystemMenuRequest::Load => {
                    let entry = (request == SystemMenuRequest::Save)
                        .then(|| self.screen.snapshot().entry.clone());
                    let mut child = SaveMenuScreen::new(
                        server.clone(),
                        audio.clone(),
                        scenario.clone(),
                        saves.clone(),
                        entry,
                        canvas,
                    );
                    child.set_pointer_extent(self.pointer_extent);
                    self.child = Some(Child::SaveMenu(child));
                }
                SystemMenuRequest::Config => {
                    let mut child = ConfigScreen::new(server.clone(), settings.clone());
                    child.set_pointer_extent(self.pointer_extent);
                    self.child = Some(Child::Config(child));
                }
                SystemMenuRequest::Manual => {
                    let mut child = ManualScreen::new(server.clone());
                    child.set_pointer_extent(self.pointer_extent);
                    self.child = Some(Child::Manual(child));
                }
                SystemMenuRequest::Characters {
                    grid, initial_page, ..
                } => {
                    self.child = Some(Child::Characters(CharacterScreen::new(
                        server,
                        scenario,
                        grid,
                        initial_page,
                    )?));
                }
                SystemMenuRequest::Information { episode, tips } => {
                    let mut child =
                        TipsScreen::new(server.clone(), scenario, episode, Some(&tips))?;
                    child.set_pointer_extent(self.pointer_extent);
                    self.child = Some(Child::Tips(child));
                }
                SystemMenuRequest::ReturnToTitle => {
                    let mut child = TitleReturnDialog::load(server, self.pointer_held)?;
                    child.set_pointer_extent(self.pointer_extent, self.pointer_held);
                    self.child = Some(Child::TitleReturn(child));
                }
            }
        }
        Ok(self.screen.model.phase() == SystemMenuPhase::Closed)
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        let mut sounds = self.screen.take_sounds();
        match &mut self.child {
            Some(Child::Manual(child)) => sounds.extend(child.take_sounds()),
            Some(Child::Characters(child)) => sounds.extend(child.model.take_sounds()),
            Some(Child::Tips(child)) => sounds.extend(child.take_sounds()),
            Some(Child::Config(child)) => sounds.extend(child.take_sounds()),
            Some(Child::SaveMenu(child)) => sounds.extend(child.take_sounds()),
            Some(Child::TitleReturn(child)) => sounds.extend(child.take_sounds()),
            None => {}
        }
        sounds
    }
    pub fn render(&self, pass: &mut RenderPass) {
        self.screen.render(pass);
        match &self.child {
            Some(Child::Manual(child)) => child.render(pass),
            Some(Child::Characters(child)) => child.render(pass),
            Some(Child::Tips(child)) => child.render(pass),
            Some(Child::Config(child)) => child.render(pass),
            Some(Child::SaveMenu(child)) => child.render(pass),
            Some(Child::TitleReturn(child)) => child.render(pass),
            None => {}
        }
    }
    pub fn take_title_save_request(&mut self) -> bool {
        match &mut self.child {
            Some(Child::TitleReturn(child)) => child.take_save_request(),
            _ => false,
        }
    }
    pub fn title_save_finished(&mut self, saves: &shin_core::format::save::SaveData) {
        if let Some(Child::TitleReturn(child)) = &mut self.child {
            child.save_finished();
            if child.scene_ready() && self.pending_title_setup.is_none() {
                self.pending_title_setup = Some(crate::title::TitleSetup::from_save(saves, -1));
                // Native title scene factory10dfa0(-1) remains owned by the
                // scene host. Keep ADV paused until that factory replaces it.
                self.pending_request = Some(SystemMenuRequest::ReturnToTitle);
            }
        }
    }
    pub fn pending_title_setup(&self) -> Option<&crate::title::TitleSetup> {
        self.pending_title_setup.as_ref()
    }
    #[cfg(test)]
    pub fn reference_title_return(&self) -> Option<&TitleReturnDialog> {
        match &self.child {
            Some(Child::TitleReturn(child)) => Some(child),
            _ => None,
        }
    }
    pub fn take_save_menu_requests(&mut self) -> Vec<crate::adv::save_menu::SaveMenuRequest> {
        match &mut self.child {
            Some(Child::SaveMenu(child)) => child.take_requests(),
            _ => Vec::new(),
        }
    }
    pub fn save_menu_write_finished(
        &mut self,
        success: bool,
        game: Option<shin_core::format::save::GameData>,
    ) {
        if let Some(Child::SaveMenu(child)) = &mut self.child {
            child.write_finished(success, game);
        }
    }
    pub fn save_menu_write_started(&mut self, game: Option<shin_core::format::save::GameData>) {
        if let Some(Child::SaveMenu(child)) = &mut self.child {
            child.write_started(game);
        }
    }
    #[cfg(test)]
    pub fn reference_save_menu(&mut self) -> Option<&mut SaveMenuScreen> {
        match &mut self.child {
            Some(Child::SaveMenu(child)) => Some(child),
            _ => None,
        }
    }
    pub fn config_settings(&self) -> Option<&shin_core::format::save::Settings> {
        match &self.child {
            Some(Child::Config(child)) => Some(child.settings()),
            _ => None,
        }
    }
    pub fn take_config_save_request(&mut self) -> bool {
        match &mut self.child {
            Some(Child::Config(child)) => child.take_save_request(),
            _ => false,
        }
    }
    pub fn config_save_finished(&mut self) {
        if let Some(Child::Config(child)) = &mut self.child {
            child.save_finished();
        }
    }
    pub fn config_save_failed(&mut self) {
        if let Some(Child::Config(child)) = &mut self.child {
            child.save_failed();
        }
    }
    pub fn prepare(&mut self, context: &mut crate::render::PreRenderContext) {
        if let Some(Child::Config(child)) = &mut self.child {
            child.prepare(context);
        }
        if let Some(Child::SaveMenu(child)) = &mut self.child {
            child.prepare(context);
        }
        if let Some(Child::TitleReturn(child)) = &mut self.child {
            child.prepare(context);
        }
    }
    pub fn take_config_edits(&mut self) -> Vec<crate::adv::config::widgets::ConfigEdit> {
        match &mut self.child {
            Some(Child::Config(child)) => child.take_edits(),
            _ => Vec::new(),
        }
    }
    #[cfg(test)]
    pub fn reference_config_if_any(&self) -> Option<&ConfigScreen> {
        match &self.child {
            Some(Child::Config(child)) => Some(child),
            _ => None,
        }
    }
    #[cfg(test)]
    pub fn wait_reference_config(&mut self) -> &mut ConfigScreen {
        let Some(Child::Config(child)) = &mut self.child else {
            panic!("Expected live Config child");
        };
        child.wait_reference_assets().unwrap();
        child
    }
    #[cfg(test)]
    pub fn reference_config(&self) -> &ConfigScreen {
        let Some(Child::Config(child)) = &self.child else {
            panic!("Expected live Config child");
        };
        child
    }
    #[cfg(test)]
    pub fn wait_reference_manual(&mut self) -> &mut ManualScreen {
        let Some(Child::Manual(child)) = &mut self.child else {
            panic!("Expected live Manual child");
        };
        child.wait_reference_assets().unwrap();
        child
    }
    #[cfg(test)]
    pub fn reference_tips_if_any(&self) -> Option<&TipsScreen> {
        match &self.child {
            Some(Child::Tips(child)) => Some(child),
            _ => None,
        }
    }
    #[cfg(test)]
    pub fn reference_tips(&self) -> &TipsScreen {
        let Some(Child::Tips(child)) = &self.child else {
            panic!("Expected live Tips child");
        };
        child
    }
    #[cfg(test)]
    pub fn wait_reference_tips(&mut self) -> &mut TipsScreen {
        let Some(Child::Tips(child)) = &mut self.child else {
            panic!("Expected live Tips child");
        };
        child.wait_reference_assets().unwrap();
        child
    }
    #[cfg(test)]
    pub fn has_child(&self) -> bool {
        self.child.is_some()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_adv_system_menu_y_hidden_window_b_task_and_scope_lifetime() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x120b0, 0xb9400fe8),
            (0x120b4, 0x37380c28),
            (0x120b8, 0x37300d08),
            (0x120c8, 0x39403fe8),
            (0x120cc, 0x37280be8),
            (0x12238, 0xaa1303e0),
            (0x1223c, 0x94000439),
            (0x12248, 0xaa1303e0),
            (0x1224c, 0x94000535),
            (0x12258, 0xaa1303e0),
            (0x1225c, 0x94000469),
            (0x13334, 0xf9437908),
            (0x13344, 0x94004fdb),
            (0x1334c, 0xb9047e7f),
            (0x13374, 0x3913e27f),
            (0x13388, 0x9400235e),
            (0x13398, 0x9403b336),
            (0x13738, 0xb9047c1f),
            (0x13774, 0x94002183),
            (0x1c1b4, 0x97ffde2f),
            (0x13a80, 0x9400001c),
            (0x13aa8, 0x320003e1),
            (0x13aac, 0x940002d1),
            (0x13ab0, 0xb9447668),
            (0x13ab4, 0x35000128),
            (0x13ac0, 0x320003e1),
            (0x13ac4, 0x940419cb),
            (0x13acc, 0x320003e1),
            (0x13ad0, 0x94037ba4),
            (0x13ad8, 0x11000508),
            (0x13adc, 0xb9047668),
            (0x14580, 0x3911c808),
            (0x1458c, 0x71000508),
            (0x14590, 0xb9047408),
            (0x14594, 0x54000101),
            (0x145a0, 0x2a1f03e1),
            (0x145a4, 0x94041713),
            (0x145ac, 0x2a1f03e1),
            (0x145b0, 0x940378ec),
            (0x145b8, 0x2a1f03e1),
            (0x145bc, 0x9400000d),
            (0x145c4, 0x94029b97),
            (0x145d0, 0x1e2703e0),
            (0x145e4, 0x14029d07),
            (0x1d2b8, 0x97ffdcae),
            (0x1d2c0, 0xb900aa68),
            (0x1111c, 0xb9447668),
            (0x11124, 0xb9447a68),
            (0x1113c, 0x9402aa2d),
            (0x11150, 0x540004cd),
            (0x112ec, 0x9403bb71),
            (0x112f8, 0x940002ea),
            (0x11304, 0xb9447668),
            (0x11308, 0x35001368),
            (0x11614, 0x9402a90f),
            (0x1162c, 0x1e202008),
            (0x11638, 0x9402a8ee),
            (0x1163c, 0x37000100),
            (0x1d064, 0x94027a63),
            (0x1d08c, 0x36001240),
            (0x1d2d8, 0x9401398a),
            (0x1d098, 0xa94bd674),
            (0x1d0ac, 0x94027a69),
            (0x1d0b8, 0x94027a66),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native menu owner{address:x}"
            );
        }
        for (address, name) in [
            (0x164d82, b"N4shin3ADV12HiddenWindowE".as_slice()),
            (0x164d9c, b"N4shin3ADV10SystemMenuE".as_slice()),
            (0x161e86, b"page".as_slice()),
        ] {
            assert_eq!(&exe[address + 256..address + 256 + name.len()], name);
        }
    }
}
