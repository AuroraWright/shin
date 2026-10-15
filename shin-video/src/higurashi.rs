//! Updated Higurashi MoviePlayer transport (8e410/8ff80/8fe6c), isolated from
//! later Umineko playback. The live owner converts retained NV12 to RGBA before
//! submitting its ordinary layer shader. No movie duration or inferred loop.
use std::{
    collections::VecDeque,
    io::{Read, Seek},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Instant,
};

use anyhow::{Context, Result, ensure};
use parking_lot::{Mutex, RwLock};
use shin_audio::{AudioData, AudioHandle, AudioManager, AudioSettings, AudioSuspension};
use shin_core::{
    format::audio::{AudioBuffer, AudioFrameSource},
    primitives::{
        exclusive::Exclusive,
        update::{FrameId, UpdateTracker},
    },
    time::Tween,
    vm::command::types::{Pan, Volume},
};

use crate::{
    VideoFrameTexture,
    audio::AacFrameSource,
    h264_decoder::{DecoderPoll, FrameTiming, H264Decoder, H264DecoderTrait, Nv12Frame},
    mp4::Mp4,
};

#[cfg(test)]
mod tests;

/// The exclusive final real PCM sample, after the source's codec pre-skip.
/// Silence keeps the native audio media clock alive after its decoded EOF.
#[derive(Clone, Default)]
struct PcmTail {
    eof: Arc<AtomicBool>,
    terminal: Arc<AtomicU32>,
}
impl PcmTail {
    fn ended(&self, terminal: u32) {
        self.terminal.store(terminal, Ordering::Release);
        self.eof.store(true, Ordering::Release);
    }
    fn drained(&self, earliest_retained: u32) -> bool {
        self.eof.load(Ordering::Acquire)
            && earliest_retained >= self.terminal.load(Ordering::Acquire)
    }
}

struct HigurashiAudioSource<S: AudioFrameSource> {
    source: S,
    tail: PcmTail,
    eof: bool,
    silence_samples: u32,
}
impl<S: AudioFrameSource> HigurashiAudioSource<S> {
    fn new(source: S) -> Self {
        Self {
            source,
            tail: PcmTail::default(),
            eof: false,
            silence_samples: 0,
        }
    }
    fn silence_size(&self) -> usize {
        (self.source.sample_rate() / 10).max(1) as usize
    }
}
impl<S: AudioFrameSource> AudioFrameSource for HigurashiAudioSource<S> {
    fn is_failed(&self) -> bool {
        self.source.is_failed()
    }
    fn max_frame_size(&self) -> usize {
        self.source.max_frame_size().max(self.silence_size())
    }
    fn sample_rate(&self) -> u32 {
        self.source.sample_rate()
    }
    fn pre_skip(&self) -> u32 {
        self.source.pre_skip()
    }
    fn pre_roll(&self) -> u32 {
        self.source.pre_roll()
    }
    fn read_frame(&mut self, destination: &mut AudioBuffer) -> bool {
        if !self.eof && self.source.read_frame(destination) {
            return true;
        }
        if self.source.is_failed() {
            return false;
        }
        if !self.eof {
            self.eof = true;
            self.tail.ended(
                self.source
                    .current_sample_position()
                    .saturating_sub(self.pre_skip()),
            );
        }
        let samples = self.silence_size();
        destination.extend(std::iter::repeat_n((0.0, 0.0), samples));
        self.silence_samples = self.silence_samples.saturating_add(samples as u32);
        true
    }
    fn samples_seek(&mut self, _position: u32) -> Result<u32> {
        anyhow::bail!("Higurashi Movie audio has no authored rewind or restored seek")
    }
    fn current_sample_position(&self) -> u32 {
        self.source
            .current_sample_position()
            .saturating_add(self.silence_samples)
    }
}

/// Native d6b00's pause/origin arithmetic, over an independent microsecond source.
/// The caller may supply its active application clock; game delta is never used.
struct WallClock {
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
    accumulated: u64,
    started: Option<u64>,
}
impl WallClock {
    fn new(now: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        Self {
            now,
            accumulated: 0,
            started: None,
        }
    }
    fn time(&self) -> u64 {
        self.accumulated.saturating_add(
            self.started
                .map_or(0, |started| (self.now)().saturating_sub(started)),
        )
    }
    fn set_running(&mut self, running: bool) {
        match (running, self.started) {
            (true, None) => self.started = Some((self.now)()),
            (false, Some(_)) => {
                self.accumulated = self.time();
                self.started = None;
            }
            _ => {}
        }
    }
}

