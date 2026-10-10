//! Native MusicMode328b0 factory, loading/capture327f0 and returningTitle32fd0.
use super::{
    captured_scene::CapturedScene,
    music_gallery::{MusicGallery, MusicSceneRequest},
    music_gallery_audio,
    music_gallery_input::MusicGalleryInput,
    music_gallery_text::MusicCompletionText,
    music_gallery_view,
};
use crate::{
    adv::{GlobalData, character_view::CharacterTextures, input_area::PointerFrame},
    app::AppAction,
    asset::{font::GpuFontLazy, mask::MaskTexture, system::AssetServer},
    audio::GlobalAudio,
};
use enum_map::EnumMap;
use shin_core::{
    format::scenario::Scenario,
    time::{Ticks, Tween},
    vm::command::types::AudioWaitStatus,
};
use shin_input::ActionState;
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};
use std::sync::Arc;

pub(crate) struct MusicGalleryScene {
    model: MusicGallery,
    input: MusicGalleryInput,
    atlas: Option<Arc<CharacterTextures>>,
    loading:
        Option<shin_tasks::AsyncTask<anyhow::Result<(Arc<CharacterTextures>, Arc<MaskTexture>)>>>,
    mask: Option<Arc<MaskTexture>>,
    system: Arc<CharacterTextures>,
    completion: MusicCompletionText,
    previous_screen: CapturedScene,
    server: Arc<AssetServer>,
    scenario: Arc<Scenario>,
    destination: Option<i32>,
    extent: (u16, u16),
}
impl MusicGalleryScene {
    pub fn new(
        server: Arc<AssetServer>,
        scenario: Arc<Scenario>,
        font: Arc<GpuFontLazy>,
        system: Arc<CharacterTextures>,
        globals: &GlobalData,
        previous_screen: RenderTexture,
        extent: (u16, u16),
    ) -> anyhow::Result<Self> {
        let mut model = MusicGallery::new(&scenario.info_tables().music_box_info, globals.save())?;
        // Constructor116930(31) only locks/unlocks a platform mutex in this
        // executable, so full completion records no additional save bit.
        let completion = MusicCompletionText::new(&server, &font, model.list());
        let assets = server.clone();
        let loading = shin_tasks::async_io::spawn(async move {
            let atlas = assets.load("/bgmmode.txa").await?;
            let mask = assets.load("/default.msk").await?;
            Ok((atlas, mask))
        });
        let mut input = MusicGalleryInput::new();
        input.resize(&mut model, extent);
        Ok(Self {
            model,
            input,
            atlas: None,
            mask: None,
            loading: Some(loading),
            system,
            completion,
            previous_screen: CapturedScene::new(previous_screen),
            server,
            scenario,
            destination: None,
            extent,
        })
    }
    pub fn resize(&mut self, extent: (u16, u16)) {
        if self.extent != extent {
            self.input.resize(&mut self.model, extent);
            self.extent = extent;
        }
    }
    pub fn update(
        &mut self,
        delta: Ticks,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        axis: f32,
        globals: &mut GlobalData,
        audio: &mut GlobalAudio,
    ) -> anyhow::Result<()> {
        if let Some(result) = self.loading.as_mut().and_then(|task| task.poll_naive()) {
            self.loading = None;
            let (atlas, mask) = result?;
            self.atlas = Some(atlas);
            self.mask = Some(mask);
            self.model.publish_resources();
        }
        audio
            .bgm_player
            .set_settings_volume(globals.save().settings.v0_bgmvol);
        audio
            .system_player
            .set_settings_volume(globals.save().settings.v3_sysvol);
        self.model.prepare_frame();
        if self.model.capture_retired() {
            self.previous_screen.retire();
            self.mask = None;
        }
        for request in self.model.take_requests() {
            match request {
                MusicSceneRequest::BeginOpening { ticks } => {
                    audio.bgm_player.stop(Tween::linear(Ticks::from_u32(ticks)))
                }
                MusicSceneRequest::ReturnToTitle {
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
        self.input.input_frame(&mut self.model, frame, state, axis);
        music_gallery_audio::consume_requests(
            &mut self.model,
            &mut audio.bgm_player,
            &mut audio.system_player,
            &self.server,
            &self.scenario,
            globals,
        )?;
        self.model.update(
            delta,
            audio
                .bgm_player
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING),
        );
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) {
        if let Some(atlas) = &self.atlas {
            music_gallery_view::render_artwork(&self.model, atlas, &self.system, pass);
            self.completion.render(pass);
        }
        self.previous_screen
            .render(pass, self.model.captured_opacity());
    }

    pub fn take_destination(&mut self) -> Option<i32> {
        self.destination.take()
    }
    #[cfg(test)]
    pub(crate) fn reference_capture_owned(&self) -> bool {
        self.previous_screen.is_owned() || self.mask.is_some()
    }
    #[cfg(test)]
    pub(crate) fn reference_state(&self) -> (bool, f32, bool, usize, Option<usize>) {
        (
            self.atlas.is_some(),
            self.model.captured_opacity(),
            self.model.accepts_input(),
            self.model.list().selected(),
            self.model.list().playing(),
        )
    }
}
