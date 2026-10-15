//! Controlled faults in actual decoder producers; no GPU/audio/window/saves.
use super::*;
use crate::{h264_decoder::H264DecoderTrait, mp4::Mp4};
use std::{
    io::{Cursor, SeekFrom},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

struct PanickingReader {
    bytes: Cursor<Vec<u8>>,
    armed: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
}
impl Read for PanickingReader {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        assert!(
            !self.armed.load(Ordering::Acquire),
            "injected MP4 callback panic"
        );
        self.bytes.read(output)
    }
}
impl Seek for PanickingReader {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.bytes.seek(position)
    }
}
impl Drop for PanickingReader {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}
fn door_bytes() -> Vec<u8> {
    use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut bytes = Vec::new();
    rom.open_file(rom.find_file("/movie/door.mp4").unwrap())
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}

#[test]
#[ignore = "requires supplied ROM and ffmpeg; no GPU/audio/window or progress writes"]
fn reference_packet_callback_panic_is_published_and_retires() {
    crate::init_test_task_pools();
    let bytes = door_bytes();
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let armed = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let track = Mp4::new(PanickingReader {
            bytes: Cursor::new(bytes.clone()),
            armed: armed.clone(),
            dropped: dropped.clone(),
        })
        .unwrap()
        .video_track;
        armed.store(true, Ordering::Release);
        let task = shin_tasks::compute::spawn_loader(move || SpawnFfmpegH264Decoder::new(track));
        let retirement = task.retirement_handle();
        let mut decoder = shin_tasks::block_on(task).unwrap();
        let failure = match decoder.read_frame() {
            Err(error) => format!("{error:#}"),
            Ok(frame) => format!("Unexpected success: frame={}", frame.is_some()),
        };
        retirement.clone().retire(decoder);
        let retired = retirement.is_finished() && dropped.load(Ordering::Acquire);
        // Prove the shared I/O executor remains usable after the producer fault.
        let mut probe = shin_tasks::async_io::spawn(async { 42 });
        let usable = probe.poll_with_timeout(Duration::from_secs(2)) == Some(42);
        let track = Mp4::new(Cursor::new(bytes)).unwrap().video_track;
        let task = shin_tasks::compute::spawn_loader(move || SpawnFfmpegH264Decoder::new(track));
        let retry_retirement = task.retirement_handle();
        let mut retry = shin_tasks::block_on(task).unwrap();
        let first = retry.read_frame().unwrap().unwrap();
        let size = first.1.size;
        retry_retirement.clone().retire(retry);
        send.send((
            failure,
            retired,
            usable,
            first.0.frame_number,
            size,
            retry_retirement.is_finished(),
        ))
        .unwrap();
    });
    let (failure, retired, usable, frame, size, retry_retired) = receive
        .recv_timeout(Duration::from_secs(15))
        .expect("Decoder callback fault or retirement stalled");
    eprintln!(
        "PACKET_CALLBACK failure={failure} retired={retired} executor_usable={usable} retry_frame={frame} retry_size={size:?} retry_retired={retry_retired}"
    );
    assert!(failure.contains("injected MP4 callback panic"), "{failure}");
    assert!(retired && usable && retry_retired);
    assert_eq!(frame, 0);
    assert!(size.width > 0 && size.height > 0);
}

struct PanickingOutput {
    bytes: Cursor<Vec<u8>>,
    fail_after: usize,
    pending_once: bool,
    dropped: Arc<AtomicBool>,
    faulted: Arc<AtomicBool>,
}
impl futures_lite::AsyncRead for PanickingOutput {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        output: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if this.pending_once {
            this.pending_once = false;
            cx.waker().wake_by_ref();
            return std::task::Poll::Pending;
        }
        let available = this
            .fail_after
            .saturating_sub(this.bytes.position() as usize);
        if available == 0 {
            this.faulted.store(true, Ordering::Release);
        }
        assert!(available > 0, "injected decoded output callback panic");
        let count = output.len().min(available);
        std::task::Poll::Ready(this.bytes.read(&mut output[..count]))
    }
}
impl Drop for PanickingOutput {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}

#[test]
fn decoded_callback_panic_after_pending_is_published_and_retires() {
    crate::init_test_task_pools();
    let valid = [
        &b"YUV4MPEG2 W2 H2 C420mpeg2\nFRAME\n"[..],
        &[1, 2, 3, 4, 5, 6],
    ]
    .concat();
    for fail_after in [0, valid.len()] {
        let dropped = Arc::new(AtomicBool::new(false));
        let output = PanickingOutput {
            bytes: Cursor::new(valid.clone()),
            fail_after,
            pending_once: true,
            dropped: dropped.clone(),
            faulted: Arc::new(AtomicBool::new(false)),
        };
        let (send, receive) = async_channel::bounded(4);
        let mut task = shin_tasks::async_io::spawn_loader(forward_decoded_frames(output, send));
        let retirement = task.retirement_handle().unwrap();
        assert_eq!(task.poll_with_timeout(Duration::from_secs(2)), Some(()));
        if fail_after > 0 {
            let frame = receive
                .recv_blocking()
                .unwrap()
                .unwrap()
                .into_nv12()
                .unwrap();
            assert_eq!(frame.y_plane, [1, 2, 3, 4]);
        }
        let error = receive.recv_blocking().unwrap().unwrap_err();
        assert!(format!("{error:#}").contains("injected decoded output callback panic"));
        assert!(receive.recv_blocking().is_err());
        retirement.clone().retire(task);
        assert!(retirement.is_finished() && dropped.load(Ordering::Acquire));
    }
}

#[test]
fn decoded_callback_error_waiting_on_full_queue_can_retire() {
    crate::init_test_task_pools();
    let valid = [
        &b"YUV4MPEG2 W2 H2 C420mpeg2\nFRAME\n"[..],
        &[1, 2, 3, 4, 5, 6],
    ]
    .concat();
    let dropped = Arc::new(AtomicBool::new(false));
    let faulted = Arc::new(AtomicBool::new(false));
    let (send, receive) = async_channel::bounded(1);
    let output = PanickingOutput {
        fail_after: valid.len(),
        bytes: Cursor::new(valid),
        pending_once: true,
        dropped: dropped.clone(),
        faulted: faulted.clone(),
    };
    let mut task = shin_tasks::async_io::spawn_loader(forward_decoded_frames(output, send));
    let retirement = task.retirement_handle().unwrap();
    let started = std::time::Instant::now();
    while !faulted.load(Ordering::Acquire) {
        assert!(started.elapsed() < Duration::from_secs(2));
        std::thread::yield_now();
    }
    assert_eq!(task.poll_with_timeout(Duration::from_millis(5)), None);
    // Consumer leaves the movie while the retained error sender is blocked.
    task.cancel_and_retire();
    assert!(retirement.is_finished() && dropped.load(Ordering::Acquire));
    assert!(receive.recv_blocking().unwrap().is_ok());
    assert!(receive.recv_blocking().is_err());
}
