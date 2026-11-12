//! Runs a platform frame decoder on its own thread, with the same contract as
//! the ffmpeg decoder: frames come out in presentation order paired with the MP4
//! sample timings in decode order, errors are delivered in place of frames, and
//! running out of frames before samples is an error.

use std::{
    io::{Read, Seek},
    marker::PhantomData,
    sync::mpsc,
};

use anyhow::{Context, Result, anyhow};
use dpi::PhysicalSize;
use tracing::{debug, warn};

use super::{DecoderPoll, FrameTiming, H264DecoderTrait, Nv12Frame, nv12::SparePlanes};
use crate::{mp4::Mp4TrackReader, mp4_bitstream_converter::Mp4BitstreamConverter};

/// Decoded frames buffered ahead of the consumer.
const FRAME_QUEUE: usize = 60;

/// Stream parameters available before the first sample.
pub(super) struct StreamInfo {
    /// MP4 time units per second.
    pub timescale: u32,
    /// Frame size declared in the MP4 track header.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub width: u32,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub height: u32,
    /// Size in bytes of the NAL unit length prefixes in [`Packet::avcc`].
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub length_size: u8,
    /// Sequence parameter sets followed by picture parameter sets.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub parameter_sets: Vec<Vec<u8>>,
    /// Presentation timestamp of every sample, in decode order.
    pub presentation_times: Vec<i64>,
}

/// One MP4 video sample, in both bitstream formats.
pub(super) struct Packet<'a> {
    /// Length-prefixed NAL units, as stored in the MP4.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub avcc: &'a [u8],
    /// Annex B with start codes, parameter sets inserted before IDR frames.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub annex_b: &'a [u8],
    /// Decode and presentation timestamps in MP4 time units.
    #[cfg_attr(windows, allow(dead_code))]
    pub decode_time: u64,
    pub presentation_time: i64,
    pub duration: u32,
}

/// Where a [`FrameDecoder`] delivers frames, in presentation order.
pub(super) struct Output {
    frames: mpsc::SyncSender<Result<Nv12Frame>>,
    closed: bool,
    /// Presentation position of the next frame emitted.
    position: u32,
    /// Frames sent so far. A software decoder taking over decodes some of
    /// them again, which are passed over.
    delivered: u32,
}

impl Output {
    pub fn emit(&mut self, frame: Nv12Frame) {
        self.position += 1;
        if self.position <= self.delivered {
            return;
        }
        self.delivered = self.position;
        if !self.closed && self.frames.send(Ok(frame)).is_err() {
            debug!("Movie consumer closed the frame channel");
            self.closed = true;
        }
    }
}

/// A synchronous H.264 decoder, created and used on the worker thread.
pub(super) trait FrameDecoder: Sized {
    const NAME: &'static str;

    fn new(info: &StreamInfo) -> Result<Self>;

    fn decode(&mut self, packet: &Packet, output: &mut Output) -> Result<()>;

    /// Emit every remaining frame after the last packet.
    fn finish(&mut self, output: &mut Output) -> Result<()>;

    /// Whether this decoder runs on the GPU, with [`FrameDecoder::software`] to
    /// take over when it fails.
    fn hardware(&self) -> bool {
        false
    }

    /// A decoder that does not use the GPU.
    fn software(info: &StreamInfo) -> Result<Self> {
        Self::new(info)
    }
}

/// Copy a plane of `rows` rows of `width` bytes from a buffer whose rows are
/// `stride` bytes apart into `plane`, replacing its contents and keeping its
/// allocation (from [`Nv12Frame::spare_planes`]).
///
/// # Safety
/// `source` must be readable for `stride * (rows - 1) + width` bytes.
pub(super) unsafe fn copy_plane(
    plane: &mut Vec<u8>,
    source: *const u8,
    stride: usize,
    width: usize,
    rows: usize,
) {
    plane.clear();
    plane.reserve(width * rows);
    for row in 0..rows {
        // SAFETY: guaranteed by the caller.
        plane.extend_from_slice(unsafe {
            std::slice::from_raw_parts(source.add(row * stride), width)
        });
    }
}

/// Presentation timestamps of all samples in decode order, from the MP4
/// sample tables.
fn presentation_times(track: &mp4::Mp4Track) -> Vec<i64> {
    let stbl = &track.trak.mdia.minf.stbl;
    let decode_times = stbl.stts.entries.iter().flat_map(|entry| {
        std::iter::repeat_n(i64::from(entry.sample_delta), entry.sample_count as usize)
    });
    let offsets = stbl
        .ctts
        .iter()
        .flat_map(|ctts| &ctts.entries)
        .flat_map(|entry| {
            std::iter::repeat_n(i64::from(entry.sample_offset), entry.sample_count as usize)
        })
        .chain(std::iter::repeat(0));
    decode_times
        .scan(0i64, |time, delta| {
            let start = *time;
            *time += delta;
            Some(start)
        })
        .zip(offsets)
        .map(|(decode, offset)| decode + offset)
        .collect()
}