struct Frame<T> {
    timing: FrameTiming,
    pts_us: u64,
    payload: T,
}
/// 90890 preserves arrival order, chooses the first future frame, and retains
/// that frame. Neither sorting nor a latest-past-frame rule matches native.
struct Frames<T> {
    queue: VecDeque<Frame<T>>,
    eof: bool,
    displayed: Option<FrameTiming>,
    terminal_presented: bool,
}
impl<T> Frames<T> {
    fn new(first: Frame<T>) -> Self {
        Self {
            queue: VecDeque::from([first]),
            eof: false,
            displayed: None,
            terminal_presented: false,
        }
    }
    fn select(&mut self, clock_us: u64) -> Option<&Frame<T>> {
        while self.queue.len() > 1 && self.queue.front().unwrap().pts_us <= clock_us {
            self.queue.pop_front();
        }
        self.queue.front()
    }
    fn presented(&mut self, timing: FrameTiming) {
        self.displayed = Some(timing);
        self.terminal_presented = self.eof && self.queue.len() == 1;
    }
    fn refresh_terminal(&mut self) {
        self.terminal_presented = self.eof
            && self.queue.len() == 1
            && self
                .queue
                .front()
                .is_some_and(|frame| self.displayed == Some(frame.timing));
    }
}
enum FramePoll<T> {
    Pending,
    Frame(Frame<T>),
    Eof,
}
/// Drain elapsed candidates while polling, rather than imposing a read-count
/// cap. Even a large clock jump reaches the first available future frame.
/// Retain that candidate plus one following frame; the lookahead distinguishes
/// the final frame from transport starvation without buffering the whole movie.
fn refill_first_future<T>(
    frames: &mut Frames<T>,
    clock_us: u64,
    mut poll: impl FnMut() -> Result<FramePoll<T>>,
) -> Result<()> {
    loop {
        frames.select(clock_us);
        if frames.eof || frames.queue.len() > 1 {
            break;
        }
        match poll()? {
            FramePoll::Pending => break,
            FramePoll::Frame(frame) => frames.queue.push_back(frame),
            FramePoll::Eof => {
                frames.eof = true;
                break;
            }
        }
    }
    Ok(())
}
fn native_status(preparing: bool, video_eof: bool, audio_drained: bool) -> u32 {
    if video_eof && audio_drained {
        2
    } else {
        u32::from(!preparing)
    }
}
fn pts_micros(pts: u64, time_base: u32) -> Result<u64> {
    ensure!(time_base > 0, "Higurashi Movie video timescale is zero");
    (u128::from(pts) * 1_000_000 / u128::from(time_base))
        .try_into()
        .context("Higurashi Movie timestamp exceeds microsecond clock")
}

struct Inner {
    decoder: Option<Exclusive<H264Decoder>>,
    frames: Frames<Nv12Frame>,
    texture: VideoFrameTexture,
    time_base: u32,
    tracker: UpdateTracker,
    wall: WallClock,
    audio: Option<Mutex<AudioHandle>>,
    audio_tail: Option<PcmTail>,
    audio_failed: Option<Arc<AtomicBool>>,
    suspension: AudioSuspension,
    playing: bool,
    suspended: bool,
    retired: bool,
    failed: bool,
}
impl Inner {
    fn time(&self) -> u64 {
        self.audio
            .as_ref()
            .map_or_else(|| self.wall.time(), |audio| audio.lock().position_micros())
    }
    fn audio_drained(&self) -> bool {
        self.audio.as_ref().is_none_or(|audio| {
            self.audio_tail
                .as_ref()
                .unwrap()
                .drained(audio.lock().drained_position_samples())
        })
    }
    fn status(&self) -> u32 {
        if self.retired || self.failed {
            // Desktop failure/retirement completes the owner's wait after an
            // explicit error or cancellation. It is not native 8fe6c's formula.
            2
        } else {
            native_status(false, self.frames.terminal_presented, self.audio_drained())
        }
    }
    fn sync_playing(&mut self) {
        self.suspended = self.suspension.is_suspended();
        let running = self.playing && !self.suspended && !self.retired && !self.failed;
        self.wall.set_running(running);
        if let Some(audio) = &self.audio {
            audio.lock().set_paused(!running);
        }
    }
    fn retire(&mut self) {
        if self.retired {
            return;
        }
        self.retired = true;
        self.sync_playing();
        if let Some(audio) = &self.audio {
            audio.lock().stop_immediate();
        }
        // Existing codec workers are owned by the preparation loader scope.
        // Dropping their codec requests cancellation/FFmpeg kill; the caller's
        // retained LoaderRetirement drains those workers, including stale results.
        self.decoder = None;
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.retire();
    }
}

