use anyhow::{anyhow, Result};
use shin_core::format::audio::{AudioBuffer, AudioFrameSource, AudioSource};

/// A sample-exact slice of a decoded source. Seeking and Opus pre-roll are
/// handled by the inner AudioSource; the outer source starts at sample zero.
pub struct AudioSegment<S: AudioFrameSource> {
    source: AudioSource<S>,
    start: u32,
    length: Option<u32>,
    position: u32,
}

impl<S: AudioFrameSource> AudioSegment<S> {
    pub fn new(source: S, start: u32, length: Option<u32>) -> Result<Self> {
        let mut source = AudioSource::new(source);
        if start != 0 {
            source.samples_seek(start)?;
        }
        Ok(Self {
            source,
            start,
            length,
            position: 0,
        })
    }
}

impl<S: AudioFrameSource> AudioFrameSource for AudioSegment<S> {
    fn is_failed(&self) -> bool {
        self.source.inner().is_failed()
    }

    fn failure_ends_playback(&self) -> bool {
        self.source.inner().failure_ends_playback()
    }

    fn max_frame_size(&self) -> usize {
        self.source.inner().max_frame_size()
    }
    fn sample_rate(&self) -> u32 {
        self.source.sample_rate()
    }
    fn pre_skip(&self) -> u32 {
        0
    }
    fn pre_roll(&self) -> u32 {
        0
    }

    fn read_frame(&mut self, destination: &mut AudioBuffer) -> bool {
        let mut read = 0;
        for _ in 0..self.max_frame_size() {
            if self.length.is_some_and(|length| self.position >= length) {
                break;
            }
            let Some(sample) = self.source.read_sample() else {
                break;
            };
            destination.push(sample);
            self.position += 1;
            read += 1;
        }
        read != 0
    }

    fn samples_seek(&mut self, sample_position: u32) -> Result<u32> {
        let position = self
            .length
            .map_or(sample_position, |length| sample_position.min(length));
        let absolute = self
            .start
            .checked_add(position)
            .ok_or_else(|| anyhow!("Audio segment seek overflow"))?;
        self.source.samples_seek(absolute)?;
        self.position = position;
        Ok(0)
    }

    fn current_sample_position(&self) -> u32 {
        self.position
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ramp {
        position: u32,
    }
    impl AudioFrameSource for Ramp {
        fn max_frame_size(&self) -> usize {
            8
        }
        fn sample_rate(&self) -> u32 {
            1000
        }
        fn pre_skip(&self) -> u32 {
            2
        }
        fn pre_roll(&self) -> u32 {
            3
        }
        fn read_frame(&mut self, destination: &mut AudioBuffer) -> bool {
            let end = (self.position + 8).min(34);
            let read = self.position < end;
            for sample in self.position..end {
                destination.push((sample as f32, sample as f32));
            }
            self.position = end;
            read
        }
        fn samples_seek(&mut self, sample_position: u32) -> Result<u32> {
            self.position = sample_position / 8 * 8;
            Ok(sample_position % 8)
        }
        fn current_sample_position(&self) -> u32 {
            self.position
        }
    }

    #[test]
    fn segment_boundaries_include_pre_skip_and_pre_roll() {
        let segment = AudioSegment::new(Ramp { position: 0 }, 7, Some(10)).unwrap();
        let mut source = AudioSource::new(segment);
        for sample in 9..19 {
            assert_eq!(source.read_sample(), Some((sample as f32, sample as f32)));
        }
        assert_eq!(source.read_sample(), None);
        assert_eq!(source.current_samples_position(), 10);
        source.samples_seek(2).unwrap();
        assert_eq!(source.read_sample(), Some((11.0, 11.0)));
        source.samples_seek(100).unwrap();
        assert_eq!(source.read_sample(), None);
    }

    #[test]
    fn unbounded_and_empty_segments() {
        let mut source =
            AudioSource::new(AudioSegment::new(Ramp { position: 0 }, 30, None).unwrap());
        assert_eq!(source.read_sample(), Some((32.0, 32.0)));
        assert_eq!(source.read_sample(), Some((33.0, 33.0)));
        assert_eq!(source.read_sample(), None);
        let mut source =
            AudioSource::new(AudioSegment::new(Ramp { position: 0 }, 0, Some(0)).unwrap());
        assert_eq!(source.read_sample(), None);
    }
}
