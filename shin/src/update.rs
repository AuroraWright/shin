use std::sync::Arc;

use shin_core::{primitives::update::FrameId, time::Ticks};

use crate::{asset::system::AssetServer, render::PreRenderContext};

pub struct UpdateContext<'immutable, 'pre_render, 'pipelines, 'dynbuffer, 'encoder> {
    pub frame_id: FrameId,
    /// Wall-clock start shared by every command in this presentation frame.
    pub frame_started_at: std::time::Instant,
    pub delta_ticks: Ticks,
    pub asset_server: &'immutable Arc<AssetServer>,
    pub pre_render: &'pre_render mut PreRenderContext<'immutable, 'pipelines, 'dynbuffer, 'encoder>,
}

impl UpdateContext<'_, '_, '_, '_, '_> {
    pub fn loader_wait_budget(&self) -> std::time::Duration {
        loader_wait_duration(self.frame_started_at.elapsed())
    }
}

fn loader_wait_duration(elapsed: std::time::Duration) -> std::time::Duration {
    // Native(delta_us >>4) <=624 permits elapsed0..9999us.
    if elapsed.as_micros() < 10_000 {
        std::time::Duration::from_millis(5)
    } else {
        std::time::Duration::ZERO
    }
}

pub struct AdvUpdateContext<'a> {
    pub frame_id: FrameId,
    pub delta_ticks: Ticks,
    #[expect(unused)] // for future stuff
    pub asset_server: &'a Arc<AssetServer>,
    // we do not provide access to pre-render context here because there is another method for it (at least on layers)
    // you can create render resources and schedule transmissions though
    #[allow(dead_code)] // Retained alongside queue for layer resource creation in this context.
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,

    pub are_animations_allowed: bool,
    /// Native bustup listener: pan the character voice and return its mouth level.
    pub voice_callback: Option<&'a dyn Fn(i32, f32) -> Option<u8>>,
    pub movie_volume_settings: crate::audio::MovieVolumeSettings,
}

pub trait Updatable {
    fn update(&mut self, context: &mut UpdateContext);
}

impl<T: Updatable> Updatable for Box<T> {
    #[inline]
    fn update(&mut self, context: &mut UpdateContext) {
        (**self).update(context)
    }
}

pub trait AdvUpdatable {
    fn update(&mut self, context: &AdvUpdateContext);
}

impl<T: AdvUpdatable> AdvUpdatable for Box<T> {
    #[inline]
    fn update(&mut self, context: &AdvUpdateContext) {
        (**self).update(context)
    }
}

#[cfg(test)]
mod loader_budget_tests {
    #[test]
    fn native_frame_gate_matches_host_wait_budget_at_microsecond_boundaries() {
        for elapsed_us in 0u64..=20000 {
            let native_permitted = (elapsed_us >> 4) <= 624;
            let expected = if native_permitted {
                std::time::Duration::from_millis(5)
            } else {
                std::time::Duration::ZERO
            };
            assert_eq!(
                super::loader_wait_duration(std::time::Duration::from_micros(elapsed_us)),
                expected
            );
        }
    }
    #[test]
    #[ignore = "requires supplied decompressed executable"]
    fn reference_native_loader_wait_frame_gate() {
        let root = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe =
            std::fs::read(std::path::Path::new(&root).with_file_name("uncompressed_executable"))
                .unwrap();
        for (address, expected) in [
            (0x8f08, 0x9403ca1a),
            (0x8f10, 0x9403cae0),
            (0x8f14, 0xcb000288),
            (0x8f18, 0xd344fd08),
            (0x8f1c, 0xf109c11f),
            (0x8f20, 0x540000c8),
            (0x8f30, 0x52827101),
            (0x8f34, 0xd63f0100),
            (0x9150, 0x9403c988),
            (0x9158, 0x9403ca4e),
            (0x915c, 0xcb000288),
            (0x9160, 0xd344fd08),
            (0x9164, 0xf109c11f),
            (0x9168, 0x54ffe248),
            (0x9178, 0x52827101),
            (0x917c, 0xd63f0100),
            (0x65f0, 0x9403d460),
            (0x65f8, 0x9403d526),
            (0x65fc, 0xcb000288),
            (0x6600, 0xd344fd08),
            (0x6604, 0xf109c11f),
            (0x6608, 0x540000c8),
            (0x6618, 0x52827101),
            (0x661c, 0xd63f0100),
            (0xfb7c4, 0x529ef9ea),
            (0xfb7c8, 0x72bc6a6a),
            (0xfb7d8, 0x529374ab),
            (0xfb7dc, 0x72a4188b),
            (0xfb800, 0x9347fd08),
            (0xfba94, 0xf9403100),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "Native wait instruction {address:x}"
            );
        }
    }
}
