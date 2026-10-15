//! Native SystemMenu screen: retained VM metadata, authored rows and live clock.
//! The ADV owner supplies loaded artwork, modal ownership and child routing.
use super::{
    character_view::CharacterTextures,
    system_menu::{SystemMenuModel, SystemMenuSnapshot},
    system_menu_view::{self, SystemMenuDetail},
};
use crate::{app::AppAction, asset::system::AssetServer};
use enum_map::EnumMap;
use shin_core::time::Ticks;
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemMenuRequest {
    Save,
    Load,
    Config,
    Characters {
        episode: i32,
        grid: i32,
        initial_page: i32,
    },
    Information {
        episode: i32,
        tips: Vec<i32>,
    },
    Manual,
    ReturnToTitle,
}
impl SystemMenuSnapshot {
    /// Native1c1c0 selects the active40byte save entry, then1c1cc reads its
    /// scenario_id. Unsigned comparison excludes negative and non-story IDs.
    pub fn episode(&self) -> i32 {
        if (self.entry.entry.scenario_id as u32) < 24 {
            self.entry.entry.scenario_id & 7
        } else {
            -1
        }
    }
    fn request(&self, item: u8) -> SystemMenuRequest {
        match item {
            0 => SystemMenuRequest::Save,
            1 => SystemMenuRequest::Load,
            2 => SystemMenuRequest::Config,
            3 => SystemMenuRequest::Characters {
                episode: self.episode(),
                grid: self.information.character_grid,
                initial_page: self.information.character_grid_argument,
            },
            4 => SystemMenuRequest::Information {
                episode: self.episode(),
                tips: self.information.acquired_tips.clone(),
            },
            5 => SystemMenuRequest::Manual,
            6 => SystemMenuRequest::ReturnToTitle,
            _ => unreachable!("Native menu has only seven authored items"),
        }
    }
}
pub struct SystemMenuScreen {
    pub model: SystemMenuModel,
    snapshot: SystemMenuSnapshot,
    textures: Arc<CharacterTextures>,
    detail: SystemMenuDetail,
    input_area: super::input_area::InputArea,
}
impl SystemMenuScreen {
    /// Native ADV owns adv.txa before menu construction. The caller supplies it;
    /// the screen retains its own title/character/tip snapshot and reads live time.
    pub fn new(
        server: &AssetServer,
        snapshot: SystemMenuSnapshot,
        textures: Arc<CharacterTextures>,
        play_ticks: f64,
    ) -> Self {
        let model = snapshot.model();
        let detail = SystemMenuDetail::load(server, &snapshot.scene_title, play_ticks);
        Self {
            model,
            snapshot,
            textures,
            detail,
            input_area: super::input_area::InputArea::canvas(),
        }
    }
    pub fn snapshot(&self) -> &SystemMenuSnapshot {
        &self.snapshot
    }
    /// Set physical viewport dimensions, matching native6afc0.
    /// A desktop resize cancels an existing capture before replacing its transform.
    pub fn set_pointer_extent(&mut self, extent: (u16, u16)) {
        self.input_area.disable(&mut self.model);
        self.input_area = super::input_area::InputArea::new(extent.0, extent.1);
    }
    /// Dispatch pointer contacts before controller input, once per frame.
    /// Coordinates use the configured physical viewport (canvas by default).
    pub fn input_frame(
        &mut self,
        frame: &super::input_area::PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
    ) {
        self.input_frame_with_mouse_release(frame, state, None);
    }
    fn input_frame_with_mouse_release(
        &mut self,
        frame: &super::input_area::PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        mouse_release: Option<super::input_area::MouseRelease>,
    ) {
        self.input_area.input_frame_with_mouse_release(
            &mut self.model,
            frame,
            state,
            mouse_release,
        );
    }
    pub fn input(&mut self, state: &EnumMap<AppAction, ActionState>) {
        self.model.input(state);
    }
    /// Native1d050 prepares task eligibility, delivers input, then1d098 advances clocks.
    pub(super) fn update_frame(
        &mut self,
        delta: Ticks,
        server: &AssetServer,
        play_ticks: f64,
        frame: &super::input_area::PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
    ) {
        self.update_frame_with_mouse_release(delta, server, play_ticks, frame, state, None);
    }
    pub(super) fn update_frame_with_mouse_release(
        &mut self,
        delta: Ticks,
        server: &AssetServer,
        play_ticks: f64,
        frame: &super::input_area::PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        mouse_release: Option<super::input_area::MouseRelease>,
    ) {
        self.model.prepare_frame();
        self.input_frame_with_mouse_release(frame, state, mouse_release);
        self.model.advance_clocks(delta.as_f32());
        self.detail.update_time(server, play_ticks);
    }
    pub fn update(&mut self, delta: Ticks, server: &AssetServer, play_ticks: f64) {
        self.model.update(delta.as_f32());
        self.detail.update_time(server, play_ticks);
    }
    pub fn take_request(&mut self) -> Option<SystemMenuRequest> {
        self.model
            .take_item()
            .map(|item| self.snapshot.request(item))
    }
    pub fn child_response(&mut self, response: i32) {
        self.model.child_response(response);
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        self.model.take_sounds()
    }
    pub fn render(&self, pass: &mut RenderPass) {
        system_menu_view::render_rows(
            &self.model,
            self.textures.get("sysmenu"),
            pass,
            crate::render::top_left_projection_matrix(),
        );
        self.detail
            .render(&self.model, self.textures.get("info"), pass);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_system_menu_episode_mapping_and_child_arguments() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x1c1b8, 0xf9400a68),
            (0x1c1bc, 0xf9423509),
            (0x1c1c0, 0xb9a53d2a),
            (0x1c1c4, 0x5280050b),
            (0x1c1c8, 0x9b0b2549),
            (0x1c1cc, 0xb9654129),
            (0x1c1d0, 0x1200092a),
            (0x1c1d4, 0x7100613f),
            (0x1c1d8, 0x5a9f3149),
            (0x1c1dc, 0x5292230a),
            (0x1c1e0, 0xb9008269),
            (0xf730, 0xb9a53c08),
            (0xf734, 0x5284a80a),
            (0xf738, 0x52800509),
            (0xf73c, 0x8b0a000a),
            (0xf740, 0x9b092908),
            (0x1e110, 0xb980ac08),
            (0x1e114, 0x5280170a),
            (0x1e118, 0x9b0a7d08),
            (0x1e11c, 0xf9405c09),
            (0x1e120, 0xb8686928),
            (0x1e124, 0xaa0003f3),
            (0x1e128, 0x7100191f),
            (0x1e12c, 0x540002e8),
            (0x1e130, 0xd0000a29),
            (0x1e134, 0x91354129),
            (0x1e138, 0x1000008a),
            (0x1e13c, 0x3868692b),
            (0x1e140, 0x8b0b094a),
            (0x1e144, 0xd61f0140),
            (0x1e264, 0x29500a61),
            (0x1e268, 0xb9408a63),
            (0x1e26c, 0xd10103a8),
            (0x1e270, 0x910003e0),
            (0x1e274, 0x9400dba3),
            (0x1e278, 0x91056280),
            (0x1e27c, 0xd10103a1),
            (0x1e280, 0x9403877c),
            (0x1e2a0, 0xb9408261),
            (0x1e2a4, 0x91024262),
            (0x1e2a8, 0xd10103a8),
            (0x1e2ac, 0x910003e0),
            (0x1e2b0, 0x9403ae8c),
            (0x1e2b4, 0x91056280),
            (0x1e2b8, 0xd10103a1),
            (0x1e2bc, 0x9403876d),
            (0x1e2c4, 0xf9400a74),
            (0x1e2c8, 0x90000da8),
            (0x1e2cc, 0x912d2108),
            (0x1e2d0, 0xa9004fe8),
            (0x1e2d4, 0x910003e8),
            (0x1e2d8, 0xf90013e8),
            (0x1e2dc, 0xd10103a8),
            (0x1e2e0, 0x910003e0),
            (0x1e2e4, 0x9401a387),
            (0x1e2e8, 0x91056280),
            (0x1e2ec, 0xd10103a1),
            (0x1e2f0, 0x94038760),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native instruction at {address:x}"
            );
        }
    }
    #[test]
    fn native_episode_mapping_and_child_payloads_use_retained_menu_metadata() {
        for scenario in (-2..=26).chain([i32::MIN, i32::MAX]) {
            let mut vm = super::super::VmState::for_scenario(scenario, 42);
            vm.information.select_character_grid(12, -3);
            vm.information.acquire_tips(&[7, 2]);
            let snapshot = SystemMenuSnapshot::from_vm(&vm);
            let episode = if (0..24).contains(&scenario) {
                scenario % 8
            } else {
                -1
            };
            assert_eq!(snapshot.episode(), episode);
            assert_eq!(snapshot.request(0), SystemMenuRequest::Save);
            assert_eq!(snapshot.request(1), SystemMenuRequest::Load);
            assert_eq!(snapshot.request(2), SystemMenuRequest::Config);
            assert_eq!(
                snapshot.request(3),
                SystemMenuRequest::Characters {
                    episode,
                    grid: 12,
                    initial_page: -3
                }
            );
            assert_eq!(
                snapshot.request(4),
                SystemMenuRequest::Information {
                    episode,
                    tips: vec![7, 2]
                }
            );
            assert_eq!(snapshot.request(5), SystemMenuRequest::Manual);
            assert_eq!(snapshot.request(6), SystemMenuRequest::ReturnToTitle);
        }
    }
}
