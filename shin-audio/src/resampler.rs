use std::sync::OnceLock;

use kira::Frame;

/// Taps on each side of the interpolation point. The output lies between the
/// window's frames `HALF_TAPS - 1` and `HALF_TAPS`.
const HALF_TAPS: usize = 12;
const TAPS: usize = 2 * HALF_TAPS;
/// Sub-sample phases of the kernel; weights between two phases are
/// interpolated linearly.
const PHASES: usize = 256;
/// Kernel cutoffs are cached in steps of `1 / CUTOFF_STEPS` of the source
/// Nyquist rate, so a playback-rate tween reuses a few shared tables.
const CUTOFF_STEPS: usize = 256;
/// Below this the 24-tap window cannot hold the kernel's main lobe.
const MIN_CUTOFF_STEP: usize = CUTOFF_STEPS / 8;

static KERNELS: [OnceLock<Box<[f32]>>; CUTOFF_STEPS + 1] =
    [const { OnceLock::new() }; CUTOFF_STEPS + 1];

/// Blackman-windowed sinc kernel band-limited to `cutoff` (a fraction of the
/// source Nyquist rate): `PHASES + 1` rows of `TAPS` weights, one row per
/// sub-sample position of the output between the window's middle frames.
fn build_kernel(cutoff: f64) -> Box<[f32]> {
    let mut table = vec![0f32; (PHASES + 1) * TAPS];
    for (phase, row) in table.as_chunks_mut::<TAPS>().0.iter_mut().enumerate() {
        let frac = phase as f64 / PHASES as f64;
        let mut weights = [0f64; TAPS];
        for (k, weight) in weights.iter_mut().enumerate() {
            let x = (k as f64 + 1.0 - HALF_TAPS as f64) - frac;
            let sinc = if x.abs() < 1e-9 {
                1.0
            } else {
                let a = std::f64::consts::PI * cutoff * x;
                a.sin() / a
            };
            let u = (x / HALF_TAPS as f64 + 1.0) * 0.5;
            let window = if (0.0..=1.0).contains(&u) {
                let a = 2.0 * std::f64::consts::PI * u;
                0.42 - 0.5 * a.cos() + 0.08 * (2.0 * a).cos()
            } else {
                0.0
            };
            *weight = sinc * window;
        }
        // unity gain at DC
        let sum: f64 = weights.iter().sum();
        for (t, weight) in row.iter_mut().zip(weights) {
            *t = (weight / sum) as f32;
        }
    }
    table.into_boxed_slice()
}

/// The cached kernel for a source step (source frames per output frame):
/// upsampling keeps the full band, downsampling cuts at the output's Nyquist
/// rate so a 48 kHz source on a 44.1 kHz device does not alias.
fn kernel_index(step: f64) -> usize {
    let cutoff = (1.0 / step).min(1.0);
    ((cutoff * CUTOFF_STEPS as f64) as usize).clamp(MIN_CUTOFF_STEP, CUTOFF_STEPS)
}

fn kernel(index: usize) -> &'static [f32] {
    KERNELS[index].get_or_init(|| build_kernel(index as f64 / CUTOFF_STEPS as f64))
}

pub(super) struct Resampler {
    /// The window twice over, so the latest `TAPS` frames are always the
    /// contiguous slice starting at `head`.
    frames: [Frame; 2 * TAPS],
    /// The source frame index of each frame at the time it was pushed.
    frame_indices: [u32; TAPS],
    /// Position of the oldest frame.
    head: usize,
}

impl Resampler {
    pub fn new(starting_frame_index: u32) -> Self {
        Self {
            frames: [Frame::ZERO; 2 * TAPS],
            frame_indices: [starting_frame_index; TAPS],
            head: 0,
        }
    }

    fn window(&self) -> &[Frame] {
        &self.frames[self.head..self.head + TAPS]
    }

    fn frame_index(&self, age: usize) -> u32 {
        self.frame_indices[(self.head + age) % TAPS]
    }

    pub fn push_frame(&mut self, frame: Frame, sample_index: u32) {
        self.frames[self.head] = frame;
        self.frames[self.head + TAPS] = frame;
        self.frame_indices[self.head] = sample_index;
        self.head = (self.head + 1) % TAPS;
    }