/// A scene-owned Higurashi media transport. Never clone to copy a layer; native
/// MovieLayer's virtual38 returns NULL. Ordinary shader/RGBA output is root-owned.
/// Initialization prepares codecs and a first decoded frame synchronously on a
/// caller-owned loader worker. The live owner publishes native preparing status0
/// while that initializer is pending, then starts this prepared status1 transport.
pub struct HigurashiVideoPlayerHandle {
    inner: RwLock<Inner>,
}
impl HigurashiVideoPlayerHandle {
    pub fn new_paused_with_volume<S: Read + Seek + Send + 'static>(
        device: &wgpu::Device,
        audio_manager: &AudioManager,
        mp4: Mp4<S>,
        volume: Volume,
    ) -> Result<Self> {
        let origin = Instant::now();
        Self::new_paused_with_clock(
            device,
            audio_manager,
            mp4,
            volume,
            Arc::new(move || origin.elapsed().as_micros().try_into().unwrap_or(u64::MAX)),
        )
    }
    /// An application active-time provider may model native cc6e0 suspension;
    /// it must be monotonic, microseconds, and independent of layer delta. Call
    /// from a retained loader scope, then start only when the owner is published.
    pub fn new_paused_with_clock<S: Read + Seek + Send + 'static>(
        device: &wgpu::Device,
        audio_manager: &AudioManager,
        mp4: Mp4<S>,
        volume: Volume,
        now: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Result<Self> {
        // Catch on the caller's actual preparation worker, as the shared
        // player does: catching its awaiting future cannot contain a Rayon
        // panic after preparation has been detached.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Self::prepare_with_clock(device, audio_manager, mp4, volume, now)
        }))
        .map_err(|panic| {
            let message = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("unknown panic");
            anyhow::anyhow!("Higurashi Movie preparation panicked: {message}")
        })?
    }
    fn prepare_with_clock<S: Read + Seek + Send + 'static>(
        device: &wgpu::Device,
        audio_manager: &AudioManager,
        mp4: Mp4<S>,
        volume: Volume,
        now: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Result<Self> {
        ensure!(volume.0.is_finite(), "Higurashi Movie volume is not finite");
        let time_base = mp4
            .video_track
            .get_mp4_track_info(|track| track.timescale());
        ensure!(time_base > 0, "Higurashi Movie video timescale is zero");
        let mut decoder =
            H264Decoder::new(mp4.video_track).context("Preparing Higurashi H264 decoder")?;
        let first = match decoder.try_read_frame()? {
            DecoderPoll::Frame(timing, frame) => Some((timing, frame)),
            DecoderPoll::Pending => decoder
                .read_frame()
                .context("Preparing Higurashi first video frame")?,
            DecoderPoll::Eof => None,
        }
        .context("Higurashi Movie contains no decoded video frame")?;
        validate_frame(&first.1)?;
        let texture = VideoFrameTexture::new_checked(device, first.1.size)?;
        let frame = Frame {
            pts_us: pts_micros(first.0.start_time, time_base)?,
            timing: first.0,
            payload: first.1,
        };
        let mut audio_tail = None;
        let mut audio_failed = None;
        let audio = if let Some(track) = mp4.audio_track {
            let aac = AacFrameSource::new(track).context("Preparing Higurashi AAC source")?;
            audio_failed = Some(aac.decode_failed());
            let source = HigurashiAudioSource::new(aac);
            audio_tail = Some(source.tail.clone());
            Some(Mutex::new(audio_manager.play(AudioData {
                source,
                settings: AudioSettings {
                    paused: true,
                    fade_in: Tween::IMMEDIATE,
                    loop_start: None,
                    loop_count: 0,
                    volume,
                    pan: Pan::default(),
                    playback_rate: 1.0,
                },
            })))
        } else {
            None
        };
        Ok(Self {
            inner: RwLock::new(Inner {
                decoder: Some(Exclusive::new(decoder)),
                frames: Frames::new(frame),
                texture,
                time_base,
                tracker: UpdateTracker::new(),
                wall: WallClock::new(now),
                audio,
                audio_tail,
                audio_failed,
                suspension: audio_manager.suspension(),
                playing: false,
                suspended: false,
                retired: false,
                failed: false,
            }),
        })
    }
    pub fn start(&self) {
        let mut inner = self.inner.write();
        if !inner.retired && !inner.failed {
            inner.playing = true;
            inner.sync_playing();
        }
    }
    /// Pause the source/clock, retaining its final or current frame and status.
    pub fn stop(&self) {
        let mut inner = self.inner.write();
        inner.playing = false;
        inner.sync_playing();
    }
    pub fn is_playing(&self) -> bool {
        // Native 8fe34 reads desired a88, independent of external gate a89.
        self.inner.read().playing
    }
    pub fn is_paused(&self) -> bool {
        !self.is_playing()
    }
    /// Notify a transport when application suspension changes, even if scene
    /// updates will stop. In particular, freeze a silent movie's wall clock at
    /// focus loss rather than discovering the suspension on the next update.
    /// The native desired-playing flag remains independent of this gate.
    pub fn sync_suspension(&self) {
        self.inner.write().sync_playing();
    }
    pub fn terminate(&self) {
        self.inner.write().retire();
    }
    pub fn set_volume(&self, volume: Volume) {
        let inner = self.inner.read();
        if !inner.retired && !inner.failed {
            if let Some(audio) = &inner.audio {
                audio.lock().set_volume_immediate(volume);
            }
        }
    }
    /// Original 8fe6c status for a prepared live player:1 until video EOF and
    /// actual PCM drain, then2. Explicit desktop failure/retirement also returns
    ///2 to release the owning wait; that boundary is not original EOF behavior.
    pub fn status(&self) -> u32 {
        self.inner.read().status()
    }
    pub fn current_time_micros(&self) -> u64 {
        self.inner.read().time()
    }
    pub fn current_frame_timing(&self) -> Option<FrameTiming> {
        self.inner.read().frames.displayed
    }
    pub fn with_frame_texture<R>(&self, f: impl FnOnce(&VideoFrameTexture) -> R) -> Option<R> {
        let inner = self.inner.read();
        inner.frames.displayed.map(|_| f(&inner.texture))
    }
    /// Poll codec output and present the first future frame. No decoder wait,
    /// game-delta clock, seek, still-copy, loop, or movie-duration deadline.
    /// A new retained NV12 frame requires root's original program20 RGBA pass.
    pub fn update(
        &self,
        frame_id: FrameId,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<bool> {
        let mut inner = self.inner.write();
        if inner.retired {
            return Ok(false);
        }
        if inner
            .audio_failed
            .as_ref()
            .is_some_and(|failed| failed.load(Ordering::Acquire))
        {
            inner.failed = true;
            inner.sync_playing();
            anyhow::bail!("Higurashi Movie AAC decoding failed");
        }
        inner.suspended = inner.suspension.is_suspended();
        inner.sync_playing();
        if inner.suspended || inner.failed || !inner.tracker.update(frame_id) {
            return Ok(false);
        }
        // Native +58 can convert even while locally paused. The media clock is
        // frozen; a newly queued frame can still be the first future candidate.
        let result = poll_and_present(&mut inner, device, queue);
        if result.is_err() {
            inner.failed = true;
            inner.sync_playing();
        }
        result
    }
}
fn validate_frame(frame: &Nv12Frame) -> Result<()> {
    let size = frame.size;
    ensure!(
        size.width > 0 && size.height > 0 && size.width % 2 == 0 && size.height % 2 == 0,
        "Higurashi Movie has invalid NV12 extent"
    );
    let y = usize::try_from(u64::from(size.width) * u64::from(size.height))?;
    ensure!(
        frame.y_plane.len() == y && frame.uv_plane.len() == y / 2,
        "Higurashi Movie NV12 planes do not match their extent"
    );
    Ok(())
}
fn poll_and_present(inner: &mut Inner, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<bool> {
    let clock = inner.time();
    let time_base = inner.time_base;
    let decoder = &mut inner.decoder;
    refill_first_future(&mut inner.frames, clock, || {
        let Some(transport) = decoder else {
            return Ok(FramePoll::Eof);
        };
        match transport
            .get_mut()
            .try_read_frame()
            .context("Polling Higurashi video output")?
        {
            DecoderPoll::Pending => Ok(FramePoll::Pending),
            DecoderPoll::Frame(timing, frame) => {
                validate_frame(&frame)?;
                Ok(FramePoll::Frame(Frame {
                    pts_us: pts_micros(timing.start_time, time_base)?,
                    timing,
                    payload: frame,
                }))
            }
            DecoderPoll::Eof => {
                *decoder = None;
                Ok(FramePoll::Eof)
            }
        }
    })?;
    let selected = inner
        .frames
        .select(clock)
        .context("Higurashi Movie lost its retained decoded frame")?;
    let timing = selected.timing;
    if inner.frames.displayed == Some(timing) {
        inner.frames.refresh_terminal();
        return Ok(false);
    }
    let selected = inner.frames.queue.front().unwrap();
    if inner.texture.get_size().x as u32 != selected.payload.size.width
        || inner.texture.get_size().y as u32 != selected.payload.size.height
    {
        inner.texture = VideoFrameTexture::new_checked(device, selected.payload.size)?;
    }
    inner
        .texture
        .write_data_nv12_checked(queue, &selected.payload)?;
    inner.frames.presented(timing);
    Ok(true)
}
