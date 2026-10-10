//! Glue together mp4 demuxing, h264 and aac decoding and `shin-render` APIs to implement video playback in `shin`.

mod audio;
mod h264_decoder;
pub mod mp4;
mod mp4_bitstream_converter;
mod texture;
mod timer;
mod video_player;

pub use h264_decoder::{FrameTiming, Nv12Frame};
pub use texture::VideoFrameTexture;
pub use video_player::VideoPlayerHandle;
#[cfg(any(test, feature = "reference-tests"))]
pub use video_player::{ReferenceLoopPreparationGate, ReferenceLoopWorker};

#[cfg(test)]
fn init_test_task_pools() {
    static POOLS: std::sync::Once = std::sync::Once::new();
    POOLS.call_once(shin_tasks::create_task_pools);
}

#[cfg(test)]
mod test_support {
    use std::{
        io::{Cursor, Read, Seek, SeekFrom},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    };

    pub(crate) struct FaultingReader {
        pub bytes: Cursor<Vec<u8>>,
        pub fail: Arc<AtomicBool>,
    }
    impl Read for FaultingReader {
        fn read(&mut self, destination: &mut [u8]) -> std::io::Result<usize> {
            if self.fail.load(Ordering::Acquire) {
                return Err(std::io::Error::other(
                    "injected movie AAC source read failure",
                ));
            }
            self.bytes.read(destination)
        }
    }
    impl Seek for FaultingReader {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            self.bytes.seek(position)
        }
    }

    pub(crate) fn opening_bytes() -> Vec<u8> {
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file("/movie/op1.mp4").unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    }
}
