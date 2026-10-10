use std::{
    io::{Read, Seek},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result};
use glam::{Mat4, Vec4};
use kira::track::TrackId;
use parking_lot::{Mutex, RwLock, RwLockReadGuard, RwLockUpgradableReadGuard};
use shin_audio::{AudioData, AudioManager, AudioSettings};
use shin_core::{
    primitives::{
        exclusive::Exclusive,
        update::{FrameId, UpdateTracker},
    },
    time::{Ticks, Tween},
    vm::command::types::{Pan, Volume},
};
use shin_render::{RenderRequestBuilder, render_pass::RenderPass};
use tracing::{error, info, trace, warn};

use crate::{
    VideoFrameTexture,
    audio::{AacFrameSource, MovieAudioSource, RepeatingMovieAudioSource},
    h264_decoder::{FrameTiming, H264Decoder, H264DecoderTrait, Nv12Frame},
    mp4::Mp4,
    timer::Timer,
};

/// Native terminal query requires both decoder EOF flags and the maximum
/// declared track deadline. Decoder EOF alone can precede queued playback.
struct MovieDeadline {
    duration: u64,
    audio_eof: Option<Arc<AtomicBool>>,
}

#[cfg(test)]
mod native_output_tests;

impl MovieDeadline {
    fn is_finished(&self, video_eof: bool, time: u64) -> bool {
        video_eof
            && self
                .audio_eof
                .as_ref()
                .is_none_or(|eof| eof.load(Ordering::Acquire))
            && time >= self.duration
    }
}

struct PreparedVideo {
    decoder: H264Decoder,
    first: (FrameTiming, Nv12Frame),
}

#[cfg(any(test, feature = "reference-tests"))]
pub struct ReferenceLoopPreparationGate {
    pub entered: std::sync::mpsc::Sender<()>,
    pub resume: std::sync::mpsc::Receiver<()>,
    pub prepared: std::sync::mpsc::Sender<FrameTiming>,
    pub finish: std::sync::mpsc::Receiver<()>,
}

#[cfg(any(test, feature = "reference-tests"))]
pub struct ReferenceLoopWorker {
    pub result_ready: bool,
    pub transferred: bool,
    pub retirement: shin_tasks::LoaderRetirement,
}

struct MovieLoop {
    prepare: Arc<dyn Fn() -> Result<PreparedVideo> + Send + Sync>,
    task: Option<Exclusive<shin_tasks::LoaderComputeTask<Result<PreparedVideo>>>>,
    cycle: u64,
    duration_us: u64,
    time_base: u32,
}

impl MovieLoop {
    fn offset(&self) -> u64 {
        (u128::from(self.cycle) * u128::from(self.duration_us) * u128::from(self.time_base)
            / 1_000_000) as u64
    }
    fn rewind(&mut self) {
        self.cycle += 1;
        let prepare = self.prepare.clone();
        self.task = Some(Exclusive::new(shin_tasks::compute::spawn_loader(
            move || prepare(),
        )));
    }

    fn retire_task(&mut self) {
        if let Some(task) = self.task.take() {
            #[cfg(not(target_arch = "wasm32"))]
            task.into_inner().cancel_and_retire();
            #[cfg(target_arch = "wasm32")]
            drop(task);
        }
    }
}

struct VideoPlayerInner {
    audio_failed: Option<Arc<AtomicBool>>,
    suspension: shin_audio::AudioSuspension,
    update_tracker: UpdateTracker,
    // we could use [`Exclusive`] instead of [`Mutex`] here, but `set_volume` will then have to take a lock that is much larger than necessary
    timer: Mutex<Timer>,
    deadline: MovieDeadline,
    movie_loop: Option<MovieLoop>,
    video_decoder: Option<Exclusive<H264Decoder>>,
    decoder_retirement: Option<shin_tasks::LoaderRetirement>,
    video_texture: VideoFrameTexture,
    pending_frame: Option<(FrameTiming, Nv12Frame)>,
    displayed_frame: Option<FrameTiming>,
    stopped: bool,
    paused: bool,
}

impl Drop for VideoPlayerInner {
    fn drop(&mut self) {
        // Failed playback keeps a paused voice until its owner retires. Normal
        // completion may already have stopped it; timer retirement is idempotent.
        self.timer.get_mut().stop_audio();
        self.retire_loop();
        self.retire_video_decoder();
    }
}

