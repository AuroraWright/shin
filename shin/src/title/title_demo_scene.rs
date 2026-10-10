//! Native TitleDemo113b00 owner: prepare movie0, publish, play, then Title(-1).
use super::title_demo::TitleDemoMotion;
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
use enum_map::EnumMap;
use glam::{Mat4, Vec2, Vec4};
use shin_core::{
    format::scenario::{Scenario, info::MovieTransparencyMode},
    vm::command::types::Volume,
};
use shin_input::ActionState;
use shin_render::{ColorBlendType, RenderRequestBuilder, render_pass::RenderPass};
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

pub(crate) struct TitleDemoScene {
    motion: TitleDemoMotion,
    loading: Option<shin_tasks::AsyncTask<anyhow::Result<Option<VideoPlayerHandle>>>>,
    player: Option<VideoPlayerHandle>,
    transparent: bool,
    area: InputArea,
    input: SkipInput,
    extent: (u16, u16),
    applied_volume: Volume,
}
impl TitleDemoScene {
    pub fn new(
        server: Arc<AssetServer>,
        scenario: &Scenario,
        globals: &mut GlobalData,
        audio: &GlobalAudio,
    ) -> Self {
        let info = scenario.info_tables().movie_info.first();
        let path = info.map(|info| {
            globals.unlock_startup_movie(info);
            info.path()
        });
        let transparent =
            info.is_some_and(|info| info.transparency == MovieTransparencyMode::Transparent);
        let volume = Volume(globals.save().settings.v0_bgmvol as f32 * 0.01);
        let manager = audio.manager.clone();
        let loading = shin_tasks::async_io::spawn(async move {
            let Some(path) = path else { return Ok(None) };
            let movie = server.load::<Movie, _>(path).await?;
            let device = server.context().wgpu_device.clone();
            // FFmpeg's pipes run on the single I/O executor. Its blocking first
            // frame read must yield that executor while the compute pool waits.
            shin_tasks::compute::spawn(move || {
                movie
                    .prepare_with_volume(&device, &manager, volume)
                    .map(Some)
            })
            .await
        });
        Self {
            motion: TitleDemoMotion::new(),
            loading: Some(loading),
            player: None,
            transparent,
            area: InputArea::new(1920, 1080),
            input: SkipInput::default(),
            extent: (1920, 1080),
            applied_volume: volume,
        }
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
        if self.extent != extent {
            self.area.disable(&mut self.input);
            self.area = InputArea::new(extent.0, extent.1);
            self.extent = extent;
        }
        let mut published = false;
        if let Some(result) = self.loading.as_mut().and_then(|task| task.poll_naive()) {
            self.loading = None;
            self.player = result?;
            if let Some(player) = &self.player {
                player.start();
            }
            self.motion.worker_ready(self.player.is_some());
            // Task group iteration does not invoke a newly added monitor until
            // its next poll; this frame still advances clock and player below.
            published = true;
        }
        let finished = self
            .player
            .as_ref()
            .is_some_and(VideoPlayerHandle::is_finished);
        self.input.requested = false;
        let skip = if !published && !finished && self.motion.accepts_input() {
            let pointer_captured = self.area.update(&mut self.input, frame);
            self.input.requested
                || (!pointer_captured
                    && (state[AppAction::Enter].is_clicked || state[AppAction::Cancel].is_clicked))
        } else {
            self.area.disable(&mut self.input);
            false
        };
        if !published && self.motion.prepare_frame(finished, skip) {
            audio
                .system_player
                .set_settings_volume(globals.save().settings.v3_sysvol);
            audio
                .system_player
                .play(ctx.asset_server, "decide", Volume::default());
        }
        self.motion.advance(ctx.delta_ticks);
        if let Some(player) = &self.player {
            player.update(ctx.frame_id, ctx.delta_ticks, ctx.pre_render.queue);
            let volume =
                Volume(self.motion.opacity() * globals.save().settings.v0_bgmvol as f32 * 0.01);
            if volume != self.applied_volume {
                player.set_volume(volume);
                self.applied_volume = volume;
            }
        }
        Ok(())
    }

    pub fn take_destination(&mut self) -> Option<i32> {
        self.motion.take_title_argument()
    }

    #[cfg(test)]
    pub fn reference_ready(&self) -> bool {
        self.loading.is_none() && self.player.is_some()
    }
    #[cfg(test)]
    pub fn reference_opacity(&self) -> f32 {
        self.motion.opacity()
    }
    #[cfg(test)]
    pub fn reference_finish(&self) {
        self.player.as_ref().unwrap().terminate();
    }
    #[cfg(test)]
    pub fn reference_advance_fade(&mut self, delta: shin_core::time::Ticks) {
        self.motion.advance(delta);
    }

    pub fn render(&self, pass: &mut RenderPass) {
        let Some(frame) = self.player.as_ref().and_then(VideoPlayerHandle::get_frame) else {
            return;
        };
        // The native player draws in the full1920x1080 scene viewport.
        let size = frame.get_frame().get_size();
        let visible_height = if self.transparent {
            (size.y - 8.0).max(1.0)
        } else {
            size.y
        };
        frame.render_with_options(
            pass,
            RenderRequestBuilder::new().color_blend_type(if self.motion.opacity() < 1.0 {
                ColorBlendType::Layer1
            } else {
                ColorBlendType::Opaque
            }),
            top_left_projection_matrix()
                * Mat4::from_scale(glam::vec3(1920.0 / size.x, 1080.0 / visible_height, 1.0)),
            self.transparent,
            // Native c0428 packs the float color through56840: truncate alpha
            // to a byte before multiplying the converted movie texture.
            Vec4::new(
                1.0,
                1.0,
                1.0,
                (self.motion.opacity() * 255.0) as u8 as f32 / 255.0,
            ),
        );
    }
}
