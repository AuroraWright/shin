//! Concrete scene ownership around the shared picker and its private modes.
//! Preflight is pure; source globals and the captured composition move only
//! after the concrete owner has accepted its constructor inputs.
use super::{Model, SceneRequest, owner::Owner};
use crate::{
    adv::{higurashi::input::Buttons, input_area::PointerFrame},
    app::higurashi::{
        runtime_game_data::RuntimeGameData, save_menu::Environment, scene_host::Globals,
    },
    render::PreRenderContext,
};
use anyhow::{Context, Result, ensure};
use shin_core::time::Ticks;
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};
use std::sync::Arc;

pub(in crate::app::higurashi) struct Prepared {
    pub data: RuntimeGameData,
    kind: u32,
    selected: i32,
    append: bool,
    extent: (u16, u16),
}

impl Prepared {
    pub fn new(
        scenario: &shin_core::format::scenario::higurashi::HigurashiScenario,
        settings: &shin_core::format::save::higurashi::SaveData,
        append: bool,
        data: RuntimeGameData,
        kind: i32,
        selected: i32,
        extent: (u16, u16),
    ) -> Result<Self> {
        ensure!(extent.0 > 0 && extent.1 > 0, "Invalid picker input extent");
        ensure!(
            scenario.variant == shin_core::format::scenario::higurashi::HigurashiVariant::Updated,
            "ScenarioSelect requires updated Higurashi"
        );
        ensure!(
            (0..6).contains(&kind),
            "Unsupported ScenarioSelect kind {kind}"
        );
        let kind = kind as u32;
        let origin_bgm = scenario
            .bgm_info
            .iter()
            .position(|row| row.name.as_str() == "org/2/it_moves")
            .map(|index| index as i32);
        Model::new(kind, selected, append, &data, settings, origin_bgm, true)?;
        Ok(Self {
            data,
            kind,
            selected,
            append,
            extent,
        })
    }
}

pub(in crate::app::higurashi) struct Opening {
    // The concrete scene's scope/workers retire before global audio/assets.
    owner: Option<Owner>,
    pub globals: Globals,
}

impl Opening {
    pub fn new_retained(
        globals: &mut Option<Globals>,
        prepared: &Prepared,
        capture: &mut Option<RenderTexture>,
        next_seed: Arc<dyn Fn() -> u32 + Send + Sync>,
    ) -> Result<Self> {
        let source = globals
            .as_ref()
            .context("Picker factory lost its globals")?;
        let owner = Owner::new_retained(
            Environment {
                common: source.common.clone(),
                server: source.server.clone(),
                sounds: source.sounds.clone(),
                storage: source.storage.clone(),
            },
            prepared.kind,
            prepared.selected,
            prepared.append,
            &prepared.data,
            &source.data,
            prepared.extent,
            next_seed,
            capture,
        )?;
        Ok(Self {
            owner: Some(owner),
            globals: globals.take().unwrap(),
        })
    }

    pub fn update(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        self.owner.as_mut().unwrap().poll(
            delta,
            permission,
            buttons,
            pointer,
            &mut self.globals.audio,
        )?;
        // Native27200 updates global music after the active scene callback.
        while let Some(event) = self.globals.audio.pop_music_event() {
            if let crate::adv::higurashi::audio::MusicEvent::Started(info) = event {
                for id in [info.id, i32::from(info.linked_id)]
                    .into_iter()
                    .filter(|id| *id >= 0)
                {
                    let words = &mut self.globals.data.save_vectors.unlocked_bgms;
                    let index = id as usize / 32;
                    if index >= words.len() {
                        words.resize((index + 1).next_multiple_of(256), 0);
                    }
                    words[index] |= 1 << (id as usize % 32);
                }
            }
        }
        self.globals.audio.update_global_music(delta);
        self.globals.sounds.lock().update();
        Ok(())
    }

    pub fn prepare(&mut self, context: &mut PreRenderContext) -> Result<()> {
        self.owner.as_ref().unwrap().prepare(context);
        Ok(())
    }

    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        self.owner.as_ref().unwrap().render(pass)
    }

    pub fn resize_input(&mut self, extent: (u16, u16)) -> Result<()> {
        self.owner.as_mut().unwrap().resize(extent)
    }

    pub fn cancel_input(&mut self) -> Result<()> {
        self.owner.as_mut().unwrap().cancel_input();
        Ok(())
    }

    pub fn has_save_task(&self) -> bool {
        self.owner.as_ref().unwrap().has_save_task()
    }

    pub fn pending_scene_request(&self) -> Option<&SceneRequest> {
        self.owner.as_ref().unwrap().pending_request()
    }

    pub fn take_scene_request(&mut self) -> Option<SceneRequest> {
        self.owner.as_mut().unwrap().take_request()
    }

    pub fn into_globals(mut self) -> Globals {
        self.owner.take();
        self.globals
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub(in crate::app::higurashi) fn reference_owner(&self) -> &Owner {
        self.owner.as_ref().unwrap()
    }
}

/// Original b8274 initializes record0/scenario10000 and all four vectors. Date
/// and dormant scalar fields were unwritten native storage: explicit zeros are
/// the safe desktop initialization boundary, not inferred native field values.
pub(in crate::app::higurashi) fn origin_data(seed: u32) -> RuntimeGameData {
    use crate::adv::higurashi::autosave::{ProgressEntry, SaveProgress};
    use shin_core::format::save::higurashi::SaveDate;
    let entry = |scenario_id, random_seed| ProgressEntry {
        scenario_id,
        random_seed,
        save_position: 0,
        replay_target: 0,
        response_data: vec![],
    };
    RuntimeGameData::from_progress(
        SaveProgress::new(0, u32::MAX, [entry(10000, seed), entry(0, 0)]).unwrap(),
        SaveDate {
            year: 0,
            month: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
        },
        vec![],
        vec![],
    )
}
