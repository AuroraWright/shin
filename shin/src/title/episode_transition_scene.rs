//! Native ScenarioSelectBridge e32f0 owner and e4330 fresh entry dispatch.
use super::{episode_transition::EpisodeMotion, episode_transition_backdrop::EpisodeBackdrop};
use crate::render::PreRenderContext;
use shin_core::{
    format::save::{GameDataEntry, RouteData},
    time::Ticks,
};
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};

pub(crate) struct EpisodeScene {
    previous_screen: RenderTexture,
    motion: EpisodeMotion,
    backdrop: EpisodeBackdrop,
    destination: Option<GameDataEntry>,
}
impl EpisodeScene {
    /// Capture the completed picker before retiring it, then transfer that
    /// texture here. Native e33bc retains its own clone of the previous screen.
    pub fn new(
        episode: u8,
        previous_screen: RenderTexture,
        ctx: &PreRenderContext,
        extent: (u16, u16),
    ) -> Self {
        Self {
            previous_screen,
            motion: EpisodeMotion::new(episode),
            backdrop: EpisodeBackdrop::new(ctx, extent),
            destination: None,
        }
    }
    pub fn update(&mut self, delta: Ticks, ctx: &mut PreRenderContext) {
        self.motion.update(delta);
        self.backdrop.pre_render(
            self.previous_screen.as_texture_source(),
            self.motion.frame(),
            ctx,
        );
        // Native e4338 draws from fbac0 after this frame's pre-render and only
        // at completion. Keep that seed in the deferred destination record.
        if let Some(episode) = self.motion.take_episode() {
            self.destination = Some(GameDataEntry {
                scenario_id: episode as i32,
                random_seed: crate::engine_random::next_seed(),
                save_position: 0,
                route_data: RouteData::default(),
            });
        }
    }
    pub fn take_destination(&mut self) -> Option<GameDataEntry> {
        self.destination.take()
    }
    pub fn render(&self, pass: &mut RenderPass) {
        self.backdrop.render(
            self.previous_screen.as_texture_source(),
            self.motion.frame(),
            pass,
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_episode_owner_seed_entry_and_zero_position_loader_gate() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (address, expected) in [
            (0xe4310, 0x97ff5db8),
            (0xe4314, 0x370004a0),
            (0xe432c, 0x390043e8),
            (0xe4330, 0xb9400a68),
            (0xe4334, 0x2903a3ff),
            (0xe4338, 0x94005de2),
            (0xe433c, 0xb90027e0),
            (0xe4340, 0xf90017ff),
            (0xe434c, 0x2a1f03e1),
            (0xe4350, 0x97fcc830),
            (0xe435c, 0x97fd0c05),
            (0x16434, 0xb9006bf4),
            (0x16468, 0xb9006a88),
            (0x19518, 0xb9406a82),
            (0x19524, 0x97ffd983),
            (0xfd30, 0xb9002b78),
            (0x19964, 0xf9400676),
            (0x199c4, 0x91004263),
            (0x199d4, 0x97ffd417),
            (0xea98, 0xb9800e88),
            (0xea9c, 0x52800509),
            (0xeaa0, 0x9b095108),
            (0xeaa4, 0x29421103),
            (0xeab8, 0x940372ea),
            (0x19ee0, 0xb9401a88),
            (0x19ee8, 0x34000ec8),
            (0x1a0cc, 0xf943a529),
            (0x1a0dc, 0xa906ffe9),
            (0x1a0f0, 0x9400347c),
            (0x1b364, 0xf9400800),
            (0x1b368, 0xb9a53c08),
            (0x1b370, 0x9b090108),
            (0x1b374, 0xb96db009),
            (0x1b378, 0xb9654908),
            (0x1b37c, 0x6b09011f),
            (0x1b380, 0x54000449),
            (0x1b3fc, 0xb96db009),
            (0x1b400, 0x6b09011f),
            (0x1b404, 0x54fffcc8),
            (0xeb68, 0x5285b608),
            (0xeb6c, 0x8b080260),
            (0xeb70, 0x97fffec4),
            (0xe6a8, 0xf900001f),
        ] {
            assert_eq!(word(address), expected, "native episode entry {address:x}");
        }
        let relocation = |target: u64| {
            (0x11e040..0x152eb8)
                .step_by(24)
                .find_map(|a| {
                    let slot = u64::from_le_bytes(exe[a + 256..a + 264].try_into().unwrap());
                    (slot == target)
                        .then(|| u64::from_le_bytes(exe[a + 272..a + 280].try_into().unwrap()))
                })
                .unwrap()
        };
        assert_eq!(relocation(0x224748), 0x1b340);
        assert_eq!(relocation(0x1d24a8), 0x19ea0);
    }
}
