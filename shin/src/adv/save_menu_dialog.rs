//! Native SaveMenu generic Yes/No confirmation child.
use super::{
    backlog_rollback::{Confirmation, Outcome},
    dialog_input::DialogInput,
    dialog_view::DialogView,
    save_menu::SlotAction,
};
use crate::{app::AppAction, asset::system::AssetServer, render::PreRenderContext};
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::time::Ticks;
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveMenuDialogResponse {
    pub final_flag: bool,
    pub button: i32,
}
pub struct SaveMenuDialog {
    confirmation: Confirmation,
    input: DialogInput,
    view: DialogView,
}
impl SaveMenuDialog {
    #[cfg(test)]
    pub(super) fn reference_is_open(&self) -> bool {
        self.confirmation.is_open()
    }
    pub fn load(
        server: &AssetServer,
        action: SlotAction,
        overwrite: bool,
        held: bool,
    ) -> anyhow::Result<Self> {
        Self::load_prompt(server, action.prompt(overwrite), held)
    }
    /// Shared native222d0 Yes/No dialog, also used by SystemMenu title return.
    pub(crate) fn load_prompt(
        server: &AssetServer,
        prompt: &str,
        held: bool,
    ) -> anyhow::Result<Self> {
        let view = DialogView::load_text(server, prompt, &["はい", "いいえ"])?;
        let confirmation = Confirmation::new(2, 1, true);
        let mut input = DialogInput::default();
        input.ignore_pointer(held);
        Ok(Self {
            confirmation,
            input,
            view,
        })
    }
    #[cfg(test)]
    pub(crate) fn reference_button_center(&self, index: usize) -> Vec2 {
        let rect = self.view.geometry.buttons[index];
        self.view.geometry.origin
            + Vec2::new(30.0, 46.0)
            + Vec2::new(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5)
    }
    #[cfg(test)]
    pub(crate) fn reference_origin(&self) -> Vec2 {
        self.view.geometry.origin
    }
    pub fn set_pointer_extent(&mut self, extent: (u16, u16), held: bool) {
        self.input.ignore_pointer(held);
        if extent.0 > 0 && extent.1 > 0 {
            self.input.set_pointer_scale(Vec2::new(
                1920.0 / extent.0 as f32,
                1080.0 / extent.1 as f32,
            ));
        }
    }
    pub fn pointer_state(&mut self, position: Vec2, held: bool, elapsed: f64) {
        self.input.pointer_timed(
            &mut self.confirmation,
            &mut self.view.geometry,
            position,
            held,
            elapsed,
        );
    }
    pub fn keyboard(&mut self, state: &EnumMap<AppAction, ActionState>) {
        self.input
            .keyboard(&mut self.confirmation, self.view.geometry.vertical, state);
    }
    pub(super) fn cancel(&mut self) {
        self.confirmation.cancel();
    }
    pub fn update(&mut self, delta: Ticks) -> Vec<SaveMenuDialogResponse> {
        self.input.update_motion(&mut self.view.geometry, delta);
        self.confirmation.update(delta);
        let mut result = Vec::new();
        for (final_flag, outcome) in [
            (false, self.confirmation.take_intermediate_outcome()),
            (true, self.confirmation.take_outcome()),
        ] {
            if let Some(outcome) = outcome {
                result.push(SaveMenuDialogResponse {
                    final_flag,
                    button: match outcome {
                        Outcome::Yes => 0,
                        Outcome::No => 1,
                        Outcome::Cancel => -1,
                    },
                });
            }
        }
        result
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        self.input.take_sounds()
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) {
        self.view.prepare(context, &self.confirmation);
    }
    pub fn render(&self, pass: &mut RenderPass) {
        self.view.render(pass, &self.confirmation);
    }
}
