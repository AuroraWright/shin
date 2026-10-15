//! This module implements an H264 decoder that spawns an ffmpeg process to delegate the decoding.
//!
//! It pipes the H264 Annex B bitstream to ffmpeg, and reads the YUV420P frames wrapped in y4m format from ffmpeg's stdout.

use std::{
    io::{Read, Seek},
    process::Stdio,
};

use anyhow::{Context, Result};
use dpi::PhysicalSize;
use futures_lite::{AsyncBufReadExt, AsyncWriteExt, io::BufReader};
use shin_tasks::AsyncTask;
use tracing::{debug, error, trace, warn};

use crate::{
    h264_decoder::{
        FrameTiming, Nv12Frame,
        y4m::{self, FrameSize, PlanarFrame},
    },
    mp4::Mp4TrackReader,
    mp4_bitstream_converter::Mp4BitstreamConverter,
};

/// Decodes h264 Annex B format to YUV420P (or, possibly, some other frame format).
///
/// Currently implemented as a pipe to the ffmpeg binary, other options are possible in the future.
pub struct SpawnFfmpegH264Decoder {
    process: async_process::Child,
    frame_receiver: async_channel::Receiver<Result<PlanarFrame>>,
    buffered_frame: Option<PlanarFrame>,
    frame_timing_receiver: std::sync::mpsc::Receiver<FrameTiming>,
    #[allow(unused)]
    frame_sender_task: AsyncTask<()>,
    #[allow(unused)]
    packet_sender_task: AsyncTask<()>,
    #[allow(unused)]
    stderr_task: AsyncTask<()>,

    frame_size: Option<FrameSize>,
}

// const FFMPEG_LOG_LEVEL: &str = "debug";
const FFMPEG_LOG_LEVEL: &str = "info";

async fn guard_decoder_worker(
    name: &'static str,
    errors: async_channel::Sender<Result<PlanarFrame>>,
    worker: impl std::future::Future<Output = ()>,
) {
    let mut worker = std::pin::pin!(worker);
    let result = std::future::poll_fn(|cx| {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker.as_mut().poll(cx))) {
            Ok(std::task::Poll::Pending) => std::task::Poll::Pending,
            Ok(std::task::Poll::Ready(())) => std::task::Poll::Ready(Ok(())),
            Err(panic) => std::task::Poll::Ready(Err(panic)),
        }
    })
    .await;
    if let Err(panic) = result {
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("unknown panic");
        // Keep the failed worker's pipe/channel owners alive until this error
        // is published. Closing stdin first can race with FFmpeg EOF and hide
        // the input failure behind an unrelated output-header error.
        let _ = errors
            .send(Err(anyhow::anyhow!(
                "Movie {name} callback panicked: {message}"
            )))
            .await;
    }
}

async fn forward_decoded_frames<R: futures_lite::AsyncRead + Unpin>(
    stdout: R,
    frame_sender: async_channel::Sender<Result<PlanarFrame>>,
) {
    guard_decoder_worker(
        "decoded output",
        frame_sender.clone(),
        forward_decoded_frames_inner(stdout, frame_sender),
    )
    .await;
}

async fn forward_decoded_frames_inner<R: futures_lite::AsyncRead + Unpin>(
    stdout: R,
    frame_sender: async_channel::Sender<Result<PlanarFrame>>,
) {
    let mut decoder = match y4m::Decoder::new(stdout).await {
        Ok(r) => r,
        Err(e) => {
            error!(
                "Error creating y4m decoder: {}. The stream will be closed",
                e
            );
            let _ = frame_sender
                .send(Err(
                    anyhow::Error::new(e).context("Creating FFmpeg y4m decoder")
                ))
                .await;
            return;
        }
    };
    loop {
        match decoder.read_frame().await {
            Ok(frame) => {
                trace!("Sending frame to game");
                if frame_sender.send(Ok(frame)).await.is_err() {
                    debug!("Game closed the channel, stopping sending frames");
                    break;
                }
            }
            Err(y4m::Error::EndOfFile) => {
                debug!("EOF from ffmpeg, stopping sending to game");
                break;
            }
            Err(e) => {
                error!("Error reading frame from ffmpeg: {}", e);
                let _ = frame_sender
                    .send(Err(anyhow::Error::new(e).context("Reading FFmpeg frame")))
                    .await;
                break;
            }
        }
    }
}

