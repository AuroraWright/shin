use std::sync::{Arc, mpsc::Sender};

use shin_core::{
    time::{Ticks, Tween},
    vm::command::types::{AudioWaitStatus, Pan, Volume},
};

use crate::sound::{Command, Shared};

pub struct AudioHandle {
    pub(super) command_producer: Sender<Command>,
    pub(super) shared: Arc<Shared>,
}

impl AudioHandle {
    /// Preserve every authored command in FIFO order, even when a game update
    /// outpaces the audio callback. A disconnected receiver means the sound
    /// has already retired, so late parameter/stop requests have no target.
    fn send_command(&self, command: Command) -> anyhow::Result<()> {
        let _ = self.command_producer.send(command);
        Ok(())
    }

    /// Published source sample position, preserving submillisecond media timing.
    pub fn position_samples(&self) -> u32 {
        self.shared
            .position_samples
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Exclusive lower bound of PCM that has left all resampler inputs.
    /// An EOF source can compare its exclusive last real sample against this
    /// value while supplying silence to retain its media clock.
    pub fn drained_position_samples(&self) -> u32 {
        self.shared
            .drained_position_samples
            .load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn sample_rate(&self) -> u32 {
        self.shared.sample_rate
    }

    /// Published source clock in integer microseconds for native media waits.
    /// Preserve the sample fraction rather than passing through milliseconds.
    pub fn position_micros(&self) -> u64 {
        u64::from(self.position_samples()) * 1_000_000 / u64::from(self.sample_rate())
    }

    pub fn get_wait_status(&self) -> AudioWaitStatus {
        AudioWaitStatus::from_bits_truncate(
            self.shared
                .wait_status
                .load(std::sync::atomic::Ordering::SeqCst),
        )
    }

    #[allow(unused)] // TODO: use it for lip-sync
    pub fn get_amplitude(&self) -> f32 {
        f32::from_bits(
            self.shared
                .amplitude
                .load(std::sync::atomic::Ordering::SeqCst),
        )
    }

    /// Suspend source playback without losing its position or wait state.
    /// Atomic state avoids queueing redundant frame-by-frame pause commands.
    pub fn set_paused(&self, paused: bool) {
        self.shared
            .paused
            .store(paused, std::sync::atomic::Ordering::Release);
    }

    /// Publish the latest gain without queuing intermediate frame values.
    /// The next audio callback applies it even after a prolonged backend stall.
    /// Use this for continuously sampled movie gain, without tweened commands.
    pub fn set_volume_immediate(&self, volume: Volume) {
        self.shared.immediate_volume.store(
            (1u64 << 32) | u64::from(volume.0.to_bits()),
            std::sync::atomic::Ordering::Release,
        );
    }

    /// Retire playback at the next callback, bypassing pending ordered commands.
    /// This is infallible so an owner's destructor can always request cleanup.
    pub fn stop_immediate(&self) {
        self.shared
            .immediate_stop
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Sets the volume of the sound.
    /// The volume is a value between 0.0 and 1.0, on the linear scale.
    pub fn set_volume(&mut self, volume: Volume, tween: Tween) -> anyhow::Result<()> {
        self.send_command(Command::SetVolume(volume, tween))
    }

    /// Append a volume transition without replacing the current ramp. Older
    /// Higurashi BGMVOL calls its native interpolator with flags0, not bit8.
    pub fn enqueue_volume(&mut self, volume: Volume, tween: Tween) -> anyhow::Result<()> {
        self.send_command(Command::EnqueueVolume(volume, tween))
    }

    /// Sets the panning of the sound
    pub fn set_panning(&mut self, panning: Pan, tween: Tween) -> anyhow::Result<()> {
        self.send_command(Command::SetPanning(panning, tween))
    }

    /// Append a pan transition, preserving the native SE interpolator queue.
    pub fn enqueue_panning(&mut self, panning: Pan, tween: Tween) -> anyhow::Result<()> {
        self.send_command(Command::EnqueuePanning(panning, tween))
    }

    /// Retarget source playback speed without restarting the decoder.
    /// The multiplier must be positive and finite; the tween uses wall-clock time.
    pub fn set_playback_rate(&mut self, rate: f32, tween: Tween) -> anyhow::Result<()> {
        anyhow::ensure!(rate.is_finite() && rate > 0.0, "Invalid playback rate");
        self.send_command(Command::SetPlaybackRate(rate, tween))
    }

    /// Retarget volume, pan, and source speed as one audio-thread command.
    pub fn set_parameters(
        &mut self,
        volume: Volume,
        pan: Pan,
        rate: f32,
        tween: Tween,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(rate.is_finite() && rate > 0.0, "Invalid playback rate");
        self.send_command(Command::SetParameters(volume, pan, rate, tween))
    }

    /// Older SE repeats may reuse a matching source during its stop fade.
    /// Retarget its parameters and cancel that fade without restarting samples.
    pub fn resume_parameters(
        &mut self,
        volume: Volume,
        pan: Pan,
        rate: f32,
        tween: Tween,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(rate.is_finite() && rate > 0.0, "Invalid playback rate");
        self.send_command(Command::ResumeParameters(volume, pan, rate, tween))
    }

    /// Fades out the sound to silence with the given tween and then
    /// stops playback.
    ///
    /// Once the sound is stopped, it cannot be restarted.
    pub fn stop(&mut self, tween: Tween) -> anyhow::Result<()> {
        self.send_command(Command::Stop(tween))
    }

    /// Older SE stop/reconstruction finishes the queued volume target before
    /// fading. A stopping source restarts its retirement timer from silence.
    pub fn finish_volume_and_stop(&mut self, tween: Tween) -> anyhow::Result<()> {
        self.send_command(Command::FinishVolumeAndStop(tween))
    }
    /// Higurashi c3cc8 finishes volume and pan without changing source speed.
    pub fn finish_volume_and_pan(&mut self) -> anyhow::Result<()> {
        self.send_command(Command::FinishVolumeAndPan)
    }

    /// Playback position in integer milliseconds, without a floating-point
    /// conversion at lip-animation window boundaries.
    pub fn position_millis(&self) -> u32 {
        self.shared
            .position
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Returns the current playback position of the sound.
    pub fn position(&self) -> Ticks {
        Ticks::from_millis(self.position_millis() as f32)
    }
}