impl VideoPlayerInner {
    fn retire_loop(&mut self) {
        if let Some(looping) = &mut self.movie_loop {
            looping.retire_task();
        }
        self.movie_loop = None;
    }

    fn retire_video_decoder(&mut self) {
        let decoder = self.video_decoder.take();
        if let Some(retirement) = self.decoder_retirement.take() {
            #[cfg(not(target_arch = "wasm32"))]
            retirement.retire(decoder);
            #[cfg(target_arch = "wasm32")]
            drop((retirement, decoder));
        } else {
            // Initial preparation remains owned by its existing loader scope.
            drop(decoder);
        }
    }
    fn audio_failed(&self) -> bool {
        self.audio_failed
            .as_ref()
            .is_some_and(|failed| failed.load(Ordering::Acquire))
    }
    fn is_finished(&self) -> bool {
        self.stopped
            || self.audio_failed()
            || (self.movie_loop.is_none()
                && self
                    .deadline
                    .is_finished(self.pending_frame.is_none(), self.timer.lock().time()))
    }
}

pub struct VideoPlayerHandle {
    inner: RwLock<VideoPlayerInner>,
}

impl VideoPlayerHandle {
    /// Snapshot the actual restart worker's ownership without polling its result.
    #[cfg(any(test, feature = "reference-tests"))]
    pub fn reference_loop_worker(&self) -> Option<ReferenceLoopWorker> {
        let mut inner = self.inner.write();
        if let Some(task) = inner.movie_loop.as_mut()?.task.as_mut() {
            let task = task.get_mut();
            Some(ReferenceLoopWorker {
                result_ready: task.reference_result_is_ready(),
                transferred: false,
                retirement: task.retirement_handle(),
            })
        } else {
            inner
                .decoder_retirement
                .clone()
                .map(|retirement| ReferenceLoopWorker {
                    result_ready: false,
                    transferred: true,
                    retirement,
                })
        }
    }