pub struct ThreadedDecoder<D> {
    frames: mpsc::Receiver<Result<Nv12Frame>>,
    timings: mpsc::Receiver<FrameTiming>,
    buffered_frame: Option<Nv12Frame>,
    frame_size: Option<PhysicalSize<u32>>,
    decoder: PhantomData<fn() -> D>,
}

fn run<D: FrameDecoder, S: Read + Seek>(
    track: Mp4TrackReader<S>,
    converter: Mp4BitstreamConverter,
    info: StreamInfo,
    ready: mpsc::SyncSender<Result<()>>,
    output: &mut Output,
    timings: mpsc::Sender<FrameTiming>,
) -> Result<()> {
    let mut decoder = match D::new(&info) {
        Ok(decoder) => {
            let _ = ready.send(Ok(()));
            decoder
        }
        Err(error) => {
            let _ = ready.send(Err(error));
            return Ok(());
        }
    };
    let mut restart = track.clone();
    let mut timed = 0;
    let result = decode(
        &mut decoder,
        track,
        converter,
        output,
        &timings,
        0,
        &mut timed,
    );
    let error = match result {
        Err(error) if decoder.hardware() && !output.closed => error,
        result => return result,
    };
    warn!(
        "{} hardware decoding failed, decoding in software: {error:#}",
        D::NAME
    );
    // The failed decoder's device and surfaces go before the software one starts.
    drop(decoder);
    let mut software = D::software(&info).map_err(|software_error| {
        error.context(format!("Software decoding also failed: {software_error:#}"))
    })?;
    let sync = restart.get_mp4_track_info(|track| {
        let stss = track.trak.mdia.minf.stbl.stss.as_ref();
        stss.map(|stss| stss.entries.clone())
    });
    let first = resume_sample(&info.presentation_times, sync.as_deref(), output.delivered);
    restart.seek(first);
    output.position = first;
    let converter = restart
        .get_mp4_track_info(Mp4BitstreamConverter::for_mp4_track)
        .context("Preparing AVC bitstream converter")?;
    decode(
        &mut software,
        restart,
        converter,
        output,
        &timings,
        first,
        &mut timed,
    )
}

/// The sample a software decoder taking over resumes from: the last sync sample
/// at or before the first frame not yet delivered whose frames present after
/// every earlier sample's, so that it decodes the rest from there. The start
/// otherwise.
fn resume_sample(presentation_times: &[i64], sync: Option<&[u32]>, delivered: u32) -> u32 {
    // the latest presentation before each sample, and the earliest from it on
    let before: Vec<i64> = presentation_times
        .iter()
        .scan(i64::MIN, |latest, &time| {
            let before = *latest;
            *latest = (*latest).max(time);
            Some(before)
        })
        .collect();
    let mut from: Vec<i64> = presentation_times
        .iter()
        .rev()
        .scan(i64::MAX, |earliest, &time| {
            *earliest = (*earliest).min(time);
            Some(*earliest)
        })
        .collect();
    from.reverse();
    let candidates: Box<dyn Iterator<Item = u32>> = match sync {
        // stss numbers samples from 1
        Some(sync) => Box::new(sync.iter().rev().map(|&number| number.saturating_sub(1))),
        // without one every sample is a sync sample
        None => Box::new((0..=delivered).rev()),
    };
    candidates
        .filter(|&index| index <= delivered)
        .find(|&index| {
            let index = index as usize;
            index < presentation_times.len() && before[index] < from[index]
        })
        .unwrap_or(0)
}

/// Decode `track` from the sample at `frame_number` to its end into `output`,
/// publishing the timings of samples from `timed` on.
fn decode<D: FrameDecoder, S: Read + Seek>(
    decoder: &mut D,
    mut track: Mp4TrackReader<S>,
    mut converter: Mp4BitstreamConverter,
    output: &mut Output,
    timings: &mpsc::Sender<FrameTiming>,
    mut frame_number: u32,
    timed: &mut u32,
) -> Result<()> {
    let mut annex_b = Vec::new();
    while let Some(sample) = track.next_sample().context("Reading MP4 video sample")? {
        if frame_number >= *timed {
            if timings
                .send(FrameTiming {
                    frame_number,
                    start_time: sample.start_time,
                    duration: sample.duration,
                })
                .is_err()
            {
                return Ok(());
            }
            *timed = frame_number + 1;
        }
        frame_number += 1;
        converter
            .convert_packet(&sample.bytes, &mut annex_b)
            .context("Converting MP4 sample to Annex B")?;
        let packet = Packet {
            avcc: &sample.bytes,
            annex_b: &annex_b,
            decode_time: sample.start_time,
            presentation_time: sample.start_time as i64 + i64::from(sample.rendering_offset),
            duration: sample.duration,
        };
        decoder
            .decode(&packet, output)
            .with_context(|| format!("Decoding movie frame with {}", D::NAME))?;
        if output.closed {
            return Ok(());
        }
    }
    decoder
        .finish(output)
        .with_context(|| format!("Finishing movie with {}", D::NAME))
}

