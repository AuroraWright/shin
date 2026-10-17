//! Debug-build frame dumps for headless investigation of the real window loop.
//!
//! With `SHIN_DEBUG_CAPTURE_DIR=<dir>`, every `SHIN_DEBUG_CAPTURE_EVERY` (60)
//! presented frames are written as `<dir>/frame-<n>.rgba`: a 12-byte header
//! (`RGBA`, u32 width, u32 height, little endian) and tightly packed rows in
//! the surface byte order. `SHIN_DEBUG_CAPTURE_FROM` skips earlier frames,
//! `SHIN_DEBUG_CAPTURE_AFTER_SECS` earlier wall-clock time, and
//! `SHIN_DEBUG_CAPTURE_LIMIT` caps the number of dumps.
//! `SHIN_DEBUG_KEEP_RENDERING=1` keeps updating and rendering while the
//! window is unfocused, hidden or offscreen (occluded frames render into an
//! offscreen texture, so captures continue) for unattended playtests.
//! Release builds and unset variables do nothing.
use std::{
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

struct Settings {
    dir: PathBuf,
    every: u64,
    from: u64,
    after: std::time::Instant,
    limit: u64,
}

fn settings() -> Option<&'static Settings> {
    static SETTINGS: OnceLock<Option<Settings>> = OnceLock::new();
    SETTINGS
        .get_or_init(|| {
            if !cfg!(debug_assertions) {
                return None;
            }
            let dir = PathBuf::from(std::env::var_os("SHIN_DEBUG_CAPTURE_DIR")?);
            std::fs::create_dir_all(&dir).ok()?;
            let every = std::env::var("SHIN_DEBUG_CAPTURE_EVERY")
                .ok()
                .and_then(|value| value.parse().ok())
                .filter(|&value| value > 0)
                .unwrap_or(60);
            let from = std::env::var("SHIN_DEBUG_CAPTURE_FROM")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            let secs = |name| {
                std::env::var(name)
                    .ok()
                    .and_then(|value| value.parse().ok())
            };
            let after = std::time::Instant::now()
                + std::time::Duration::from_secs(secs("SHIN_DEBUG_CAPTURE_AFTER_SECS").unwrap_or(0));
            let limit = secs("SHIN_DEBUG_CAPTURE_LIMIT").unwrap_or(u64::MAX);
            Some(Settings {
                dir,
                every,
                from,
                after,
                limit,
            })
        })
        .as_ref()
}

/// Playtest switch: ignore focus loss and render occluded frames offscreen.
pub fn keep_rendering() -> bool {
    static KEEP: OnceLock<bool> = OnceLock::new();
    *KEEP.get_or_init(|| {
        cfg!(debug_assertions)
            && std::env::var_os("SHIN_DEBUG_KEEP_RENDERING").is_some_and(|value| value != "0")
    })
}

/// macOS App Nap would throttle a hidden playtest window; hold a
/// user-initiated activity (idle system sleep still allowed) for the process.
#[cfg(target_os = "macos")]
pub fn disable_app_nap() {
    use objc2::{class, msg_send, runtime::AnyObject};
    // NSActivityUserInitiatedAllowingIdleSystemSleep.
    const OPTIONS: u64 = 0x00FF_FFFF;
    unsafe {
        let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
        let reason: *mut AnyObject = msg_send![
            class!(NSString),
            stringWithUTF8String: c"Shin playtest".as_ptr()
        ];
        let activity: *mut AnyObject =
            msg_send![info, beginActivityWithOptions: OPTIONS, reason: reason];
        // Keep the activity token for the lifetime of the process.
        let _: *mut AnyObject = msg_send![activity, retain];
    }
}
#[cfg(not(target_os = "macos"))]
pub fn disable_app_nap() {}

static FRAME: AtomicU64 = AtomicU64::new(0);

/// Count one presented frame; true when it should be captured.
pub fn due() -> bool {
    let Some(settings) = settings() else {
        return false;
    };
    static TAKEN: AtomicU64 = AtomicU64::new(0);
    let frame = FRAME.fetch_add(1, Ordering::Relaxed);
    frame >= settings.from
        && frame % settings.every == 0
        && std::time::Instant::now() >= settings.after
        && TAKEN.fetch_add(1, Ordering::Relaxed) < settings.limit
}

pub struct Pending {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row: u32,
    frame: u64,
}

pub fn copy(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
) -> Pending {
    let (width, height) = (texture.width(), texture.height());
    let padded_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("debug capture"),
        size: u64::from(padded_row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        texture.size(),
    );
    Pending {
        buffer,
        width,
        height,
        padded_row,
        frame: FRAME.load(Ordering::Relaxed) - 1,
    }
}

/// Wait for the copy (after submission) and write the frame file.
pub fn write(device: &wgpu::Device, pending: Pending) {
    let Some(settings) = settings() else {
        return;
    };
    let slice = pending.buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    if device.poll(wgpu::PollType::wait_indefinitely()).is_err() {
        return;
    }
    let Ok(data) = slice.get_mapped_range() else {
        return;
    };
    let mut bytes = Vec::with_capacity(12 + (pending.width * pending.height * 4) as usize);
    bytes.extend_from_slice(b"RGBA");
    bytes.extend_from_slice(&pending.width.to_le_bytes());
    bytes.extend_from_slice(&pending.height.to_le_bytes());
    for row in 0..pending.height {
        let start = (row * pending.padded_row) as usize;
        bytes.extend_from_slice(&data[start..start + (pending.width * 4) as usize]);
    }
    drop(data);
    pending.buffer.unmap();
    let path = settings.dir.join(format!("frame-{:06}.rgba", pending.frame));
    let _ = std::fs::write(path, bytes);
}
