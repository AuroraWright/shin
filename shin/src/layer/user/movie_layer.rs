#[cfg(test)]
use shin_render::shaders::types::texture::TextureTarget;
use std::{fmt::Debug, sync::Arc};

use glam::{Mat4, vec3};
use parking_lot::Mutex;
use shin_audio::AudioManager;
use shin_core::{
    format::scenario::info::{MovieTransparencyMode, MovieVolumeSource},
    primitives::update::{FrameId, UpdateTracker},
    vm::command::types::Volume,
};
use shin_render::{
    ColorBlendType, LayerBlendType, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{RenderClone, RenderCloneCtx},
};
use shin_video::VideoPlayerHandle;
use tracing::error;

use crate::{
    asset::{movie::Movie, picture::Picture},
    layer::{
        DrawableLayer, Layer, NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{
            NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass, NewDrawableLayerState,
        },
        properties::LayerProperties,
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
        user::picture_layer::PictureLayer,
    },
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext, Updatable, UpdateContext},
};

struct Shared {
    update_tracker: UpdateTracker,
    volume_source: MovieVolumeSource,
    local_volume: Volume,
    applied_volume: Volume,
    video_player: Arc<VideoPlayerHandle>,
    audio_manager: Arc<AudioManager>,
}

#[cfg(test)]
pub(crate) struct ReferencePreparationGate {
    pub label: String,
    pub entered: std::sync::mpsc::Sender<std::sync::Weak<Movie>>,
    pub resume: std::sync::mpsc::Receiver<()>,
    pub prepared: std::sync::mpsc::Sender<std::sync::Weak<VideoPlayerHandle>>,
    pub finish: std::sync::mpsc::Receiver<()>,
    pub cancelled: std::sync::mpsc::Sender<()>,
}
#[cfg(test)]
static PREPARATION_GATE: std::sync::Mutex<Option<ReferencePreparationGate>> =
    std::sync::Mutex::new(None);
#[cfg(test)]
pub(crate) fn reference_preparation_gate(gate: ReferencePreparationGate) {
    assert!(PREPARATION_GATE.lock().unwrap().replace(gate).is_none());
}
#[cfg(test)]
struct ReferenceFutureDrop(std::sync::mpsc::Sender<()>);
#[cfg(test)]
impl Drop for ReferenceFutureDrop {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[cfg(test)]
pub(crate) struct ReferenceMovieOwner {
    backend: std::sync::Weak<Mutex<Shared>>,
    player: std::sync::Weak<VideoPlayerHandle>,
}
#[cfg(test)]
impl ReferenceMovieOwner {
    pub(crate) fn loop_worker(&self) -> Option<shin_video::ReferenceLoopWorker> {
        self.player.upgrade().unwrap().reference_loop_worker()
    }

    pub(crate) fn gate_next_loop(&self, gate: shin_video::ReferenceLoopPreparationGate) {
        self.player
            .upgrade()
            .unwrap()
            .reference_gate_next_loop_preparation(gate);
    }
    pub(crate) fn identity(&self) -> (usize, usize) {
        (
            self.backend.as_ptr() as usize,
            self.player.as_ptr() as usize,
        )
    }
    pub(crate) fn owners(&self) -> usize {
        self.backend.strong_count()
    }
    pub(crate) fn time(&self) -> u64 {
        self.player.upgrade().unwrap().current_time()
    }
    pub(crate) fn frame_start_time(&self) -> Option<u64> {
        self.player
            .upgrade()
            .unwrap()
            .current_frame_timing()
            .map(|timing| timing.start_time)
    }
    pub(crate) fn assert_released(&self) {
        assert!(
            self.backend.upgrade().is_none(),
            "Final layer releases shared movie backend"
        );
        assert!(
            self.player.upgrade().is_none(),
            "Final layer releases decoder and movie voice owner"
        );
    }
}

impl Shared {
    fn new(
        volume_source: MovieVolumeSource,
        local_volume: Volume,
        video_player: impl Into<Arc<VideoPlayerHandle>>,
        audio_manager: Arc<AudioManager>,
        applied_volume: Volume,
    ) -> Self {
        let video_player = video_player.into();
        video_player.set_volume(applied_volume);
        Self {
            update_tracker: UpdateTracker::new(),
            volume_source,
            local_volume,
            applied_volume,
            video_player,
            audio_manager,
        }
    }

    fn is_finished(&self) -> bool {
        self.video_player.is_finished()
    }

    fn update(&mut self, ctx: &AdvUpdateContext) {
        // Front/back page render clones share playback, including loop restarts.
        if !self.update_tracker.update(ctx.frame_id) {
            return;
        }
        let volume = ctx
            .movie_volume_settings
            .volume(self.volume_source, self.local_volume);
        self.video_player
            .update(ctx.frame_id, ctx.delta_ticks, ctx.queue);
        // Native shared update advances playback before publishing settings gain
        // (c2b30, c2bac). The same voice retains its gain across decoder rewinds.
        if volume != self.applied_volume {
            self.video_player.set_volume(volume);
            self.applied_volume = volume;
        }
    }
}

#[derive(RenderClone)]
pub struct MovieLayerImpl {
    movie_label: String,
    #[render_clone(needs_render)]
    still_picture: Option<PictureLayer>,
    shared: Option<Arc<Mutex<Shared>>>,
    transparency: MovieTransparencyMode,
    repeat: bool,
}

pub struct MovieArgs {
    pub volume_source: MovieVolumeSource,
    pub transparency: MovieTransparencyMode,
    pub local_volume: Volume,
    pub repeat: bool,
}

struct MovieBackendArgs {
    pub is_bgm: bool,
    pub transparency: MovieTransparencyMode,
    pub repeat: bool,
}

#[cfg(test)]
impl MovieLayer {
    pub(super) fn reference_composed_owner_flags(&self) -> (u8, bool, bool, usize) {
        let inner = self.inner_ref();
        (
            u8::from(inner.transparency == MovieTransparencyMode::Transparent),
            inner.repeat,
            inner.shared.is_some(),
            inner.still_picture.as_ref().unwrap() as *const PictureLayer as usize,
        )
    }
    pub(super) fn reference_composed_owner_detach(&mut self) {
        // Prepared per-owner terminal pointer retirement,without timed update.
        self.inner_mut().shared = None;
    }
}

pub type MovieLayer = NewDrawableLayerWrapper<MovieLayerImpl>;

impl MovieLayer {
    pub async fn new(
        device: &wgpu::Device,
        audio_manager: &Arc<AudioManager>,
        movie: Arc<Movie>,
        MovieArgs {
            volume_source,
            transparency,
            local_volume,
            repeat,
        }: MovieArgs,
        // NB: the original engine uses PictureLayer here
        // we have decoupled asset type from the layer type though
        still_picture: Option<Arc<Picture>>,
        volume_settings: crate::audio::MovieVolumeSettings,
    ) -> Self {
        // Native loader83a4c constructs the linked PictureLayer before
        // MovieLayer83b9c. Each drawable owns a seed and lazy property state.
        let mut layer = NewDrawableLayerWrapper::from_inner(MovieLayerImpl {
            movie_label: movie.label().to_string(),
            still_picture: still_picture.map(|picture| PictureLayer::new(picture, None)),
            shared: None,
            transparency,
            repeat,
        });
        let applied_volume = volume_settings.volume(volume_source, local_volume);
        let device = device.clone();
        let audio_manager = audio_manager.clone();
        // Decoder initialization waits for FFmpeg output. Keep the shared I/O
        // executor free to feed its input and drain stdout/stderr concurrently.
        let retained_audio = audio_manager.clone();
        #[cfg(test)]
        let gate = {
            let mut slot = PREPARATION_GATE.lock().unwrap();
            if slot
                .as_ref()
                .is_some_and(|gate| gate.label == movie.label())
            {
                slot.take()
            } else {
                None
            }
        };
        #[cfg(test)]
        let _future_drop = gate
            .as_ref()
            .map(|gate| ReferenceFutureDrop(gate.cancelled.clone()));
        let video_player = shin_tasks::compute::spawn(move || {
            #[cfg(test)]
            if let Some(gate) = &gate {
                gate.entered.send(Arc::downgrade(&movie)).unwrap();
                gate.resume
                    .recv_timeout(std::time::Duration::from_secs(15))
                    .unwrap();
            }
            let result = if repeat {
                movie.play_looping_with_volume(&device, &audio_manager, applied_volume)
            } else {
                movie.play_with_volume(&device, &audio_manager, applied_volume)
            }
            .map(Arc::new);
            #[cfg(test)]
            if let Some(gate) = &gate {
                gate.prepared
                    .send(Arc::downgrade(
                        result.as_ref().expect("Real Movie preparation"),
                    ))
                    .unwrap();
                gate.finish
                    .recv_timeout(std::time::Duration::from_secs(15))
                    .unwrap();
            }
            result
        })
        .await;
        let shared = match video_player {
            Ok(player) => Some(Arc::new(Mutex::new(Shared::new(
                volume_source,
                local_volume,
                player,
                retained_audio,
                applied_volume,
            )))),
            Err(error) => {
                // Native decoder failure is terminal. Its layer drops the
                // backend while retaining its separately owned still picture.
                error!(movie = %layer.inner_ref().movie_label, %error, "Movie preparation failed");
                None
            }
        };
        layer.inner_mut().shared = shared;
        layer
    }