impl super::H264DecoderTrait for SpawnFfmpegH264Decoder {
    fn new<S: Read + Seek + Send + 'static>(track: Mp4TrackReader<S>) -> Result<Self> {
        let mut bitstream_converter = track
            .get_mp4_track_info(Mp4BitstreamConverter::for_mp4_track)
            .context("Preparing AVC bitstream converter")?;

        // TODO: use a more robust way to find the ffmpeg binary
        let ffmpeg = which::which("ffmpeg").context("Could not locate ffmpeg binary")?;

        // let timescale = track.get_mp4_track_info(|t| t.timescale());

        let mut process = async_process::Command::new(ffmpeg)
            .arg("-loglevel")
            .arg(FFMPEG_LOG_LEVEL)
            .arg("-f")
            .arg("h264")
            .arg("-flags")
            .arg("low_delay")
            .arg("-analyzeduration")
            .arg("0")
            .arg("-probesize")
            .arg("32")
            .arg("-i")
            .arg("pipe:0")
            .arg("-f")
            .arg("yuv4mpegpipe")
            .arg("pipe:1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Could not spawn ffmpeg process")?;

        let stdin = process.stdin.take().unwrap();
        let stdout = process.stdout.take().unwrap();
        let stderr = process.stderr.take().unwrap();

        // send the decoded frames from ffmpeg to the game
        let (frame_sender, frame_receiver) = async_channel::bounded(60);
        // send the frame timings from the mp4 stream to the game (without passing through ffmpeg)
        // Timing records must not limit FFmpeg input: decoder lookahead can
        // exceed any small frame queue before producing its first output.
        let (frame_timing_sender, frame_timing_receiver) = std::sync::mpsc::channel();

        let input_error_sender = frame_sender.clone();
        let frame_sender_task =
            shin_tasks::async_io::spawn(forward_decoded_frames(stdout, frame_sender));

        let packet_sender_task = shin_tasks::async_io::spawn(guard_decoder_worker(
            "MP4 input",
            input_error_sender.clone(),
            async move {
                let mut track = track;
                let mut buffer = Vec::new();
                let mut stdin = stdin;

                let mut frame_number = 0;

                loop {
                    match track.next_sample() {
                        Ok(Some(sample)) => {
                            // MP4 can do frame reordering if B-frames are used
                            // this seems to be indicated by the sample.rendering_offset field
                            // it also seems that this info can be duplicated in the h264 bistream in a form of picture_timing SEI NALUs
                            // it also seems that ffmpeg handles the picture_timing SEI NALUs correctly, so we don't need to do anything with the rendering offset if they are present
                            // NOTE: maybe what I said above is not correct, as, after stripping the SEI NALUs, the video still looks correct...
                            // I'll ignore the rendering offset for now, but this should be accounted for if other decoders are implemented

                            let frame_timing = FrameTiming {
                                frame_number,
                                start_time: sample.start_time,
                                duration: sample.duration,
                            };

                            frame_number += 1;

                            if frame_timing_sender.send(frame_timing).is_err() {
                                debug!("Game closed the channel, stopping sending frame timings");
                                break;
                            }

                            if let Err(error) =
                                bitstream_converter.convert_packet(&sample.bytes, &mut buffer)
                            {
                                let _ = input_error_sender
                                    .send(Err(error.context("Converting MP4 sample to Annex B")))
                                    .await;
                                break;
                            }
                            trace!("Sending sample to ffmpeg ({} bytes)", buffer.len());
                            match stdin.write_all(&buffer).await {
                                Ok(_) => {}
                                Err(e) => {
                                    error!("Error writing sample to ffmpeg: {}", e);
                                    let _ = input_error_sender
                                        .send(Err(anyhow::Error::new(e)
                                            .context("Writing MP4 sample to FFmpeg")))
                                        .await;
                                    break;
                                }
                            }
                        }
                        Ok(None) => {
                            debug!("EOF from mp4, stopping sending to ffmpeg");
                            break;
                        }
                        Err(e) => {
                            error!("Error reading sample from mp4: {}", e);
                            let _ = input_error_sender
                                .send(Err(e.context("Reading MP4 sample for FFmpeg")))
                                .await;
                            break;
                        }
                    }
                }
            },
        ));

