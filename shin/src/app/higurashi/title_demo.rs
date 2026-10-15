//! Updated e6328/e66ec/e6d40 opening movie, with older queued fade/render gain.
use crate::{
    adv::{
        higurashi::input::Buttons,
        input_area::{InputArea, InputAreaTarget, PointerFrame},
    },
    asset::{movie::Movie, system::AssetServer},
    audio::SystemSoundPlayer,
    layer::higurashi_properties::HigurashiTweener,
    render::top_left_projection_matrix,
};
use anyhow::{Result, bail, ensure};
use glam::{Mat4, Vec2, Vec4};
use parking_lot::Mutex;
use shin_core::{
    format::{save::higurashi::SaveData, scenario::higurashi::HigurashiScenario},
    primitives::update::FrameId,
    time::Ticks,
    vm::command::types::Volume,
};
use shin_render::{ColorBlendType, RenderRequestBuilder, render_pass::RenderPass};
use shin_video::VideoPlayerHandle;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TitleRequest {
    pub selected: i32,
    pub mode: u32,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Loading,
    Playing,
    Fading,
    Queued,
}
struct Motion {
    phase: Phase,
    opacity: HigurashiTweener,
    mode: u32,
    pending: Option<TitleRequest>,
}
impl Motion {
    fn new(mode: u32) -> Self {
        Self {
            phase: Phase::Loading,
            opacity: HigurashiTweener::new(0.0),
            mode,
            pending: None,
        }
    }
    fn publish(&mut self, has_player: bool) {
        self.opacity.fast_forward_to(1.0);
        if has_player {
            self.phase = Phase::Playing;
        } else {
            self.queue();
        }
    }
    fn queue(&mut self) {
        self.phase = Phase::Queued;
        self.pending = Some(TitleRequest {
            selected: -1,
            mode: self.mode,
        });
    }
    fn poll(
        &mut self,
        status: u32,
        permission: bool,
        captured: bool,
        pressed: u32,
        pointer_release: bool,
    ) -> Result<bool> {
        if self.phase == Phase::Playing {
            if status == 2 {
                self.queue();
            } else if permission && (pointer_release || (!captured && pressed & 0x01000008 != 0)) {
                self.phase = Phase::Fading;
                self.opacity.control(0.0, Ticks::from_f32(90.0), 0)?;
                return Ok(true);
            }
        } else if self.phase == Phase::Fading && (self.opacity.is_idle() || status == 2) {
            self.queue();
        }
        Ok(false)
    }
    fn update(&mut self, delta: Ticks) {
        self.opacity.update(delta);
    }
    fn gain(&self, percent: u8) -> f32 {
        self.opacity.value() * (f32::from(percent) * f32::from_bits(0x3c23d70a))
    }
}
fn contains(words: &[u32], id: usize) -> bool {
    words
        .get(id / 32)
        .is_some_and(|w| w & (1 << (id % 32)) != 0)
}
fn packed_alpha(value: f32) -> f32 {
    f32::from((value.max(0.0) * 255.0) as u8) * f32::from_bits(0x3b808081)
}
fn unlock(words: &mut Vec<u32>, id: usize) {
    let at = id / 32;
    if at >= words.len() {
        words.resize((at + 1).next_multiple_of(256), 0);
    }
    words[at] |= 1 << (id % 32);
}
fn mode(argument: u32, data: &SaveData) -> u32 {
    let flag = argument & 1;
    if flag != 0 && !contains(&data.save_vectors.unlocked_movies, 0) {
        0
    } else {
        flag
    }
}
#[derive(Clone, Debug)]
struct Descriptor {
    index: usize,
    path: String,
    bgm: i16,
    cg: i16,
}
fn descriptor(scenario: &HigurashiScenario, mode: u32) -> Option<Descriptor> {
    let index = if mode == 0 { 0 } else { 9 };
    scenario.movie_info.get(index).map(|info| Descriptor {
        index,
        path: format!(
            "/movie/{}.mp4",
            info.name.as_str().replace('?', "pc").to_ascii_lowercase()
        ),
        bgm: info.raw_fields[0],
        cg: info.raw_fields[1],
    })
}
impl Descriptor {
    fn record(&self, data: &mut SaveData) {
        unlock(&mut data.save_vectors.unlocked_movies, self.index);
        if self.bgm >= 0 {
            unlock(&mut data.save_vectors.unlocked_bgms, self.bgm as usize);
        }
        if self.cg >= 0 {
            unlock(&mut data.save_vectors.unlocked_cgs, self.cg as usize);
        }
    }
}
#[derive(Default)]
struct Skip {
    released: bool,
}
impl InputAreaTarget for Skip {
    fn probe(&mut self, _: Vec2) -> bool {
        true
    }
    fn reset(&mut self) {}
    fn press(&mut self, _: Vec2) {}
    fn release(&mut self, _: Vec2) {
        self.released = true;
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, _: Vec2, _: Vec2) {}
    fn gesture(&mut self, _: Vec2) {}
    fn cancel(&mut self) {}
}
struct Job {
    task: Option<shin_tasks::AsyncTask<Result<VideoPlayerHandle>>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl Drop for Job {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(task) = self.task.take() {
                task.cancel_and_retire();
            }
            self.retirement.clone().retire(());
        }
    }
}
struct Initializer {
    task: Option<shin_tasks::LoaderComputeTask<()>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl Drop for Initializer {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(task) = self.task.take() {
                task.cancel_and_retire();
            }
            self.retirement.clone().retire(());
        }
    }
}
pub(super) struct TitleDemo {
    // Producer retirement precedes the published player's decoder/audio teardown.
    loading: Option<Job>,
    initial: Option<Initializer>,
    descriptor: Option<Descriptor>,
    player: Option<VideoPlayerHandle>,
    motion: Motion,
    area: InputArea,
    skip: Skip,
    server: Arc<AssetServer>,
    sounds: Arc<Mutex<SystemSoundPlayer>>,
    manager: Arc<shin_audio::AudioManager>,
    fault: Option<String>,
}
impl TitleDemo {
    /// A decoded, retained frame and live playing phase precede skip input.
    pub(super) fn touch_diagnostic_phase(&self) -> Option<&'static str> {
        (self.fault.is_none()
            && self.motion.phase == Phase::Playing
            && self
                .player
                .as_ref()
                .is_some_and(|player| player.get_retained_frame().is_some()))
        .then_some("demo")
    }
    pub fn new(
        server: Arc<AssetServer>,
        manager: Arc<shin_audio::AudioManager>,
        sounds: Arc<Mutex<SystemSoundPlayer>>,
        scenario: &HigurashiScenario,
        data: &SaveData,
        argument: u32,
    ) -> Self {
        let mode = mode(argument, data);
        let motion = Motion::new(mode);
        let descriptor = descriptor(scenario, mode);
        let task = shin_tasks::compute::spawn_loader(|| ());
        let retirement = task.retirement_handle();
        Self {
            loading: None,
            initial: Some(Initializer {
                task: Some(task),
                retirement,
            }),
            descriptor,
            player: None,
            motion,
            area: InputArea::new(1280, 720),
            skip: Skip::default(),
            server,
            sounds,
            manager,
            fault: None,
        }
    }
    fn begin(&mut self, data: &mut SaveData) {
        self.motion.opacity.fast_forward_to(1.0);
        if let Some(info) = self.descriptor.take() {
            // e690c/e6920/e6934 commit unlocks before the movie file is read.
            info.record(data);
            let assets = self.server.clone();
            let manager = self.manager.clone();
            let task = shin_tasks::async_io::spawn_loader(async move {
                let movie: Arc<Movie> = assets.load(info.path).await?;
                let device = assets.context().wgpu_device.clone();
                shin_tasks::compute::spawn(move || {
                    movie.prepare_with_volume(&device, &manager, Volume(1.0))
                })
                .await
            });
            let retirement = task.retirement_handle().unwrap();
            self.loading = Some(Job {
                task: Some(task),
                retirement,
            });
        } else {
            self.motion.publish(false);
        }
    }
    pub fn update(
        &mut self,
        frame_id: FrameId,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        queue: &wgpu::Queue,
        data: &mut SaveData,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.0,
            "Invalid TitleDemo delta"
        );
        if let Some(fault) = &self.fault {
            bail!("Higurashi opening movie failed: {fault}");
        }
        let result = (|| {
            let mut published = false;
            if let Some(initial) = &mut self.initial {
                if initial
                    .task
                    .as_mut()
                    .and_then(|task| task.poll_naive())
                    .is_some()
                {
                    self.initial.take();
                    self.begin(data);
                    published = true;
                }
            }
            if self.initial.is_none() && self.motion.phase == Phase::Loading {
                if let Some(job) = &mut self.loading {
                    if let Some(result) = job.task.as_mut().and_then(|task| task.poll_naive()) {
                        job.task = None;
                        let player = result?;
                        player.start();
                        self.player = Some(player);
                        self.motion.publish(true);
                        published = true;
                    }
                }
            }
            if !published {
                self.skip.released = false;
                let status = if self
                    .player
                    .as_ref()
                    .is_some_and(VideoPlayerHandle::is_finished)
                {
                    2
                } else {
                    1
                };
                let captured = if permission && self.motion.phase == Phase::Playing && status != 2 {
                    self.area.update(&mut self.skip, pointer)
                } else {
                    self.area.disable(&mut self.skip);
                    false
                };
                if self.motion.poll(
                    status,
                    permission,
                    captured,
                    buttons.pressed,
                    self.skip.released,
                )? {
                    self.sounds.lock().play(&self.server, "decide", Volume(1.0));
                }
            }
            // Original owner polls modes before advancing tween/player.
            self.motion.update(delta);
            if let Some(player) = &self.player {
                player.update(frame_id, delta, queue);
            }
            Ok(())
        })();
        if let Err(error) = &result {
            self.fault = Some(format!("{error:#}"));
        }
        result
    }
    pub fn take_title_request(&mut self) -> Option<TitleRequest> {
        self.motion.pending.take()
    }
    pub fn cancel_input(&mut self) {
        self.area.disable(&mut self.skip);
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub(super) fn reference_frame_ready(&self) -> bool {
        self.player
            .as_ref()
            .is_some_and(|player| player.get_retained_frame().is_some())
    }
    #[cfg(test)]
    pub(super) fn reference_motion(&self) -> (f32, bool) {
        (
            self.motion.opacity.value(),
            self.motion.phase == Phase::Fading,
        )
    }
    #[cfg(test)]
    pub(super) fn reference_retirements(&self) -> Vec<shin_tasks::LoaderRetirement> {
        self.loading
            .iter()
            .map(|job| job.retirement.clone())
            .chain(self.initial.iter().map(|job| job.retirement.clone()))
            .collect()
    }
    pub fn render(&self, pass: &mut RenderPass, data: &SaveData) {
        let Some(player) = &self.player else {
            return;
        };
        player.set_volume(Volume(self.motion.gain(data.settings.common.v0_bgmvol)));
        let Some(frame) = player.get_retained_frame() else {
            return;
        };
        let size = frame.get_frame().get_size();
        frame.render_with_options(
            pass,
            RenderRequestBuilder::new().color_blend_type(if self.motion.opacity.value() < 1.0 {
                ColorBlendType::Layer1
            } else {
                ColorBlendType::Opaque
            }),
            top_left_projection_matrix()
                * Mat4::from_scale(glam::vec3(1920.0 / size.x, 1080.0 / size.y, 1.0)),
            false,
            Vec4::new(1.0, 1.0, 1.0, packed_alpha(self.motion.opacity.value())),
        );
    }
}
impl Drop for TitleDemo {
    fn drop(&mut self) {
        // Published decoders are descendants of the preparation producer;
        // retire the player before joining that producer's complete scope.
        if let Some(player) = self.player.take() {
            player.terminate();
            drop(player);
        }
        self.loading.take();
        self.initial.take();
    }
}

#[cfg(test)]
mod tests;
