mod nv12;

use std::io::{Read, Seek};

use anyhow::Result;
use cfg_if::cfg_if;
use dpi::PhysicalSize;
pub use nv12::Nv12Frame;

use tracing::warn;

use crate::mp4::Mp4TrackReader;

/// Polling decoded output preserves a pending transport separately from EOF.
pub enum DecoderPoll {
    Pending,
    Frame(FrameTiming, Nv12Frame),
    Eof,
}

pub trait H264DecoderTrait: Sized {
    fn new<S: Read + Seek + Send + 'static>(track: Mp4TrackReader<S>) -> Result<Self>;

    fn read_frame(&mut self) -> Result<Option<(FrameTiming, Nv12Frame)>>;

    fn try_read_frame(&mut self) -> Result<DecoderPoll>;

    fn frame_size(&mut self) -> Result<PhysicalSize<u32>>;
}

mod spawn_ffmpeg;
mod worker;
mod y4m;

cfg_if! {
    if #[cfg(target_os = "macos")] {
        mod videotoolbox;
        type PlatformDecoder = worker::ThreadedDecoder<videotoolbox::VideoToolbox>;
    } else if #[cfg(windows)] {
        mod media_foundation;
        type PlatformDecoder = worker::ThreadedDecoder<media_foundation::MediaFoundation>;
    } else {
        mod gstreamer;
        type PlatformDecoder = worker::ThreadedDecoder<gstreamer::GStreamer>;
    }
}

use spawn_ffmpeg::SpawnFfmpegH264Decoder;

/// Decodes movies with the platform decoder (VideoToolbox, Media Foundation or
/// GStreamer), falling back to an ffmpeg executable when it is unavailable.
/// The `ffmpeg` feature always uses ffmpeg.
pub enum H264Decoder {
    Platform(PlatformDecoder),
    Ffmpeg(SpawnFfmpegH264Decoder),
}

impl H264DecoderTrait for H264Decoder {
    fn new<S: Read + Seek + Send + 'static>(track: Mp4TrackReader<S>) -> Result<Self> {
        if cfg!(feature = "ffmpeg") {
            return SpawnFfmpegH264Decoder::new(track).map(Self::Ffmpeg);
        }
        let platform_error = match PlatformDecoder::new(track.clone()) {
            Ok(decoder) => return Ok(Self::Platform(decoder)),
            Err(error) => error,
        };
        warn!("Platform movie decoder unavailable, trying ffmpeg: {platform_error:#}");
        SpawnFfmpegH264Decoder::new(track)
            .map(Self::Ffmpeg)
            .map_err(|ffmpeg_error| {
                platform_error.context(format!("ffmpeg fallback also failed: {ffmpeg_error:#}"))
            })
    }

    fn read_frame(&mut self) -> Result<Option<(FrameTiming, Nv12Frame)>> {
        match self {
            Self::Platform(decoder) => decoder.read_frame(),
            Self::Ffmpeg(decoder) => decoder.read_frame(),
        }
    }

    fn try_read_frame(&mut self) -> Result<DecoderPoll> {
        match self {
            Self::Platform(decoder) => decoder.try_read_frame(),
            Self::Ffmpeg(decoder) => decoder.try_read_frame(),
        }
    }

    fn frame_size(&mut self) -> Result<PhysicalSize<u32>> {
        match self {
            Self::Platform(decoder) => decoder.frame_size(),
            Self::Ffmpeg(decoder) => decoder.frame_size(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameTiming {
    pub frame_number: u32,
    pub start_time: u64,
    pub duration: u32,
}