    #[cfg(test)]
    pub(crate) fn reference_playback_status(&self) -> String {
        let inner = self.inner_ref();
        let backend = inner.shared.as_ref().map(|shared| {
            let shared = shared.lock();
            let player = &shared.video_player;
            format!(
                "time={} paused={} finished={} frame={:?}",
                player.current_time(),
                player.is_paused(),
                player.is_finished(),
                player.current_frame_timing()
            )
        });
        format!(
            "label={} repeat={} backend={backend:?}",
            inner.movie_label, inner.repeat
        )
    }

    #[cfg(test)]
    pub(crate) fn reference_publication_owner(&self) -> ReferenceMovieOwner {
        let backend = self
            .inner_ref()
            .shared
            .as_ref()
            .expect("Prepared movie backend");
        ReferenceMovieOwner {
            backend: Arc::downgrade(backend),
            player: Arc::downgrade(&backend.lock().video_player),
        }
    }

    #[cfg(test)]
    pub(crate) fn reference_still_owner(&self) -> std::sync::Weak<Picture> {
        self.inner_ref()
            .still_picture
            .as_ref()
            .unwrap()
            .reference_resource_owner()
    }

    #[cfg(test)]
    pub(crate) fn reference_has_still_picture(&self) -> bool {
        self.inner_ref().still_picture.is_some()
    }

    #[cfg(test)]
    pub(crate) fn reference_still_properties(&self) -> crate::layer::properties::LayerProperties {
        self.inner_ref()
            .still_picture
            .as_ref()
            .unwrap()
            .properties()
            .retain_for_clone()
    }

    #[cfg(test)]
    pub(crate) fn reference_still_state(&self) -> (usize, u32, usize) {
        let picture = self.inner_ref().still_picture.as_ref().unwrap();
        (
            picture.reference_resource_identity(),
            picture.reference_dissolve_seed(),
            picture.properties().identity(),
        )
    }

    pub fn is_finished(&self) -> bool {
        self.inner_ref()
            .shared
            .as_ref()
            .is_none_or(|shared| shared.lock().is_finished())
    }