    #[cfg(any(test, feature = "reference-tests"))]
    pub fn reference_gate_next_loop_preparation(&self, gate: ReferenceLoopPreparationGate) {
        let mut inner = self.inner.write();
        let looping = inner.movie_loop.as_mut().expect("Repeating player");
        assert!(looping.task.is_none());
        let prepare = looping.prepare.clone();
        let gate = std::sync::Mutex::new(Some(gate));
        looping.prepare = Arc::new(move || {
            let gate = gate.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.entered.send(()).unwrap();
                gate.resume
                    .recv_timeout(std::time::Duration::from_secs(15))
                    .unwrap();
                let result = prepare()?;
                gate.prepared.send(result.first.0).unwrap();
                gate.finish
                    .recv_timeout(std::time::Duration::from_secs(15))
                    .unwrap();
                Ok(result)
            } else {
                prepare()
            }
        });
    }

    pub fn new<S: Read + Seek + Send + 'static>(
        device: &wgpu::Device,
        audio_manager: &AudioManager,
        mp4: Mp4<S>,
    ) -> Result<VideoPlayerHandle> {
        Self::new_with_volume(device, audio_manager, mp4, Volume::default())
    }

    pub fn new_with_volume<S: Read + Seek + Send + 'static>(
        device: &wgpu::Device,
        audio_manager: &AudioManager,
        mp4: Mp4<S>,
        volume: Volume,
    ) -> Result<VideoPlayerHandle> {
        Self::new_with_playback_state(device, audio_manager, mp4, volume, false, false)
    }

    /// Prepare a player on a loading worker without advancing its audio before
    /// the scene's completion task publishes and starts it.
    pub fn new_paused_with_volume<S: Read + Seek + Send + 'static>(
        device: &wgpu::Device,
        audio_manager: &AudioManager,
        mp4: Mp4<S>,
        volume: Volume,
    ) -> Result<VideoPlayerHandle> {
        Self::new_with_playback_state(device, audio_manager, mp4, volume, true, false)
    }

    pub fn new_looping_with_volume<S: Read + Seek + Send + 'static>(
        device: &wgpu::Device,
        audio_manager: &AudioManager,
        mp4: Mp4<S>,
        volume: Volume,
        paused: bool,
    ) -> Result<Self> {
        Self::new_with_playback_state(device, audio_manager, mp4, volume, paused, true)
    }

    fn new_with_playback_state<S: Read + Seek + Send + 'static>(
        device: &wgpu::Device,
        audio_manager: &AudioManager,
        mp4: Mp4<S>,
        volume: Volume,
        paused: bool,
        repeat: bool,
    ) -> Result<VideoPlayerHandle> {
        let time_base = mp4
            .video_track
            .get_mp4_track_info(|track| track.timescale());

        let duration_us = mp4.duration_us();
        let duration = (u128::from(duration_us) * u128::from(time_base) / 1_000_000) as u64;
        anyhow::ensure!(!repeat || duration > 0, "Repeating movie has zero duration");
        let movie_loop = if repeat {
            let track = mp4.video_track.clone();
            Some(MovieLoop {
                prepare: Arc::new(move || {
                    let mut decoder = H264Decoder::new(track.clone())?;
                    let first = decoder.read_frame()?.context("Empty repeating movie")?;
                    Ok(PreparedVideo { decoder, first })
                }),
                task: None,
                cycle: 0,
                duration_us,
                time_base,
            })
        } else {
            None
        };
        let start = std::time::Instant::now();
        let mut video_decoder =
            H264Decoder::new(mp4.video_track).context("Initializing H264Decoder")?;
        let pending_frame = video_decoder.read_frame().context("Reading first frame")?;
        let initialization_time = start.elapsed();

        info!("H264Decoder::new took {:?}", initialization_time);

        let video_texture = VideoFrameTexture::new(
            device,
            video_decoder
                .frame_size()
                .context("Getting H264 frame size")?,
        );

        let mut audio_eof = None;
        let mut audio_failed = None;
        let audio_handle = if let Some(track) = mp4.audio_track {
            let frame_source = AacFrameSource::new(track).context("Initializing AacFrameSource")?;
            let settings = AudioSettings {
                paused,
                playback_rate: 1.0,
                track: TrackId::Main,
                fade_in: Tween::MS_15,
                loop_start: None,
                loop_count: 0,
                volume,
                pan: Pan::default(),
            };
            Some(if repeat {
                let frame_source = RepeatingMovieAudioSource::new(frame_source, duration_us)?;
                audio_failed = Some(frame_source.decode_failed());
                audio_manager.play(AudioData {
                    source: frame_source,
                    settings,
                })
            } else {
                audio_failed = Some(frame_source.decode_failed());
                let frame_source = MovieAudioSource::new(frame_source);
                audio_eof = Some(frame_source.decoder_eof());
                audio_manager.play(AudioData {
                    source: frame_source,
                    settings,
                })
            })
        } else {
            None
        };

        let timer = match audio_handle {
            Some(handle) => Timer::new_audio_tied(time_base, handle),
            None => Timer::new_independent(time_base),
        };

        let inner = VideoPlayerInner {
            audio_failed,
            suspension: audio_manager.suspension(),
            update_tracker: UpdateTracker::new(),
            timer: Mutex::new(timer),
            deadline: MovieDeadline {
                duration,
                audio_eof,
            },
            movie_loop,
            video_decoder: Some(Exclusive::new(video_decoder)),
            decoder_retirement: None,
            video_texture,
            pending_frame,
            displayed_frame: None,
            stopped: false,
            paused,
        };

        Ok(VideoPlayerHandle {
            inner: RwLock::new(inner),
        })
    }

    pub fn set_volume(&self, volume: Volume) {
        let read_guard = self.inner.read();
        let mut timer = read_guard.timer.lock();

        // timer keeps the audio handle, so we need to call it on the timer
        // maybe we should restructure this stuff a bit...
        timer.set_audio_volume(volume);
    }

    /// Whether the caller has locally paused this player. Platform suspension
    /// is tracked separately and does not change this requested state.
    pub fn is_paused(&self) -> bool {
        self.inner.read().paused
    }

    /// Published playback position in the MP4 video track's timescale.
    pub fn current_frame_timing(&self) -> Option<FrameTiming> {
        self.inner.read().displayed_frame
    }

    pub fn current_time(&self) -> u64 {
        self.inner.read().timer.lock().time()
    }

    pub fn start(&self) {
        let mut inner = self.inner.write();
        if inner.paused && !inner.is_finished() {
            inner.timer.get_mut().start_audio();
            inner.paused = false;
        }
    }

    /// Freeze playback and audio without consuming decoder frames. Native Stop
    /// clears the started flag; a later Start resumes the same movie clock.
    pub fn stop(&self) {
        let mut inner = self.inner.write();
        inner.paused = true;
        inner.timer.get_mut().pause_audio();
    }

    /// Permanently end playback, for an ADV skip that releases its movie owner.
    pub fn terminate(&self) {
        let mut inner = self.inner.write();
        inner.stopped = true;
        inner.pending_frame = None;
        inner.timer.get_mut().stop_audio();
        inner.retire_loop();
        inner.retire_video_decoder();
    }

    pub fn update(&self, game_frame_id: FrameId, delta_time: Ticks, queue: &wgpu::Queue) {
        let read_guard = self.inner.upgradable_read();
        // Decoder failure is asynchronous and terminal even on a duplicate,
        // locally paused or platform-suspended presentation frame.
        if !read_guard.stopped && read_guard.audio_failed() {
            let mut inner = RwLockUpgradableReadGuard::upgrade(read_guard);
            inner.stopped = true;
            inner.timer.get_mut().pause_audio();
            return;
        }
        if read_guard.stopped
            || read_guard.suspension.is_suspended()
            || read_guard.paused
            || !read_guard.update_tracker.needs_update(game_frame_id)
        {
            return;
        }

        let mut write_guard = RwLockUpgradableReadGuard::upgrade(read_guard);

        let this = &mut *write_guard;

        if this.update_tracker.update(game_frame_id) {
            let current_time = {
                let timer = this.timer.get_mut();
                timer.update(delta_time);
                timer.time()
            };

            let loop_result = this.movie_loop.as_mut().and_then(|looping| {
                let result = looping.task.as_mut()?.get_mut().poll_naive()?;
                let task = looping.task.take().unwrap().into_inner();
                Some((result, task.retirement_handle(), looping.offset()))
            });
            if let Some((result, retirement, offset)) = loop_result {
                match result {
                    Ok(mut prepared) => {
                        prepared.first.0.start_time += offset;
                        this.retire_video_decoder();
                        this.video_decoder = Some(Exclusive::new(prepared.decoder));
                        this.decoder_retirement = Some(retirement);
                        this.pending_frame = Some(prepared.first);
                    }
                    Err(err) => {
                        error!(%err, "Repeating video decoder preparation failed");
                        this.stopped = true;
                        this.timer.get_mut().pause_audio();
                        #[cfg(not(target_arch = "wasm32"))]
                        retirement.retire(err);
                        #[cfg(target_arch = "wasm32")]
                        drop((retirement, err));
                        return;
                    }
                }
            }

            let mut skipped_frames = 0;
            // find the latest frame that is ready for display
            // this might be the currently pending frame, or any of the frames after it (shouldn't happen often I think)
            while let Some((timing, ref frame)) = this.pending_frame {
                // if it's not time to display the frame yet - stop the loop
                if timing.start_time > current_time {
                    // very noisy
                    // trace!(
                    //     "Not time to display frame #{}, time: {}",
                    //     timing.frame_number,
                    //     timing.start_time
                    // );
                    break;
                }

                // look at the frame after the pending one
                let next_frame = match this
                    .video_decoder
                    .as_mut()
                    .expect("Pending frame without decoder")
                    .get_mut()
                    .read_frame()
                {
                    Ok(frame) => frame.map(|(mut timing, frame)| {
                        if let Some(looping) = &this.movie_loop {
                            timing.start_time += looping.offset();
                        }
                        (timing, frame)
                    }),
                    Err(err) => {
                        error!(%err, "Error reading movie frame");
                        this.stopped = true;
                        this.timer.get_mut().pause_audio();
                        return;
                    }
                };

                // if the next frame is not ready for display yet...
                if next_frame
                    .as_ref()
                    .map_or(true, |(timing, _)| timing.start_time > current_time)
                {
                    if skipped_frames > 0 {
                        warn!("Skipped {} frames", skipped_frames);
                    }

                    // then update the texture with the pending frame
                    trace!(
                        "Displaying frame #{}, time: {}",
                        timing.frame_number, timing.start_time
                    );
                    this.video_texture.write_data_nv12(queue, frame);
                    this.displayed_frame = Some(timing);
                    // the loop will not enter again, so the pending frame will now be displayed
                } else {
                    skipped_frames += 1;
                    // if the next frame is also ready for display, then we should skip the pending frame
                }

                if next_frame.is_none() {
                    info!("No more frames; displaying the final frame until its end time");
                    this.retire_video_decoder();
                    if let Some(looping) = &mut this.movie_loop {
                        looping.rewind();
                    }
                }

                this.pending_frame = next_frame;
            }
            if !this.stopped && this.is_finished() {
                this.stopped = true;
                this.timer.get_mut().stop_audio();
            }
        }
    }

    pub fn is_finished(&self) -> bool {
        let read_guard = self.inner.read();
        read_guard.is_finished()
    }

    pub fn get_frame(&self) -> Option<VideoPlayerFrameHandle<'_>> {
        let read_guard = self.inner.read();

        if read_guard.paused || read_guard.displayed_frame.is_none() || read_guard.is_finished() {
            return None;
        }

        Some(VideoPlayerFrameHandle { guard: read_guard })
    }

    /// Keep the last uploaded frame visible while an asynchronous loop restart
    /// prepares its next player. Never exposes an uninitialized texture.
    pub fn get_retained_frame(&self) -> Option<VideoPlayerFrameHandle<'_>> {
        let guard = self.inner.read();
        guard.displayed_frame?;
        Some(VideoPlayerFrameHandle { guard })
    }
}

