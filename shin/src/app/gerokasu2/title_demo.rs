//! Native DF9B0/DEB4C conditional boot movie owner (TitleDemo).
//!
//! DF9B0 creates TitleDemo only when unlocked movie bit1 is set; otherwise it
//! publishes CreateTitle(-1). First (DF360) records movie0 and its direct
//! linked picture/BGM bits before loading. Start (DEE7C) sets fade +B0 to one.
//! DF170 publishes CreateTitle(-1) once the movie finishes, and A/Plus or a
//! tap plays the decide cue and starts the 90-tick Exit fade (DF02C). The
//! Exit task (DFAE8) publishes CreateTitle(-1) on an idle fade or a finished
//! movie. DEDBC draws (1,1,1,fade) and drives volume fade*bgmvol*0.01.
use super::{AppAction, Viewport};
use crate::{
    adv::input_area::{PointerKind, PointerSampler},
    asset::{movie::Movie, system::AssetServer},
    audio::SystemSoundPlayer,
    render::top_left_projection_matrix,
};
use anyhow::{Result, bail};
use enum_map::EnumMap;
use glam::{Mat4, Vec2, Vec4};
use shin_core::{
    format::{save::gerokasu2::SaveData, scenario::gerokasu2::Gerokasu2Scenario},
    primitives::update::FrameId,
    time::{Easing, Ticks, Tweener},
    vm::command::types::Volume,
};
use shin_input::ActionState;
use shin_render::{ColorBlendType, RenderRequestBuilder, render_pass::RenderPass};
use shin_video::VideoPlayerHandle;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Loading,
    Playing,
    Exiting,
    Done,
}

/// DF9B0 reads settings +0x98 bit1, i.e. movie index one (the story opening).
pub(super) fn enabled(data: &SaveData) -> bool {
    data.save_vectors
        .unlocked_movies
        .first()
        .is_some_and(|word| word & 2 != 0)
}

fn unlock(words: &mut Vec<u32>, id: usize) -> bool {
    super::menu_audio::merge_unlocks(words, [id as u32])
}

#[derive(Clone, Debug)]
struct Descriptor {
    path: String,
    picture: i16,
    bgm: i16,
    transparent: bool,
}
impl Descriptor {
    fn new(scenario: &Gerokasu2Scenario) -> Option<Self> {
        scenario.movie_info.first().map(|info| Self {
            path: format!("/movie/{}.mp4", info.name.as_str()),
            picture: info.linked_picture_id,
            bgm: info.linked_bgm_id,
            transparent: info.transparency & 1 != 0,
        })
    }
    /// DF360: movie bit0, then the direct picture (+0x80) and BGM (+0x68)
    /// links, before the file is read.
    fn record(&self, data: &mut SaveData) -> bool {
        let vectors = &mut data.save_vectors;
        let mut changed = unlock(&mut vectors.unlocked_movies, 0);
        if let Ok(picture) = usize::try_from(self.picture) {
            changed |= unlock(&mut vectors.unlocked_cgs, picture);
        }
        if let Ok(bgm) = usize::try_from(self.bgm) {
            changed |= unlock(&mut vectors.unlocked_bgms, bgm);
        }
        changed
    }
}

struct Job {
    task: Option<shin_tasks::AsyncTask<Result<VideoPlayerHandle>>>,
    retirement: Option<shin_tasks::LoaderRetirement>,
}
impl Drop for Job {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(task) = self.task.take() {
                task.cancel_and_retire();
            }
            if let Some(retirement) = self.retirement.take() {
                retirement.retire(());
            }
        }
    }
}

/// Exit motion and publication rules, kept separate from playback.
struct Motion {
    phase: Phase,
    fade: Tweener,
}
impl Motion {
    fn new() -> Self {
        Self {
            phase: Phase::Loading,
            fade: Tweener::new_gerokasu2(0.0),
        }
    }
    fn start(&mut self, has_player: bool) {
        self.fade.fast_forward_to(1.0);
        self.phase = if has_player {
            Phase::Playing
        } else {
            Phase::Done
        };
    }
    /// Returns true when Exit begins (the caller plays the decide cue).
    fn poll(&mut self, finished: bool, skip: bool) -> bool {
        match self.phase {
            Phase::Playing if finished => self.phase = Phase::Done,
            Phase::Playing if skip => {
                self.phase = Phase::Exiting;
                self.fade
                    .gerokasu2_control(0.0, Ticks::from_f32(90.0), 0, Easing::Linear, false);
                return true;
            }
            Phase::Exiting if finished || self.fade.is_idle() => self.phase = Phase::Done,
            _ => {}
        }
        false
    }
    fn gain(&self, percent: u8) -> f32 {
        self.fade.value() * (f32::from(percent) * f32::from_bits(0x3c23d70a))
    }
}

