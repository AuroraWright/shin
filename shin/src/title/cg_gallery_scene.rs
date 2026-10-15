//! Native CGMode470e0: capture4a300, opening4a6a0 and child task ownership.
use super::{
    captured_scene::CapturedScene,
    cg_gallery::{CgGalleryList, CgRequest},
    cg_gallery_input::CgGalleryInput,
    cg_gallery_text::CgCompletionText,
    cg_gallery_view,
    cg_viewer_scene::CgViewerScene,
};
use crate::{
    adv::{GlobalData, character_view::CharacterTextures, input_area::PointerFrame},
    app::AppAction,
    asset::{font::GpuFontLazy, mask::MaskTexture, system::AssetServer},
    audio::GlobalAudio,
    update::UpdateContext,
};
use enum_map::EnumMap;
use shin_core::{
    format::scenario::Scenario,
    time::{Ticks, Tween},
};
use shin_input::ActionState;
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};
use std::sync::{Arc, mpsc};
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Loading,
    Opening,
    InstallingInput,
    Active,
}
pub(crate) struct CgGalleryScene {
    list: CgGalleryList,
    input: CgGalleryInput,
    atlas: Option<Arc<CharacterTextures>>,
    system: Arc<CharacterTextures>,
    loading: Option<shin_tasks::AsyncTask<anyhow::Result<Arc<MaskTexture>>>>,
    atlas_received: mpsc::Receiver<Arc<CharacterTextures>>,
    _mask: Option<Arc<MaskTexture>>,
    completion: CgCompletionText,
    previous: CapturedScene,
    phase: Phase,
    capture_elapsed: f32,
    children: Vec<CgViewerScene>,
    server: Arc<AssetServer>,
    scenario: Arc<Scenario>,
    extent: (u16, u16),
    destination: Option<i32>,
    input_enabled: bool,
    #[cfg(test)]
    resource_failures: Vec<String>,
}
impl CgGalleryScene {
    pub fn new(
        server: Arc<AssetServer>,
        scenario: Arc<Scenario>,
        font: Arc<GpuFontLazy>,
        globals: &GlobalData,
        previous: RenderTexture,
        extent: (u16, u16),
    ) -> anyhow::Result<Self> {
        let mut list =
            CgGalleryList::new(&scenario.info_tables().picture_box_info, globals.save())?;
        let system: Arc<CharacterTextures> = server.load_sync("/systex.txa")?;
        if list.content_height() > 726.0 {
            system.require_names("/systex.txa", &["scroll"])?;
        }
        //116930(30) is the platform mutex-only gallery notification.
        let completion = CgCompletionText::new(&server, &font, &list);
        let (send, atlas_received) = mpsc::channel();
        let assets = server.clone();
        let thumbnails: Vec<_> = list
            .rows()
            .iter()
            .map(|row| row.thumbnail.clone())
            .collect();
        let needs_logo = list.rows().iter().any(|row| row.unlocked_count() == 0);
        let needs_num = list
            .rows()
            .iter()
            .any(|row| row.unlocked_count() > 0 && row.unlocked_count() < row.pictures.len());
        // Locked rows cannot open a viewer; require indicators only for rows
        // the current progress can actually open.
        let needs_check = list
            .rows()
            .iter()
            .any(|row| row.pictures.len() >= 2 && row.unlocked_count() > 0);
        let loading = shin_tasks::async_io::spawn(async move {
            let atlas: Arc<CharacterTextures> = assets.load("/cgmode.txa").await?;
            atlas.require_names("/cgmode.txa", &["bg", "close", "help_cgmode", "key"])?;
            if needs_check {
                atlas.require_names("/cgmode.txa", &["check"])?;
            }
            if needs_logo {
                atlas.require_names("/cgmode.txa", &["logo"])?;
            }
            if needs_num {
                atlas.require_names("/cgmode.txa", &["num"])?;
            }
            for name in thumbnails {
                atlas.require_names("/cgmode.txa", &[&name])?;
            }
            let _ = send.send(atlas); //4d3b4 publishes atlas before mask loading.
            assets.load("/default.msk").await
        });
        let mut input = CgGalleryInput::new();
        input.resize(&mut list, extent);
        Ok(Self {
            list,
            input,
            atlas: None,
            system,
            _mask: None,
            loading: Some(loading),
            atlas_received,
            completion,
            previous: CapturedScene::new(previous),
            phase: Phase::Loading,
            capture_elapsed: 0.0,
            children: Vec::new(),
            server,
            scenario,
            extent,
            destination: None,
            input_enabled: false,
            #[cfg(test)]
            resource_failures: Vec::new(),
        })
    }
    pub fn resize(&mut self, extent: (u16, u16)) {
        if self.extent != extent {
            self.input.resize(&mut self.list, extent);
            self.extent = extent;
        }
    }
    pub fn captured_opacity(&self) -> f32 {
        match self.phase {
            Phase::Loading => 1.0,
            Phase::Opening | Phase::InstallingInput => 1.0 - self.capture_elapsed / 60.0,
            Phase::Active => 0.0,
        }
    }
    pub fn update(
        &mut self,
        ctx: &mut UpdateContext,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        globals: &GlobalData,
        audio: &mut GlobalAudio,
        scroll_axis: f32,
        viewer_axes: glam::Vec2,
    ) -> anyhow::Result<()> {
        //1000b0 retires finished children BEFORE taking its update snapshot.
        self.children.retain(|child| !child.is_finished());
        let enabled = matches!(self.phase, Phase::InstallingInput | Phase::Active)
            && self.children.is_empty()
            && !self.list.has_blocking_tasks();
        self.input_enabled = enabled;
        if let Ok(atlas) = self.atlas_received.try_recv() {
            self.atlas = Some(atlas);
        }
        if let Some(result) = self.loading.as_mut().and_then(|task| task.poll_naive()) {
            self.loading = None;
            let mask = match result {
                Ok(mask) => mask,
                Err(error) => {
                    tracing::warn!(%error, "Loading CG gallery artwork failed");
                    self.destination = Some(6);
                    return Ok(());
                }
            };
            if self.atlas.is_none() {
                self.atlas = self.atlas_received.try_recv().ok();
            }
            self._mask = Some(mask);
            self.phase = Phase::Opening;
            audio.bgm_player.stop(Tween::linear(Ticks::from_u32(60)));
        } else if self.phase == Phase::Opening && self.capture_elapsed >= 60.0 {
            //4a738 adds input task49100; snapshot excludes it this frame.
            use shin_core::format::scenario::{info::BgmId, instruction_elements::FromNumber};
            let id = BgmId::from_number(6);
            audio.bgm_player.request(
                self.server.clone(),
                self.scenario.info_tables().bgm_info(id).path(),
                id,
                true,
                shin_core::vm::command::types::Volume(1.0),
                Tween::linear(Ticks::ZERO),
            );
            self.phase = Phase::InstallingInput;
        } else if self.phase == Phase::InstallingInput {
            //The finished capture task is retired on the following frame.
            self.phase = Phase::Active;
            self.previous.retire();
            self._mask = None;
        }
        // Failed children retire locally; input eligibility stays at this frame
        // snapshot so the completion action cannot reach a parent or sibling.
        //Newest child owns input even while loading/fading. Older children and
        //parent clocks continue to run while deferred play tasks gate input.
        if let Some(atlas) = &self.atlas {
            let row = &self.list.rows()[self.list.selected()];
            let newest_index = self.children.len().checked_sub(1);
            for index in (0..self.children.len()).rev() {
                let (older, rest) = self.children.split_at_mut(index);
                let (child, newer) = rest.split_first_mut().unwrap();
                let list = &self.list;
                let completion = &self.completion;
                let previous = &self.previous;
                let opacity = match self.phase {
                    Phase::Loading => 1.0,
                    Phase::Opening | Phase::InstallingInput => 1.0 - self.capture_elapsed / 60.0,
                    Phase::Active => 0.0,
                };
                let result = child.update(
                    ctx,
                    row,
                    frame,
                    state,
                    viewer_axes,
                    self.extent,
                    globals,
                    audio,
                    Some(index) == newest_index,
                    |pass| {
                        cg_gallery_view::render_artwork(list, atlas, &self.system, pass);
                        completion.render(pass);
                        for sibling in older.iter() {
                            sibling.render(row, atlas, pass);
                        }
                    },
                    |pass| {
                        for sibling in newer.iter() {
                            sibling.render(row, atlas, pass);
                        }
                        previous.render(pass, opacity);
                    },
                );
                if let Err(error) = result {
                    tracing::warn!(%error, "CG viewer failed; returning to the gallery");
                    #[cfg(test)]
                    self.resource_failures.push(format!("{error:#}"));
                    self.children.remove(index);
                }
            }
        }
        self.list.prepare_frame();
        for request in self.list.take_requests() {
            match request {
                CgRequest::OpenRow(index) => match CgViewerScene::new(
                    self.server.clone(),
                    self.scenario.clone(),
                    &self.list.rows()[index],
                    self.extent,
                ) {
                    Ok(child) => self.children.push(child),
                    Err(error) => {
                        tracing::warn!(%error, "Creating CG viewer failed; retaining the gallery");
                        #[cfg(test)]
                        self.resource_failures.push(format!("{error:#}"));
                    }
                },
                CgRequest::ReturnTitle {
                    argument,
                    music_stop_ticks,
                } => {
                    audio
                        .bgm_player
                        .stop(Tween::linear(Ticks::from_u32(music_stop_ticks)));
                    self.destination = Some(i32::from(argument));
                }
            }
        }
        self.input
            .input_frame(&mut self.list, enabled, frame, state, scroll_axis);
        audio
            .system_player
            .set_settings_volume(globals.save().settings.v3_sysvol);
        for sound in self.list.take_sounds() {
            audio
                .system_player
                .play(&self.server, sound, Default::default());
        }
        self.list.update(ctx.delta_ticks);
        //62f40 adds even nonpositive time; bba50 row clocks independently skip
        //nonpositive deltas. Keep the capture task until next-frame retirement.
        if matches!(self.phase, Phase::Opening | Phase::InstallingInput) {
            self.capture_elapsed = (self.capture_elapsed + ctx.delta_ticks.as_f32()).min(60.0);
        }
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) {
        if let Some(atlas) = &self.atlas {
            cg_gallery_view::render_artwork(&self.list, atlas, &self.system, pass);
            self.completion.render(pass);
        }
        //48c34 draws task-group children after gallery/text, in insertion order.
        for child in &self.children {
            if let Some(atlas) = &self.atlas {
                child.render(&self.list.rows()[self.list.selected()], atlas, pass);
            }
        }
        self.previous.render(pass, self.captured_opacity());
    }