pub struct VideoPlayerFrameHandle<'a> {
    guard: RwLockReadGuard<'a, VideoPlayerInner>,
}

impl<'a> VideoPlayerFrameHandle<'a> {
    pub fn get_frame(&self) -> &VideoFrameTexture {
        &self.guard.video_texture
    }

    pub fn render(&self, pass: &mut RenderPass, builder: RenderRequestBuilder, transform: Mat4) {
        self.render_with_options(pass, builder, transform, false, Vec4::ONE);
    }

    pub fn render_with_options(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        transform: Mat4,
        alpha_mode: bool,
        color_multiplier: Vec4,
    ) {
        self.guard.video_texture.render_with_options(
            pass,
            builder,
            transform,
            alpha_mode,
            color_multiplier,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires supplied ROM, ffmpeg, GPU and Loopback Audio (SHIN_REFERENCE_ROM)"]
    fn reference_aac_input_failure_is_terminal_while_voice_remains_owned() {
        use crate::test_support::{FaultingReader, opening_bytes};
        use std::{
            io::Cursor,
            time::{Duration, Instant},
        };
        crate::init_test_task_pools();
        let instance = wgpu::Instance::default();
        let adapter =
            shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .unwrap();
        let (device, queue) =
            shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
                .unwrap();
        let audio = AudioManager::new_for_tests();
        let bytes = opening_bytes();
        for repeat in [false, true] {
            let fail = Arc::new(AtomicBool::new(false));
            // The video reader stays healthy, isolating the AAC input failure.
            let mut mp4 = Mp4::new(FaultingReader {
                bytes: Cursor::new(bytes.clone()),
                fail: Arc::new(AtomicBool::new(false)),
            })
            .unwrap();
            let faulty_audio = Mp4::new(FaultingReader {
                bytes: Cursor::new(bytes.clone()),
                fail: fail.clone(),
            })
            .unwrap();
            mp4.audio_track = faulty_audio.audio_track;
            let player = VideoPlayerHandle::new_with_playback_state(
                &device,
                &audio,
                mp4,
                Volume(0.0),
                true,
                repeat,
            )
            .unwrap();
            player.start();
            let mut frame = FrameId::default();
            frame.advance();
            player.update(frame, Ticks::ZERO, &queue);
            assert!(player.get_frame().is_some());
            std::thread::sleep(Duration::from_millis(150));
            assert!(player.current_time() > 0);
            fail.store(true, Ordering::Release);
            let deadline = Instant::now() + Duration::from_secs(5);
            // Failure must become observable without a presentation update.
            while !player.is_finished() {
                assert!(Instant::now() < deadline, "AAC failure was not published");
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(player.inner.read().audio_failed());
            assert!(!player.inner.read().stopped);
            assert!(player.get_frame().is_none());
            assert!(player.get_retained_frame().is_some());
            std::thread::sleep(Duration::from_millis(50));
            let frozen = player.current_time();
            let image = player.current_frame_timing();
            assert_eq!(audio.kira_manager().lock().num_sounds(), 1);
            player.stop();
            player.start();
            assert!(player.is_paused(), "A failed decoder cannot restart");
            audio.set_suspended(true);
            // Reuse the same frame while both pause gates are closed. Failure
            // retirement precedes the guarded ordinary presentation update.
            player.update(frame, Ticks::from_seconds(60.0), &queue);
            assert!(player.inner.read().stopped);
            audio.set_suspended(false);
            for _ in 0..32 {
                player.set_volume(Volume(0.5));
                player.start();
                player.update(frame, Ticks::from_seconds(60.0), &queue);
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(player.current_time(), frozen);
            assert_eq!(player.current_frame_timing(), image);
            assert_eq!(audio.kira_manager().lock().num_sounds(), 1);
            drop(player);
            let deadline = Instant::now() + Duration::from_secs(5);
            while audio.kira_manager().lock().num_sounds() != 0 {
                assert!(
                    Instant::now() < deadline,
                    "AAC failed voice survived owner retirement"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    #[test]
    #[ignore = "requires supplied ROM, ffmpeg, GPU and audio device (SHIN_REFERENCE_ROM)"]
    fn reference_failed_loop_pauses_retained_audio_until_owner_retirement() {
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::{
            fs::File,
            io::Cursor,
            time::{Duration, Instant},
        };
        crate::init_test_task_pools();
        let instance = wgpu::Instance::default();
        let adapter =
            shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .unwrap();
        let (device, queue) =
            shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
                .unwrap();
        let audio = AudioManager::new_for_tests();
        assert_eq!(audio.kira_manager().lock().num_sounds(), 0);
        let mut rom = RomReader::new(
            StatelessFile::new(File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
        let file = rom.find_file("/movie/op1.mp4").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let player = VideoPlayerHandle::new_looping_with_volume(
            &device,
            &audio,
            Mp4::new(Cursor::new(bytes)).unwrap(),
            Volume(0.0),
            true,
        )
        .unwrap();
        player.start();
        let mut frame = FrameId::default();
        frame.advance();
        player.update(frame, Ticks::ZERO, &queue);
        assert!(player.get_frame().is_some());
        std::thread::sleep(Duration::from_millis(150));
        assert!(player.current_time() > 0);
        {
            let mut inner = player.inner.write();
            let looping = inner.movie_loop.as_mut().unwrap();
            looping.task = Some(Exclusive::new(shin_tasks::compute::spawn_loader(|| {
                anyhow::bail!("injected repeat preparation failure")
            })));
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while !player.is_finished() {
            frame.advance();
            player.update(frame, Ticks::from_seconds(60.0), &queue);
            assert!(Instant::now() < deadline, "Failure result was not retired");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(player.get_frame().is_none());
        assert!(player.get_retained_frame().is_some());
        std::thread::sleep(Duration::from_millis(50));
        let frozen = player.current_time();
        assert_eq!(
            audio.kira_manager().lock().num_sounds(),
            1,
            "Failure pauses the retained voice"
        );
        for index in 0..32 {
            player.set_volume(Volume(index as f32 / 32.0));
            player.start();
            frame.advance();
            player.update(frame, Ticks::from_seconds(60.0), &queue);
            std::thread::sleep(Duration::from_millis(15));
        }
        assert_eq!(player.current_time(), frozen);
        assert_eq!(audio.kira_manager().lock().num_sounds(), 1);
        drop(player);
        let deadline = Instant::now() + Duration::from_secs(5);
        while audio.kira_manager().lock().num_sounds() != 0 {
            assert!(
                Instant::now() < deadline,
                "Failed movie owner leaked its paused voice"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn movie_completion_requires_both_decoder_eofs_and_the_full_media_deadline() {
        let audio_eof = Arc::new(AtomicBool::new(false));
        // One second of video with two seconds of audio, at timescale60000.
        let deadline = MovieDeadline {
            duration: 120_000,
            audio_eof: Some(audio_eof.clone()),
        };
        assert!(!deadline.is_finished(true, 60_000));
        assert!(
            !deadline.is_finished(true, 120_000),
            "Time alone cannot replace decoder EOF"
        );
        audio_eof.store(true, Ordering::Release);
        assert!(!deadline.is_finished(false, 120_000));
        assert!(!deadline.is_finished(true, 119_999));
        assert!(deadline.is_finished(true, 120_000));
        assert!(deadline.is_finished(true, 120_001));
        // An audio-free movie treats the missing decoder as already exhausted.
        let silent = MovieDeadline {
            duration: 300_300,
            audio_eof: None,
        };
        assert!(!silent.is_finished(true, 300_299));
        assert!(silent.is_finished(true, 300_300));
    }
}