pub(super) struct TitleDemo {
    loading: Option<Job>,
    player: Option<VideoPlayerHandle>,
    transparent: bool,
    motion: Motion,
    pointer: PointerSampler,
    pressed: bool,
    armed: bool,
    sounds: SystemSoundPlayer,
    assets: Arc<AssetServer>,
    fault: Option<String>,
    requested: bool,
}
impl TitleDemo {
    /// Constructs the owner and runs First. Returns whether First changed
    /// persistent gallery bits.
    pub fn new(
        assets: Arc<AssetServer>,
        audio: Arc<shin_audio::AudioManager>,
        scenario: &Gerokasu2Scenario,
        data: &mut SaveData,
    ) -> (Self, bool) {
        let mut sounds = SystemSoundPlayer::new(audio.clone());
        sounds.set_settings_volume(data.settings.v3_sysvol);
        let mut motion = Motion::new();
        let descriptor = Descriptor::new(scenario);
        let mut changed = false;
        let loading = match &descriptor {
            Some(info) => {
                changed = info.record(data);
                let path = info.path.clone();
                let loader = assets.clone();
                let task = shin_tasks::async_io::spawn_loader(async move {
                    let movie: Arc<Movie> = loader.load(path).await?;
                    let device = loader.context().wgpu_device.clone();
                    shin_tasks::compute::spawn(move || {
                        movie.prepare_with_volume(&device, &audio, Volume(0.0))
                    })
                    .await
                });
                let retirement = task.retirement_handle();
                Some(Job {
                    task: Some(task),
                    retirement,
                })
            }
            None => {
                // DEE7C without a movie publishes CreateTitle(-1) directly.
                motion.start(false);
                None
            }
        };
        (
            Self {
                loading,
                player: None,
                transparent: descriptor.is_some_and(|d| d.transparent),
                motion,
                pointer: PointerSampler::default(),
                pressed: false,
                armed: false,
                sounds,
                assets,
                fault: None,
                requested: false,
            },
            changed,
        )
    }

    pub fn cancel_input(&mut self) {
        self.pointer.cancel_contact(true);
        self.pressed = false;
        self.armed = false;
    }

