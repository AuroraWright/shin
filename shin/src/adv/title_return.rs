//! SystemMenu title-return confirmation1e344 and callback1ff70.
//! The title scene itself is a separate factory, not a restart of ADV bytecode.
use super::save_menu_dialog::SaveMenuDialog;
use crate::{app::AppAction, asset::system::AssetServer, render::PreRenderContext};
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::time::Ticks;
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;

pub(super) const PROMPT: &str = "ゲームを終了して、タイトル画面に戻ります。@rよろしいですか？";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Confirming,
    Declined,
    WaitingSave,
    Ready,
}

struct TitleReturnModel {
    phase: Phase,
    reopen: bool,
    save: bool,
    dialog_done: bool,
}
impl Default for TitleReturnModel {
    fn default() -> Self {
        Self {
            phase: Phase::Confirming,
            reopen: false,
            save: false,
            dialog_done: false,
        }
    }
}
impl TitleReturnModel {
    fn response(&mut self, final_flag: bool, button: i32) {
        self.dialog_done |= final_flag;
        if self.phase != Phase::Confirming {
            return;
        }
        // Native1ff88..1ff90: No or Cancel reopens at intermediate0;
        // the final callback must not restart the parent animation again.
        if !final_flag && button != 0 {
            self.phase = Phase::Declined;
            self.reopen = true;
        } else if final_flag && button == 0 {
            // Native1ff94 ->147e0 starts SaveGame with flag1, then187c0
            // queues title factory10dfa0(-1) after storage completion.
            self.phase = Phase::WaitingSave;
            self.save = true;
        }
    }
    fn save_finished(&mut self) {
        if self.phase == Phase::WaitingSave && !self.save {
            self.phase = Phase::Ready;
        }
    }
}

pub(super) struct TitleReturnDialog {
    dialog: SaveMenuDialog,
    model: TitleReturnModel,
}
impl TitleReturnDialog {
    pub fn load(server: &AssetServer, held: bool) -> anyhow::Result<Self> {
        Ok(Self {
            dialog: SaveMenuDialog::load_prompt(server, PROMPT, held)?,
            model: TitleReturnModel::default(),
        })
    }
    pub fn set_pointer_extent(&mut self, extent: (u16, u16), held: bool) {
        self.dialog.set_pointer_extent(extent, held);
    }
    pub fn pointer_state(&mut self, position: Vec2, held: bool, elapsed: f64) {
        self.dialog.pointer_state(position, held, elapsed);
    }
    pub fn keyboard(&mut self, state: &EnumMap<AppAction, ActionState>) {
        self.dialog.keyboard(state);
    }
    pub fn update(&mut self, delta: Ticks) -> bool {
        for response in self.dialog.update(delta) {
            self.model.response(response.final_flag, response.button);
        }
        std::mem::take(&mut self.model.reopen)
    }
    pub fn is_declined_and_done(&self) -> bool {
        self.model.phase == Phase::Declined && self.model.dialog_done
    }
    pub fn take_save_request(&mut self) -> bool {
        std::mem::take(&mut self.model.save)
    }
    pub fn save_finished(&mut self) {
        self.model.save_finished();
    }
    pub fn scene_ready(&self) -> bool {
        self.model.phase == Phase::Ready
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        self.dialog.take_sounds()
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) {
        self.dialog.prepare(context);
    }
    pub fn render(&self, pass: &mut RenderPass) {
        self.dialog.render(pass);
    }
    #[cfg(test)]
    pub fn reference_is_open(&self) -> bool {
        self.dialog.reference_is_open()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_and_cancel_reopen_once_at_intermediate_callback() {
        for button in [1, -1] {
            let mut model = TitleReturnModel::default();
            model.response(false, button);
            assert!(std::mem::take(&mut model.reopen));
            assert!(!model.dialog_done && !model.save);
            model.response(false, button);
            model.response(true, button);
            assert!(model.dialog_done);
            assert!(!model.reopen && !model.save);
            model.save_finished();
            assert_eq!(model.phase, Phase::Declined);
        }
    }
    #[test]
    fn yes_waits_for_final_callback_and_storage_completion() {
        let mut model = TitleReturnModel::default();
        model.response(false, 0);
        model.save_finished();
        assert_eq!(model.phase, Phase::Confirming);
        assert!(!model.reopen && !model.save && !model.dialog_done);
        model.response(true, 0);
        assert_eq!(model.phase, Phase::WaitingSave);
        model.save_finished(); // Cannot complete an undelivered request.
        assert_eq!(model.phase, Phase::WaitingSave);
        assert!(std::mem::take(&mut model.save));
        model.response(true, 0);
        assert!(!model.save);
        model.save_finished();
        assert_eq!(model.phase, Phase::Ready);
        model.save_finished();
        assert_eq!(model.phase, Phase::Ready);
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_title_return_prompt_and_callback_save_title_factory_chain() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        assert_eq!(
            &exe[0x161dfe + 256..0x161dfe + 256 + PROMPT.len()],
            PROMPT.as_bytes()
        );
        for (address, expected) in [
            (0x1e358, 0x9137f821),
            (0x1e364, 0x320003e1),
            (0x1e368, 0x94000fda),
            (0x1ff70, 0xb9400029),
            (0x1ff78, 0x7100053f),
            (0x1ff80, 0x7a400900),
            (0x1ff90, 0x17fff534),
            (0x1ff98, 0x17ffd212),
            (0x14818, 0x320003e2),
            (0x1481c, 0x940302d5),
            (0x187e4, 0x12800000),
            (0x187f0, 0x9403d5ec),
            (0x187f8, 0x94003ade),
            (0x10dfa8, 0xb9000900),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
    }
}
