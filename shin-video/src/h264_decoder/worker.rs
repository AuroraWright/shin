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
use tracing::debug;

use super::{DecoderPoll, FrameTiming, H264DecoderTrait, Nv12Frame};
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
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
    pub decode_time: u64,
    pub presentation_time: i64,
    pub duration: u32,
}

/// Where a [`FrameDecoder`] delivers frames, in presentation order.
pub(super) struct Output {
    frames: mpsc::SyncSender<Result<Nv12Frame>>,
    closed: bool,
}

impl Output {
    pub fn emit(&mut self, frame: Nv12Frame) {
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
}

/// Copy a plane of `rows` rows of `width` bytes from a buffer whose rows are
/// `stride` bytes apart.
///
/// # Safety
/// `source` must be readable for `stride * (rows - 1) + width` bytes.
pub(super) unsafe fn copy_plane(
    source: *const u8,
    stride: usize,
    width: usize,
    rows: usize,
) -> Vec<u8> {
    let mut plane = Vec::with_capacity(width * rows);
    for row in 0..rows {
        // SAFETY: guaranteed by the caller.
        plane.extend_from_slice(unsafe {
            std::slice::from_raw_parts(source.add(row * stride), width)
        });
    }
    plane
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
    mut track: Mp4TrackReader<S>,
    mut converter: Mp4BitstreamConverter,
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
    let mut annex_b = Vec::new();
    let mut frame_number = 0;
    while let Some(sample) = track.next_sample().context("Reading MP4 video sample")? {
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
                let mut output = Output {
                    frames: frame_sender,
                    closed: false,
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
