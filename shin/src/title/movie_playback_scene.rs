//! Native MovieMode child89e60 resource/player owner. Parent keeps drawing below.
use super::{
    movie_playback::{MoviePlaybackMotion, PlaybackEvent},
    view::rgba,
};
use crate::{
    adv::{
        GlobalData,
        input_area::{InputArea, InputAreaTarget, PointerFrame},
    },
    app::AppAction,
    asset::{movie::Movie, system::AssetServer},
    audio::GlobalAudio,
    render::top_left_projection_matrix,
    update::UpdateContext,
};
use anyhow::Context;
use enum_map::EnumMap;
use glam::{Mat4, Vec2, Vec4};
use shin_core::{
    format::scenario::{Scenario, info::MovieId},
    vm::command::types::Volume,
};
use shin_input::ActionState;
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
};
use shin_video::VideoPlayerHandle;
use std::sync::Arc;

#[derive(Default)]
struct SkipInput {
    requested: bool,
}
impl InputAreaTarget for SkipInput {
    fn probe(&mut self, _: Vec2) -> bool {
        true
    }
    fn reset(&mut self) {}
    fn press(&mut self, _: Vec2) {}
    fn release(&mut self, _: Vec2) {
        self.requested = true;
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, _: Vec2, _: Vec2) {}
    fn gesture(&mut self, _: Vec2) {}
    fn cancel(&mut self) {}
}

pub(super) struct MoviePlaybackScene {
    motion: MoviePlaybackMotion,
    loading: Option<shin_tasks::AsyncTask<anyhow::Result<VideoPlayerHandle>>>,
    player: Option<VideoPlayerHandle>,
    area: InputArea,
    input: SkipInput,
    extent: (u16, u16),
    applied_volume: Volume,
}
impl MoviePlaybackScene {
    pub fn new(
        server: Arc<AssetServer>,
        scenario: &Scenario,
        movie: MovieId,
        globals: &GlobalData,
        audio: &GlobalAudio,
        extent: (u16, u16),
    ) -> anyhow::Result<Self> {
        let index = usize::try_from(movie.into_i32()).context("Negative MovieMode movie ID")?;
        let info = scenario
            .info_tables()
            .movie_info
            .get(index)
            .context("Missing MovieMode movie metadata")?;
        //89fc8 writes parameters [1,0,0]; bc568 copies byte1 to player21,
        //c04b8 uses that byte for alpha mode. Gallery forces opaque/non-looping.
        //Only the filename is read here: no startup movie/music unlock mutation.
        let path = info.path();
        let volume = Volume(globals.save().settings.v0_bgmvol as f32 * 0.01);
        let manager = audio.manager.clone();
        let loading = shin_tasks::async_io::spawn(async move {
            let asset = server.load::<Movie, _>(path).await?;
            let device = server.context().wgpu_device.clone();
            //Keep the I/O executor available to drain FFmpeg's pipes.
            shin_tasks::compute::spawn(move || asset.prepare_with_volume(&device, &manager, volume))
                .await
        });
        Ok(Self {
            motion: MoviePlaybackMotion::new(),
            loading: Some(loading),
            player: None,
            area: InputArea::new(extent.0, extent.1),
            input: SkipInput::default(),
            extent,
            applied_volume: volume,
        })
    }
    pub fn update(
        &mut self,
        ctx: &UpdateContext,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        extent: (u16, u16),
        globals: &GlobalData,
        audio: &mut GlobalAudio,
    ) -> anyhow::Result<()> {
        self.update_with_input(ctx, frame, state, extent, globals, audio, true)
    }
    pub fn update_with_input(
        &mut self,
        ctx: &UpdateContext,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        extent: (u16, u16),
        globals: &GlobalData,
        audio: &mut GlobalAudio,
        input_enabled: bool,
    ) -> anyhow::Result<()> {
        if self.extent != extent {
            self.area.disable(&mut self.input);
            self.area = InputArea::new(extent.0, extent.1);
            self.extent = extent;
        }
        if let Some(result) = self.loading.as_mut().and_then(|task| task.poll_naive()) {
            self.loading = None;
            self.player = Some(result?);
        }
        let finished = self
            .player
            .as_ref()
            .is_some_and(VideoPlayerHandle::is_finished);
        self.input.requested = false;
        let mut cue = "cancel";
        let skip = if input_enabled && self.motion.polls_input() && !finished {
            let captured = self.area.update(&mut self.input, frame);
            if self.input.requested {
                cue = "decide";
                true
            } else {
                !captured
                    && (state[AppAction::Enter].is_clicked || state[AppAction::Cancel].is_clicked)
            }
        } else {
            self.area.disable(&mut self.input);
            false
        };
        if self
            .motion
            .prepare_frame(self.player.is_some(), i32::from(finished), skip)
        {
            audio
                .system_player
                .set_settings_volume(globals.save().settings.v3_sysvol);
            audio
                .system_player
                .play(ctx.asset_server, cue, Volume::default());
            self.area.disable(&mut self.input);
        }
        for event in self.motion.take_events() {
            let player = self
                .player
                .as_ref()
                .expect("MovieMode event without ready player");
            match event {
                PlaybackEvent::Start => player.start(),
                PlaybackEvent::Stop => player.stop(),
            }
        }
        //8a570 tasks,8a584 player,8a590 movie clock,8a59c cover clock.
        if let Some(player) = &self.player {
            player.update(ctx.frame_id, ctx.delta_ticks, ctx.pre_render.queue);
        }
        self.motion.advance(ctx.delta_ticks);
        if self.motion.renders_movie() {
            let volume = Volume(
                self.motion.movie_opacity() * globals.save().settings.v0_bgmvol as f32 * 0.01,
            );
            if volume != self.applied_volume {
                self.player.as_ref().unwrap().set_volume(volume);
                self.applied_volume = volume;
            }
        }
        Ok(())
    }
    pub fn is_finished(&self) -> bool {
        self.motion.is_finished()
    }
    #[cfg(test)]
    pub fn reference_ready(&self) -> bool {
        self.loading.is_none() && self.player.is_some()
    }
    pub fn render(&self, pass: &mut RenderPass) {
        let opacity = self.motion.cover_opacity();
        QuadVertices::new()
            .with_box(0.0, 0.0, 1920.0, 1080.0)
            .with_color(rgba(0.0, 0.0, 0.0, opacity))
            .fill(
                pass,
                RenderRequestBuilder::new().color_blend_type(if opacity < 1.0 {
                    ColorBlendType::Layer1
                } else {
                    ColorBlendType::Opaque
                }),
                top_left_projection_matrix(),
            );
        if !self.motion.renders_movie() {
            return;
        }
        //EOF can precede the next monitor poll. Keep the uploaded final frame
        //until the native Stop event clears the started-frame render gate.
        let Some(frame) = self
            .player
            .as_ref()
            .and_then(VideoPlayerHandle::get_retained_frame)
        else {
            return;
        };
        let size = frame.get_frame().get_size();
        let alpha = self.motion.movie_opacity();
        frame.render_with_options(
            pass,
            RenderRequestBuilder::new().color_blend_type(if alpha < 1.0 {
                ColorBlendType::Layer1
            } else {
                ColorBlendType::Opaque
            }),
            top_left_projection_matrix()
                * Mat4::from_scale(glam::vec3(1920.0 / size.x, 1080.0 / size.y, 1.0)),
            false,
            Vec4::new(1.0, 1.0, 1.0, (alpha * 255.0) as u8 as f32 / 255.0),
        );
    }
}

#[cfg(test)]
mod reference;
