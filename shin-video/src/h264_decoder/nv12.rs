use dpi::PhysicalSize;
use parking_lot::Mutex;

#[derive(Debug)]
pub struct Nv12Frame {
    pub y_plane: Vec<u8>,
    pub uv_plane: Vec<u8>,
    pub size: PhysicalSize<u32>,
}

/// The planes of dropped frames, for the platform decoders to copy the next
/// frames into while any of them runs: a fresh 1080p frame's 3 MB would be
/// faulted in page by page 30 times a second.
static SPARES: Mutex<Spares> = parking_lot::const_mutex(Spares {
    decoders: 0,
    planes: Vec::new(),
});

struct Spares {
    /// Live [`SparePlanes`] guards.
    decoders: usize,
    planes: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Spares {
    /// Planes kept: a playing movie frees one frame as its decoder fills one.
    const KEEP: usize = 4;
}

impl Nv12Frame {
    /// Empty planes to fill, reusing a dropped frame's buffers when there is one.
    pub(super) fn spare_planes() -> (Vec<u8>, Vec<u8>) {
        SPARES.lock().planes.pop().unwrap_or_default()
    }
}

impl Drop for Nv12Frame {
    fn drop(&mut self) {
        let mut spares = SPARES.lock();
        if spares.decoders > 0 && spares.planes.len() < Spares::KEEP {
            spares.planes.push((
                std::mem::take(&mut self.y_plane),
                std::mem::take(&mut self.uv_plane),
            ));
        }
    }
}

/// Keeps dropped frames' planes for reuse while a decoder runs; the last guard
/// to go frees them.
pub(super) struct SparePlanes(());

impl SparePlanes {
    pub fn hold() -> Self {
        SPARES.lock().decoders += 1;
        Self(())
    }
}

impl Drop for SparePlanes {
    fn drop(&mut self) {
        let mut spares = SPARES.lock();
        spares.decoders -= 1;
        let unused = if spares.decoders == 0 {
            std::mem::take(&mut spares.planes)
        } else {
            Vec::new()
        };
        drop(spares);
        drop(unused);
    }
}
