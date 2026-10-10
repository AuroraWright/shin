//! Native asynchronous ManualViewer asset lifetime and completion callback.
//! The runtime menu/ADV host supplies modal ownership and raw viewport events.
use std::sync::Arc;

use enum_map::EnumMap;
use shin_core::time::Ticks;
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;

use super::{
    character_view::CharacterTextures,
    manual::{ManualModel, ManualPhase},
    manual_view,
};
use crate::{app::AppAction, asset::system::AssetServer};

pub struct ManualScreen {
    loading: Option<shin_tasks::AsyncTask<anyhow::Result<Arc<CharacterTextures>>>>,
    textures: Option<Arc<CharacterTextures>>,
    model: Option<ManualModel>,
    response_sent: bool,
    input_area: super::input_area::InputArea,
}
impl ManualScreen {
    /// Native86e80/878e0 loads manual.txa before870b0 calls86370.
    pub fn new(server: Arc<AssetServer>) -> Self {
        Self {
            loading: Some(shin_tasks::async_io::spawn(async move {
                server.load("/manual.txa").await
            })),
            textures: None,
            model: None,
            response_sent: false,
            input_area: super::input_area::InputArea::canvas(),
        }
    }
    fn accept_loaded(&mut self, textures: Arc<CharacterTextures>) -> anyhow::Result<()> {
        // Native86370 stops at the first absent numbered image, not total entries.
        let count = (1..)
            .take_while(|page| textures.contains(&format!("{page:03}")))
            .count();
        let model = ManualModel::new(count)
            .ok_or_else(|| anyhow::anyhow!("Manual archive has no numbered pages"))?;
        self.textures = Some(textures);
        self.model = Some(model);
        Ok(())
    }
    /// Completed task objects retire on the owner's following task-group poll.
    pub fn is_finished(&self) -> bool {
        self.response_sent
    }
    pub fn model(&self) -> Option<&ManualModel> {
        self.model.as_ref()
    }
    /// Pointer callbacks consume already transformed canvas coordinates.
    pub fn model_mut(&mut self) -> Option<&mut ManualModel> {
        self.model.as_mut()
    }
    /// Set physical viewport dimensions, matching native6afc0.
    /// A desktop resize cancels an existing capture before replacing its transform.
    pub fn set_pointer_extent(&mut self, extent: (u16, u16)) {
        if let Some(model) = &mut self.model {
            self.input_area.disable(model);
        }
        self.input_area = super::input_area::InputArea::new(extent.0, extent.1);
    }
    /// Dispatch pointer contacts before controller input, once per frame.
    /// Coordinates use the configured physical viewport (canvas by default).
    pub fn input_frame(
        &mut self,
        frame: &super::input_area::PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
    ) {
        if let Some(model) = &mut self.model {
            self.input_area.input_frame(model, frame, state);
        }
    }
    pub fn input(&mut self, state: &EnumMap<AppAction, ActionState>) {
        if let Some(model) = &mut self.model {
            model.input(state);
        }
    }
    /// Native875b4 emits response0 when closing starts;877dc emits1 at its end.
    /// Asset completion starts at progress0; loading frames consume no animation.
    pub fn update(&mut self, delta: Ticks) -> anyhow::Result<Option<i32>> {
        if let Some(task) = &mut self.loading {
            if let Some(result) = task.poll_naive() {
                self.loading = None;
                self.accept_loaded(result?)?;
            }
            return Ok(None);
        }
        if let Some(model) = &mut self.model {
            let previous = model.phase();
            model.update(delta.as_f32());
            if previous == ManualPhase::CloseFlash && model.phase() == ManualPhase::Closing {
                return Ok(Some(0));
            }
            if model.phase() == ManualPhase::Closed && !self.response_sent {
                self.response_sent = true;
                return Ok(Some(1));
            }
        }
        Ok(None)
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        self.model
            .as_mut()
            .map(ManualModel::take_sounds)
            .unwrap_or_default()
    }
    pub fn render(&self, pass: &mut RenderPass) {
        if let (Some(model), Some(textures)) = (&self.model, &self.textures) {
            manual_view::render(
                model,
                textures,
                pass,
                crate::render::top_left_projection_matrix(),
            );
        }
    }
    #[cfg(test)]
    pub(super) fn wait_reference_assets(&mut self) -> anyhow::Result<Arc<CharacterTextures>> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while self.model.is_none() {
            // Exercise the production completion poll. Even a large loading-frame
            // delta must leave newly loaded artwork at opening progress0.
            assert_eq!(self.update(Ticks::from_f32(1000.0))?, None);
            anyhow::ensure!(
                std::time::Instant::now() < deadline,
                "Manual asset load timed out"
            );
            std::thread::yield_now();
        }
        Ok(self.textures.as_ref().unwrap().clone())
    }
}