    #[cfg(test)]
    pub(super) fn latest_frame(&self) -> Frame {
        self.window()[TAPS - 1]
    }

    /// The output `fractional_position` (0..1) of the way between the
    /// window's middle frames, for a source advancing `step` frames per
    /// output frame.
    pub fn get(&self, fractional_position: f64, step: f64) -> Frame {
        let window = self.window();
        let index = kernel_index(step);
        if fractional_position == 0.0 && index == CUTOFF_STEPS {
            // the source rate: pass the frame through unchanged
            return window[HALF_TAPS - 1];
        }
        let kernel = kernel(index);
        let x = fractional_position * PHASES as f64;
        let phase = (x as usize).min(PHASES - 1);
        let t = (x - phase as f64) as f32;
        let (row0, row1) = (
            &kernel[phase * TAPS..(phase + 1) * TAPS],
            &kernel[(phase + 1) * TAPS..(phase + 2) * TAPS],
        );
        let mut out = Frame::ZERO;
        for ((frame, w0), w1) in window.iter().zip(row0).zip(row1) {
            let weight = w0 + (w1 - w0) * t;
            out.left += frame.left * weight;
            out.right += frame.right * weight;
        }
        out
    }

    /// Returns the index of the frame in the source sound
    /// that the user is currently hearing from this resampler.
    ///
    /// This is not the same as the most recently pushed frame:
    /// the output lies between the window's middle frames, and
    /// the frames around them shape the band-limited result.
    #[allow(unused)] // TODO: use to implement BGMSYNC
    pub fn current_frame_index(&self) -> u32 {
        self.frame_index(HALF_TAPS - 1)
    }

    /// Earliest source index still retained for interpolation. All samples
    /// before this index have left the resampler, including silent PCM.
    pub fn drained_frame_index(&self) -> u32 {
        self.frame_index(0)
    }

    pub fn outputting_silence(&self) -> bool {
        self.window().iter().all(|frame| *frame == Frame::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 48 kHz audio on a 44.1 kHz device: a 10 kHz tone comes out clean.
    #[test]
    fn resamples_band_limited() {
        let frequency = 10_000.0f64;
        let tone = |i: f64| (2.0 * std::f64::consts::PI * frequency * i / 48_000.0).sin();
        let step = 48_000.0 / 44_100.0;
        let mut resampler = Resampler::new(0);
        let (mut source, mut fractional_position) = (0u32, 0.0f64);
        let (mut error, mut signal) = (0.0f64, 0.0f64);
        for _ in 0..40_000 {
            // the output sits HALF_TAPS frames behind the latest push
            let latest = f64::from(source) - 1.0;
            let position = latest - HALF_TAPS as f64 + fractional_position;
            if position > 100.0 {
                let got = f64::from(resampler.get(fractional_position, step).left);
                let want = tone(position);
                error += (got - want).powi(2);
                signal += want * want;
            }
            fractional_position += step;
            while fractional_position >= 1.0 {
                fractional_position -= 1.0;
                let value = tone(f64::from(source)) as f32;
                resampler.push_frame(Frame::new(value, value), source);
                source += 1;
            }
        }
        let snr = 10.0 * (signal / error).log10();
        assert!(snr > 60.0, "SNR {snr:.1} dB");
    }

    /// At the source rate every frame passes through unchanged.
    #[test]
    fn source_rate_passes_frames_through() {
        let mut resampler = Resampler::new(0);
        for i in 0..TAPS as u32 * 3 {
            let value = (i as f32 * 0.37).sin();
            resampler.push_frame(Frame::new(value, -value), i);
            if let Some(base) = i.checked_sub(HALF_TAPS as u32) {
                let expected = (base as f32 * 0.37).sin();
                let out = resampler.get(0.0, 1.0);
                assert_eq!((out.left, out.right), (expected, -expected));
                assert_eq!(resampler.current_frame_index(), base);
            }
        }
        assert_eq!(
            resampler.drained_frame_index(),
            TAPS as u32 * 3 - TAPS as u32
        );
    }
}