impl<D: FrameDecoder + 'static> H264DecoderTrait for ThreadedDecoder<D> {
    fn new<S: Read + Seek + Send + 'static>(track: Mp4TrackReader<S>) -> Result<Self> {
        let converter = track
            .get_mp4_track_info(Mp4BitstreamConverter::for_mp4_track)
            .context("Preparing AVC bitstream converter")?;
        let info = StreamInfo {
            timescale: track.get_mp4_track_info(|track| track.timescale()),
            width: track.get_mp4_track_info(|track| u32::from(track.width())),
            height: track.get_mp4_track_info(|track| u32::from(track.height())),
            length_size: converter.length_size(),
            parameter_sets: converter.parameter_sets().map(<[u8]>::to_vec).collect(),
            presentation_times: track.get_mp4_track_info(presentation_times),
        };
        let (ready_sender, ready) = mpsc::sync_channel(1);
        let (frame_sender, frames) = mpsc::sync_channel(FRAME_QUEUE);
        let (timing_sender, timings) = mpsc::channel();
        std::thread::Builder::new()
            .name(format!("movie decoder ({})", D::NAME))
            .spawn(move || {
                let _spares = SparePlanes::hold();
                let mut output = Output {
                    frames: frame_sender,
                    closed: false,
                    position: 0,
                    delivered: 0,
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run::<D, S>(
                        track,
                        converter,
                        info,
                        ready_sender,
                        &mut output,
                        timing_sender,
                    )
                }));
                let error = match result {
                    Ok(Ok(())) => return,
                    Ok(Err(error)) => error,
                    Err(panic) => anyhow!(
                        "Movie decoder panicked: {}",
                        panic
                            .downcast_ref::<String>()
                            .map(String::as_str)
                            .or_else(|| panic.downcast_ref::<&str>().copied())
                            .unwrap_or("unknown panic")
                    ),
                };
                let _ = output.frames.send(Err(error));
            })
            .context("Starting the movie decoder thread")?;
        ready
            .recv()
            .map_err(|_| anyhow!("{} failed to start", D::NAME))??;
        Ok(Self {
            frames,
            timings,
            buffered_frame: None,
            frame_size: None,
            decoder: PhantomData,
        })
    }

    fn read_frame(&mut self) -> Result<Option<(FrameTiming, Nv12Frame)>> {
        let frame = match self.buffered_frame.take() {
            Some(frame) => frame,
            None => match self.frames.recv() {
                Ok(frame) => frame?,
                Err(mpsc::RecvError) => return self.end_of_stream().map(|()| None),
            },
        };
        let timing = self
            .timings
            .recv()
            .context("Decoded movie frame has no MP4 timing")?;
        self.frame_size = Some(frame.size);
        Ok(Some((timing, frame)))
    }

    fn try_read_frame(&mut self) -> Result<DecoderPoll> {
        let frame = match self.buffered_frame.take() {
            Some(frame) => frame,
            None => match self.frames.try_recv() {
                Ok(frame) => frame?,
                Err(mpsc::TryRecvError::Empty) => return Ok(DecoderPoll::Pending),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return self.end_of_stream().map(|()| DecoderPoll::Eof);
                }
            },
        };
        // Sample timing is published before the sample is decoded.
        let timing = self
            .timings
            .try_recv()
            .context("Decoded movie frame has no queued MP4 timing")?;
        self.frame_size = Some(frame.size);
        Ok(DecoderPoll::Frame(timing, frame))
    }

    fn frame_size(&mut self) -> Result<PhysicalSize<u32>> {
        if let Some(size) = self.frame_size {
            return Ok(size);
        }
        let frame = self
            .frames
            .recv()
            .context("No frames available, don't know the format")??;
        let size = frame.size;
        self.frame_size = Some(size);
        self.buffered_frame = Some(frame);
        Ok(size)
    }
}

impl<D> ThreadedDecoder<D> {
    fn end_of_stream(&self) -> Result<()> {
        anyhow::ensure!(
            self.timings.try_recv().is_err(),
            "Movie decoder ended before decoding all MP4 video samples"
        );
        Ok(())
    }
}

#[cfg(test)]
mod resume_tests {
    use super::resume_sample;

    #[test]
    fn software_resumes_at_the_last_sync_sample_before_undelivered_frames() {
        // I P B B | I P B B in decode order, presenting I B B P each.
        let times = [0, 3, 1, 2, 4, 7, 5, 6];
        let sync = [1, 5];
        assert_eq!(resume_sample(&times, Some(&sync), 0), 0);
        assert_eq!(resume_sample(&times, Some(&sync), 3), 0);
        assert_eq!(resume_sample(&times, Some(&sync), 4), 4);
        assert_eq!(resume_sample(&times, Some(&sync), 7), 4);
        // Without sync samples listed, every sample is one, but not one that
        // presents after a later sample (the second B frame).
        assert_eq!(resume_sample(&times, None, 3), 1);
        assert_eq!(resume_sample(&times, None, 5), 5);
        // A sync sample with a later sample presenting before it is passed over.
        assert_eq!(resume_sample(&[0, 2, 3, 1], Some(&[1, 3]), 3), 0);
    }
}