    pub fn take_destination(&mut self) -> Option<i32> {
        self.destination.take()
    }
    #[cfg(test)]
    pub fn reference_resource_failures(&self) -> &[String] {
        &self.resource_failures
    }
    #[cfg(test)]
    pub fn reference_capture_owned(&self) -> bool {
        self.previous.is_owned() || self._mask.is_some()
    }
    #[cfg(test)]
    pub fn reference_child_state(&self) -> Option<(bool, bool, i32, bool)> {
        self.children.last().map(CgViewerScene::reference_state)
    }
    #[cfg(test)]
    pub fn reference_state(&self) -> (bool, f32, bool, usize, usize, bool) {
        (
            self.atlas.is_some(),
            self.captured_opacity(),
            self.input_enabled,
            self.list.selected(),
            self.children.len(),
            self.children
                .last()
                .is_some_and(CgViewerScene::reference_ready),
        )
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_cg_gallery_publication_opening_bgm_and_title_return() {
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (a, expected) in [
        (0x496c8, 0x52802029),
        (0x496d0, 0x79008009),
        (0x4d3b4, 0xc809fd00),
        (0x4d494, 0x94014c0b),
        (0x4d4a4, 0xc809fd00),
        (0x4a6d0, 0x97ff7824),
        (0x4a6d4, 0x7100081f),
        (0x4a6e0, 0x321e0fe1),
        (0x4a6e4, 0x940061ef),
        (0x4a6f4, 0x321e0fe1),
        (0x4a6f8, 0x97ffa54e),
        (0x4a70c, 0x94006201),
        (0x4a710, 0x370001a0),
        (0x4a724, 0x321f07e1),
        (0x4a730, 0x97ffa434),
        (0x4a738, 0x97fffa72),
        (0x4a73c, 0x12800008),
        (0x4a740, 0xb9001268),
        (0x4a74c, 0x940061fd),
        (0x4a758, 0x2a1f03e0),
        (0x4a774, 0x140061ff),
        (0x48c34, 0x9402ddcf),
        (0x480b4, 0x9402dfff),
        (0x4d02c, 0x321f07e0),
        (0x4d034, 0x940303db),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
            expected,
            "Native CG gallery owner {a:x}"
        );
    }
}
