//! Actual AAC packet callback failures without GPU, audio hardware or saves.
use super::*;
use crate::{mp4::Mp4, test_support::opening_bytes};
use std::{
    io::{Cursor, SeekFrom},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::atomic::AtomicUsize,
};

struct PanickingReader {
    bytes: Cursor<Vec<u8>>,
    armed: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
}
impl Read for PanickingReader {
    fn read(&mut self, destination: &mut [u8]) -> std::io::Result<usize> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        assert!(
            !self.armed.load(Ordering::Acquire),
            "injected AAC packet callback panic"
        );
        self.bytes.read(destination)
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

#[test]
#[ignore = "requires supplied ROM; no GPU/audio/window or progress writes"]
fn reference_aac_callback_panics_are_terminal_and_retire() {
    let bytes = opening_bytes();
    let mut expected = AacFrameSource::new(
        Mp4::new(Cursor::new(bytes.clone()))
            .unwrap()
            .audio_track
            .unwrap(),
    )
    .unwrap();
    let expected_frame = expected.decode_frame().unwrap().unwrap();
    drop(expected);

    let mut escapes = Vec::new();
    for (name, seek, warmup) in [
        ("read-cold", false, 0),
        ("read-warm", false, 1),
        ("seek", true, 1),
    ] {
        let armed = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicBool::new(false));
        let mut source = AacFrameSource::new(
            Mp4::new(PanickingReader {
                bytes: Cursor::new(bytes.clone()),
                armed: armed.clone(),
                calls: calls.clone(),
                dropped: dropped.clone(),
            })
            .unwrap()
            .audio_track
            .unwrap(),
        )
        .unwrap();
        let failed = source.decode_failed();
        let mut destination = AudioBuffer::with_capacity(source.max_frame_size());
        for _ in 0..warmup {
            assert!(source.read_frame(&mut destination));
        }
        // Preserve previously decoded caller-owned PCM when the next packet fails.
        destination.push((0.25, -0.25));
        let retained = destination.len();
        armed.store(true, Ordering::Release);
        let result = catch_unwind(AssertUnwindSafe(|| {
            if seek {
                source.samples_seek(10_000).map(|_| true)
            } else {
                Ok(source.read_frame(&mut destination))
            }
        }));
        let escaped = result.is_err();
        eprintln!(
            "AAC_CALLBACK profile={name} escaped={escaped} failed={} position={} pcm_length_preserved={}",
            failed.load(Ordering::Acquire),
            source.current_sample_position(),
            destination.len() == retained,
        );
        if escaped {
            escapes.push(name);
        } else {
            let result = result.unwrap();
            if seek {
                let error = result.unwrap_err();
                assert!(format!("{error:#}").contains("injected AAC packet callback panic"));
            } else {
                assert!(!result.unwrap());
            }
            assert!(failed.load(Ordering::Acquire));
            assert_eq!(destination.len(), retained);
            let position = source.current_sample_position();
            let reads = calls.load(Ordering::Relaxed);
            armed.store(false, Ordering::Release);
            assert!(!source.read_frame(&mut destination));
            assert!(source.samples_seek(0).is_err());
            assert_eq!(source.current_sample_position(), position);
            assert_eq!(calls.load(Ordering::Relaxed), reads);
            assert_eq!(destination.len(), retained);
        }
        drop(source);
        assert!(
            dropped.load(Ordering::Acquire),
            "{name}: reader still owned"
        );
        // A new decoder of the same asset must remain healthy after retirement.
        let mut retry = AacFrameSource::new(
            Mp4::new(Cursor::new(bytes.clone()))
                .unwrap()
                .audio_track
                .unwrap(),
        )
        .unwrap();
        let frame = retry.decode_frame().unwrap().unwrap();
        assert_eq!(frame, expected_frame);
        assert!(!retry.is_failed());
        eprintln!(
            "AAC_CALLBACK profile={name} retired=true retry_samples={}",
            frame.len()
        );
    }
    assert!(escapes.is_empty(), "AAC callback escaped: {escapes:?}");
}