        // pump ffmpeg's stderr to the logs
        // TODO: parse the ffmpeg's log leve?
        let stderr_task = shin_tasks::async_io::spawn(async move {
            let mut stderr = BufReader::new(stderr);
            let mut buffer = String::new();

            loop {
                match stderr.read_line(&mut buffer).await {
                    Ok(0) => {
                        debug!("EOF from ffmpeg stderr, stopping reading");
                        break;
                    }
                    Ok(_) => {
                        buffer.pop(); // remove newline
                        debug!("ffmpeg: {}", buffer);
                        buffer.clear();
                    }
                    Err(e) => {
                        error!("Error reading ffmpeg stderr: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(Self {
            process,
            frame_receiver,
            buffered_frame: None,
            frame_timing_receiver,
            frame_sender_task,
            packet_sender_task,
            stderr_task,
            frame_size: None,
        })
    }

    fn read_frame(&mut self) -> Result<Option<(FrameTiming, Nv12Frame)>> {
        trace!("Reading frame from ffmpeg...");
        match self
            .buffered_frame
            .take()
            .map(Ok)
            .or_else(|| self.frame_receiver.recv_blocking().ok())
        {
            Some(frame) => {
                let frame = frame?;
                let timing = self
                    .frame_timing_receiver
                    .recv()
                    .context("FFmpeg frame has no MP4 timing")?;

                let frame_size = *frame.size();
                // TODO: this causes an extra avoidable copy. Should ideally be integrated into the y4m parser
                let frame = frame
                    .into_nv12()
                    .context("Converting FFmpeg frame to NV12")?;
                self.frame_size = Some(frame_size);

                Ok(Some((timing, frame)))
            }
            None => {
                anyhow::ensure!(
                    self.frame_timing_receiver.try_recv().is_err(),
                    "FFmpeg ended before decoding all MP4 video samples"
                );
                Ok(None)
            }
        }
    }

    fn try_read_frame(&mut self) -> Result<super::DecoderPoll> {
        let frame = if let Some(frame) = self.buffered_frame.take() {
            frame
        } else {
            match self.frame_receiver.try_recv() {
                Ok(frame) => frame?,
                Err(async_channel::TryRecvError::Empty) => return Ok(super::DecoderPoll::Pending),
                Err(async_channel::TryRecvError::Closed) => {
                    anyhow::ensure!(
                        self.frame_timing_receiver.try_recv().is_err(),
                        "FFmpeg ended before decoding all MP4 video samples"
                    );
                    return Ok(super::DecoderPoll::Eof);
                }
            }
        };
        // Packet timing is published before its decoded frame can be queued.
        // Never block the Higurashi update if that codec transport invariant fails.
        let timing = self.frame_timing_receiver.try_recv()
            .context("FFmpeg decoded frame has no queued MP4 timing")?;
        let frame_size = *frame.size();
        let frame = frame.into_nv12().context("Converting FFmpeg frame to NV12")?;
        self.frame_size = Some(frame_size);
        Ok(super::DecoderPoll::Frame(timing, frame))
    }

    fn frame_size(&mut self) -> Result<PhysicalSize<u32>> {
        debug!("Reading frame info from ffmpeg");
        match self.frame_size {
            Some(info) => {
                let size = info.plane_sizes[0];
                Ok(PhysicalSize::new(size.width, size.height))
            }
            None => {
                let frame = self
                    .frame_receiver
                    .recv_blocking()
                    .context("No frames available, don't know the format")??;
                frame
                    .validate_nv12()
                    .context("Validating FFmpeg frame layout")?;
                let size = frame.size().plane_sizes[0];
                self.frame_size = Some(*frame.size());
                self.buffered_frame = Some(frame);
                Ok(PhysicalSize::new(size.width, size.height))
            }
        }
    }
}

impl Drop for SpawnFfmpegH264Decoder {
    fn drop(&mut self) {
        if let Err(e) = self.process.kill() {
            warn!("Error killing ffmpeg: {:?}", e);
        }
    }
}

#[cfg(test)]
#[path = "callback_failure_reference.rs"]
mod callback_failure_reference;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{h264_decoder::H264DecoderTrait, mp4::Mp4};

    #[test]
    fn decoded_output_worker_publishes_truncation_after_complete_frame() {
        futures_lite::future::block_on(async {
            let bytes = [
                &b"YUV4MPEG2 W2 H2 C420mpeg2\nFRAME\n"[..],
                &[1, 2, 3, 4, 5, 6],
                b"FRAME\n12345",
            ]
            .concat();
            let (send, receive) = async_channel::bounded(4);
            forward_decoded_frames(futures_lite::io::Cursor::new(bytes), send).await;
            let frame = receive.recv().await.unwrap().unwrap().into_nv12().unwrap();
            assert_eq!(frame.y_plane, [1, 2, 3, 4]);
            let error = receive.recv().await.unwrap().unwrap_err();
            assert!(format!("{error:#}").contains("Reading FFmpeg frame"));
            match error.downcast_ref::<y4m::Error>().unwrap() {
                y4m::Error::Io(io_error) => {
                    assert_eq!(io_error.kind(), std::io::ErrorKind::UnexpectedEof)
                }
                other => panic!("Expected truncated frame I/O error: {other:?}"),
            }
            assert!(receive.recv().await.is_err());
        });
    }

    #[test]
    fn decoded_output_worker_publishes_header_parse_failure() {
        futures_lite::future::block_on(async {
            for bytes in [
                &b"not y4m\n"[..],
                &b"YUV4MPEG2 W2 H2 C420mpeg2\nFRAMEX\n"[..],
            ] {
                let (send, receive) = async_channel::bounded(4);
                forward_decoded_frames(futures_lite::io::Cursor::new(bytes), send).await;
                let error = receive.recv().await.unwrap().unwrap_err();
                assert!(format!("{error:#}").contains("Error parsing y4m header"));
                assert!(receive.recv().await.is_err());
            }
        });
    }

    #[test]
    #[ignore = "requires ffmpeg/libx264; no supplied assets, GPU or audio required"]
    fn reference_unsupported_ffmpeg_output_returns_error_before_texture_creation() {
        crate::init_test_task_pools();
        let path = std::env::temp_dir().join(format!(
            "shin-unsupported-movie-format-{}.mp4",
            std::process::id()
        ));
        let output = std::process::Command::new(which::which("ffmpeg").unwrap())
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=red:size=16x16:rate=30",
                "-frames:v",
                "3",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv444p",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        for query_size_first in [false, true] {
            let mp4 = Mp4::new(std::io::Cursor::new(bytes.clone())).unwrap();
            let mut decoder = SpawnFfmpegH264Decoder::new(mp4.video_track).unwrap();
            let error = if query_size_first {
                decoder.frame_size().unwrap_err()
            } else {
                decoder.read_frame().unwrap_err()
            };
            assert!(format!("{error:#}").contains("Unsupported decoded frame layout for NV12"));
        }
    }

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM) and ffmpeg; no GPU or audio required"]
    fn reference_damaged_avc_sample_propagates_conversion_error() {
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::{fs::File, io::Cursor};
        crate::init_test_task_pools();
        let mut rom = RomReader::new(
            StatelessFile::new(File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
        let mut original = Vec::new();
        rom.open_file(rom.find_file("/movie/door.mp4").unwrap())
            .unwrap()
            .read_to_end(&mut original)
            .unwrap();
        let mut mp4 = Mp4::new(Cursor::new(original.clone())).unwrap();
        let sample = mp4.video_track.next_sample().unwrap().unwrap();
        let offset = original
            .windows(sample.bytes.len())
            .position(|bytes| bytes == sample.bytes.as_ref())
            .unwrap();
        for (prefix, expected) in [
            (
                &[0xff, 0xff, 0xff, 0xff][..],
                "payload exceeds MP4 sample size",
            ),
            (&[0, 0, 0, 1, 0x65][..], "Truncated AVC IDR slice header"),
        ] {
            let mut damaged = original.clone();
            damaged[offset..offset + prefix.len()].copy_from_slice(prefix);
            let mp4 = Mp4::new(Cursor::new(damaged)).unwrap();
            let mut decoder = SpawnFfmpegH264Decoder::new(mp4.video_track).unwrap();
            let error = decoder.read_frame().unwrap_err();
            let error = format!("{error:#}");
            assert!(
                error.contains("Converting MP4 sample to Annex B"),
                "{error}"
            );
            assert!(error.contains(expected), "{error}");
        }
        // Validate missing configuration and overflow before worker startup.
        for missing_config in [true, false] {
            let mp4 = Mp4::new(Cursor::new(original.clone())).unwrap();
            let mut track = ::mp4::Mp4Track {
                trak: mp4
                    .video_track
                    .get_mp4_track_info(|track| track.trak.clone()),
                trafs: Vec::new(),
                default_sample_duration: 0,
            };
            if missing_config {
                track.trak.mdia.minf.stbl.stsd.avc1 = None;
            } else {
                track
                    .trak
                    .mdia
                    .minf
                    .stbl
                    .stsd
                    .avc1
                    .as_mut()
                    .unwrap()
                    .avcc
                    .length_size_minus_one = 255;
            }
            let error = match Mp4BitstreamConverter::for_mp4_track(&track) {
                Ok(_) => panic!("Invalid AVC configuration was accepted"),
                Err(error) => format!("{error:#}"),
            };
            assert!(
                error.contains(if missing_config {
                    "does not contain AVC1"
                } else {
                    "Invalid AVC NAL length size"
                }),
                "{error}"
            );
        }
    }

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM) and ffmpeg; no GPU required"]
    fn reference_mp4_input_failure_propagates_instead_of_becoming_video_eof() {
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::{
            fs::File,
            io::{Cursor, SeekFrom},
            sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
            },
        };
        struct FailingReader {
            inner: Cursor<Vec<u8>>,
            fail: Arc<AtomicBool>,
        }
        impl Read for FailingReader {
            fn read(&mut self, destination: &mut [u8]) -> std::io::Result<usize> {
                if self.fail.load(Ordering::Acquire) {
                    return Err(std::io::Error::other("injected MP4 source read failure"));
                }
                self.inner.read(destination)
            }
        }
        impl Seek for FailingReader {
            fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
                self.inner.seek(position)
            }
        }
        crate::init_test_task_pools();
        let mut rom = RomReader::new(
            StatelessFile::new(File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
        let file = rom.find_file("/movie/door.mp4").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let fail = Arc::new(AtomicBool::new(false));
        let mp4 = Mp4::new(FailingReader {
            inner: Cursor::new(bytes),
            fail: fail.clone(),
        })
        .unwrap();
        fail.store(true, Ordering::Release);
        let mut decoder = SpawnFfmpegH264Decoder::new(mp4.video_track).unwrap();
        let error = decoder.read_frame().unwrap_err();
        assert!(format!("{error:#}").contains("injected MP4 source read failure"));
    }

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM) and ffmpeg; no GPU required"]
    fn reference_door_decodes_all_frames_with_shared_io_worker() {
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::{fs::File, io::Cursor, sync::mpsc, time::Duration};
        crate::init_test_task_pools();
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let mut rom =
                RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
            let file = rom.find_file("/movie/door.mp4").unwrap();
            let mut bytes = Vec::new();
            rom.open_file(file)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            let mp4 = Mp4::new(Cursor::new(bytes)).unwrap();
            assert!(mp4.audio_track.is_none());
            let timescale = mp4
                .video_track
                .get_mp4_track_info(|track| track.timescale());
            let mut decoder = SpawnFfmpegH264Decoder::new(mp4.video_track).unwrap();
            // Querying dimensions first must retain the first decoded frame.
            let size = decoder.frame_size().unwrap();
            let mut count = 0;
            let mut end = 0;
            while let Some((timing, frame)) = decoder.read_frame().unwrap() {
                assert_eq!(timing.frame_number, count);
                assert_eq!(frame.size, size);
                assert_eq!(timing.start_time, end);
                assert!(timing.duration > 0);
                end = timing.start_time + u64::from(timing.duration);
                count += 1;
            }
            send.send((count, timescale, end)).unwrap();
        });
        assert_eq!(
            receive
                .recv_timeout(Duration::from_secs(30))
                .expect("Movie decoder stalled or failed"),
            (150, 60_000, 300_300)
        );
    }
}