    #[cfg(test)]
    pub(crate) fn assert_reference_backend_retirement_with_clones(
        &self,
        context: &mut PreRenderContext,
        server: &Arc<crate::asset::system::AssetServer>,
    ) {
        use shin_core::time::Ticks;
        let audio = Arc::new(AudioManager::new_for_tests());
        for (path, skip) in [("/movie/op1.mp4", true), ("/movie/door.mp4", false)] {
            let load_server = server.clone();
            let movie = shin_tasks::block_on(shin_tasks::async_io::spawn(async move {
                load_server.load::<Movie, _>(path).await.unwrap()
            }));
            let device = context.device.clone();
            let worker_audio = audio.clone();
            let mut layer = shin_tasks::block_on(shin_tasks::async_io::spawn(async move {
                MovieLayer::new(
                    &device,
                    &worker_audio,
                    movie,
                    MovieArgs {
                        volume_source: MovieVolumeSource::Bgm,
                        transparency: MovieTransparencyMode::Opaque,
                        local_volume: Volume(0.0),
                        repeat: false,
                    },
                    None,
                    Default::default(),
                )
                .await
            }));
            layer.inner_mut().still_picture = self
                .inner_ref()
                .still_picture
                .render_clone(&mut context.render_clone_ctx());
            let mut clone = layer.render_clone(&mut context.render_clone_ctx());
            let backend = Arc::downgrade(layer.inner_ref().shared.as_ref().unwrap());
            let player = Arc::downgrade(
                &layer
                    .inner_ref()
                    .shared
                    .as_ref()
                    .unwrap()
                    .lock()
                    .video_player,
            );
            let mut frame = FrameId::default();
            frame.advance();
            let update = AdvUpdateContext {
                frame_id: frame,
                delta_ticks: if skip {
                    Ticks::ZERO
                } else {
                    Ticks::from_seconds(5.01)
                },
                asset_server: server,
                device: context.device,
                queue: context.queue,
                are_animations_allowed: true,
                voice_callback: None,
                movie_volume_settings: Default::default(),
            };
            if skip {
                Layer::fast_forward(&mut layer);
                assert!(layer.is_finished());
                assert!(
                    !clone.is_finished(),
                    "Another render owner keeps playback alive"
                );
                assert_eq!(backend.strong_count(), 1);
                let before = player.upgrade().unwrap().current_time();
                std::thread::sleep(std::time::Duration::from_millis(150));
                assert!(player.upgrade().unwrap().current_time() > before);
                assert_eq!(audio.kira_manager().lock().num_sounds(), 1);
                Layer::fast_forward(&mut clone);
            } else {
                AdvUpdatable::update(&mut layer, &update);
                assert!(
                    layer.inner_ref().shared.is_none(),
                    "Completion retires this layer's backend reference"
                );
                assert!(
                    clone.inner_ref().shared.is_some(),
                    "Other owner retires on its own visit"
                );
                assert!(clone.is_finished());
                assert_eq!(backend.strong_count(), 1);
                assert!(player.upgrade().unwrap().get_retained_frame().is_some());
                AdvUpdatable::update(&mut clone, &update);
            }
            assert!(layer.inner_ref().shared.is_none());
            assert!(clone.inner_ref().shared.is_none());
            assert!(backend.upgrade().is_none());
            assert!(
                player.upgrade().is_none(),
                "Final reference releases decoder/texture owner"
            );
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while audio.kira_manager().lock().num_sounds() != 0 {
                assert!(
                    std::time::Instant::now() < deadline,
                    "Retired movie leaked audio capacity"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            layer.assert_reference_still_fallback(context);
            clone.assert_reference_still_fallback(context);
        }
        eprintln!(
            "Movie backend retirement: skip drops one owner while AAC continues for a render clone; terminal update releases each owner independently; last owner frees audio/decoder/texture and still pixels pass"
        );
    }

    #[cfg(test)]
    pub(crate) fn reference_failed_preparation_with_still(
        &self,
        context: &mut PreRenderContext,
        server: &Arc<crate::asset::system::AssetServer>,
    ) -> Self {
        use crate::asset::system::{
            AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache,
        };
        use shin_core::time::Ticks;
        let directory =
            std::env::temp_dir().join(format!("shin-movie-init-failure-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("broken.mp4");
        let output = std::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=16x16:r=30:d=0.1",
                "-c:v",
                "libx264",
                "-bf",
                "0",
                "-an",
                "-threads",
                "1",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        // Preserve all MP4 boxes and NAL lengths, but change slice NAL types to
        // filler. Asset/header parsing succeeds; the real decoder emits no image.
        let mut bytes = std::fs::read(&path).unwrap();
        let mut box_start = 0;
        let mut slices = 0;
        while box_start + 8 <= bytes.len() {
            let size =
                u32::from_be_bytes(bytes[box_start..box_start + 4].try_into().unwrap()) as usize;
            assert!(size >= 8 && box_start + size <= bytes.len());
            if &bytes[box_start + 4..box_start + 8] == b"mdat" {
                let mut nal = box_start + 8;
                while nal < box_start + size {
                    let length =
                        u32::from_be_bytes(bytes[nal..nal + 4].try_into().unwrap()) as usize;
                    assert!(length > 0 && nal + 4 + length <= box_start + size);
                    let header = &mut bytes[nal + 4];
                    if matches!(*header & 31, 1 | 5) {
                        *header = (*header & 224) | 12;
                        slices += 1;
                    }
                    nal += 4 + length;
                }
            }
            box_start += size;
        }
        assert!(slices > 0);
        std::fs::write(&path, bytes).unwrap();
        let mut io = LayeredAssetIo::new();
        io.try_with_dir(&directory).unwrap();
        let test_server = Arc::new(AssetServer::new(
            io.into(),
            AssetLoadContext {
                wgpu_device: context.device.clone(),
                wgpu_queue: context.queue.clone(),
                bustup_cache: AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ));
        let load_server = test_server.clone();
        let broken_movie = shin_tasks::block_on(shin_tasks::async_io::spawn(async move {
            load_server.load::<Movie, _>("/broken.mp4").await.unwrap()
        }));
        let audio = Arc::new(AudioManager::new_for_tests());
        let mut failed_repeat = None;
        for repeat in [false, true] {
            let movie = broken_movie.clone();
            let device = context.device.clone();
            let worker_audio = audio.clone();
            let mut layer = shin_tasks::block_on(shin_tasks::async_io::spawn(async move {
                MovieLayer::new(
                    &device,
                    &worker_audio,
                    movie,
                    MovieArgs {
                        volume_source: MovieVolumeSource::Bgm,
                        transparency: MovieTransparencyMode::Opaque,
                        local_volume: Volume(1.0),
                        repeat,
                    },
                    None,
                    Default::default(),
                )
                .await
            }));
            layer.inner_mut().still_picture = self
                .inner_ref()
                .still_picture
                .render_clone(&mut context.render_clone_ctx());
            assert!(
                layer.is_finished(),
                "Initialization error must be terminal even for repeat"
            );
            assert!(layer.inner_ref().shared.is_none());
            assert_eq!(audio.kira_manager().lock().num_sounds(), 0);
            let mut clone = layer.render_clone(&mut context.render_clone_ctx());
            let update = AdvUpdateContext {
                frame_id: FrameId::default(),
                delta_ticks: Ticks::from_seconds(60.0),
                asset_server: server,
                device: context.device,
                queue: context.queue,
                are_animations_allowed: true,
                voice_callback: None,
                movie_volume_settings: Default::default(),
            };
            AdvUpdatable::update(&mut layer, &update);
            AdvUpdatable::update(&mut clone, &update);
            Layer::fast_forward(&mut layer);
            assert!(clone.is_finished());
            layer.assert_reference_still_fallback(context);
            clone.assert_reference_still_fallback(context);
            if repeat {
                failed_repeat = Some(layer);
            }
        }
        drop(broken_movie);
        drop(test_server);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
        eprintln!(
            "Movie initial decoder failure: asset header loads, both repeat modes retire without panic/audio, linked still pixel equality and render clones pass"
        );
        failed_repeat.unwrap()
    }

    #[cfg(test)]
    pub(crate) fn assert_reference_mismatched_movie_durations(
        context: &mut PreRenderContext,
        audio: &Arc<AudioManager>,
    ) {
        use shin_core::time::Ticks;
        use shin_video::mp4::Mp4;
        for (video_seconds, audio_seconds) in [(1, 2), (2, 1)] {
            let path = std::env::temp_dir().join(format!(
                "shin-movie-deadline-{}-{video_seconds}-{audio_seconds}.mp4",
                std::process::id(),
            ));
            let output = std::process::Command::new("ffmpeg")
                .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
                .arg(format!("color=c=red:s=16x16:r=30:d={video_seconds}"))
                .args(["-f", "lavfi", "-i"])
                .arg(format!("anullsrc=r=48000:cl=stereo:d={audio_seconds}"))
                .args([
                    "-c:v", "libx264", "-bf", "0", "-c:a", "aac", "-threads", "1",
                ])
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let mp4 = Mp4::new(std::fs::File::open(&path).unwrap()).unwrap();
            let timescale = mp4
                .video_track
                .get_mp4_track_info(|track| track.timescale());
            let video_duration = mp4
                .video_track
                .get_mp4_track_info(|track| track.duration().as_micros() as u64);
            let audio_duration = mp4
                .audio_track
                .as_ref()
                .unwrap()
                .get_mp4_track_info(|track| track.duration().as_micros() as u64);
            let duration_us = mp4.duration_us();
            assert_eq!(duration_us, video_duration.max(audio_duration));
            assert!(duration_us >= 2_000_000 && duration_us < 2_050_000);
            let expected_end = (u128::from(duration_us) * u128::from(timescale) / 1_000_000) as u64;
            let device = context.device.clone();
            let worker_audio = audio.clone();
            let player = shin_tasks::block_on(shin_tasks::compute::spawn(move || {
                VideoPlayerHandle::new_paused_with_volume(&device, &worker_audio, mp4, Volume(0.0))
                    .unwrap()
            }));
            player.start();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let mut frame = FrameId::default();
            loop {
                frame.advance();
                // Audio drives this clock, so oversized deltas cannot skip it.
                player.update(frame, Ticks::from_seconds(60.0), context.queue);
                if player.current_time() >= u64::from(timescale) * 11 / 10 {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "Movie audio clock stalled"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(
                !player.is_finished(),
                "Shorter media track cannot finish the movie"
            );
            assert!(
                player.get_frame().is_some(),
                "Retain a drawable frame through the audio tail"
            );
            let frozen = player.current_time();
            player.stop();
            std::thread::sleep(std::time::Duration::from_millis(50));
            // One callback may publish the last source position before pause.
            let frozen = player.current_time().max(frozen);
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert_eq!(player.current_time(), frozen);
            assert!(!player.is_finished());
            player.start();
            while !player.is_finished() {
                frame.advance();
                player.update(frame, Ticks::ZERO, context.queue);
                assert!(
                    std::time::Instant::now() < deadline,
                    "Full movie deadline stalled"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(player.current_time() >= expected_end);
            frame.advance();
            player.update(frame, Ticks::ZERO, context.queue);
            let ended = player.current_time();
            std::thread::sleep(std::time::Duration::from_millis(100));
            // Completion update issues the owner's stop, then the callback
            // publishes its last processed block. It must subsequently freeze.
            let stopped = player.current_time().max(ended);
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert_eq!(player.current_time(), stopped);
            assert!(player.get_frame().is_none());
            assert!(player.get_retained_frame().is_some());
            let mp4 = Mp4::new(std::fs::File::open(&path).unwrap()).unwrap();
            let worker_audio = audio.clone();
            let device = context.device.clone();
            let looping = shin_tasks::block_on(shin_tasks::compute::spawn(move || {
                VideoPlayerHandle::new_looping_with_volume(
                    &device,
                    &worker_audio,
                    mp4,
                    Volume(0.0),
                    true,
                )
                .unwrap()
            }));
            assert!(looping.is_paused());
            looping.start();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let mut last_time = 0;
            loop {
                frame.advance();
                looping.update(frame, Ticks::from_seconds(60.0), context.queue);
                let time = looping.current_time();
                assert!(time >= last_time, "Repeating audio clock cannot reset");
                last_time = time;
                assert!(!looping.is_finished());
                if looping
                    .current_frame_timing()
                    .is_some_and(|timing| timing.start_time >= 2 * expected_end)
                {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "AAC/video loop boundary stalled"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            looping.stop();
            assert!(looping.get_frame().is_none());
            assert!(looping.get_retained_frame().is_some());
            std::thread::sleep(std::time::Duration::from_millis(50));
            let frozen = looping.current_time();
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert_eq!(looping.current_time(), frozen);
            looping.start();
            audio.set_suspended(true);
            std::thread::sleep(std::time::Duration::from_millis(50));
            let frozen = looping.current_time();
            frame.advance();
            looping.update(frame, Ticks::from_seconds(60.0), context.queue);
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert_eq!(looping.current_time(), frozen);
            audio.set_suspended(false);
            looping.terminate();
            assert!(looping.is_finished());
            std::fs::remove_file(path).unwrap();
        }
        eprintln!(
            "Movie unequal track deadlines and continuous repeats: both audio/video duration orders, two AAC/video boundaries without clock reset, local/global pause and owner release pass"
        );
    }

    #[cfg(test)]
    pub(crate) fn assert_reference_audio_restart_publication(
        context: &mut PreRenderContext,
        server: &Arc<crate::asset::system::AssetServer>,
        audio: &Arc<AudioManager>,
    ) {
        use shin_core::time::Ticks;
        let server_for_load = server.clone();
        let movie = shin_tasks::block_on(shin_tasks::async_io::spawn(async move {
            server_for_load
                .load::<Movie, _>("/movie/op1.mp4")
                .await
                .unwrap()
        }));
        let worker_movie = movie.clone();
        let device = context.device.clone();
        let worker_audio = audio.clone();
        let player = shin_tasks::block_on(shin_tasks::compute::spawn(move || {
            // A prepared repeated opening still has a single, locally paused voice.
            worker_movie
                .prepare_looping_with_volume(&device, &worker_audio, Volume(0.0))
                .unwrap()
        }));
        assert!(player.is_paused());
        assert_eq!(player.current_time(), 0);
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert_eq!(player.current_time(), 0);
        let mut shared = Shared::new(
            MovieVolumeSource::Bgm,
            Volume(0.0),
            player,
            audio.clone(),
            Volume(0.0),
        );
        let original = shared.video_player.clone();
        let mut frame = FrameId::default();
        frame.advance();
        let mut update = AdvUpdateContext {
            frame_id: frame,
            delta_ticks: Ticks::ZERO,
            asset_server: server,
            device: context.device,
            queue: context.queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        };
        shared.update(&update);
        assert!(shared.video_player.is_paused());
        shared.video_player.start();
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(shared.video_player.current_time() > 0);
        frame.advance();
        update.frame_id = frame;
        shared.update(&update);
        assert!(shared.video_player.get_frame().is_some());
        shared.video_player.stop();
        std::thread::sleep(std::time::Duration::from_millis(50));
        let frozen = shared.video_player.current_time();
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert_eq!(shared.video_player.current_time(), frozen);
        assert!(Arc::ptr_eq(&original, &shared.video_player));
        shared.video_player.start();
        shared.video_player.terminate();
        assert!(shared.is_finished());
        eprintln!(
            "Supplied opening AAC preparation: one retained looping player/voice, paused clock until Start, local Stop freezes samples pass"
        );
    }

    #[cfg(test)]
    pub(crate) fn assert_reference_loop_restarts(
        &mut self,
        context: &mut PreRenderContext,
        server: &Arc<crate::asset::system::AssetServer>,
    ) {
        use shin_core::time::Ticks;
        assert!(self.inner_ref().repeat);
        let mut clone = self.render_clone(&mut context.render_clone_ctx());
        assert!(Arc::ptr_eq(
            self.inner_ref().shared.as_ref().unwrap(),
            clone.inner_ref().shared.as_ref().unwrap()
        ));
        let mut frame = FrameId::default();
        // A settings mute changes gain without stopping the movie clock or
        // replacing the player. The next frame can restore its script volume.
        let mut settings = shin_core::format::save::Settings::default();
        settings.v0_bgmvol = 0;
        settings.v1_sfxvol = 0;
        settings.v2_voicevol = 0;
        let update = AdvUpdateContext {
            frame_id: frame,
            delta_ticks: Ticks::ZERO,
            asset_server: server,
            device: context.device,
            queue: context.queue,
            are_animations_allowed: false,
            voice_callback: None,
            movie_volume_settings: (&settings).into(),
        };
        let previous = self
            .inner_ref()
            .shared
            .as_ref()
            .unwrap()
            .lock()
            .video_player
            .clone();
        let initial_volume = self
            .inner_ref()
            .shared
            .as_ref()
            .unwrap()
            .lock()
            .applied_volume;
        AdvUpdatable::update(self, &update);
        AdvUpdatable::update(&mut clone, &update);
        assert_eq!(
            self.inner_ref()
                .shared
                .as_ref()
                .unwrap()
                .lock()
                .applied_volume,
            initial_volume
        );
        frame.advance();
        let update = AdvUpdateContext {
            frame_id: frame,
            ..update
        };
        // Movie clocks and gain ignore the layer animation permission. Frame
        // zero and a second scene visiting the same frame still skip both.
        AdvUpdatable::update(self, &update);
        {
            let shared = self.inner_ref().shared.as_ref().unwrap().lock();
            assert_eq!(
                shared.applied_volume,
                update
                    .movie_volume_settings
                    .volume(shared.volume_source, shared.local_volume)
            );
            assert!(Arc::ptr_eq(&previous, &shared.video_player));
            assert!(!shared.is_finished());
        }
        settings.v0_bgmvol = 100;
        settings.v1_sfxvol = 100;
        settings.v2_voicevol = 100;
        let duplicate = AdvUpdateContext {
            delta_ticks: Ticks::from_seconds(60.0),
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: (&settings).into(),
            ..update
        };
        AdvUpdatable::update(&mut clone, &duplicate);
        {
            let shared = self.inner_ref().shared.as_ref().unwrap().lock();
            assert_eq!(
                shared.applied_volume,
                update
                    .movie_volume_settings
                    .volume(shared.volume_source, shared.local_volume)
            );
            assert!(Arc::ptr_eq(&previous, &shared.video_player));
            assert!(
                !previous.is_finished(),
                "Duplicate scene update cannot consume movie time"
            );
        }
        let player = self
            .inner_ref()
            .shared
            .as_ref()
            .unwrap()
            .lock()
            .video_player
            .clone();
        let mut minimum_timestamp = 0;
        for cycle in 1..=2 {
            Layer::fast_forward(self);
            frame.advance();
            let update = AdvUpdateContext {
                frame_id: frame,
                delta_ticks: Ticks::from_seconds(5.01),
                asset_server: server,
                device: context.device,
                queue: context.queue,
                are_animations_allowed: true,
                voice_callback: None,
                movie_volume_settings: (&settings).into(),
            };
            AdvUpdatable::update(self, &update);
            let after_delta = player.current_time();
            assert!(!player.is_finished());
            assert!(
                player.get_frame().is_some(),
                "Keep final texture during decoder rewind"
            );
            assert!(
                !self.is_finished(),
                "MOVIEWAIT must keep waiting for repeat"
            );
            AdvUpdatable::update(&mut clone, &update);
            assert_eq!(
                player.current_time(),
                after_delta,
                "Shared scene cannot consume elapsed twice"
            );
            assert!(Arc::ptr_eq(
                &player,
                &self
                    .inner_ref()
                    .shared
                    .as_ref()
                    .unwrap()
                    .lock()
                    .video_player
            ));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                frame.advance();
                let update = AdvUpdateContext {
                    frame_id: frame,
                    delta_ticks: Ticks::ZERO,
                    ..update
                };
                AdvUpdatable::update(self, &update);
                AdvUpdatable::update(&mut clone, &update);
                let timing = player.current_frame_timing().unwrap();
                if timing.start_time >= cycle * 300_300 {
                    assert_eq!(timing.frame_number, 0);
                    assert!(timing.start_time > minimum_timestamp);
                    minimum_timestamp = timing.start_time;
                    assert_eq!(
                        player.current_time(),
                        after_delta,
                        "Decoder rewind cannot reset or advance the clock"
                    );
                    assert!(Arc::ptr_eq(
                        &player,
                        &clone
                            .inner_ref()
                            .shared
                            .as_ref()
                            .unwrap()
                            .lock()
                            .video_player
                    ));
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "Movie decoder rewind stalled"
                );
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn assert_reference_door_completion(
        &mut self,
        context: &mut PreRenderContext,
        server: &Arc<crate::asset::system::AssetServer>,
    ) {
        use shin_core::{primitives::color::UnormColor, time::Ticks};
        use shin_render::shaders::types::texture::TextureTargetKind;
        let inner = self.inner_ref();
        let player = inner.shared.as_ref().unwrap().lock().video_player.clone();
        assert!(!player.is_finished());
        assert!(
            player.get_frame().is_none(),
            "No uploaded frame before first update"
        );
        let mut frame = FrameId::default();
        frame.advance();
        player.update(frame, Ticks::from_seconds(0.1), context.queue);
        assert!(player.get_frame().is_some());
        let suspension = inner
            .shared
            .as_ref()
            .unwrap()
            .lock()
            .audio_manager
            .suspension();
        suspension.set_suspended(true);
        frame.advance();
        player.update(frame, Ticks::from_seconds(60.0), context.queue);
        assert!(
            !player.is_finished(),
            "Platform pause cannot consume movie time"
        );
        assert!(
            player.get_frame().is_some(),
            "Platform pause retains requested-started visibility"
        );
        player.stop();
        player.start();
        player.update(frame, Ticks::from_seconds(60.0), context.queue);
        assert!(
            !player.is_finished(),
            "Start cannot override platform suspension"
        );
        player.stop();
        suspension.set_suspended(false);
        player.update(frame, Ticks::from_seconds(60.0), context.queue);
        assert!(!player.is_finished());
        assert!(
            player.get_frame().is_none(),
            "Focus resume cannot override local Stop"
        );
        player.start();
        player.update(frame, Ticks::from_seconds(0.1), context.queue);
        player.stop();
        assert!(
            !player.is_finished(),
            "Stop must not mark an unfinished movie terminal"
        );
        assert!(
            player.get_frame().is_none(),
            "Stopped movie hides its started-frame draw"
        );
        assert!(player.get_retained_frame().is_some());
        self.assert_reference_retained_movie_frame(context);
        frame.advance();
        player.update(frame, Ticks::from_seconds(60.0), context.queue);
        assert!(
            !player.is_finished(),
            "Stopped clock must not consume elapsed time"
        );
        player.start();
        // An inactive visit does not consume the backend's frame update.
        // Supplied door.mp4: 150 frames, timescale60000, duration5.005s.
        player.update(frame, Ticks::from_seconds(4.78), context.queue);
        assert!(
            !player.is_finished(),
            "Final frame must retain its duration"
        );
        assert!(
            player.get_frame().is_some(),
            "Final frame must remain drawable"
        );
        frame.advance();
        player.update(frame, Ticks::from_seconds(0.03), context.queue);
        assert!(player.is_finished());
        assert!(player.get_frame().is_none());
        frame.advance();
        player.update(frame, Ticks::from_seconds(1.0), context.queue);
        assert!(player.is_finished());

        // Native c0460 still exposes the uploaded texture at EOF. Movie
        // owners switch to the linked picture only on their own update visit.
        self.assert_reference_retained_movie_frame(context);
        let mut unvisited = self.render_clone(&mut context.render_clone_ctx());
        frame.advance();
        let update = AdvUpdateContext {
            frame_id: frame,
            delta_ticks: Ticks::ZERO,
            asset_server: server,
            device: context.device,
            queue: context.queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        };
        self.update(&update);
        assert!(self.inner_ref().shared.is_none());
        self.assert_reference_still_fallback(context);
        assert!(unvisited.inner_ref().shared.is_some());
        unvisited.assert_reference_retained_movie_frame(context);
        unvisited.update(&update);
        assert!(unvisited.inner_ref().shared.is_none());
        unvisited.assert_reference_still_fallback(context);
        eprintln!(
            "Native Movie terminal presentation: local Stop and EOF retain uploaded frame; visited owner retires to linked still while unvisited clone keeps video until its own update pass"
        );
    }

    #[cfg(test)]
    fn assert_reference_retained_movie_frame(&self, context: &mut PreRenderContext) {
        use shin_core::primitives::color::UnormColor;
        use shin_render::shaders::types::texture::TextureTargetKind;
        let inner = self.inner_ref();
        let player = inner.shared.as_ref().unwrap().lock().video_player.clone();
        let properties = LayerProperties::new();
        let transform = TransformParams::default();
        let mut outputs = Vec::new();
        for expected in [false, true] {
            let target = context.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Retained movie frame at Stop/EOF"),
                size: wgpu::Extent3d {
                    width: 1920,
                    height: 1080,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = target.create_view(&Default::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    None,
                    "Retained movie texture comparison",
                );
                pass.clear(Some(UnormColor::from_rgba(255, 0, 255, 255)), None, None);
                if expected {
                    player.get_retained_frame().unwrap().render_with_options(
                        &mut pass,
                        RenderRequestBuilder::new()
                            .depth_stencil_shorthand(0, false, false)
                            .color_blend_type(ColorBlendType::Opaque),
                        transform.compute_final_transform()
                            * Mat4::from_translation(vec3(-960.0, -540.0, 0.0)),
                        false,
                        glam::Vec4::ONE,
                    );
                } else {
                    for kind in [PassKind::Opaque, PassKind::Transparent] {
                        inner.render_drawable_direct(
                            &mut pass,
                            &transform,
                            &properties.get_drawable_params(),
                            &properties.get_clip_params(),
                            0,
                            kind,
                        );
                    }
                }
            }
            outputs.push(readback_test_texture(context, &target));
        }
        assert_eq!(
            outputs[0], outputs[1],
            "Retained Movie owner draws uploaded video rather than early still"
        );
        assert!(
            outputs[0]
                .chunks_exact(4)
                .any(|pixel| pixel != [255, 0, 255, 255]),
            "Retained movie texture reaches the framebuffer"
        );
    }

    #[cfg(test)]
    fn assert_reference_still_fallback(&self, context: &mut PreRenderContext) {
        use shin_core::primitives::color::UnormColor;
        use shin_render::shaders::types::texture::TextureTargetKind;
        let inner = self.inner_ref();
        let picture = inner
            .still_picture
            .as_ref()
            .expect("Scenario still picture");
        let props = LayerProperties::new();
        let drawable = props.get_drawable_params();
        let clip = props.get_clip_params();
        let transform = TransformParams::default();
        let mut outputs = Vec::new();
        for expected in [false, true] {
            let target = context.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Movie completion comparison"),
                size: wgpu::Extent3d {
                    width: 1920,
                    height: 1080,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = target.create_view(&Default::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    None,
                    "Movie completion",
                );
                pass.clear(Some(UnormColor::from_rgba(255, 0, 255, 255)), None, None);
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    if expected {
                        picture.inner_ref().render_drawable_direct(
                            &mut pass, &transform, &drawable, &clip, 0, kind,
                        );
                    } else {
                        inner.render_drawable_direct(
                            &mut pass, &transform, &drawable, &clip, 0, kind,
                        );
                    }
                }
            }
            outputs.push(readback_test_texture(context, &target));
        }
        assert_eq!(
            outputs[0], outputs[1],
            "Movie fallback must render the linked picture"
        );
        assert!(
            outputs[0]
                .chunks_exact(4)
                .any(|pixel| pixel != [255, 0, 255, 255]),
            "Still picture must draw"
        );
    }
    #[cfg(test)]
    pub(crate) fn assert_reference_alpha_rendering(&self, context: &mut PreRenderContext) {
        use glam::{Vec3, Vec4};
        use shin_core::{primitives::color::UnormColor, time::Ticks};
        use shin_render::shaders::types::texture::TextureTargetKind;
        let texture = shin_video::VideoFrameTexture::new(
            context.device,
            winit::dpi::PhysicalSize::new(4, 16),
        );
        texture.write_data_nv12(
            context.queue,
            &shin_video::Nv12Frame {
                size: winit::dpi::PhysicalSize::new(4, 16),
                y_plane: [vec![235; 16], vec![16; 32], [16, 125, 235, 16].repeat(4)].concat(),
                uv_plane: vec![128; 32],
            },
        );
        let transform = crate::render::centered_projection_matrix()
            * Mat4::from_scale(Vec3::new(480.0, 135.0, 1.0))
            * Mat4::from_translation(Vec3::new(-2.0, -4.0, 0.0));
        let mut outputs = Vec::new();
        for (alpha_mode, opacity) in [(true, 1.0), (true, 0.5), (false, 1.0)] {
            let target = context.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Packed movie alpha test"),
                size: wgpu::Extent3d {
                    width: 1920,
                    height: 1080,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = target.create_view(&Default::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    None,
                    "Packed movie alpha",
                );
                pass.clear(Some(UnormColor::from_rgba(0, 64, 128, 255)), None, None);
                texture.render_with_options(
                    &mut pass,
                    RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                    transform,
                    alpha_mode,
                    Vec4::new(0.5, 1.0, 0.25, opacity),
                );
            }
            outputs.push(readback_test_texture(context, &target));
        }
        for (test, opacity) in [1.0f32, 0.5].into_iter().enumerate() {
            for (x, mask) in [(239, 0.0), (719, 109.0 / 219.0), (1199, 1.0), (1679, 0.0)] {
                for y in [0, 540, 1079] {
                    let pixel = &outputs[test][(y * 1920 + x) * 4..][..4];
                    for (channel, (color, background)) in
                        [(127.5, 0.0), (255.0, 64.0), (63.75, 128.0)]
                            .into_iter()
                            .enumerate()
                    {
                        let expected = color * mask * opacity + background * (1.0 - mask * opacity);
                        assert!(
                            (f32::from(pixel[channel]) - expected).abs() < 3.0,
                            "Packed alpha {test} at{x},{y}, channel{channel}: {pixel:?}, expected{expected}"
                        );
                    }
                }
            }
        }
        assert_ne!(
            outputs[0], outputs[2],
            "Opaque mode must sample the complete frame"
        );
        let inner = self.inner_ref();
        let player = inner.shared.as_ref().unwrap().lock().video_player.clone();
        assert_eq!(inner.transparency, MovieTransparencyMode::Transparent);
        let mut frame = FrameId::default();
        frame.advance();
        player.update(frame, Ticks::from_seconds(0.5), context.queue);
        assert!(player.get_frame().is_some());
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Supplied no03 alpha"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let props = LayerProperties::new();
        let mut rendered = Vec::new();
        for background in [
            UnormColor::from_rgba(0, 64, 128, 255),
            UnormColor::from_rgba(128, 64, 0, 255),
        ] {
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    None,
                    "Supplied no03 alpha",
                );
                pass.clear(Some(background), None, None);
                inner.render_drawable_direct(
                    &mut pass,
                    &TransformParams::default(),
                    &props.get_drawable_params(),
                    &props.get_clip_params(),
                    0,
                    PassKind::Transparent,
                );
            }
            rendered.push(readback_test_texture(context, &target));
        }
        let translucent = rendered[0]
            .chunks_exact(4)
            .zip(rendered[1].chunks_exact(4))
            .filter(|(a, b)| a[0].abs_diff(b[0]) > 2)
            .count();
        let opaque = rendered[0]
            .chunks_exact(4)
            .zip(rendered[1].chunks_exact(4))
            .filter(|(a, b)| a[0].abs_diff(b[0]) < 2 && a[2].abs_diff(b[2]) < 2)
            .count();
        assert!(
            translucent > 100_000 && opaque > 1000,
            "Real mask needs background-dependent pixels and opaque highlights: {translucent}, {opaque}"
        );
        let pixels = &rendered[0];
        let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
        for pixel in pixels.chunks_exact(4) {
            ppm.extend_from_slice(&pixel[..3]);
        }
        std::fs::write("/tmp/shin-transparent-movie.ppm", ppm).unwrap();
        let mut faded = LayerProperties::new();
        for (property, value) in [
            (
                shin_core::vm::command::types::LayerProperty::MulColorAlpha,
                500.0,
            ),
            (
                shin_core::vm::command::types::LayerProperty::MulColorRed,
                500.0,
            ),
        ] {
            faded.property_tweener_mut(property).fast_forward_to(value);
        }
        assert!(
            !inner.needs_separate_pass(&faded),
            "Tint and opacity use the movie shader directly"
        );
    }
}

impl NewDrawableLayerNeedsSeparatePass for MovieLayerImpl {
    fn needs_separate_pass(&self, props: &LayerProperties) -> bool {
        // Nativec3930 forces a source pass for the separately owned still
        // child. Its own properties compose first; movie shading applies after.
        if self.shared.is_none() && self.still_picture.is_some() {
            return true;
        }
        if props.get_clip_mode() != DrawableClipMode::None
            || !props
                .get_fragment_shader()
                .is_equivalent_to_default(props.get_fragment_shader_param())
        {
            return true;
        }

        false
    }
}

impl NewDrawableLayer for MovieLayerImpl {
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        drawable: &DrawableParams,
        clip: &DrawableClipParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        let shared = self.shared.as_ref().map(|shared| shared.lock());
        if shared.is_none() {
            if let Some(picture) = &self.still_picture {
                picture.render(pass, transform, stencil_ref, pass_kind);
            }
            return;
        }
        let shared = shared.as_ref().expect("Active movie has a shared backend");
        // TODO: I think some of these conditions conflict with the needs_separate_pass logic
        // (aka they will never be true)
        let target_pass = if self.transparency != MovieTransparencyMode::Opaque
            || drawable.blend_type != LayerBlendType::Type1
            || drawable.color_multiplier.a < 1.0
        {
            PassKind::Transparent
        } else {
            PassKind::Opaque
        };

        if pass_kind != target_pass {
            return;
        }

        // Nativec0460 tests only uploaded texture ownership, independently
        // of Stop or terminal status. This layer draws that retained texture
        // until its update releases Shared and selects the linked picture.
        let frame = shared.video_player.get_retained_frame();
        let Some(frame) = frame else {
            return;
        };

        // NB: the original engine uses a generic layer shader here, because it does YUV->RGB conversion in a separate pass
        // we try to do better, so we do the conversion in the main pass

        let transform =
            transform.compute_final_transform() * Mat4::from_translation(vec3(-960.0, -540.0, 0.0));

        frame.render_with_options(
            pass,
            RenderRequestBuilder::new()
                .depth_stencil_shorthand(stencil_ref, false, false)
                .color_blend_type(if target_pass == PassKind::Opaque {
                    ColorBlendType::Opaque
                } else {
                    ColorBlendType::from_regular_layer(drawable.blend_type)
                }),
            transform,
            self.transparency == MovieTransparencyMode::Transparent,
            drawable.color_multiplier.into_vec4(),
        );
    }
}

impl NewDrawableLayerFastForward for MovieLayerImpl {
    fn fast_forward_properties(&self) -> bool {
        // Both native Movie skip handlers c3440/c3470 bypass base property
        // fast-forward. Linked Picture clocks also continue independently.
        false
    }

    fn fast_forward(&mut self) {
        // Native 0xc3440/0xc3470 keeps repeating movies and releases others.
        if !self.repeat {
            self.shared = None;
        }
    }
}

impl AdvUpdatable for MovieLayerImpl {
    fn update(&mut self, ctx: &AdvUpdateContext) {
        if let Some(shared) = &self.shared {
            let finished = {
                let mut shared = shared.lock();
                shared.update(ctx);
                shared.is_finished()
            };
            if finished {
                // c3574 releases this layer's backend reference; render clones
                // retire their own references independently on their update.
                self.shared = None;
            }
        }
        // Nativec3590 updates the linked picture even while video is active.
        if let Some(picture) = &mut self.still_picture {
            picture.update(ctx);
        }
    }
}

impl Debug for MovieLayerImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("MovieLayer")
            .field(&self.movie_label)
            .finish()
    }
}

#[cfg(test)]
pub(crate) fn readback_test_texture(
    context: &mut PreRenderContext,
    target: &wgpu::Texture,
) -> Vec<u8> {
    let size = target.size();
    let row_bytes = size.width * 4;
    let padded_row_bytes =
        row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Movie completion readback"),
        size: u64::from(padded_row_bytes) * u64::from(size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    context.encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row_bytes),
                rows_per_image: Some(size.height),
            },
        },
        target.size(),
    );
    let mut upload = context.device.create_command_encoder(&Default::default());
    context.dynamic_buffer.finish(&mut upload);
    let encoder = std::mem::replace(
        context.encoder,
        context.device.create_command_encoder(&Default::default()),
    );
    context.queue.submit([upload.finish(), encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap()
        });
    context.device.poll(wgpu::Maintain::Wait);
    receive.recv().unwrap().unwrap();
    let mapped = readback.slice(..).get_mapped_range();
    if padded_row_bytes == row_bytes {
        return mapped.to_vec();
    }
    let mut pixels = Vec::with_capacity((row_bytes * size.height) as usize);
    for row in mapped.chunks_exact(padded_row_bytes as usize) {
        pixels.extend_from_slice(&row[..row_bytes as usize]);
    }
    pixels
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_movie_retained_texture_visibility_before_owner_retirement() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0xc3624, 0x9101a008u32),
            (0xc362c, 0xb40013a9),
            (0xc3698, 0xf9404108),
            (0xc36a0, 0xd63f0100),
            (0xc36a4, 0x36000ee0),
            (0xc046c, 0x91342008),
            (0xc0470, 0xc8dffd09),
            (0xc0474, 0xb4000349),
            (0xc0478, 0xf9466809),
            (0xc04cc, 0x320003e0),
            (0xbee54, 0x3931029f), // Stop clears started, not retained texture.
            (0xc3574, 0xc85ffec0),
            (0xc3578, 0xc808fedf),
            (0xc358c, 0xd63f0100),
        ] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected,
                "Native Movie retained draw instruction {address:x}"
            );
        }
        let relocation = |slot: u64| {
            executable[0x11e040 + 256..0x152eb8 + 256]
                .chunks_exact(24)
                .find_map(|entry| {
                    (u64::from_le_bytes(entry[..8].try_into().unwrap()) == slot)
                        .then(|| u64::from_le_bytes(entry[16..24].try_into().unwrap()))
                })
                .unwrap()
        };
        assert_eq!(
            relocation(relocation(0x225120) + 16 + 0x80),
            0xc0460,
            "Movie draw getter reads retained texture independently of terminal/started status"
        );
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_movie_platform_focus_suspension() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let u64_at = |address: usize| {
            u64::from_le_bytes(executable[address + 256..address + 264].try_into().unwrap())
        };
        let relocs: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
            .step_by(24)
            .map(|o| (u64_at(o), u64_at(o + 16)))
            .collect();
        for (address, target) in [
            (0x2248d8, 0x1d3bb0), // global audio suspension delegate
            (0x1d3ba0, 0x2c1e0),  // delegate clears mixer run event
            (0x1d3ba8, 0x2c1f0),  // delegate signals mixer run event
            (0x225590, 0x220ef0), // StreamSound vtable
            (0x220f48, 0xf51b0),  // StreamSound global pause interface +10
            (0x220f50, 0xf5290),  // StreamSound global resume interface +18
        ] {
            assert_eq!(
                relocs[&address], target,
                "Native audio suspension relocation at {address:x}"
            );
        }
        for (relocation, got, symbol, name) in [
            (
                0x1534e8,
                0x223cc0,
                76,
                "_ZN2nn2os14WaitLightEventEPNS0_14LightEventTypeE",
            ),
            (
                0x1542b0,
                0x224158,
                133,
                "_ZN2nn2os17TryWaitLightEventEPNS0_14LightEventTypeE",
            ),
            (
                0x154340,
                0x224188,
                217,
                "_ZN2nn2oe22PopNotificationMessageEv",
            ),
            (0x154358, 0x224190, 215, "_ZN2nn2oe20GetCurrentFocusStateEv"),
        ] {
            assert_eq!(u64_at(relocation), got);
            assert_eq!(u64_at(relocation + 8), (symbol << 32) | 0x402);
            let address = 0x154e88 + symbol as usize * 24 + 256;
            let name_offset =
                u32::from_le_bytes(executable[address..address + 4].try_into().unwrap()) as usize;
            let start = 0x1568f8 + name_offset + 256;
            let end = start
                + executable[start..]
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap();
            assert_eq!(&executable[start..end], name.as_bytes());
        }
        for (address, instruction) in [
            (0xbcd84, 0x91006260u32), // registers movie's Voice::Delegate at +18
            (0xbcd88, 0x9400fd6a),    // calls global delegate registration fc330
            (0xbec90, 0x91006260),    // releases the same interface when worker retires
            (0xbec94, 0x9400f5ff),    // unregisters through fc490
            (0xfc414, 0x39400509),    // registration samples global suspension byte241001
            (0xfc424, 0xf9400908),    // newly registered delegate pauses when suspended
            (0xfc434, 0xf9400d08),    // otherwise resumes
            (0xf9ce0, 0x39000288),    // main loop sets suspension flag before callbacks
            (0xf9cc0, 0x37001da0),    // signaled run event skips suspension path
            (0xf9e3c, 0xf9400908),    // each still-registered delegate pause virtual+10
            (0xf9e90, 0x94008be8),    // waits for run permission before scene updates
            (0xf9f50, 0xf9400d08),    // each still-registered delegate resume virtual+18
            (0xfa048, 0x3900029f),    // clears suspension flag after resume
            (0xfb440, 0x940088e0),    // notification reader
            (0xfb444, 0x71003c1f),    // only message15 changes focus event
            (0xfb44c, 0x940088e1),    // samples platform focus state
            (0xfb454, 0x7100091f),    // unsigned(state-1)<2 signals run permission
            (0xfb460, 0x54ffff01),    // state3 path clears event
            (0xfb468, 0x9400868a),    // ClearLightEvent
            (0x2aea8, 0x94034522),    // registers global audio delegate
            (0x2c1e8, 0x1403c32a),    // pause tail calls ClearLightEvent
            (0x2c1f8, 0x1403c32a),    // resume tail calls SignalLightEvent
            (0x2af44, 0x91006280),    // mixer run event22a150+18
            (0x2af48, 0x9403c7ba),    // waits before first mixer block
            (0x2b190, 0x91006260),    // same mixer run event at loop boundary
            (0x2b194, 0x9403c727),    // waits before next mixer block
            (0xf3ed0, 0xf8008ee9),    // StreamSound delegate interface at object+8
            (0xf3fc4, 0x940020db),    // registers interface with global focus registry
            (0xf51d4, 0x390a9288),    // pause sets independent platform gate
            (0xf52b0, 0x390a929f),    // resume clears platform gate
            (0xf52bc, 0x394ab909),    // requested-started state remains independent
            (0xf52c4, 0x394ac109),    // additional gate still prevents restarting
            (0xf52cc, 0x394abd08),    // additional gate still prevents restarting
            (0xf52f4, 0xf9400908),    // starts audio only when all gates permit
        ] {
            let actual =
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap());
            assert_eq!(
                actual, instruction,
                "Native focus instruction at {address:x}"
            );
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_movie_shared_frame_guard_and_gain_order() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let relocs: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
            .step_by(24)
            .map(|offset| {
                let address =
                    u64::from_le_bytes(executable[offset + 256..offset + 264].try_into().unwrap());
                let target =
                    u64::from_le_bytes(executable[offset + 272..offset + 280].try_into().unwrap());
                (address, target)
            })
            .collect();
        for (address, target) in [
            (0x1db858, 0xcd8c0),  // shared reference release
            (0x1db868, 0xc29b0),  // final shared deleting destructor
            (0x1db4e0, 0xbea70),  // shared destructor closes backend
            (0x225178, 0x1db840), // shared movie vtable
            (0x1db898, 0x6aa40),  // update interface dispatches generic frame guard
            (0x1db8a0, 0xc2b10),  // guarded shared body
            (0x225120, 0x1db4b0), // backend vtable
            (0x1db520, 0xbefb0),  // volume setter
            (0x1db530, 0xbf070),  // playback updater
            (0x1db500, 0xbed60),  // Start
            (0x1db508, 0xbee30),  // Stop
            (0x1db518, 0xbeee0),  // terminal status distinct from started flag
            (0x1db550, 0xc0c40),  // explicit pause
            (0x1db558, 0xc0d40),  // explicit resume
            (0x1db5a8, 0xc0cc0),  // audio Voice::Delegate pause thunk
            (0x1db5b0, 0xc0e10),  // audio Voice::Delegate resume thunk
        ] {
            assert_eq!(relocs[&address], target, "Native relocation at {address:x}");
        }
        for (address, instruction) in [
            (0xc3444, 0x34000048),    // skip preserves repeating owner
            (0xc3454, 0xc809fd1f),    // skip clears only this layer backend reference
            (0xc3464, 0xf9400501),    // skip invokes shared reference release
            (0xc3484, 0xc809fd1f),    // alternate skip clears only this layer reference
            (0xcd8c8, 0x5100050a),    // shared release decrements reference count
            (0xcd8d8, 0x7100051f),    // test previous reference count was one
            (0xcd8dc, 0x54000081),    // other owners prevent destructor
            (0xcd8e4, 0xf9400d01),    // last reference selects deleting destructor
            (0xc28c8, 0xf9401108),    // last shared owner selects backend Close virtual slot
            (0xc28cc, 0xd63f0100),    // close backend at final shared retirement
            (0xc28d4, 0xc808fe9f),    // clear final shared backend ownership
            (0xbea90, 0x080afd3f),    // backend Close clears open gate
            (0xc3548, 0xf9402d08),    // layer queries backend terminal status after shared update
            (0xc354c, 0xd63f0100),    // invoke terminal query virtual slot
            (0xc3570, 0x34000115),    // retain backend while nonterminal
            (0xc3574, 0xc85ffec0),    // exchange layer shared pointer for retirement
            (0xc3578, 0xc808fedf),    // clear layer shared pointer
            (0xc3588, 0xf9400508),    // load reference-release slot
            (0xc358c, 0xd63f0100),    // release layer backend ownership
            (0xc3590, 0x9101c288),    // still-picture owner is separate
            (0xc35a4, 0xf9400908),    // still picture continues its independent update
            (0xbdce8, 0xf9453669),    // video decode loads cumulative cycle offset
            (0xbdcec, 0x8b080128),    // video presentation timestamp includes cycle offset
            (0xbe400, 0xf9453669),    // audio decode loads cumulative cycle offset
            (0xbe404, 0x8b080128),    // audio presentation timestamp includes cycle offset
            (0xbe7b0, 0xf9453669),    // second audio path loads cumulative cycle offset
            (0xbe7b4, 0x8b080128),    // second audio path includes cycle offset
            (0xbf0ac, 0x9400037d),    // refill current audio voice
            (0xbf0c0, 0x14000424),    // then check repeat decoder EOF
            (0xc016c, 0x36000c88),    // require video decoder EOF
            (0xc017c, 0x36000c08),    // require audio decoder EOF
            (0xc0184, 0x34000bc8),    // require repeat flag
            (0xc01ac, 0xf9401908),    // video decoder finish/reset slot
            (0xc01bc, 0xf9401d08),    // video decoder rewind slot
            (0xc01d4, 0x089ffd1f),    // clear video EOF for next cycle
            (0xc01e4, 0xf9401908),    // audio decoder finish/reset slot
            (0xc01f4, 0xf9401d08),    // audio decoder rewind slot
            (0xc020c, 0x089ffd1f),    // clear audio EOF for next cycle
            (0xc0210, 0xf9453288),    // load one cycle duration
            (0xc0218, 0x8b080128),    // add duration to cumulative timestamp offset
            (0xc0220, 0xf9053688),    // retain cumulative offset
            (0xc02c8, 0x9401063e),    // dispatch next decode job without replacing audio voice
            (0xc2654, 0x97fea0e3u32), // constructor initializes zero frame marker
            (0xc2674, 0x91012129),    // shared update interface at vtable +48
            (0xc34ec, 0x4ea81d00),    // forwards delta to shared updater
            (0xc34f0, 0x2a1503e1),    // forwards animation permission
            (0xc34f4, 0xd63f0100),    // invokes guarded update
            (0xc2b2c, 0xf9403908),    // shared body loads playback update +70
            (0xc2b30, 0xd63f0100),    // advances backend before reading gain
            (0xc2b34, 0xb9400e68),    // reads volume source after playback
            (0xc2bac, 0xf9403101),    // loads volume setter +60
            (0xc2bb4, 0xd61f0020),    // final tail call applies gain
            (0xbefd8, 0xbd0d2688),    // setter saves gain for future backend restarts
            (0xbefec, 0xd63f0100),    // setter forwards gain to existing audio
            (0xbf090, 0xb94c4675),    // backend reads actual clock-playing status
            (0xbf09c, 0x34000155),    // paused backend skips clock/frame work
            (0xbf0a4, 0x9400000f),    // active backend advances playback
            (0xbed80, 0x39310288),    // Start sets requested-started flag
            (0xbee54, 0x3931029f),    // Stop clears requested-started flag
            (0xbee68, 0xf9400d08),    // Stop invokes audio pause virtual +18
            (0xbee74, 0x94011ebb),    // Stop pauses independent clock
            (0xbeefc, 0x39710e88),    // terminal status uses failure flag +c43
            (0xbc6e8, 0x39310adb),    // initializes preparation gate +c42
            (0xbdec8, 0x39310a7f),    // worker clears preparation gate
            (0xbd7ec, 0x39310e68),    // worker failure sets terminal gate
            (0xc0c60, 0x39310688),    // explicit pause sets +c41
            (0xc0d60, 0x3931069f),    // explicit resume clears +c41
        ] {
            let actual =
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap());
            assert_eq!(actual, instruction, "Native instruction at {address:x}");
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_movie_padding_and_repeat_handlers() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let path = std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable");
        let executable = std::fs::read(path).unwrap();
        for (address, instruction) in [
            (0xbf500, 0x51001d0au32), // subtract7 from inclusive crop height
            (0xbf504, 0x53017d4a),    // divide visible color height by2
            (0xbfccc, 0x11002108),    // add8 to mask start before normalization
            (0xc04c4, 0x1e202800),    // double transparent movie height for display
            (0xc0180, 0x39408a88),    // repeat byte gates decoder restart
            (0xbef28, 0x35000268),    // repeat suppresses terminal status
            (0xc2768, 0x3d42c100),    // initial BGM settings byte
            (0xc2778, 0x3d42c500),    // initial SE settings byte
            (0xc2788, 0x3d42c900),    // initial voice settings byte
            (0xc2aa8, 0x3d42c100),    // per-frame BGM settings byte
            (0xc2ab8, 0x3d42c900),    // per-frame voice settings byte
            (0xc2ac8, 0x3d42c500),    // per-frame SE settings byte
            (0xc2ae0, 0x1e2e1000),    // ignore settings uses unit gain
            (0xc2af4, 0x1e210800),    // multiply category and script gain
        ] {
            let actual = u32::from_le_bytes(
                executable[address + 0x100..address + 0x104]
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(actual, instruction, "Native instruction at {address:x}");
        }
    }
}
