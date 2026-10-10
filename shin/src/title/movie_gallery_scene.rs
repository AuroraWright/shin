//! Native MovieMode87ae0: capture89a00, opening89da0 and child task ownership.
use super::{
    captured_scene::CapturedScene,
    movie_gallery::{MovieGalleryList, MovieRequest},
    movie_gallery_input::MovieGalleryInput,
    movie_gallery_text::MovieCompletionText,
    movie_gallery_view,
    movie_playback_scene::MoviePlaybackScene,
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
use std::sync::Arc;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Loading,
    Opening,
    InstallingInput,
    Active,
}
pub(crate) struct MovieGalleryScene {
    list: MovieGalleryList,
    input: MovieGalleryInput,
    atlas: Option<Arc<CharacterTextures>>,
    loading:
        Option<shin_tasks::AsyncTask<anyhow::Result<(Arc<CharacterTextures>, Arc<MaskTexture>)>>>,
    _mask: Option<Arc<MaskTexture>>,
    completion: MovieCompletionText,
    previous: CapturedScene,
    phase: Phase,
    capture_elapsed: f32,
    children: Vec<MoviePlaybackScene>,
    server: Arc<AssetServer>,
    scenario: Arc<Scenario>,
    extent: (u16, u16),
    destination: Option<i32>,
    input_enabled: bool,
}
impl MovieGalleryScene {
    pub fn new(
        server: Arc<AssetServer>,
        scenario: Arc<Scenario>,
        font: Arc<GpuFontLazy>,
        globals: &GlobalData,
        previous: RenderTexture,
        extent: (u16, u16),
    ) -> anyhow::Result<Self> {
        let mut list = MovieGalleryList::new(&scenario.info_tables().movie_info, globals.save())?;
        //116930(32) is the same platform mutex-only stub as MusicMode(31).
        let completion = MovieCompletionText::new(&server, &font, &list);
        let assets = server.clone();
        let loading = shin_tasks::async_io::spawn(async move {
            let atlas = assets.load("/moviemode.txa").await?;
            let mask = assets.load("/default.msk").await?;
            Ok((atlas, mask))
        });
        let mut input = MovieGalleryInput::new();
        input.resize(&mut list, extent);
        Ok(Self {
            list,
            input,
            atlas: None,
            _mask: None,
            loading: Some(loading),
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
        ctx: &UpdateContext,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        globals: &GlobalData,
        audio: &mut GlobalAudio,
    ) -> anyhow::Result<()> {
        //1000b0 retires finished children BEFORE taking its update snapshot.
        self.children.retain(|child| !child.is_finished());
        let enabled = matches!(self.phase, Phase::InstallingInput | Phase::Active)
            && self.children.is_empty();
        self.input_enabled = enabled;
        if let Some(result) = self.loading.as_mut().and_then(|task| task.poll_naive()) {
            self.loading = None;
            let (atlas, mask) = result?;
            self.atlas = Some(atlas);
            self._mask = Some(mask);
            self.phase = Phase::Opening;
            audio.bgm_player.stop(Tween::linear(Ticks::from_u32(60)));
        } else if self.phase == Phase::Opening && self.capture_elapsed >= 60.0 {
            //89e18 adds input task88be0; snapshot excludes it this frame.
            self.phase = Phase::InstallingInput;
        } else if self.phase == Phase::InstallingInput {
            //The finished capture task is retired on the following frame.
            self.phase = Phase::Active;
            self.previous.retire();
            self._mask = None;
        }
        //Newest child owns input even while loading/fading. Older children and
        //parent clocks continue to run, preserving repeated deferred play tasks.
        let mut newest = true;
        for child in self.children.iter_mut().rev() {
            child.update_with_input(ctx, frame, state, self.extent, globals, audio, newest)?;
            newest = false;
        }
        self.list.prepare_frame();
        for request in self.list.take_requests() {
            match request {
                MovieRequest::OpenMovie(movie) => self.children.push(MoviePlaybackScene::new(
                    self.server.clone(),
                    &self.scenario,
                    movie,
                    globals,
                    audio,
                    self.extent,
                )?),
                MovieRequest::ReturnTitle {
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
            .input_frame(&mut self.list, enabled, frame, state);
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
            movie_gallery_view::render_artwork(&self.list, atlas, pass);
            self.completion.render(pass);
        }
        //889e8 draws task-group children after gallery/text, in insertion order.
        for child in &self.children {
            child.render(pass);
        }
        self.previous.render(pass, self.captured_opacity());
    }

    pub fn take_destination(&mut self) -> Option<i32> {
        self.destination.take()
    }
    #[cfg(test)]
    pub fn reference_capture_owned(&self) -> bool {
        self.previous.is_owned() || self._mask.is_some()
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
                .is_some_and(MoviePlaybackScene::reference_ready),
        )
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_movie_parent_opening_text_and_task_group() {
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, expected) in [
        (0x87f20, 0xd2a88408),
        (0x87f24, 0xf2e84408),
        (0x87f28, 0xb26003e9),
        (0x87f80, 0x97fe5b90),
        (0x87f9c, 0xbd4a6d00),
        (0x87fa4, 0xbd43e101),
        (0x87fac, 0xbd447902),
        (0x88354, 0x9401df57),
        (0x8836c, 0x9400cdb9),
        (0x88378, 0x9400cdb6),
        (0x883a0, 0x1400cdac),
        (0x889e0, 0x9401e1fc),
        (0x889e8, 0x9401de62),
        (0x89dd4, 0x7100081f),
        (0x89de0, 0x321e0fe1),
        (0x89de4, 0x97ff642f),
        (0x89df4, 0x321e0fe1),
        (0x89df8, 0x97fea78e),
        (0x89e0c, 0x97ff6441),
        (0x89e10, 0x370000a0),
        (0x89e18, 0x97fffb72),
        (0x89e1c, 0x12800008),
        (0x89e2c, 0x97ff6445),
        (0x1000f8, 0x37000180),
        (0x100318, 0xd63f0100),
        (0x10031c, 0x12000014),
        (0x8b278, 0x97fffafa),
        (0x8b288, 0x9401d37a),
        (0x8b50c, 0x97fed549),
        (0x8b52c, 0x940053e5),
        (0x62ed4, 0xc85ffd00),
        (0x62ef4, 0x39000268),
        (0x62f94, 0x14000003),
        (0x631b0, 0x528000a0),
        (0x6320c, 0x1e211800),
        (0x63210, 0x1e203900),
        (0x63214, 0x97ffcd47),
        (0x56730, 0x4ea01c01),
        (0x56734, 0x4ea01c02),
        (0x56738, 0x4ea01c03),
    ] {
        assert_eq!(word(a), expected, "Native Movie parent{a:x}");
    }
    for (a, value) in [
        (0x159a6c, 47.0f32 / 255.0),
        (0x1593e0, 25.0 / 255.0),
        (0x159478, 20.0 / 255.0),
        (0x159584, 48.0),
        (0x15965c, 1012.0),
    ] {
        assert_eq!(word(a), value.to_bits());
    }
}
