//! Desktop adapter for storage errors reported by the Switch OS error service.
use super::{backlog_rollback::Confirmation, dialog_input::DialogInput, dialog_view::DialogView};
use crate::{app::AppAction, asset::system::AssetServer, render::PreRenderContext};
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::time::Ticks;
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;

pub(crate) struct StorageRetryDialog {
    confirmation: Confirmation,
    input: DialogInput,
    view: DialogView,
    extent: (u16, u16),
}
impl StorageRetryDialog {
    pub(crate) fn suspend_pointer(&mut self, held: bool) {
        self.input.ignore_pointer(held);
    }
    pub(crate) fn load_shutdown_error(
        server: &AssetServer,
        held: bool,
        extent: (u16, u16),
    ) -> anyhow::Result<Self> {
        let mut dialog = Self {
            confirmation: Confirmation::new(2, 0, true),
            input: DialogInput::default(),
            view: DialogView::load_text(
                server,
                "セーブデータを保存できませんでした。@r保存先をご確認のうえ、再試行してください。",
                &["再試行", "キャンセル"],
            )?,
            extent,
        };
        dialog.input.ignore_pointer(held);
        dialog.set_scale(extent);
        Ok(dialog)
    }
    pub(crate) fn retry_requested(&mut self) -> bool {
        self.confirmation.take_outcome() == Some(super::backlog_rollback::Outcome::Yes)
    }
    /// Desktop resource/reconstruction failure notice. Switch OS error-service
    /// presentation is a separate platform contract.
    pub fn load_scene_error(
        server: &AssetServer,
        held: bool,
        extent: (u16, u16),
    ) -> anyhow::Result<Self> {
        Self::load_text(
            server,
            held,
            extent,
            "シーンを読み込めませんでした。@rゲームデータをご確認ください。",
            "確認",
            false,
        )
    }
    pub fn load(server: &AssetServer, held: bool, extent: (u16, u16)) -> anyhow::Result<Self> {
        Self::load_text(
            server,
            held,
            extent,
            "セーブデータを読み込めませんでした。@r保存先をご確認のうえ、再試行してください。",
            "再試行",
            false,
        )
    }
    pub fn load_recovery(
        server: &AssetServer,
        held: bool,
        extent: (u16, u16),
        kind: super::SaveRecoveryKind,
    ) -> anyhow::Result<Self> {
        Self::load_text(server, held, extent, kind.prompt(), "確認", true)
    }
    pub fn load_save_error(
        server: &AssetServer,
        held: bool,
        extent: (u16, u16),
    ) -> anyhow::Result<Self> {
        Self::load_text(
            server,
            held,
            extent,
            super::save_game::ERROR_PROMPT,
            "確認",
            false,
        )
    }
    pub fn load_backup_error(
        server: &AssetServer,
        held: bool,
        extent: (u16, u16),
    ) -> anyhow::Result<Self> {
        Self::load_text(
            server,
            held,
            extent,
            "元のセーブデータを保護できませんでした。@r保存先をご確認のうえ、再試行してください。",
            "再試行",
            false,
        )
    }
    fn load_text(
        server: &AssetServer,
        held: bool,
        extent: (u16, u16),
        prompt: &str,
        label: &str,
        cancellable: bool,
    ) -> anyhow::Result<Self> {
        let mut dialog = Self {
            confirmation: Confirmation::new(1, 0, cancellable),
            input: DialogInput::default(),
            view: DialogView::load_text(server, prompt, &[label])?,
            extent,
        };
        dialog.input.ignore_pointer(held);
        dialog.set_scale(extent);
        Ok(dialog)
    }
    fn set_scale(&mut self, extent: (u16, u16)) {
        self.extent = extent;
        self.input.set_pointer_scale(Vec2::new(
            1920.0 / extent.0.max(1) as f32,
            1080.0 / extent.1.max(1) as f32,
        ));
    }
    pub fn input(
        &mut self,
        position: Vec2,
        held: bool,
        extent: (u16, u16),
        elapsed: f64,
        state: &EnumMap<AppAction, ActionState>,
    ) {
        self.pointer_state(position, held, extent, elapsed);
        self.keyboard(state);
    }
    /// Position is already in canvas coordinates. Scale only controls the
    /// native physical-pixel drag thresholds, as in the other dialog owners.
    pub fn pointer_state(&mut self, position: Vec2, held: bool, extent: (u16, u16), elapsed: f64) {
        if self.extent != extent {
            self.input.ignore_pointer(held);
            self.set_scale(extent);
        }
        self.input.pointer_timed(
            &mut self.confirmation,
            &mut self.view.geometry,
            position,
            held,
            elapsed,
        );
    }
    pub fn update(&mut self, delta: Ticks) -> bool {
        self.input.update_motion(&mut self.view.geometry, delta);
        self.confirmation.update(delta);
        self.confirmation.is_done()
    }
    pub fn keyboard(&mut self, state: &EnumMap<AppAction, ActionState>) {
        self.input
            .keyboard(&mut self.confirmation, self.view.geometry.vertical, state);
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
    #[cfg(test)]
    pub fn reference_button(&self) -> Vec2 {
        self.reference_button_at(0)
    }
    #[cfg(test)]
    pub fn reference_is_open(&self) -> bool {
        self.confirmation.is_open()
    }
    #[cfg(test)]
    pub fn reference_button_at(&self, index: usize) -> Vec2 {
        let rect = self.view.geometry.buttons[index];
        self.view.geometry.origin
            + Vec2::new(30.0, 46.0)
            + rect.truncate().truncate()
            + Vec2::new(rect.z, rect.w) * 0.5
    }
}