    /// Native A (bit28) or Plus (bit8, per the A14D0 producer).
    fn skip_input(
        &mut self,
        input: &EnumMap<AppAction, ActionState>,
        point: Vec2,
        viewport: Viewport,
        seconds: f64,
    ) -> bool {
        if !input[AppAction::Enter].is_held
            && !input[AppAction::UnlockScenarios].is_held
            && !input[AppAction::PointerClick].is_held
        {
            self.armed = true;
        }
        let point = viewport.map(point);
        self.pointer.sample(
            point.unwrap_or(Vec2::splat(-1.0)),
            input[AppAction::PointerClick].is_held,
            seconds,
        );
        if !self.armed || self.motion.phase != Phase::Playing {
            self.pressed = false;
            return false;
        }
        let mut tapped = false;
        for event in self.pointer.frame().events {
            match event.kind {
                // DF2CC/DF300: the full-screen input area accepts a tap.
                PointerKind::Down => self.pressed = point.is_some(),
                PointerKind::Up => tapped |= std::mem::take(&mut self.pressed),
                PointerKind::Cancel => self.pressed = false,
                _ => {}
            }
        }
        tapped
            || (!self.pressed
                && (input[AppAction::Enter].is_clicked
                    || input[AppAction::UnlockScenarios].is_clicked))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        frame_id: FrameId,
        delta: Ticks,
        input: &EnumMap<AppAction, ActionState>,
        point: Vec2,
        viewport: Viewport,
        seconds: f64,
        queue: &wgpu::Queue,
    ) -> Result<()> {
        if let Some(fault) = &self.fault {
            bail!("Ger TitleDemo movie failed: {fault}");
        }
        let result = (|| {
            self.sounds.update();
            if self.motion.phase == Phase::Loading {
                if let Some(job) = &mut self.loading
                    && let Some(result) = job.task.as_mut().and_then(|t| t.poll_naive())
                {
                    job.task = None;
                    let player = result?;
                    player.start();
                    self.player = Some(player);
                    self.motion.start(true);
                }
                return Ok(());
            }
            let finished = self
                .player
                .as_ref()
                .is_none_or(VideoPlayerHandle::is_finished);
            let skip = self.skip_input(input, point, viewport, seconds);
            if self.motion.poll(finished, skip) {
                self.sounds.play(&self.assets, "decide", Volume(1.0));
            }
            self.motion.fade.update(delta);
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

    /// (playing, fade, retained frame)
    #[cfg(test)]
    pub(super) fn reference_state(&self) -> (bool, f32, bool) {
        (
            self.motion.phase == Phase::Playing,
            self.motion.fade.value(),
            self.player
                .as_ref()
                .is_some_and(|p| p.get_retained_frame().is_some()),
        )
    }

    pub fn update_failed(&self) -> bool {
        self.fault.is_some()
    }

    /// One CreateTitle(-1) publication per owner.
    pub fn take_title_request(&mut self) -> bool {
        if self.motion.phase == Phase::Done && !self.requested {
            self.requested = true;
            return true;
        }
        false
    }

    pub fn render(&self, pass: &mut RenderPass, data: &SaveData) {
        let Some(player) = &self.player else {
            return;
        };
        player.set_volume(Volume(self.motion.gain(data.settings.v0_bgmvol)));
        let Some(frame) = player.get_retained_frame() else {
            return;
        };
        let alpha = self.motion.fade.value();
        let size = frame.get_frame().get_size();
        frame.render_with_options(
            pass,
            RenderRequestBuilder::new().color_blend_type(if self.transparent || alpha < 1.0 {
                ColorBlendType::Layer1
            } else {
                ColorBlendType::Opaque
            }),
            top_left_projection_matrix()
                * Mat4::from_scale(glam::vec3(1920.0 / size.x, 1080.0 / size.y, 1.0)),
            self.transparent,
            Vec4::new(1.0, 1.0, 1.0, alpha),
        );
    }
}
impl Drop for TitleDemo {
    fn drop(&mut self) {
        if let Some(player) = self.player.take() {
            player.terminate();
        }
        self.loading.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_fades_ninety_ticks_and_finish_publishes_immediately() {
        let mut motion = Motion::new();
        motion.start(true);
        assert!(!motion.poll(false, false));
        assert!(motion.poll(false, true));
        assert_eq!(motion.phase, Phase::Exiting);
        assert!(!motion.poll(false, true), "Exit plays the cue once");
        for _ in 0..89 {
            motion.fade.update(Ticks::from_f32(1.0));
            motion.poll(false, false);
        }
        assert_eq!(motion.phase, Phase::Exiting);
        assert!((motion.gain(100) - 1.0 / 90.0).abs() < 1e-4);
        motion.fade.update(Ticks::from_f32(1.0));
        motion.poll(false, false);
        assert_eq!(motion.phase, Phase::Done);

        let mut motion = Motion::new();
        motion.start(true);
        motion.poll(true, true);
        assert_eq!(motion.phase, Phase::Done, "finish wins over input");
    }

    #[test]
    fn gate_reads_opening_bit_and_first_records_direct_links() {
        let mut data = SaveData::default();
        assert!(!enabled(&data));
        data.save_vectors.unlocked_movies = vec![2];
        assert!(enabled(&data));
        let info = Descriptor {
            path: String::new(),
            picture: 3,
            bgm: -1,
            transparent: false,
        };
        assert!(info.record(&mut data));
        assert_eq!(data.save_vectors.unlocked_movies[0], 3);
        assert_eq!(data.save_vectors.unlocked_cgs[0], 8);
        assert!(data.save_vectors.unlocked_bgms.is_empty());
        assert!(!info.record(&mut data));
    }
}
